//! The export file `myriad.her-archive`: all of her, sealed with the
//! owner's passphrase. One file serves every export: the owner's own
//! 「导出她」, the copy a side keeps when she moves off it, and a Mac
//! host's daily backup. It is always sealed.
//!
//! A file is one header line, then the sealed stream:
//!
//! ```text
//! {the header, canonical JSON}\n
//! chunk 0 ‖ chunk 1 ‖ … ‖ the last chunk
//! ```
//!
//! - The key is argon2id of the passphrase (NFC) with the header's salt and
//!   costs ([`derive_key`]).
//! - Every chunk but the last seals exactly `chunkBytes` of the stream; the
//!   last seals at most that, possibly nothing. Each is XChaCha20-Poly1305,
//!   its nonce the header's 19-byte prefix, then the chunk's index as a
//!   big-endian `u32`, then one byte: 1 on the last chunk, 0 on every other.
//!   The header line, without its newline, is every chunk's associated data.
//! - So a file cut at a chunk's edge ends on a chunk not sealed as the last,
//!   a moved or repeated chunk is under the wrong index, a changed header
//!   opens nothing, and a cut inside a chunk fails its tag. A wrong
//!   passphrase and a damaged file look the same: chunk 0 does not open.
//!
//! The stream is a run of [`Part`]s, each its head's length (`u32`
//! big-endian), the head (canonical JSON), then `bytes` of payload:
//! envelope pages (canonical [`Envelope`] JSON, vectors included), then
//! the files they refer to, then one [`Part::End`] carrying the
//! [`Manifest`] the header's `digest` names. Nothing follows the end.
//!
//! Pure like the rest of the crate: the salt and the nonce prefix come in
//! from the caller, which takes them from its system's random source.
//!
//! What is in a file is exactly what the envelopes carry: her, never a
//! model key, a device token, a keychain account, a data key or a grant.

use std::collections::BTreeMap;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    Key, KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use myriad_merope::RigManifest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::{
    SyncError,
    canonical::{canonical_bytes, canonical_of, hex},
    entity::{
        AssetRef, AssetType, Embedding, EntityVersion, Envelope, FORMAT_VERSION,
        canonicalize_envelope,
    },
    hlc::check_node,
    rig::rig_package_id,
};

/// The header's fixed `format`.
pub const ARCHIVE_FORMAT: &str = "myriad.her-archive";
/// The version of the file's own frame (header, chunks, parts). What the
/// envelopes inside are is `formatVersion`.
pub const ARCHIVE_VERSION: u32 = 1;
pub const CIPHER: &str = "xchacha20poly1305";
pub const KDF_ALG: &str = "argon2id";
/// What a writer seals per chunk.
pub const CHUNK_BYTES: u32 = 1 << 20;
/// What a reader takes as a chunk size: a file written with another size
/// still opens.
pub const MIN_CHUNK_BYTES: u32 = 1 << 10;
pub const MAX_CHUNK_BYTES: u32 = 16 << 20;
/// A sealed chunk is its plaintext and this tag.
pub const TAG_BYTES: usize = 16;
pub const SALT_BYTES: usize = 16;
pub const NONCE_PREFIX_BYTES: usize = 19;
pub const KEY_BYTES: usize = 32;
/// Longest header line, newline excluded.
pub const MAX_HEADER_BYTES: usize = 16 << 10;
/// Longest head of a part.
pub const MAX_PART_HEAD_BYTES: u32 = 64 << 10;

/// The argon2id costs a writer uses: 64 MiB, three passes, one lane.
pub const DEFAULT_MEM_KIB: u32 = 64 << 10;
pub const DEFAULT_ITERATIONS: u32 = 3;
pub const DEFAULT_PARALLELISM: u32 = 1;
/// Costs a file may state. Below the floor a guessed passphrase is too
/// cheap to try; above the ceiling, opening a file someone handed over
/// would take the machine's memory.
pub const MIN_MEM_KIB: u32 = 19 << 10;
pub const MAX_MEM_KIB: u32 = 256 << 10;
pub const MIN_ITERATIONS: u32 = 2;
pub const MAX_ITERATIONS: u32 = 10;
pub const MAX_PARALLELISM: u32 = 4;

