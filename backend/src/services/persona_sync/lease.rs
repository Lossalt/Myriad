//! Her host lease: the one end she lives on (`agent_host_lease`, one row).
//!
//! She lives in one place, her host; every other end is a child. The site is
//! the registry of where that is: `holder` is `site` or the id of the device
//! she lives on, `lineage` is her birth, `epoch` goes up by one each time she
//! changes host (or is taken back) and never down, and `state` says whether
//! she is there (`live`), moving (`frozen`: no end has her) or not on this
//! site at all (`vacant`). A move is a small state machine kept on the row
//! (`move`): proposed → frozen → transferring → applied → committed →
//! retired. Before the commit the old host still holds her and a move can be
//! cancelled, which gives everything back as it was: freezing her only puts
//! things aside, it destroys nothing.
//!
//! Anything that writes to the site's her needs the site owner's logged-in
//! session once: a device token can propose a move, never confirm one.
//!
//! The row carries no key, token or grant, and neither does anything made
//! from it. Every change is announced by the database on [`CHANNEL`]; each
//! process keeps the lease it last read ([`current`]) and reads it again on
//! an announcement, and every [`REREAD_EVERY`] in case one was missed. Runs,
//! her own life and her link check that copy.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex, OnceLock, RwLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use myriad_error::AppError;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, QueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Where changes to the lease are announced (`persona_sync.sql`).
pub const CHANNEL: &str = "myriad_her_host";
/// The holder when she lives on the site.
pub const SITE: &str = "site";
/// How often a process reads the lease again without being told to.
pub const REREAD_EVERY: Duration = Duration::from_secs(30);

/// Whether she is on the end that holds her.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Live,
    /// A move is under way: no end has her until it is committed or cancelled.
    Frozen,
    /// She is not on this site and the site does not know where she is (the
    /// device she lived on was removed).
    Vacant,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Live => "live",
            State::Frozen => "frozen",
            State::Vacant => "vacant",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "live" => Some(State::Live),
            "frozen" => Some(State::Frozen),
            "vacant" => Some(State::Vacant),
            _ => None,
        }
    }
}

/// Where a move stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Asked for, waiting for the owner to confirm on the site. She is not
    /// touched yet.
    Proposed,
    /// She is frozen where she was; the manifest of what moves is made.
    Frozen,
    /// What moves is being read or written, page by page.
    Transferring,
    /// The target has all of it, checked, waiting to be committed.
    Applied,
    /// The site changed the holder and the epoch: she lives on the target.
    Committed,
    /// The old host let its copy go.
    Retired,
}

impl Phase {
    /// The step, from 1, as both ends show it ("停在第 N 步").
    pub fn step(self) -> u8 {
        match self {
            Phase::Proposed => 1,
            Phase::Frozen => 2,
            Phase::Transferring => 3,
            Phase::Applied => 4,
            Phase::Committed => 5,
            Phase::Retired => 6,
        }
    }

    /// The step after this one, the only one a move may take next.
    pub fn next(self) -> Option<Phase> {
        match self {
            Phase::Proposed => Some(Phase::Frozen),
            Phase::Frozen => Some(Phase::Transferring),
            Phase::Transferring => Some(Phase::Applied),
            Phase::Applied => Some(Phase::Committed),
            Phase::Committed => Some(Phase::Retired),
            Phase::Retired => None,
        }
    }

    /// Before the commit: the old host still holds her, so cancelling gives
    /// everything back.
    pub fn cancellable(self) -> bool {
        self.step() < Phase::Committed.step()
    }

    /// She is frozen at this step: no end has her.
    pub fn freezes(self) -> bool {
        matches!(self, Phase::Frozen | Phase::Transferring | Phase::Applied)
    }
}

/// What a move does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// She goes to live on another end (`to`).
    Move,
    /// The her on a device (`from`) is merged into the site's, the first
    /// time it connects with a her of its own.
    Merge,
}

impl Kind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "move" => Some(Kind::Move),
            "merge" => Some(Kind::Merge),
            _ => None,
        }
    }
}

/// Who asked for a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Starter {
    /// The owner, logged in on the site: that is the confirmation too.
    Owner,
    /// A device with its token: the owner still confirms on the site.
    Device,
}

