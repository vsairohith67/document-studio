#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

pub fn write_fixture(directory: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = directory.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

pub fn partial_files(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".partial"))
        })
        .collect()
}

pub fn write_pdf_fixture(directory: &Path, name: &str, marker: &str, width: u32) -> PathBuf {
    let stream = format!("% {marker}\nq\nQ\n");
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} 792] /Resources << >> /Contents 4 0 R >>"
        ),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
    ];
    write_pdf_objects(directory, name, &objects, 1)
}

pub fn write_multi_page_pdf_fixture(
    directory: &Path,
    name: &str,
    marker: &str,
    pages: usize,
) -> PathBuf {
    assert!(pages > 0);
    let kids = (0..pages)
        .map(|index| format!("{} 0 R", 3 + index * 2))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!("<< /Type /Pages /Count {pages} /Kids [{kids}] >>"),
    ];
    for index in 0..pages {
        let stream = format!("% {marker}-{index}\nq\nQ\n");
        let stream_object = 4 + index * 2;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} 792] /Resources << >> /Contents {stream_object} 0 R >>",
            600 + (index % 10)
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{stream}endstream",
            stream.len()
        ));
    }
    write_pdf_objects(directory, name, &objects, 1)
}

pub fn write_semantic_pdf_fixture(directory: &Path, name: &str, page_markers: &[&str]) -> PathBuf {
    assert!(!page_markers.is_empty());
    let kids = (0..page_markers.len())
        .map(|index| format!("{} 0 R", 3 + index * 2))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Count {} /Kids [{kids}] >>",
            page_markers.len()
        ),
    ];
    for (index, marker) in page_markers.iter().enumerate() {
        assert!(
            !marker.is_empty()
                && marker.len() <= 64
                && marker
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
        );
        let stream = format!("% DS-G02-MARKER:{marker}\nq\nQ\n");
        let stream_object = 4 + index * 2;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} 792] /Resources << >> /Contents {stream_object} 0 R >>",
            600 + (index % 10)
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{stream}endstream",
            stream.len()
        ));
    }
    write_pdf_objects(directory, name, &objects, 1)
}

pub fn write_zero_page_pdf(directory: &Path, name: &str) -> PathBuf {
    write_pdf_objects(
        directory,
        name,
        &[
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Count 0 /Kids [] >>".to_owned(),
        ],
        1,
    )
}

fn write_pdf_objects(
    directory: &Path,
    name: &str,
    objects: &[String],
    root_object: usize,
) -> PathBuf {
    let mut bytes = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    bytes.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root {root_object} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    write_fixture(directory, name, &bytes)
}
