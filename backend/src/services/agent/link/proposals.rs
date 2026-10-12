//! Her Work proposals as cards on the app: `proposals` down, the owner's
//! `proposal.answer` up. Accepting one there is the owner's own consent, as
//! it is on the site. Where accepting runs it is `places`, from where she
//! proposed it: a site proposal starts here, on the site, for them as their
//! current role allows, and its outcome reaches the app as a notice; one for
//! this Mac runs on the Mac itself, under its own grants and without its
//! switch for the site asking it to act, since the owner accepted it there in
//! person. Its experience names the proposal, which then ends as the run did.

use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::services::agent::AgentInteractionMode;
use crate::services::agent::consciousness::{
    AcceptSource, IntentRecord, IntentStatus, IntentStore, WorkPlace,
};
use crate::services::agent::run::{ProcessContext, ProcessRequest, start_for_user};

use super::connection::Outgoing;

/// At most this many open proposals are sent, newest first (as the site lists them).
const OPEN: u64 = 20;

/// Where accepting a proposal runs its Work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    /// On the site, her host.
    Site,
    /// On this Mac.
    Mac,
}

impl Place {
    fn as_str(self) -> &'static str {
        match self {
            Place::Site => "site",
            Place::Mac => "mac",
        }
    }

    /// `proposal.answer`'s place; none is the site.
    pub(super) fn of(place: Option<&str>) -> Option<Self> {
        match place {
            None | Some("site") => Some(Place::Site),
            Some("mac") => Some(Place::Mac),
            Some(_) => None,
        }
    }
}

/// Where accepting the proposal on the app `device_id` runs it: where she
/// proposed it, and a Mac only for her proposals for that Mac.
pub(super) fn places(record: &IntentRecord, device_id: &str) -> Vec<Place> {
    if !record.proposal.on_mac() {
        return vec![Place::Site];
    }
    match record.proposal.device_id.as_deref() {
        Some(device) if device != device_id => Vec::new(),
        _ => vec![Place::Mac],
    }
}

/// Her proposals to the owner still open: proposed, or accepted and not
/// started yet.
pub(super) async fn open(
    db: &DatabaseConnection,
    owner: i32,
) -> Result<Vec<IntentRecord>, sea_orm::DbErr> {
    IntentStore::new(db.clone()).actionable(owner, OPEN).await
}

fn card(record: &IntentRecord, device_id: &str) -> Value {
    let place = record.proposal.place.unwrap_or(WorkPlace::Site);
    let mut card = json!({
        "id": record.id,
        "title": record.proposal.title,
        "instruction": record.proposal.instruction,
        "expectedOutcome": record.proposal.expected_outcome,
        "status": record.status.as_str(),
        "createdAt": record.created_at.timestamp_millis(),
        "place": place.as_str(),
        "places": places(record, device_id).iter().map(|place| place.as_str()).collect::<Vec<_>>(),
    });
    if let Some(name) = record
        .proposal
        .device_name
        .as_deref()
        .filter(|_| record.proposal.on_mac())
    {
        card["deviceName"] = name.into();
    }
    if let Some(expires) = record.expires_at {
        card["expiresAt"] = expires.timestamp_millis().into();
    }
    card
}

/// The `proposals` frame for these, as the app `device_id` can answer them.
pub(super) fn frame(records: &[IntentRecord], device_id: &str) -> Outgoing {
    Outgoing::latest(
        "proposals",
        json!({ "proposals": records.iter().map(|record| card(record, device_id)).collect::<Vec<_>>() }),
    )
}

/// What the app was last told, to tell it again only when this changes.
pub(super) fn digest(records: &[IntentRecord]) -> Vec<(String, &'static str, i64)> {
    records
        .iter()
        .map(|record| {
            (
                record.id.clone(),
                record.status.as_str(),
                record.updated_at.timestamp_millis(),
            )
        })
        .collect()
}

fn answered(proposal_id: &str, accepted: bool) -> Value {
    json!({ "proposalId": proposal_id, "accepted": accepted })
}

pub(super) fn refused(proposal_id: &str, code: &str) -> Outgoing {
    let mut body = answered(proposal_id, false);
    body["code"] = code.into();
    Outgoing::latest("proposal.answered", body)
}