/// The move in progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    pub id: String,
    pub kind: Kind,
    /// `site` or a device id, and the same for `to`.
    pub from: String,
    pub to: String,
    pub phase: Phase,
    pub by: Starter,
    /// The digest of the manifest of what moves, once it is made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<DateTime<Utc>>,
}

impl Move {
    /// The device a move is with, whichever way it goes.
    pub fn device(&self) -> &str {
        if self.from == SITE {
            &self.to
        } else {
            &self.from
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lease {
    pub holder: String,
    pub lineage: String,
    pub epoch: i64,
    pub state: State,
    pub moving: Option<Move>,
    pub updated_at: DateTime<Utc>,
}

/// Where she is, as this site sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    /// On the site (or not born yet: nothing to keep from anyone).
    Here,
    /// Moving: on no end.
    Moving,
    /// On another end, the device `holder`.
    Elsewhere(String),
    /// Not here, and the site does not know where.
    Vacant,
}

impl Where {
    pub fn here(&self) -> bool {
        *self == Where::Here
    }
}

impl Lease {
    pub fn place(&self) -> Where {
        match self.state {
            State::Frozen => Where::Moving,
            State::Vacant => Where::Vacant,
            State::Live if self.holder == SITE => Where::Here,
            State::Live => Where::Elsewhere(self.holder.clone()),
        }
    }

    /// As the site owner's page and a device read it: where she lives, her
    /// lineage and epoch, and the move in progress. `name` gives a device id
    /// the name the owner gave it. Nothing secret is in it.
    pub fn json(&self, name: impl Fn(&str) -> Option<String>) -> Value {
        let named = |id: &str| (id != SITE).then(|| name(id)).flatten();
        let moving = self.moving.as_ref().map(|moving| {
            json!({
                "id": moving.id,
                "kind": moving.kind,
                "from": moving.from,
                "fromName": named(&moving.from),
                "to": moving.to,
                "toName": named(&moving.to),
                "phase": moving.phase,
                "step": moving.phase.step(),
                "by": moving.by,
                "startedAt": moving.started_at.to_rfc3339(),
                "confirmedAt": moving.confirmed_at.map(|at| at.to_rfc3339()),
            })
        });
        json!({
            "holder": self.holder,
            "holderName": named(&self.holder),
            "lineage": self.lineage,
            "epoch": self.epoch,
            "state": self.state.as_str(),
            "move": moving,
            "updatedAt": self.updated_at.to_rfc3339(),
        })
    }

    fn from_row(row: &QueryResult) -> Result<Self, DbErr> {
        let state: String = row.try_get("", "state")?;
        let moving: Option<Value> = row.try_get("", "move")?;
        Ok(Self {
            holder: row.try_get("", "holder")?,
            lineage: row.try_get("", "lineage")?,
            epoch: row.try_get("", "epoch")?,
            state: State::parse(&state)
                .ok_or_else(|| DbErr::Custom(format!("unknown host lease state {state}")))?,
            // A move this build cannot read is shown as none; the row keeps it.
            moving: moving.and_then(|raw| serde_json::from_value(raw).ok()),
            updated_at: row
                .try_get::<DateTime<chrono::FixedOffset>>("", "updated_at")?
                .with_timezone(&Utc),
        })
    }
}

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

const COLUMNS: &str = "holder, lineage, epoch, state, move, updated_at";

/// The lease as stored. A site that has her and no lease yet (she was born
/// before leases, or just now) is issued one: the site holds her, epoch 1,
/// a new lineage. None while the site has no her.
pub async fn read<C: ConnectionTrait>(db: &C) -> Result<Option<Lease>, DbErr> {
    let stored = db
        .query_one_raw(statement(
            &format!("SELECT {COLUMNS} FROM agent_host_lease WHERE id = 'her'"),
            vec![],
        ))
        .await?;
    if let Some(row) = stored {
        return Lease::from_row(&row).map(Some);
    }
    let issued = db
        .query_one_raw(statement(
            &format!(
                "INSERT INTO agent_host_lease (id, holder, lineage, epoch, state, updated_at) \
                 SELECT 'her', $1, gen_random_uuid()::text, 1, 'live', NOW() \
                 WHERE EXISTS (SELECT 1 FROM agent_persona WHERE id = 'site') \
                 ON CONFLICT (id) DO NOTHING RETURNING {COLUMNS}"
            ),
            vec![SITE.into()],
        ))
        .await?;
    match issued {
        Some(row) => {
            let lease = Lease::from_row(&row)?;
            tracing::info!(lineage = %lease.lineage, "[host] her host lease issued: the site holds her");
            Ok(Some(lease))
        }
        // Issued by someone else meanwhile, or no her.
        None => db
            .query_one_raw(statement(
                &format!("SELECT {COLUMNS} FROM agent_host_lease WHERE id = 'her'"),
                vec![],
            ))
            .await?
            .as_ref()
            .map(Lease::from_row)
            .transpose(),
    }
}

/// Her persona on the site was deleted while the site held her: the next one
/// is another birth, with another lineage. A lease that names another end is
/// kept (she lives there).
pub async fn forget_birth<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    db.execute_raw(statement(
        "DELETE FROM agent_host_lease WHERE id = 'her' AND holder = $1",
        vec![SITE.into()],
    ))
    .await?;
    Ok(())
}

/// Why a move could not be started or taken a step.
#[derive(Debug)]
pub enum MoveError {
    /// The site has no her to move.
    NoHer,
    /// Another move is under way.
    InProgress,
    /// She does not live on the site now, so the site cannot start a move.
    NotHere,
    /// No such move now.
    Unknown,
    /// The move is not at a step this can be done at.
    Phase(Phase),
    Db(DbErr),
}

impl From<DbErr> for MoveError {
    fn from(error: DbErr) -> Self {
        Self::Db(error)
    }
}

impl MoveError {
    /// The code and status a request is refused with.
    pub fn refusal(&self) -> (axum::http::StatusCode, &'static str, &'static str) {
        use axum::http::StatusCode;
        match self {
            MoveError::NoHer => (
                StatusCode::CONFLICT,
                "move_no_her",
                "The site has no her to move",
            ),
            MoveError::InProgress => (
                StatusCode::CONFLICT,
                "move_in_progress",
                "Another move is under way",
            ),
            MoveError::NotHere => (
                StatusCode::CONFLICT,
                "host_moved",
                "She does not live on this site now",
            ),
            MoveError::Unknown => (StatusCode::NOT_FOUND, "move_unknown", "No such move now"),
            MoveError::Phase(_) => (
                StatusCode::CONFLICT,
                "move_phase",
                "The move is not at a step this can be done at",
            ),
            MoveError::Db(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Could not change her host lease",
            ),
        }
    }
}

async fn locked(txn: &sea_orm::DatabaseTransaction) -> Result<Option<Lease>, DbErr> {
    // Issue first if she has none yet, then hold the row.
    read(txn).await?;
    txn.query_one_raw(statement(
        &format!("SELECT {COLUMNS} FROM agent_host_lease WHERE id = 'her' FOR UPDATE"),
        vec![],
    ))
    .await?
    .as_ref()
    .map(Lease::from_row)
    .transpose()
}

async fn write(
    txn: &sea_orm::DatabaseTransaction,
    state: State,
    moving: Option<&Move>,
) -> Result<Lease, DbErr> {
    let moving = moving
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| DbErr::Custom(error.to_string()))?;
    let row = txn
        .query_one_raw(statement(
            &format!(
                "UPDATE agent_host_lease SET state = $1, move = $2, updated_at = NOW() \
                 WHERE id = 'her' RETURNING {COLUMNS}"
            ),
            vec![state.as_str().into(), moving.into()],
        ))
        .await?
        .ok_or(DbErr::RecordNotUpdated)?;
    Lease::from_row(&row)
}