/// How the key comes from the passphrase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Kdf {
    /// Always `argon2id` (version 0x13).
    pub alg: String,
    pub mem_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
    /// [`SALT_BYTES`] in lowercase hex.
    pub salt: String,
}

impl Kdf {
    /// The writer's costs with this salt.
    pub fn new(salt: &[u8; SALT_BYTES]) -> Self {
        Self {
            alg: KDF_ALG.into(),
            mem_kib: DEFAULT_MEM_KIB,
            iterations: DEFAULT_ITERATIONS,
            parallelism: DEFAULT_PARALLELISM,
            salt: hex(salt),
        }
    }

    pub fn check(&self) -> Result<(), SyncError> {
        if self.alg != KDF_ALG {
            return Err(SyncError::new(format!("the key comes from {KDF_ALG}")));
        }
        if !(MIN_MEM_KIB..=MAX_MEM_KIB).contains(&self.mem_kib)
            || !(MIN_ITERATIONS..=MAX_ITERATIONS).contains(&self.iterations)
            || !(1..=MAX_PARALLELISM).contains(&self.parallelism)
        {
            return Err(SyncError::new(format!(
                "argon2id costs are {MIN_MEM_KIB}..={MAX_MEM_KIB} KiB, \
                 {MIN_ITERATIONS}..={MAX_ITERATIONS} passes, 1..={MAX_PARALLELISM} lanes"
            )));
        }
        unhex::<SALT_BYTES>(&self.salt, "the salt")?;
        Ok(())
    }
}

/// The header line: what a file is, how to open it, and whose it is.
/// Readable without the passphrase, so it holds nothing of hers but where
/// she came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArchiveHeader {
    pub format: String,
    pub version: u32,
    /// The `myriad.her` version of the envelopes inside.
    pub format_version: u32,
    pub kdf: Kdf,
    pub cipher: String,
    /// [`NONCE_PREFIX_BYTES`] in lowercase hex.
    pub nonce_prefix: String,
    pub chunk_bytes: u32,
    /// Which birth of her this is.
    #[serde(default)]
    pub lineage: Option<String>,
    /// Her host lease's epoch when the file was written.
    #[serde(default)]
    pub epoch: Option<u64>,
    /// The side that wrote it.
    pub origin: String,
    /// Unix microseconds.
    pub created_at: i64,
    /// The [`Manifest`]'s digest: what the file holds, checked after it is
    /// opened.
    pub digest: String,
}

/// What a header is made from, besides the frame's constants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeaderFields {
    #[serde(default)]
    pub lineage: Option<String>,
    #[serde(default)]
    pub epoch: Option<u64>,
    pub origin: String,
    pub created_at: i64,
    pub digest: String,
}

impl ArchiveHeader {
    /// A writer's header: the format's costs and chunk size, the caller's
    /// fresh salt and nonce prefix.
    pub fn new(
        salt: &[u8; SALT_BYTES],
        nonce_prefix: &[u8; NONCE_PREFIX_BYTES],
        fields: HeaderFields,
    ) -> Result<Self, SyncError> {
        let header = Self {
            format: ARCHIVE_FORMAT.into(),
            version: ARCHIVE_VERSION,
            format_version: FORMAT_VERSION,
            kdf: Kdf::new(salt),
            cipher: CIPHER.into(),
            nonce_prefix: hex(nonce_prefix),
            chunk_bytes: CHUNK_BYTES,
            lineage: fields.lineage,
            epoch: fields.epoch,
            origin: fields.origin,
            created_at: fields.created_at,
            digest: fields.digest,
        };
        header.check()?;
        Ok(header)
    }

