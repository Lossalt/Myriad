//! The export file (`myriad_persona_sync::archive`) one piece at a time,
//! so a platform streams a file of any size through calls that each read
//! only their input: bytes cross as base64, a chunk at a time.
//!
//! Writing: `sync.archive_seal` with the caller's fresh salt and nonce
//! prefix gives the header and its line; `sync.archive_key` the key;
//! `sync.archive_part` each part's head; `sync.archive_seal` each chunk
//! of the stream (`chunkBytes` exactly, the last at most that).
//! Reading: `sync.archive_open` with the file's start gives the header and
//! where the chunks start; `sync.archive_key` the key from the header's
//! costs; `sync.archive_open` each chunk; `sync.archive_part` each head;
//! `sync.manifest` of the pages to check against the end.
//!
//! The passphrase and the key never appear in an answer's error.

use base64::Engine as _;
use myriad_persona_sync::{
    Envelope,
    archive::{self, ArchiveHeader, ArchiveKey, HeaderFields, Kdf, Manifest, Part},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::input;
use crate::Failure;

fn refused(error: impl std::fmt::Display) -> Failure {
    Failure::BadInput(error.to_string())
}

fn bytes(raw: &str, what: &str) -> Result<Vec<u8>, Failure> {
    base64::engine::general_purpose::STANDARD
        .decode(raw.as_bytes())
        .map_err(|_| refused(format!("{what} is not base64")))
}

fn base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn fixed<const N: usize>(raw: &str, what: &str) -> Result<[u8; N], Failure> {
    bytes(raw, what)?
        .try_into()
        .map_err(|_| refused(format!("{what} is {N} bytes")))
}

fn key(raw: &str) -> Result<ArchiveKey, Failure> {
    ArchiveKey::from_bytes(&bytes(raw, "the key")?).map_err(refused)
}

/// `{passphrase, kdf}` → `{key}`: argon2id under the header's costs.
pub(super) fn archive_key(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        passphrase: String,
        kdf: Kdf,
    }
    let req: In = input(raw)?;
    let key = archive::derive_key(&req.passphrase, &req.kdf).map_err(refused)?;
    Ok(json!({ "key": base64(key.as_bytes()) }))
}

/// A new header, `{salt, noncePrefix, fields}` (salt 16 and prefix 19
/// random bytes, base64) → `{header, line}` (the line, newline included,
/// is the file's first bytes); or one chunk, `{key, header, index, last,
/// data}` → `{sealed}`.
pub(super) fn archive_seal(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum In {
        #[serde(rename_all = "camelCase")]
        Header {
            salt: String,
            nonce_prefix: String,
            fields: HeaderFields,
        },
        Chunk(Box<Chunk>),
    }
    match input::<In>(raw)? {
        In::Header {
            salt,
            nonce_prefix,
            fields,
        } => {
            let header = ArchiveHeader::new(
                &fixed(&salt, "the salt")?,
                &fixed(&nonce_prefix, "the nonce prefix")?,
                fields,
            )
            .map_err(refused)?;
            let mut line = header.line().map_err(refused)?;
            line.push(b'\n');
            Ok(json!({ "header": header, "line": base64(&line) }))
        }
        In::Chunk(chunk) => {
            let data = bytes(&chunk.data, "data")?;
            let sealed = archive::seal_chunk(
                &key(&chunk.key)?,
                &chunk.header,
                chunk.index,
                chunk.last,
                &data,
            )
            .map_err(refused)?;
            Ok(json!({ "sealed": base64(&sealed) }))
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    key: String,
    header: ArchiveHeader,
    index: u32,
    last: bool,
    /// The plaintext when sealing, the sealed chunk when opening.
    data: String,
}

/// The file's start, `{start}` (at least the header line, base64) →
/// `{header, offset}` (where chunk 0 begins); or one chunk, `{key, header,
/// index, last, data}` → `{data}`. Every chunk but the last is
/// `chunkBytes` + 16 bytes; the last is what remains.
pub(super) fn archive_open(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum In {
        Start { start: String },
        Chunk(Box<Chunk>),
    }
    match input::<In>(raw)? {
        In::Start { start } => {
            let (header, offset) =
                archive::read_header(&bytes(&start, "start")?).map_err(refused)?;
            Ok(json!({ "header": header, "offset": offset }))
        }
        In::Chunk(chunk) => {
            let sealed = bytes(&chunk.data, "data")?;
            let data = archive::open_chunk(
                &key(&chunk.key)?,
                &chunk.header,
                chunk.index,
                chunk.last,
                &sealed,
            )
            .map_err(refused)?;
            Ok(json!({ "data": base64(&data) }))
        }
    }
}

/// A part's head as it stands in the stream, `{part}` → `{head}`; or the
/// part whose head starts `{bytes}` → `{part, headBytes}` (its payload,
/// `part.bytes` long, follows), `{part: null}` until the whole head is
/// there.
pub(super) fn archive_part(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum In {
        Write { part: Part },
        Read { bytes: String },
    }
    match input::<In>(raw)? {
        In::Write { part } => Ok(json!({ "head": base64(&part.head().map_err(refused)?) })),
        In::Read { bytes: raw } => {
            match Part::read_head(&bytes(&raw, "bytes")?).map_err(refused)? {
                Some((part, used)) => Ok(json!({ "part": part, "headBytes": used })),
                None => Ok(json!({ "part": null })),
            }
        }
    }
}

/// `{envelopes}` → `{manifest}`: what a copy of her holds, counted and
/// digested the same on every side. Pages are put in their canonical form
/// first.
pub(super) fn manifest(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        envelopes: Vec<Envelope>,
    }
    let req: In = input(raw)?;
    let envelopes = req
        .envelopes
        .into_iter()
        .map(myriad_persona_sync::canonicalize_envelope)
        .collect::<Result<Vec<_>, _>>()
        .map_err(refused)?;
    let manifest = Manifest::of_envelopes(&envelopes).map_err(refused)?;
    Ok(json!({ "manifest": manifest }))
}