/// Starts a move of the site's her with the device `device`: `Move` takes her
/// to it, `Merge` brings its her into the site's. Asked by the owner on the
/// site (the confirmation), it freezes her at once; asked by the device, it
/// waits for the owner to confirm.
pub async fn start(
    db: &DatabaseConnection,
    kind: Kind,
    device: &str,
    by: Starter,
) -> Result<Lease, MoveError> {
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::NoHer)?;
    if lease.moving.is_some() {
        return Err(MoveError::InProgress);
    }
    if lease.place() != Where::Here {
        return Err(MoveError::NotHere);
    }
    let (from, to) = match kind {
        Kind::Move => (SITE.to_string(), device.to_string()),
        Kind::Merge => (device.to_string(), SITE.to_string()),
    };
    let now = Utc::now();
    let confirmed = by == Starter::Owner;
    let moving = Move {
        id: format!("mv_{}", uuid::Uuid::new_v4().simple()),
        kind,
        from,
        to,
        phase: if confirmed {
            Phase::Frozen
        } else {
            Phase::Proposed
        },
        by,
        digest: None,
        started_at: now,
        confirmed_at: confirmed.then_some(now),
    };
    let state = if moving.phase.freezes() {
        State::Frozen
    } else {
        State::Live
    };
    let lease = write(&txn, state, Some(&moving)).await?;
    txn.commit().await?;
    tracing::info!(
        move_id = %moving.id,
        kind = ?kind,
        device,
        by = ?by,
        phase = ?moving.phase,
        epoch = lease.epoch,
        "[host] a move of her was started"
    );
    Ok(lease)
}