    pub fn check(&self) -> Result<(), SyncError> {
        if self.format != ARCHIVE_FORMAT {
            return Err(SyncError::new(format!("format is {ARCHIVE_FORMAT:?}")));
        }
        if self.version != ARCHIVE_VERSION {
            return Err(SyncError::new(format!(
                "archive version {} is not one this side reads ({ARCHIVE_VERSION})",
                self.version
            )));
        }
        if self.format_version != FORMAT_VERSION {
            return Err(SyncError::new(format!(
                "format version {} is not one this side reads ({FORMAT_VERSION})",
                self.format_version
            )));
        }
        self.kdf.check()?;
        if self.cipher != CIPHER {
            return Err(SyncError::new(format!("the cipher is {CIPHER}")));
        }
        unhex::<NONCE_PREFIX_BYTES>(&self.nonce_prefix, "the nonce prefix")?;
        if !(MIN_CHUNK_BYTES..=MAX_CHUNK_BYTES).contains(&self.chunk_bytes) {
            return Err(SyncError::new(format!(
                "chunks are {MIN_CHUNK_BYTES}..={MAX_CHUNK_BYTES} bytes"
            )));
        }
        if let Some(lineage) = &self.lineage
            && (lineage.is_empty() || lineage.len() > 64 || lineage.chars().any(char::is_control))
        {
            return Err(SyncError::new("a lineage is 1 to 64 printable characters"));
        }
        check_node(&self.origin)?;
        check_sha256(&self.digest, "the header's digest")?;
        Ok(())
    }

    /// The header line, newline excluded: also every chunk's associated
    /// data.
    pub fn line(&self) -> Result<Vec<u8>, SyncError> {
        self.check()?;
        canonical_of(self).map_err(SyncError::from_json)
    }
}

/// Reads the header at the start of a file: the header and where the
/// first chunk starts. `bytes` need hold only the start of the file.
pub fn read_header(bytes: &[u8]) -> Result<(ArchiveHeader, usize), SyncError> {
    let window = &bytes[..bytes.len().min(MAX_HEADER_BYTES + 1)];
    let Some(end) = window.iter().position(|byte| *byte == b'\n') else {
        return Err(SyncError::new("this is not a her archive (no header line)"));
    };
    let line = &bytes[..end];
    let header: ArchiveHeader = serde_json::from_slice(line)
        .map_err(|_| SyncError::new("this is not a her archive (unreadable header)"))?;
    header.check()?;
    if header.line()? != line {
        return Err(SyncError::new(
            "the header line is not in its canonical form",
        ));
    }
    Ok((header, end + 1))
}

/// The key a passphrase gives under a header's costs. Zeroed when dropped;
/// never printed.
pub struct ArchiveKey([u8; KEY_BYTES]);

impl ArchiveKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SyncError> {
        let bytes: [u8; KEY_BYTES] = bytes
            .try_into()
            .map_err(|_| SyncError::new(format!("a key is {KEY_BYTES} bytes")))?;
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(&Key::from(self.0))
    }
}

impl std::fmt::Debug for ArchiveKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ArchiveKey(..)")
    }
}

