use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use image::{GenericImageView, ImageFormat};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/g04c2-balanced-corpus")
}

fn manifest() -> Value {
    serde_json::from_slice(&fs::read(corpus_root().join("corpus-manifest.json")).unwrap()).unwrap()
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == needle[0])
            .any(|(offset, _)| haystack[offset..].starts_with(needle))
}

#[test]
fn committed_photographs_decode_as_exact_rgb8_jpegs() {
    let manifest = manifest();
    let entries = manifest["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 6);
    for entry in entries {
        let id = entry["id"].as_str().unwrap();
        let path = corpus_root().join(entry["assetPath"].as_str().unwrap());
        let bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.len() as u64, entry["bytes"].as_u64().unwrap(), "{id}");
        assert_eq!(sha256(&bytes), entry["sha256"].as_str().unwrap(), "{id}");
        assert!(bytes.starts_with(&[0xff, 0xd8]), "{id}");
        assert!(bytes.ends_with(&[0xff, 0xd9]), "{id}");
        let decoded = image::load_from_memory_with_format(&bytes, ImageFormat::Jpeg).unwrap();
        assert_eq!(decoded.color(), image::ColorType::Rgb8, "{id}");
        assert!(!decoded.color().has_alpha(), "{id}");
        assert_eq!(
            decoded.dimensions(),
            (
                entry["dimensions"]["width"].as_u64().unwrap() as u32,
                entry["dimensions"]["height"].as_u64().unwrap() as u32,
            ),
            "{id}"
        );
        assert_eq!(decoded.into_rgb8().into_raw().len() % 3, 0, "{id}");
    }
}

#[test]
fn generated_pdfs_open_without_recovery_and_contain_unchanged_dct_bytes() {
    let manifest = manifest();
    let entries = manifest["entries"].as_array().unwrap();
    let qpdf = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2/bin/qpdf.exe");
    assert!(qpdf.is_file());
    for fixture in manifest["generatedPdfs"].as_array().unwrap() {
        let path = corpus_root().join(fixture["path"].as_str().unwrap());
        let pdf = fs::read(&path).unwrap();
        let check = Command::new(&qpdf)
            .arg(&path)
            .args(["--suppress-recovery", "--check"])
            .output()
            .unwrap();
        assert!(
            check.status.success(),
            "{}",
            String::from_utf8_lossy(&check.stderr)
        );
        let pages = Command::new(&qpdf)
            .arg(&path)
            .arg("--show-npages")
            .output()
            .unwrap();
        assert!(pages.status.success());
        assert_eq!(
            String::from_utf8(pages.stdout).unwrap().trim(),
            fixture["pageCount"].as_u64().unwrap().to_string()
        );
        for source_id in fixture["sourceIds"].as_array().unwrap() {
            let source_id = source_id.as_str().unwrap();
            let entry = entries
                .iter()
                .find(|entry| entry["id"].as_str() == Some(source_id))
                .unwrap();
            let jpeg = fs::read(corpus_root().join(entry["assetPath"].as_str().unwrap())).unwrap();
            assert!(
                contains_bytes(&pdf, &jpeg),
                "{} omits unchanged {source_id} DCT bytes",
                path.display()
            );
        }
    }
}