/// The owner confirmed on the site the move `id` a device asked for: she is
/// frozen where she is.
pub async fn confirm(db: &DatabaseConnection, id: &str) -> Result<Lease, MoveError> {
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::Unknown)?;
    let mut moving = lease
        .moving
        .filter(|moving| moving.id == id)
        .ok_or(MoveError::Unknown)?;
    if moving.phase != Phase::Proposed {
        return Err(MoveError::Phase(moving.phase));
    }
    moving.phase = moving.phase.next().ok_or(MoveError::Phase(moving.phase))?;
    moving.confirmed_at = Some(Utc::now());
    let lease = write(&txn, State::Frozen, Some(&moving)).await?;
    txn.commit().await?;
    tracing::info!(move_id = %id, epoch = lease.epoch, "[host] the owner confirmed a move: she is frozen");
    Ok(lease)
}

/// The move `id` is called off before its commit: she is where she was, as
/// she was. `device`, when a device asks, must be the one the move is with.
/// A her on her way into a site that had none (an import) is not here yet:
/// calling that off leaves the site with no her and no lease (None).
pub async fn cancel(
    db: &DatabaseConnection,
    id: &str,
    device: Option<&str>,
) -> Result<Option<Lease>, MoveError> {
    let txn = db.begin().await?;
    let lease = locked(&txn).await?.ok_or(MoveError::Unknown)?;
    let moving = lease
        .moving
        .filter(|moving| moving.id == id)
        .filter(|moving| device.is_none_or(|device| moving.device() == device))
        .ok_or(MoveError::Unknown)?;
    if !moving.phase.cancellable() {
        return Err(MoveError::Phase(moving.phase));
    }
    if super::import::is_import(id) {
        super::import::called_off_on(&txn, id).await?;
        txn.commit().await?;
        super::import::forget_files(id).await;
        tracing::info!(move_id = %id, by_device = device.is_some(), "[host] her way into the site was called off: no her here");
        return Ok(None);
    }
    let lease = write(&txn, State::Live, None).await?;
    txn.commit().await?;
    tracing::info!(
        move_id = %id,
        phase = ?moving.phase,
        by_device = device.is_some(),
        epoch = lease.epoch,
        "[host] a move was cancelled: she is back as she was"
    );
    Ok(Some(lease))
}

/// A her on her way into a site that had none, from the device `device`
/// that holds her at `lineage` and `epoch`: the site writes its first lease
/// when the owner confirms, naming that device as where she still is and
/// frozen (no end has her) until she lands here or the import is called off
/// (H3c-1). Nothing else of the site changes before she lands.
pub(super) async fn arriving(
    txn: &sea_orm::DatabaseTransaction,
    import_id: &str,
    device: &str,
    lineage: &str,
    epoch: i64,
    digest: &str,
) -> Result<Lease, DbErr> {
    let now = Utc::now();
    let moving = Move {
        id: import_id.to_string(),
        kind: Kind::Move,
        from: device.to_string(),
        to: SITE.to_string(),
        phase: Phase::Frozen,
        by: Starter::Device,
        digest: Some(digest.to_string()),
        started_at: now,
        confirmed_at: Some(now),
    };
    let moving = serde_json::to_value(&moving).map_err(|error| DbErr::Custom(error.to_string()))?;
    let row = txn
        .query_one_raw(statement(
            &format!(
                "INSERT INTO agent_host_lease (id, holder, lineage, epoch, state, move, updated_at) \
                 VALUES ('her', $1, $2, $3, 'frozen', $4, NOW()) RETURNING {COLUMNS}"
            ),
            vec![
                device.into(),
                lineage.into(),
                epoch.max(1).into(),
                moving.into(),
            ],
        ))
        .await?
        .ok_or(DbErr::RecordNotInserted)?;
    Lease::from_row(&row)
}