impl Drop for ArchiveKey {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // SAFETY: a valid, aligned `u8` of our own array.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

/// argon2id of the passphrase, read as NFC so the same words typed on
/// another system give the same key. An empty passphrase seals nothing.
pub fn derive_key(passphrase: &str, kdf: &Kdf) -> Result<ArchiveKey, SyncError> {
    kdf.check()?;
    let normalized: String = passphrase.nfc().collect();
    if normalized.is_empty() {
        return Err(SyncError::new("the passphrase is empty"));
    }
    let salt = unhex::<SALT_BYTES>(&kdf.salt, "the salt")?;
    let params = Params::new(
        kdf.mem_kib,
        kdf.iterations,
        kdf.parallelism,
        Some(KEY_BYTES),
    )
    .map_err(|_| SyncError::new("argon2id costs are out of range"))?;
    let mut out = [0u8; KEY_BYTES];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(normalized.as_bytes(), &salt, &mut out)
        .map_err(|_| SyncError::new("the key could not be derived"))?;
    let key = ArchiveKey(out);
    out.fill(0);
    Ok(key)
}

fn nonce(prefix: &[u8; NONCE_PREFIX_BYTES], index: u32, last: bool) -> XNonce {
    let mut nonce = [0u8; 24];
    nonce[..NONCE_PREFIX_BYTES].copy_from_slice(prefix);
    nonce[NONCE_PREFIX_BYTES..NONCE_PREFIX_BYTES + 4].copy_from_slice(&index.to_be_bytes());
    nonce[23] = u8::from(last);
    XNonce::from(nonce)
}

/// The one message for a chunk that does not open: the passphrase cannot
/// be told from damage, and the message holds neither.
const DOES_NOT_OPEN: &str = "the passphrase is wrong, or the file is damaged or cut short";

/// Seals one chunk. Every chunk but the last is exactly `chunkBytes`.
pub fn seal_chunk(
    key: &ArchiveKey,
    header: &ArchiveHeader,
    index: u32,
    last: bool,
    plaintext: &[u8],
) -> Result<Vec<u8>, SyncError> {
    let size = header.chunk_bytes as usize;
    if (!last && plaintext.len() != size) || plaintext.len() > size {
        return Err(SyncError::new(format!(
            "a chunk seals {size} bytes, the last at most that"
        )));
    }
    let aad = header.line()?;
    let prefix = unhex::<NONCE_PREFIX_BYTES>(&header.nonce_prefix, "the nonce prefix")?;
    key.cipher()
        .encrypt(
            &nonce(&prefix, index, last),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| SyncError::new("the chunk could not be sealed"))
}

/// Opens one chunk, which the caller cut from the file: every chunk but
/// the last is `chunkBytes` plus the tag; the last is what remains.
pub fn open_chunk(
    key: &ArchiveKey,
    header: &ArchiveHeader,
    index: u32,
    last: bool,
    sealed: &[u8],
) -> Result<Vec<u8>, SyncError> {
    let size = header.chunk_bytes as usize + TAG_BYTES;
    if (!last && sealed.len() != size) || sealed.len() > size || sealed.len() < TAG_BYTES {
        return Err(SyncError::new(DOES_NOT_OPEN));
    }
    let aad = header.line()?;
    let prefix = unhex::<NONCE_PREFIX_BYTES>(&header.nonce_prefix, "the nonce prefix")?;
    key.cipher()
        .decrypt(
            &nonce(&prefix, index, last),
            Payload {
                msg: sealed,
                aad: &aad,
            },
        )
        .map_err(|_| SyncError::new(DOES_NOT_OPEN))
}

/// Seals a stream as it comes: the header line first, then every whole
/// chunk, then on [`Sealer::finish`] the last.
pub struct Sealer {
    key: ArchiveKey,
    header: ArchiveHeader,
    index: u32,
    pending: Vec<u8>,
    started: bool,
    ended: bool,
}

impl Sealer {
    pub fn new(key: ArchiveKey, header: ArchiveHeader) -> Result<Self, SyncError> {
        header.check()?;
        Ok(Self {
            key,
            header,
            index: 0,
            pending: Vec::new(),
            started: false,
            ended: false,
        })
    }

    /// Bytes of the stream; gives back what is ready to append to the file.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<u8>, SyncError> {
        if self.ended {
            return Err(SyncError::new("nothing follows the end"));
        }
        let mut out = self.start()?;
        self.pending.extend_from_slice(bytes);
        let size = self.header.chunk_bytes as usize;
        while self.pending.len() >= size {
            let chunk: Vec<u8> = self.pending.drain(..size).collect();
            out.extend(seal_chunk(
                &self.key,
                &self.header,
                self.index,
                false,
                &chunk,
            )?);
            self.index = self
                .index
                .checked_add(1)
                .ok_or_else(|| SyncError::new("too many chunks"))?;
        }
        Ok(out)
    }