/// The owner declined the proposal on the app: it is closed, as a dismissal
/// on the site closes it.
pub(super) async fn decline(db: &DatabaseConnection, owner: i32, proposal_id: &str) -> Outgoing {
    let store = IntentStore::new(db.clone());
    let _ = store.expire_stale(owner).await;
    match store
        .transition(
            proposal_id,
            owner,
            IntentStatus::Abandoned,
            None,
            None,
            None,
        )
        .await
    {
        Ok(_) => Outgoing::latest("proposal.answered", answered(proposal_id, false)),
        Err(sea_orm::DbErr::RecordNotFound(_) | sea_orm::DbErr::Custom(_)) => {
            refused(proposal_id, "proposal_unknown")
        }
        Err(error) => {
            tracing::warn!(%error, "[link] could not close a proposal");
            refused(proposal_id, "internal_error")
        }
    }
}

/// The owner accepted the proposal on the app `device_id`: it is theirs to
/// run. At the site the Work starts as if accepted there, through the same
/// gate; on this Mac the proposal is running and the app runs it itself.
pub(super) async fn accept(
    db: &DatabaseConnection,
    owner: i32,
    device_id: &str,
    proposal_id: &str,
    place: Place,
) -> Outgoing {
    let store = IntentStore::new(db.clone());
    let _ = store.expire_stale(owner).await;
    match store.find(proposal_id, owner).await {
        Ok(record) if !record.status.is_terminal() => {
            if !places(&record, device_id).contains(&place) {
                return refused(proposal_id, "place_unknown");
            }
        }
        Ok(_) | Err(sea_orm::DbErr::RecordNotFound(_)) => {
            return refused(proposal_id, "proposal_unknown");
        }
        Err(error) => {
            tracing::warn!(%error, "[link] could not read a proposal");
            return refused(proposal_id, "internal_error");
        }
    }
    let accepted = match store
        .mark_accepted(proposal_id, owner, AcceptSource::User)
        .await
    {
        Ok(accepted) => accepted,
        Err(sea_orm::DbErr::RecordNotFound(_) | sea_orm::DbErr::Custom(_)) => {
            return refused(proposal_id, "proposal_unknown");
        }
        Err(error) => {
            tracing::warn!(%error, "[link] could not accept a proposal");
            return refused(proposal_id, "internal_error");
        }
    };
    if place == Place::Mac {
        return match store
            .transition(&accepted.id, owner, IntentStatus::Running, None, None, None)
            .await
        {
            Ok(_) => {
                tracing::info!(
                    proposal = proposal_id,
                    "[link] proposal accepted to run on the app"
                );
                let mut body = answered(proposal_id, true);
                body["place"] = Place::Mac.as_str().into();
                Outgoing::latest("proposal.answered", body)
            }
            Err(sea_orm::DbErr::RecordNotFound(_) | sea_orm::DbErr::Custom(_)) => {
                refused(proposal_id, "proposal_unknown")
            }
            Err(error) => {
                tracing::warn!(%error, "[link] could not start a proposal on the app");
                refused(proposal_id, "internal_error")
            }
        };
    }
    let request = ProcessRequest {
        input: accepted.proposal.instruction.clone(),
        context: Some(ProcessContext {
            mode: Some(AgentInteractionMode::Work),
            intention_id: Some(accepted.id.clone()),
            ..Default::default()
        }),
    };
    match start_for_user(db.clone(), owner, request).await {
        Ok(run) => {
            let mut body = answered(proposal_id, true);
            body["place"] = Place::Site.as_str().into();
            body["runId"] = run.run_id().into();
            if let Some(session) = run.session_id() {
                body["sessionId"] = session.into();
            }
            tracing::info!(
                proposal = proposal_id,
                place = place.as_str(),
                "[link] proposal accepted on the app"
            );
            Outgoing::latest("proposal.answered", body)
        }
        Err(error) => {
            let code = match error.code() {
                Some(code) if code != "unmapped" => code.to_string(),
                _ => "internal_error".to_string(),
            };
            tracing::info!(proposal = proposal_id, %code, "[link] proposal's Work refused");
            refused(proposal_id, &code)
        }
    }
}