/// The import `import_id` took a step (it is being sent, or all of it is
/// here and checked).
pub(super) async fn import_step(
    txn: &sea_orm::DatabaseTransaction,
    import_id: &str,
    phase: Phase,
) -> Result<(), DbErr> {
    let Some(lease) = locked(txn).await? else {
        return Ok(());
    };
    let Some(mut moving) = lease.moving.filter(|moving| moving.id == import_id) else {
        return Ok(());
    };
    if moving.phase.step() >= phase.step() {
        return Ok(());
    }
    moving.phase = phase;
    write(txn, State::Frozen, Some(&moving)).await?;
    Ok(())
}

/// She landed on the site, in the transaction that wrote her: the lease the
/// import wrote now names the site, one epoch on; from a file, the site's
/// first lease is written here, one epoch on from the file's (H3c-3). Her
/// lineage is the one she came with: a move is not a new birth.
pub(super) async fn landed(
    txn: &sea_orm::DatabaseTransaction,
    lineage: &str,
    came_at: i64,
) -> Result<Lease, DbErr> {
    let row = match txn
        .query_one_raw(statement(
            &format!(
                "UPDATE agent_host_lease SET holder = $1, epoch = epoch + 1, state = 'live', \
                 move = NULL, updated_at = NOW() WHERE id = 'her' AND lineage = $2 \
                 RETURNING {COLUMNS}"
            ),
            vec![SITE.into(), lineage.into()],
        ))
        .await?
    {
        Some(row) => row,
        None => txn
            .query_one_raw(statement(
                &format!(
                    "INSERT INTO agent_host_lease (id, holder, lineage, epoch, state, updated_at) \
                     VALUES ('her', $1, $2, $3, 'live', NOW()) RETURNING {COLUMNS}"
                ),
                vec![SITE.into(), lineage.into(), (came_at.max(0) + 1).into()],
            ))
            .await?
            .ok_or(DbErr::RecordNotInserted)?,
    };
    Lease::from_row(&row)
}

/// The import `import_id` was called off: the lease it wrote goes, and the
/// site is as it was, with no her.
pub(super) async fn forget_arrival<C: ConnectionTrait>(
    db: &C,
    import_id: &str,
) -> Result<(), DbErr> {
    db.execute_raw(statement(
        "DELETE FROM agent_host_lease WHERE id = 'her' AND holder <> $1 AND move->>'id' = $2",
        vec![SITE.into(), import_id.into()],
    ))
    .await?;
    Ok(())
}

/// The lease this process last read; None before the first read or while
/// the site has no her.
static CURRENT: LazyLock<RwLock<Option<Lease>>> = LazyLock::new(|| RwLock::new(None));
/// Her own drivers told they are paused, so each says so once.
static PAUSED: LazyLock<Mutex<HashSet<&'static str>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

pub fn current() -> Option<Lease> {
    CURRENT.read().ok().and_then(|lease| lease.clone())
}

/// Where she is, by the lease this process last read.
pub fn place() -> Where {
    current().map_or(Where::Here, |lease| lease.place())
}

/// What a run is refused with while she is not on the site; Ok while she is.
pub fn refuse_while_away() -> Result<(), AppError> {
    match place() {
        Where::Here => Ok(()),
        Where::Moving => Err(AppError::conflict(
            "She is moving between her ends; nothing reaches her until the move is done or cancelled",
        )
        .with_code("host_frozen")),
        Where::Elsewhere(_) | Where::Vacant => {
            Err(AppError::conflict("She does not live on this site now").with_code("host_moved"))
        }
    }
}