    /// One part: its head, then its payload.
    pub fn part(&mut self, part: &Part, payload: &[u8]) -> Result<Vec<u8>, SyncError> {
        if part.payload_bytes() != payload.len() as u64 {
            return Err(SyncError::new("a part's bytes are its payload's length"));
        }
        if let Part::End { manifest } = part
            && manifest.digest != self.header.digest
        {
            return Err(SyncError::new(
                "the end's manifest is the one the header names",
            ));
        }
        let mut out = self.push(&part.head()?)?;
        out.extend(self.push(payload)?);
        if matches!(part, Part::End { .. }) {
            self.ended = true;
        }
        Ok(out)
    }

    /// The last chunk. The stream must have ended with [`Part::End`].
    pub fn finish(mut self) -> Result<Vec<u8>, SyncError> {
        if !self.ended {
            return Err(SyncError::new("a file ends with its manifest"));
        }
        let mut out = self.start()?;
        let rest = std::mem::take(&mut self.pending);
        out.extend(seal_chunk(
            &self.key,
            &self.header,
            self.index,
            true,
            &rest,
        )?);
        Ok(out)
    }

    fn start(&mut self) -> Result<Vec<u8>, SyncError> {
        if self.started {
            return Ok(Vec::new());
        }
        self.started = true;
        let mut line = self.header.line()?;
        line.push(b'\n');
        Ok(line)
    }
}

/// Opens a stream as it comes (chunks only: the header line is read with
/// [`read_header`]). A chunk is opened once more bytes follow it, so the
/// last is always the one [`Opener::finish`] opens.
pub struct Opener {
    key: ArchiveKey,
    header: ArchiveHeader,
    index: u32,
    pending: Vec<u8>,
}

impl Opener {
    pub fn new(key: ArchiveKey, header: ArchiveHeader) -> Result<Self, SyncError> {
        header.check()?;
        Ok(Self {
            key,
            header,
            index: 0,
            pending: Vec::new(),
        })
    }

    pub fn push(&mut self, sealed: &[u8]) -> Result<Vec<u8>, SyncError> {
        self.pending.extend_from_slice(sealed);
        let size = self.header.chunk_bytes as usize + TAG_BYTES;
        let mut out = Vec::new();
        while self.pending.len() > size {
            let chunk: Vec<u8> = self.pending.drain(..size).collect();
            out.extend(open_chunk(
                &self.key,
                &self.header,
                self.index,
                false,
                &chunk,
            )?);
            self.index = self
                .index
                .checked_add(1)
                .ok_or_else(|| SyncError::new(DOES_NOT_OPEN))?;
        }
        Ok(out)
    }

    pub fn finish(self) -> Result<Vec<u8>, SyncError> {
        open_chunk(&self.key, &self.header, self.index, true, &self.pending)
    }
}

/// One part of the stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "part", rename_all = "snake_case", deny_unknown_fields)]
pub enum Part {
    /// A page of her: canonical [`Envelope`] JSON.
    Envelope { bytes: u64 },
    /// A file an entity refers to, by the id the envelopes name. A rig is
    /// two parts under one id: `manifest.json`, then `atlas.png`.
    #[serde(rename_all = "camelCase")]
    Asset {
        id: String,
        #[serde(rename = "type")]
        kind: AssetType,
        media_type: String,
        #[serde(default)]
        file: Option<String>,
        bytes: u64,
    },
    /// What the file holds. Nothing follows it.
    End { manifest: Manifest },
}

impl Part {
    pub fn payload_bytes(&self) -> u64 {
        match self {
            Part::Envelope { bytes } | Part::Asset { bytes, .. } => *bytes,
            Part::End { .. } => 0,
        }
    }

    fn check(&self) -> Result<(), SyncError> {
        if let Part::Asset {
            id,
            kind,
            media_type,
            file,
            ..
        } = self
        {
            if id.len() != 64
                || !id
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            {
                return Err(SyncError::new("an asset id is 64 lowercase hex characters"));
            }
            if media_type.trim().is_empty() {
                return Err(SyncError::new("an asset has a media type"));
            }
            let named = match kind {
                AssetType::Image => file.is_none(),
                AssetType::Rig => {
                    matches!(file.as_deref(), Some(RIG_MANIFEST_FILE | RIG_ATLAS_FILE))
                }
            };
            if !named {
                return Err(SyncError::new(
                    "a picture is one part; a rig is its manifest.json and atlas.png",
                ));
            }
        }
        Ok(())
    }

