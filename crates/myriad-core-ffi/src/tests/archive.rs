//! The golden export file through the C entry point, as another platform
//! streams it: sealed from its inputs to the same bytes, opened chunk by
//! chunk back to the same parts and manifest.

use base64::Engine as _;
use serde_json::{Value, json};

use super::{ffi, ok};
use crate::STATUS_BAD_INPUT;

pub(super) const TESTED: &[&str] = &[
    "sync.archive_key",
    "sync.archive_open",
    "sync.archive_part",
    "sync.archive_seal",
    "sync.manifest",
];

const FILE: &[u8] = include_bytes!("../../../myriad-persona-sync/fixtures/archive/her.her-archive");
const INPUTS: &[u8] = include_bytes!("../../../myriad-persona-sync/fixtures/archive/inputs.json");
const MANIFEST: &[u8] =
    include_bytes!("../../../myriad-persona-sync/fixtures/archive/manifest.json");

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn unb64(value: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(value.as_str().unwrap())
        .unwrap()
}

fn unhex(raw: &str) -> Vec<u8> {
    (0..raw.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&raw[at..at + 2], 16).unwrap())
        .collect()
}

fn inputs() -> Value {
    serde_json::from_slice(INPUTS).unwrap()
}

fn header_of(file: &[u8]) -> (Value, usize) {
    let out = ok(
        "sync.archive_open",
        json!({ "start": b64(&file[..file.len().min(4096)]) }),
    );
    (
        out["header"].clone(),
        out["offset"].as_u64().unwrap() as usize,
    )
}

fn key_for(passphrase: &str, header: &Value) -> (i32, Value) {
    ffi(
        "sync.archive_key",
        &json!({ "passphrase": passphrase, "kdf": header["kdf"] }),
    )
}

/// Opens a whole file the way a platform streams it; the stream's bytes.
fn open_stream(file: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    let (header, offset) = header_of(file);
    let (status, key) = key_for(passphrase, &header);
    assert_eq!(status, 0, "{key}");
    let size = header["chunkBytes"].as_u64().unwrap() as usize + 16;
    let chunks: Vec<&[u8]> = file[offset..].chunks(size).collect();
    let mut stream = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let (status, out) = ffi(
            "sync.archive_open",
            &json!({
                "key": key["key"], "header": header, "index": index,
                "last": index + 1 == chunks.len(), "data": b64(chunk),
            }),
        );
        if status != 0 {
            return Err(out["error"]["message"].as_str().unwrap().to_string());
        }
        stream.extend(unb64(&out["data"]));
    }
    if chunks.is_empty() {
        return Err("no chunks".into());
    }
    Ok(stream)
}

#[test]
fn the_golden_file_seals_from_its_inputs_through_the_core() {
    let inputs = inputs();
    let made = ok(
        "sync.archive_seal",
        json!({
            "salt": b64(&unhex(inputs["salt"].as_str().unwrap())),
            "noncePrefix": b64(&unhex(inputs["noncePrefix"].as_str().unwrap())),
            "fields": inputs["fields"],
        }),
    );
    let mut header = made["header"].clone();
    // The golden file is in 1 KiB chunks; a writer's own are 1 MiB.
    assert_eq!(header["chunkBytes"], 1 << 20);
    header["chunkBytes"] = inputs["chunkBytes"].clone();
    let (golden_header, offset) = header_of(FILE);
    assert_eq!(header, golden_header);
    let (_, key) = key_for(inputs["passphrase"].as_str().unwrap(), &header);
    let mut stream = Vec::new();
    for part in inputs["parts"].as_array().unwrap() {
        let head = ok("sync.archive_part", json!({ "part": part["part"] }));
        stream.extend(unb64(&head["head"]));
        stream.extend(unhex(part["payload"].as_str().unwrap()));
    }
    let mut file = FILE[..offset].to_vec();
    let size = header["chunkBytes"].as_u64().unwrap() as usize;
    let pieces: Vec<&[u8]> = stream.chunks(size).collect();
    // A stream that fills its last chunk exactly ends on an empty one.
    let mut pieces: Vec<(bool, &[u8])> = pieces
        .iter()
        .enumerate()
        .map(|(index, piece)| (index + 1 == pieces.len() && piece.len() < size, *piece))
        .collect();
    if !pieces.last().is_some_and(|piece| piece.0) {
        pieces.push((true, &[]));
    }
    for (index, (last, piece)) in pieces.iter().enumerate() {
        let sealed = ok(
            "sync.archive_seal",
            json!({ "key": key["key"], "header": header, "index": index, "last": last,
                    "data": b64(piece) }),
        );
        file.extend(unb64(&sealed["sealed"]));
    }
    assert!(
        file == FILE,
        "the core sealed other bytes than the golden file"
    );
}

#[test]
fn the_golden_file_opens_chunk_by_chunk_to_its_parts_and_manifest() {
    let inputs = inputs();
    let stream = open_stream(FILE, inputs["passphrase"].as_str().unwrap()).unwrap();
    let mut at = 0;
    let mut parts = Vec::new();
    let mut envelopes = Vec::new();
    while at < stream.len() {
        let read = ok("sync.archive_part", json!({ "bytes": b64(&stream[at..]) }));
        let head = read["headBytes"].as_u64().unwrap() as usize;
        let length = read["part"]["bytes"].as_u64().unwrap_or(0) as usize;
        let payload = &stream[at + head..at + head + length];
        if read["part"]["part"] == "envelope" {
            envelopes.push(serde_json::from_slice::<Value>(payload).unwrap());
        }
        parts.push(json!({ "part": read["part"], "payload": hex(payload) }));
        at += head + length;
    }
    assert_eq!(Value::Array(parts), inputs["parts"]);
    let manifest = ok("sync.manifest", json!({ "envelopes": envelopes }));
    let golden: Value = serde_json::from_slice(MANIFEST).unwrap();
    assert_eq!(manifest["manifest"], golden);
    assert_eq!(header_of(FILE).0["digest"], golden["digest"]);
    // A head cut short is not yet a part.
    assert_eq!(
        ok("sync.archive_part", json!({ "bytes": b64(&stream[..3]) }))["part"],
        Value::Null
    );
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn a_wrong_passphrase_or_a_cut_file_does_not_open_and_says_nothing_of_either() {
    let passphrase = inputs()["passphrase"].as_str().unwrap().to_string();
    let wrong = open_stream(FILE, "not her passphrase").unwrap_err();
    assert!(wrong.contains("passphrase is wrong"), "{wrong}");
    assert!(!wrong.contains("not her"), "{wrong}");
    let cut = open_stream(&FILE[..FILE.len() - 1], &passphrase).unwrap_err();
    assert!(cut.contains("cut short"), "{cut}");
    let (header, offset) = header_of(FILE);
    let size = header["chunkBytes"].as_u64().unwrap() as usize + 16;
    let last = (FILE.len() - offset) % size;
    assert!(open_stream(&FILE[..FILE.len() - last], &passphrase).is_err());
    let (status, out) = key_for("", &header);
    assert_eq!(status, STATUS_BAD_INPUT, "{out}");
    let (status, out) = ffi("sync.archive_open", &json!({ "start": b64(b"{}\n") }));
    assert_eq!(status, STATUS_BAD_INPUT, "{out}");
}