/// One of her own drivers asks before each tick: true while she is not on
/// the site, and the driver skips it. Each says it is paused once.
pub fn paused(driver: &'static str) -> bool {
    let place = place();
    if place.here() {
        return false;
    }
    if PAUSED.lock().is_ok_and(|mut paused| paused.insert(driver)) {
        tracing::info!(driver, place = ?place, "[host] her own life is paused here: skipped");
    }
    true
}

/// Takes a lease just read: what changed for her links and her drivers.
pub fn adopt(lease: Option<Lease>) {
    let before = place();
    if let Ok(mut current) = CURRENT.write() {
        *current = lease.clone();
    }
    let after = place();
    if before == after {
        return;
    }
    tracing::info!(
        before = ?before,
        after = ?after,
        epoch = lease.as_ref().map(|lease| lease.epoch),
        state = lease.as_ref().map(|lease| lease.state.as_str()),
        "[host] where she lives changed"
    );
    use crate::services::agent::link::{Close, disconnect_all};
    match after {
        // No end has her: every child is told, and reconnects to the refusal.
        Where::Moving => disconnect_all(Close::HostFrozen),
        Where::Elsewhere(_) | Where::Vacant => disconnect_all(Close::HostMoved),
        Where::Here => {
            if let Ok(mut paused) = PAUSED.lock() {
                paused.clear();
            }
            tracing::info!("[host] she is here again: her own life goes on");
            // What came by chat app while she was away is read now, in the
            // process that runs the chat apps (D3).
            if crate::runtime_role::PERSONA_RUNTIME_LOCAL.load(std::sync::atomic::Ordering::Acquire)
                && let Ok(db) = crate::services::process_db::database()
                && tokio::runtime::Handle::try_current().is_ok()
            {
                crate::services::channel_group::back_from_away();
                tokio::spawn(crate::services::channel_work::held::replay(db));
            }
        }
    }
}

/// Reads the lease again and takes it. Tests share one process, so there
/// this process's copy stays as it is (nobody's runs are refused by another
/// test's move).
pub async fn reload<C: ConnectionTrait>(db: &C) {
    if cfg!(test) {
        return;
    }
    match read(db).await {
        Ok(lease) => adopt(lease),
        Err(error) => tracing::warn!(%error, "[host] could not read her host lease"),
    }
}

/// Keeps this process's copy of the lease: read now, again on each
/// announcement and every [`REREAD_EVERY`]. Once per process.
pub fn keep(db: DatabaseConnection) {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    crate::services::jobs::jobs().supervised(
        "host lease listener",
        crate::services::jobs::Backoff::new(Duration::from_secs(2), Duration::from_secs(60)),
        move || listen(db.clone()),
    );
}

async fn listen(db: DatabaseConnection) {
    let pool = db.get_postgres_connection_pool().clone();
    let mut listener = match sea_orm::sqlx::postgres::PgListener::connect_with(&pool).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::warn!(%error, "[host] lease listener unavailable");
            reload(&db).await;
            return;
        }
    };
    if listener.listen(CHANNEL).await.is_err() {
        reload(&db).await;
        return;
    }
    // Listening first, then reading: nothing committed in between is missed.
    reload(&db).await;
    let mut every =
        tokio::time::interval_at(tokio::time::Instant::now() + REREAD_EVERY, REREAD_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            heard = listener.try_recv() => match heard {
                Ok(Some(_)) => reload(&db).await,
                // The connection dropped and came back: what was missed is read.
                Ok(None) => reload(&db).await,
                Err(_) => return,
            },
            _ = every.tick() => reload(&db).await,
        }
    }
}

/// Why her link is refused before the upgrade, read from the database (not
/// this process's copy): `(code, body)`; None while she is on the site.
pub async fn link_refusal<C: ConnectionTrait>(
    db: &C,
) -> Result<Option<(&'static str, Value)>, DbErr> {
    let Some(lease) = read(db).await? else {
        return Ok(None);
    };
    Ok(match lease.place() {
        Where::Here => None,
        Where::Moving => Some((
            "host_frozen",
            json!({
                "error": "She is moving between her ends; try again once the move is done or cancelled",
                "code": "host_frozen",
            }),
        )),
        Where::Elsewhere(holder) => {
            let name = device_name(db, &holder).await?;
            Some((
                "host_moved",
                json!({
                    "error": "She lives on another end now",
                    "code": "host_moved",
                    "host": name.map(|name| json!({ "kind": "app", "name": name })),
                }),
            ))
        }
        Where::Vacant => Some((
            "host_moved",
            json!({ "error": "She does not live on this site now", "code": "host_moved", "host": Value::Null }),
        )),
    })
}