    /// The head as it stands in the stream: its length, then itself.
    pub fn head(&self) -> Result<Vec<u8>, SyncError> {
        self.check()?;
        let json = canonical_of(self).map_err(SyncError::from_json)?;
        let mut out = Vec::with_capacity(4 + json.len());
        out.extend_from_slice(&(json.len() as u32).to_be_bytes());
        out.extend(json);
        Ok(out)
    }

    /// The part whose head starts `bytes`, and how many bytes the head took
    /// (its payload follows). None until the whole head is there.
    pub fn read_head(bytes: &[u8]) -> Result<Option<(Part, usize)>, SyncError> {
        let Some(length) = bytes.get(..4) else {
            return Ok(None);
        };
        let length = u32::from_be_bytes(length.try_into().expect("four bytes"));
        if length == 0 || length > MAX_PART_HEAD_BYTES {
            return Err(SyncError::new("a part's head is unreadable"));
        }
        let end = 4 + length as usize;
        let Some(json) = bytes.get(4..end) else {
            return Ok(None);
        };
        let part: Part = serde_json::from_slice(json)
            .map_err(|error| SyncError::new(format!("a part's head: {error}")))?;
        part.check()?;
        Ok(Some((part, end)))
    }
}

pub const RIG_MANIFEST_FILE: &str = "manifest.json";
pub const RIG_ATLAS_FILE: &str = "atlas.png";

/// Splits an opened stream into its parts as it comes.
#[derive(Default)]
pub struct PartReader {
    pending: Vec<u8>,
    ended: bool,
}

impl PartReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every part now whole, with its payload.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<(Part, Vec<u8>)>, SyncError> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            if self.ended {
                if !self.pending.is_empty() {
                    return Err(SyncError::new("something follows the end"));
                }
                return Ok(out);
            }
            let Some((part, head)) = Part::read_head(&self.pending)? else {
                return Ok(out);
            };
            let total = head as u64 + part.payload_bytes();
            if (self.pending.len() as u64) < total {
                return Ok(out);
            }
            let payload = self.pending[head..total as usize].to_vec();
            self.pending.drain(..total as usize);
            if matches!(part, Part::End { .. }) {
                self.ended = true;
            }
            out.push((part, payload));
        }
    }

    pub fn finish(self) -> Result<(), SyncError> {
        if !self.ended {
            return Err(SyncError::new("the file stops before its end"));
        }
        Ok(())
    }
}

/// One count and digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tally {
    pub count: u64,
    /// sha256 (hex) of the items' canonical bytes, each with a newline, in
    /// their order.
    pub digest: String,
}

impl Tally {
    fn of<'a>(items: impl IntoIterator<Item = &'a [u8]>) -> Self {
        let mut hasher = Sha256::new();
        let mut count = 0;
        for item in items {
            hasher.update(item);
            hasher.update(b"\n");
            count += 1;
        }
        Self {
            count,
            digest: hex(&hasher.finalize()),
        }
    }
}

/// What a whole copy of her holds: each kind's entities (by id), the
/// vectors (by memory) and the files (by id), each counted and digested,
/// and one digest of all of it. Two sides that hold the same her compute
/// the same manifest, whatever order they read her in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    /// By kind name.
    pub kinds: BTreeMap<String, Tally>,
    pub embeddings: Tally,
    pub assets: Tally,
    /// sha256 (hex) of the canonical JSON of the three above.
    pub digest: String,
}

