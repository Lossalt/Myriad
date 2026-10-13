//! The export file's frame: what opens and what does not.

use super::*;
use crate::tests::{SAMPLE_PASSPHRASE, archive_sample};

fn sealed() -> (ArchiveHeader, usize, Vec<u8>) {
    let file = archive_sample().seal();
    let (header, start) = read_header(&file).unwrap();
    (header, start, file)
}

fn refused(file: &[u8]) -> String {
    open(SAMPLE_PASSPHRASE, file).unwrap_err().to_string()
}

#[test]
fn the_sample_spans_several_chunks_and_opens() {
    let (header, start, file) = sealed();
    let chunk = header.chunk_bytes as usize + TAG_BYTES;
    assert!(file.len() - start > 2 * chunk, "{}", file.len());
    let opened = open(SAMPLE_PASSPHRASE, &file).unwrap();
    assert_eq!(opened.manifest.digest, header.digest);
    assert_eq!(opened.manifest.kinds["memory"].count, 1);
    assert_eq!(opened.manifest.embeddings.count, 1);
    assert_eq!(opened.manifest.assets.count, 2);
}

#[test]
fn a_wrong_passphrase_opens_nothing_and_says_nothing_of_it() {
    let (_, _, file) = sealed();
    let error = open("雨天的伞 umbrella 8", &file).unwrap_err().to_string();
    assert_eq!(error, DOES_NOT_OPEN);
    assert!(!error.contains("umbrella"));
    assert!(open("", &file).is_err());
}

#[test]
fn the_same_words_in_another_normal_form_are_the_same_passphrase() {
    let kdf = archive_sample().header.kdf;
    let composed = derive_key("caf\u{e9}", &kdf).unwrap();
    let decomposed = derive_key("cafe\u{301}", &kdf).unwrap();
    assert_eq!(composed.as_bytes(), decomposed.as_bytes());
    assert_ne!(
        derive_key("cafe", &kdf).unwrap().as_bytes(),
        composed.as_bytes()
    );
    assert_eq!(format!("{composed:?}"), "ArchiveKey(..)");
}

#[test]
fn a_file_cut_short_anywhere_is_refused() {
    let (header, start, file) = sealed();
    let chunk = header.chunk_bytes as usize + TAG_BYTES;
    // At a chunk's edge: the last chunk left is not sealed as the last.
    assert_eq!(refused(&file[..start + chunk]), DOES_NOT_OPEN);
    assert_eq!(refused(&file[..start + 2 * chunk]), DOES_NOT_OPEN);
    // The last chunk cut, or dropped whole.
    assert_eq!(refused(&file[..file.len() - 1]), DOES_NOT_OPEN);
    let last = (file.len() - start) % chunk;
    assert_eq!(refused(&file[..file.len() - last]), DOES_NOT_OPEN);
    // Inside the header line.
    assert!(open(SAMPLE_PASSPHRASE, &file[..start - 1]).is_err());
    // Nothing after the header.
    assert_eq!(refused(&file[..start]), DOES_NOT_OPEN);
}

#[test]
fn moved_changed_or_added_bytes_are_refused() {
    let (header, start, file) = sealed();
    let chunk = header.chunk_bytes as usize + TAG_BYTES;
    let mut swapped = file[..start].to_vec();
    swapped.extend_from_slice(&file[start + chunk..start + 2 * chunk]);
    swapped.extend_from_slice(&file[start..start + chunk]);
    swapped.extend_from_slice(&file[start + 2 * chunk..]);
    assert_eq!(refused(&swapped), DOES_NOT_OPEN);
    let mut flipped = file.clone();
    flipped[start + 5] ^= 1;
    assert_eq!(refused(&flipped), DOES_NOT_OPEN);
    let mut longer = file.clone();
    longer.push(0);
    assert_eq!(refused(&longer), DOES_NOT_OPEN);
    // A header changed in a way that still parses: another epoch.
    let mut other = header.clone();
    other.epoch = Some(4);
    let mut rewritten = other.line().unwrap();
    rewritten.push(b'\n');
    rewritten.extend_from_slice(&file[start..]);
    assert_eq!(refused(&rewritten), DOES_NOT_OPEN);
    // A header not in its canonical form.
    let mut spaced = b"{ ".to_vec();
    spaced.extend_from_slice(&file[1..]);
    assert!(open(SAMPLE_PASSPHRASE, &spaced).is_err());
}

#[test]
fn costs_outside_the_bounds_are_refused_before_any_work() {
    let mut kdf = Kdf::new(&[1; SALT_BYTES]);
    assert!(kdf.check().is_ok());
    kdf.mem_kib = MAX_MEM_KIB + 1;
    assert!(derive_key("x", &kdf).is_err());
    kdf.mem_kib = MIN_MEM_KIB - 1;
    assert!(derive_key("x", &kdf).is_err());
    kdf.mem_kib = DEFAULT_MEM_KIB;
    kdf.iterations = 1;
    assert!(kdf.check().is_err());
    kdf.iterations = DEFAULT_ITERATIONS;
    kdf.salt = "00".into();
    assert!(kdf.check().is_err());
    let mut header = archive_sample().header;
    header.chunk_bytes = MAX_CHUNK_BYTES + 1;
    assert!(header.check().is_err());
}