/// The name the owner gave the device `id`, removed or not.
pub async fn device_name<C: ConnectionTrait>(db: &C, id: &str) -> Result<Option<String>, DbErr> {
    db.query_one_raw(statement(
        "SELECT name FROM agent_sync_devices WHERE id = $1",
        vec![id.into()],
    ))
    .await?
    .map(|row| row.try_get::<String>("", "name"))
    .transpose()
}

/// Device names by id, for [`Lease::json`].
pub async fn names<C: ConnectionTrait>(
    db: &C,
    lease: &Lease,
) -> Result<std::collections::HashMap<String, String>, DbErr> {
    let mut names = std::collections::HashMap::new();
    let mut ids = vec![lease.holder.clone()];
    if let Some(moving) = &lease.moving {
        ids.push(moving.from.clone());
        ids.push(moving.to.clone());
    }
    for id in ids.into_iter().filter(|id| id != SITE) {
        if let Some(name) = device_name(db, &id).await? {
            names.insert(id, name);
        }
    }
    Ok(names)
}

/// The lease as JSON with its device names, or null while there is no her.
pub async fn shown<C: ConnectionTrait>(db: &C) -> Result<Value, DbErr> {
    let Some(lease) = read(db).await? else {
        return Ok(Value::Null);
    };
    let names = names(db, &lease).await?;
    Ok(lease.json(|id| names.get(id).cloned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_goes_one_step_at_a_time_and_can_be_called_off_until_its_commit() {
        let order = [
            Phase::Proposed,
            Phase::Frozen,
            Phase::Transferring,
            Phase::Applied,
            Phase::Committed,
            Phase::Retired,
        ];
        for (index, phase) in order.iter().enumerate() {
            assert_eq!(phase.step() as usize, index + 1);
            assert_eq!(phase.next(), order.get(index + 1).copied());
        }
        assert!(Phase::Applied.cancellable() && !Phase::Committed.cancellable());
        assert!(
            !Phase::Proposed.freezes() && Phase::Frozen.freezes() && !Phase::Committed.freezes()
        );
    }

    fn lease(holder: &str, state: State) -> Lease {
        Lease {
            holder: holder.into(),
            lineage: "6f1c".into(),
            epoch: 3,
            state,
            moving: None,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn where_she_is_and_what_a_run_is_refused_with() {
        assert_eq!(lease(SITE, State::Live).place(), Where::Here);
        assert_eq!(lease(SITE, State::Frozen).place(), Where::Moving);
        assert_eq!(
            lease("dev_1", State::Live).place(),
            Where::Elsewhere("dev_1".into())
        );
        assert_eq!(lease("dev_1", State::Vacant).place(), Where::Vacant);
    }

    /// The lease as it is shown carries where she lives and the move, and
    /// nothing else: no key, token or grant.
    #[test]
    fn the_lease_shown_has_no_secret_in_it() {
        let mut shown = lease(SITE, State::Frozen);
        shown.moving = Some(Move {
            id: "mv_1".into(),
            kind: Kind::Merge,
            from: "dev_1".into(),
            to: SITE.into(),
            phase: Phase::Frozen,
            by: Starter::Device,
            digest: None,
            started_at: Utc::now(),
            confirmed_at: Some(Utc::now()),
        });
        let value = shown.json(|id| (id == "dev_1").then(|| "MacBook".to_string()));
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "epoch",
                "holder",
                "holderName",
                "lineage",
                "move",
                "state",
                "updatedAt"
            ]
        );
        assert_eq!(value["holderName"], Value::Null);
        assert_eq!(value["move"]["fromName"], "MacBook");
        assert_eq!(value["move"]["step"], 2);
        assert_eq!(value["move"]["kind"], "merge");
        let moving: Vec<&str> = value["move"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            moving,
            [
                "by",
                "confirmedAt",
                "from",
                "fromName",
                "id",
                "kind",
                "phase",
                "startedAt",
                "step",
                "to",
                "toName"
            ]
        );
    }

    /// Issuing, a move asked for by a device and confirmed by the owner,
    /// cancelling, the guard on the epoch, and a new birth after her persona
    /// goes (an isolated schema; skipped without a test database).
    #[tokio::test]
    async fn the_lease_on_a_database() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        assert_eq!(read(db).await.unwrap(), None, "no her, no lease");
        assert!(matches!(
            start(db, Kind::Move, "dev_1", Starter::Owner).await,
            Err(MoveError::NoHer)
        ));
        db.execute_unprepared(
            "INSERT INTO agent_persona (id, name, updated_at) VALUES ('site', 'Merope', NOW())",
        )
        .await
        .unwrap();
        let issued = read(db).await.unwrap().unwrap();
        assert_eq!(
            (issued.holder.as_str(), issued.epoch, issued.state),
            (SITE, 1, State::Live)
        );
        assert_eq!(issued.lineage.len(), 36);
        assert_eq!(
            read(db).await.unwrap().unwrap().lineage,
            issued.lineage,
            "once"
        );

        // A device asks: nothing happens to her until the owner confirms.
        let proposed = start(db, Kind::Merge, "dev_1", Starter::Device)
            .await
            .unwrap();
        let moving = proposed.moving.clone().unwrap();
        assert_eq!(
            (
                moving.phase,
                moving.from.as_str(),
                moving.to.as_str(),
                proposed.state
            ),
            (Phase::Proposed, "dev_1", SITE, State::Live)
        );
        assert!(matches!(
            start(db, Kind::Move, "dev_2", Starter::Owner).await,
            Err(MoveError::InProgress)
        ));
        assert!(matches!(
            confirm(db, "mv_other").await,
            Err(MoveError::Unknown)
        ));
        let frozen = confirm(db, &moving.id).await.unwrap();
        assert_eq!(frozen.state, State::Frozen);
        assert_eq!(frozen.place(), Where::Moving);
        assert!(frozen.moving.as_ref().unwrap().confirmed_at.is_some());
        assert!(matches!(
            confirm(db, &moving.id).await,
            Err(MoveError::Phase(Phase::Frozen))
        ));
        // Another device cannot call it off; the one it is with can.
        assert!(matches!(
            cancel(db, &moving.id, Some("dev_2")).await,
            Err(MoveError::Unknown)
        ));
        let back = cancel(db, &moving.id, Some("dev_1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                back.state,
                back.moving.is_none(),
                back.epoch,
                back.lineage.as_str()
            ),
            (State::Live, true, 1, issued.lineage.as_str()),
            "as she was"
        );
        // The owner's own move freezes her at once.
        let owner = start(db, Kind::Move, "dev_2", Starter::Owner)
            .await
            .unwrap();
        assert_eq!(owner.state, State::Frozen);
        assert_eq!(owner.moving.as_ref().unwrap().phase, Phase::Frozen);
        let id = owner.moving.unwrap().id;
        cancel(db, &id, None).await.unwrap();

        // She lives on a device: the site starts nothing and refuses her link.
        db.execute_unprepared("UPDATE agent_host_lease SET holder = 'dev_9', epoch = 2")
            .await
            .unwrap();
        assert!(matches!(
            start(db, Kind::Move, "dev_2", Starter::Owner).await,
            Err(MoveError::NotHere)
        ));
        let (code, body) = link_refusal(db).await.unwrap().unwrap();
        assert_eq!((code, &body["host"]), ("host_moved", &Value::Null));
        assert!(
            db.execute_unprepared("UPDATE agent_host_lease SET epoch = 1")
                .await
                .is_err(),
            "the epoch never goes down"
        );
        forget_birth(db).await.unwrap();
        assert_eq!(
            read(db).await.unwrap().unwrap().holder,
            "dev_9",
            "kept: she lives there"
        );
        db.execute_unprepared("UPDATE agent_host_lease SET holder = 'site'")
            .await
            .unwrap();
        // Her persona deleted on the site: the next her is another birth.
        forget_birth(db).await.unwrap();
        let reborn = read(db).await.unwrap().unwrap();
        assert_ne!(reborn.lineage, issued.lineage);
        assert_eq!(reborn.epoch, 1);

        fixture.close().await;
    }
}