impl Manifest {
    /// The manifest of canonical entities, vectors and file references.
    pub fn of(
        entities: &[EntityVersion],
        embeddings: &[Embedding],
        assets: &[AssetRef],
    ) -> Result<Self, SyncError> {
        let mut by_kind: BTreeMap<String, Vec<(&str, Vec<u8>)>> = BTreeMap::new();
        for entity in entities {
            by_kind
                .entry(entity.kind.as_str().to_string())
                .or_default()
                .push((entity.id.as_str(), entity.canonical_bytes()));
        }
        let mut kinds = BTreeMap::new();
        for (kind, mut items) in by_kind {
            items.sort_by(|a, b| a.0.cmp(b.0));
            if items.windows(2).any(|pair| pair[0].0 == pair[1].0) {
                return Err(SyncError::new(format!("a {kind} is listed twice")));
            }
            kinds.insert(kind, Tally::of(items.iter().map(|item| item.1.as_slice())));
        }
        let mut vectors = embeddings
            .iter()
            .map(|embedding| {
                Ok((
                    embedding.memory_id.as_str(),
                    canonical_of(embedding).map_err(SyncError::from_json)?,
                ))
            })
            .collect::<Result<Vec<_>, SyncError>>()?;
        vectors.sort_by(|a, b| a.0.cmp(b.0));
        let mut files = assets
            .iter()
            .map(|asset| {
                Ok((
                    asset.id.as_str(),
                    canonical_of(asset).map_err(SyncError::from_json)?,
                ))
            })
            .collect::<Result<Vec<_>, SyncError>>()?;
        files.sort_by(|a, b| a.0.cmp(b.0));
        files.dedup();
        let embeddings = Tally::of(vectors.iter().map(|item| item.1.as_slice()));
        let assets = Tally::of(files.iter().map(|item| item.1.as_slice()));
        let digest = hex(&Sha256::digest(canonical_bytes(&serde_json::json!({
            "kinds": kinds, "embeddings": embeddings, "assets": assets,
        }))));
        Ok(Self {
            kinds,
            embeddings,
            assets,
            digest,
        })
    }

    /// The manifest of every envelope's contents together.
    pub fn of_envelopes(envelopes: &[Envelope]) -> Result<Self, SyncError> {
        let entities: Vec<EntityVersion> = envelopes
            .iter()
            .flat_map(|envelope| envelope.entities.iter().cloned())
            .collect();
        let embeddings: Vec<Embedding> = envelopes
            .iter()
            .flat_map(|envelope| envelope.embeddings.iter().cloned())
            .collect();
        let assets: Vec<AssetRef> = envelopes
            .iter()
            .flat_map(|envelope| envelope.assets.iter().cloned())
            .collect();
        Self::of(&entities, &embeddings, &assets)
    }
}

/// A file a reader opened and checked: every part read, every page in its
/// canonical form, every picture its own sha256, every rig its package id,
/// every referred file present and the manifest what the header named and
/// what the pages hold.
#[derive(Debug)]
pub struct Opened {
    pub header: ArchiveHeader,
    pub envelopes: Vec<Envelope>,
    /// By asset id: one file for a picture, `manifest.json` and
    /// `atlas.png` for a rig.
    pub assets: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    pub manifest: Manifest,
}

/// Seals whole parts into a whole file.
pub fn seal(
    key: ArchiveKey,
    header: ArchiveHeader,
    parts: &[(Part, &[u8])],
) -> Result<Vec<u8>, SyncError> {
    let mut sealer = Sealer::new(key, header)?;
    let mut out = Vec::new();
    for (part, payload) in parts {
        out.extend(sealer.part(part, payload)?);
    }
    out.extend(sealer.finish()?);
    Ok(out)
}

/// Opens and checks a whole file with the passphrase.
pub fn open(passphrase: &str, file: &[u8]) -> Result<Opened, SyncError> {
    let (header, _) = read_header(file)?;
    let key = derive_key(passphrase, &header.kdf)?;
    open_with_key(key, file)
}