#[test]
fn a_chunk_seals_its_whole_size_except_the_last() {
    let sample = archive_sample();
    let key = derive_key(sample.passphrase, &sample.header.kdf).unwrap();
    let size = sample.header.chunk_bytes as usize;
    assert!(seal_chunk(&key, &sample.header, 0, false, &vec![0; size - 1]).is_err());
    assert!(seal_chunk(&key, &sample.header, 0, true, &vec![0; size + 1]).is_err());
    let last = seal_chunk(&key, &sample.header, 7, true, b"").unwrap();
    assert_eq!(last.len(), TAG_BYTES);
    assert_eq!(
        open_chunk(&key, &sample.header, 7, true, &last).unwrap(),
        b""
    );
    assert!(open_chunk(&key, &sample.header, 7, false, &last).is_err());
    assert!(open_chunk(&key, &sample.header, 6, true, &last).is_err());
}

#[test]
fn parts_are_checked_against_the_manifest_and_their_files() {
    let sample = archive_sample();
    let key = || derive_key(sample.passphrase, &sample.header.kdf).unwrap();
    let reseal = |parts: Vec<(Part, Vec<u8>)>| {
        let mut sealer = Sealer::new(key(), sample.header.clone()).unwrap();
        let mut out = Vec::new();
        for (part, payload) in &parts {
            out.extend(sealer.push(&part.head().unwrap()).unwrap());
            out.extend(sealer.push(payload).unwrap());
        }
        // Past the sealer's own checks: a writer that lies.
        let mut hack = sealer;
        hack.ended = true;
        out.extend(hack.finish().unwrap());
        out
    };
    // A picture whose bytes are not its id.
    let mut parts = sample.parts.clone();
    parts[1].1 = b"sticker-pixelz".to_vec();
    assert!(refused(&reseal(parts)).contains("picture"));
    // A rig without its atlas.
    let mut parts = sample.parts.clone();
    parts.remove(3);
    assert!(refused(&reseal(parts)).contains("rig"));
    // A page dropped: the manifest no longer holds.
    let mut parts = sample.parts.clone();
    parts.remove(0);
    assert!(open(SAMPLE_PASSPHRASE, &reseal(parts)).is_err());
    // No end.
    let mut parts = sample.parts.clone();
    parts.pop();
    assert!(refused(&reseal(parts)).contains("end"));
    // Something after the end.
    let mut parts = sample.parts.clone();
    parts.push(parts[1].clone());
    assert!(refused(&reseal(parts)).contains("follows the end"));
    // The sealer itself will not end on another manifest than the header's.
    let mut sealer = Sealer::new(key(), sample.header.clone()).unwrap();
    let mut wrong = match &sample.parts[4].0 {
        Part::End { manifest } => manifest.clone(),
        _ => unreachable!(),
    };
    wrong.digest = "0".repeat(64);
    assert!(sealer.part(&Part::End { manifest: wrong }, b"").is_err());
}

#[test]
fn the_manifest_does_not_depend_on_the_order_her_pages_came_in() {
    let opened = open(SAMPLE_PASSPHRASE, &archive_sample().seal()).unwrap();
    let envelope = &opened.envelopes[0];
    let mut split: Vec<Envelope> = envelope
        .entities
        .iter()
        .rev()
        .map(|entity| Envelope {
            entities: vec![entity.clone()],
            embeddings: Vec::new(),
            assets: Vec::new(),
            ..envelope.clone()
        })
        .collect();
    split[0].embeddings = envelope.embeddings.clone();
    split[1].assets = envelope.assets.iter().rev().cloned().collect();
    split[2].assets = envelope.assets.clone();
    assert_eq!(Manifest::of_envelopes(&split).unwrap(), opened.manifest);
    let mut changed = envelope.clone();
    changed.entities[0].rev = "001760000000124-000000-site".parse().unwrap();
    assert_ne!(
        Manifest::of_envelopes(&[changed]).unwrap().kinds["diary"],
        opened.manifest.kinds["diary"]
    );
}

#[test]
fn streaming_in_any_window_sizes_opens_the_same() {
    let sample = archive_sample();
    let (header, start, file) = sealed();
    for window in [1usize, 7, 1040, 4096] {
        let key = derive_key(sample.passphrase, &header.kdf).unwrap();
        let mut opener = Opener::new(key, header.clone()).unwrap();
        let mut reader = PartReader::new();
        let mut parts = Vec::new();
        for piece in file[start..].chunks(window) {
            parts.extend(reader.push(&opener.push(piece).unwrap()).unwrap());
        }
        parts.extend(reader.push(&opener.finish().unwrap()).unwrap());
        reader.finish().unwrap();
        assert_eq!(parts, sample.parts, "window {window}");
    }
}