/// [`open`] with the key already derived from the header's costs.
pub fn open_with_key(key: ArchiveKey, file: &[u8]) -> Result<Opened, SyncError> {
    let (header, start) = read_header(file)?;
    let mut opener = Opener::new(key, header.clone())?;
    let mut reader = PartReader::new();
    let mut parts = Vec::new();
    for window in file[start..].chunks(header.chunk_bytes as usize + TAG_BYTES) {
        parts.extend(reader.push(&opener.push(window)?)?);
    }
    parts.extend(reader.push(&opener.finish()?)?);
    reader.finish()?;
    check_parts(header, parts)
}

/// Checks the parts of an opened file and gathers them.
pub fn check_parts(
    header: ArchiveHeader,
    parts: Vec<(Part, Vec<u8>)>,
) -> Result<Opened, SyncError> {
    let mut envelopes = Vec::new();
    let mut assets: BTreeMap<String, BTreeMap<String, Vec<u8>>> = BTreeMap::new();
    let mut kinds: BTreeMap<String, AssetType> = BTreeMap::new();
    let mut end = None;
    for (part, payload) in parts {
        match part {
            Part::Envelope { .. } => {
                let envelope: Envelope = serde_json::from_slice(&payload)
                    .map_err(|error| SyncError::new(format!("an envelope page: {error}")))?;
                let envelope = canonicalize_envelope(envelope)?;
                if envelope.lineage != header.lineage {
                    return Err(SyncError::new("a page is of another lineage than the file"));
                }
                envelopes.push(envelope);
            }
            Part::Asset { id, kind, file, .. } => {
                if *kinds.entry(id.clone()).or_insert(kind) != kind {
                    return Err(SyncError::new("one asset id is two kinds of file"));
                }
                if kind == AssetType::Image && crate::canonical::sha256_hex(&payload) != id {
                    return Err(SyncError::new("a picture is not the bytes its id names"));
                }
                let name = file.unwrap_or_default();
                if assets
                    .entry(id)
                    .or_default()
                    .insert(name, payload)
                    .is_some()
                {
                    return Err(SyncError::new("a file is in the archive twice"));
                }
            }
            Part::End { manifest } => end = Some(manifest),
        }
    }
    let Some(stated) = end else {
        return Err(SyncError::new("the file stops before its end"));
    };
    for (id, files) in &assets {
        if kinds[id] == AssetType::Rig {
            let (Some(manifest), Some(atlas)) =
                (files.get(RIG_MANIFEST_FILE), files.get(RIG_ATLAS_FILE))
            else {
                return Err(SyncError::new("a rig is its manifest.json and atlas.png"));
            };
            let manifest: RigManifest = serde_json::from_slice(manifest)
                .map_err(|error| SyncError::new(format!("a rig's manifest: {error}")))?;
            if rig_package_id(atlas, &manifest).map_err(SyncError::from_json)? != *id {
                return Err(SyncError::new("a rig is not the package its id names"));
            }
        }
    }
    for envelope in &envelopes {
        for asset in &envelope.assets {
            if kinds.get(&asset.id) != Some(&asset.kind) {
                return Err(SyncError::new(format!(
                    "the file {} a page refers to is not in the archive",
                    asset.id
                )));
            }
        }
    }
    let manifest = Manifest::of_envelopes(&envelopes)?;
    if stated != manifest || manifest.digest != header.digest {
        return Err(SyncError::new(
            "the archive does not hold what its manifest says",
        ));
    }
    Ok(Opened {
        header,
        envelopes,
        assets,
        manifest,
    })
}

fn unhex<const N: usize>(raw: &str, what: &str) -> Result<[u8; N], SyncError> {
    let bad = || SyncError::new(format!("{what} is {N} bytes in lowercase hex"));
    if raw.len() != N * 2 {
        return Err(bad());
    }
    let mut out = [0u8; N];
    for (index, pair) in raw.as_bytes().chunks(2).enumerate() {
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        };
        out[index] = (digit(pair[0]).ok_or_else(bad)? << 4) | digit(pair[1]).ok_or_else(bad)?;
    }
    Ok(out)
}

fn check_sha256(raw: &str, what: &str) -> Result<(), SyncError> {
    unhex::<32>(raw, what).map(|_| ())
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod tests;
