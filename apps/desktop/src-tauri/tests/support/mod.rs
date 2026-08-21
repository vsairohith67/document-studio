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

pub fn write_compressible_pdf_fixture(directory: &Path, name: &str) -> PathBuf {
    let stream = format!("% DS-G04A-COMPRESSIBLE\n{}\nq\nQ\n", "A".repeat(256 * 1024));
    write_pdf_objects(
        directory,
        name,
        &[
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> /Contents 4 0 R >>".to_owned(),
            format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ],
        1,
    )
}

pub fn write_structural_pdf_fixture(directory: &Path, name: &str) -> PathBuf {
    let content = "% DS-G04A-STRUCTURAL\nq\nQ\n";
    let attachment = "Document Studio attachment fixture\n";
    let metadata = "<?xpacket begin=''?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about='' xmlns:dc='http://purl.org/dc/elements/1.1/'><dc:title><rdf:Alt><rdf:li xml:lang='x-default'>G04A Structural Fixture</rdf:li></rdf:Alt></dc:title></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end='w'?>";
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R /PageLabels 7 0 R /Names << /EmbeddedFiles 8 0 R >> /AcroForm 12 0 R /Metadata 15 0 R >>".to_owned(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> /Contents 4 0 R /Annots [10 0 R 13 0 R 14 0 R] >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
        "<< /Type /Outlines /First 6 0 R /Last 6 0 R /Count 1 >>".to_owned(),
        "<< /Title (Section One) /Parent 5 0 R /Dest [3 0 R /Fit] >>".to_owned(),
        "<< /Nums [0 << /S /D /P (Page ) /St 1 >>] >>".to_owned(),
        "<< /Names [(note.txt) 9 0 R] >>".to_owned(),
        "<< /Type /Filespec /F (note.txt) /UF (note.txt) /Desc (G04A attachment) /EF << /F 11 0 R >> >>".to_owned(),
        "<< /Type /Annot /Subtype /Text /Rect [36 700 72 736] /Contents (Review note) /NM (g04a-note) /F 4 /Name /Comment >>".to_owned(),
        format!("<< /Type /EmbeddedFile /Length {} >>\nstream\n{attachment}endstream", attachment.len()),
        "<< /Fields [13 0 R 14 0 R] /NeedAppearances true >>".to_owned(),
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /TU (Name field) /V (Alice) /Rect [72 620 260 646] /P 3 0 R /F 4 >>".to_owned(),
        "<< /Type /Annot /Subtype /Widget /FT /Sig /T (Approval) /TU (Signature field) /Rect [72 560 260 596] /P 3 0 R /F 4 >>".to_owned(),
        format!("<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n{metadata}\nendstream", metadata.len() + 1),
        "<< /Title (G04A Structural Fixture) /Author (Document Studio) /Subject (Lossless preservation) >>".to_owned(),
    ];
    write_pdf_objects_with_trailer(directory, name, &objects, 1, "/Info 16 0 R")
}

fn write_pdf_objects(
    directory: &Path,
    name: &str,
    objects: &[String],
    root_object: usize,
) -> PathBuf {
    write_pdf_objects_with_trailer(directory, name, objects, root_object, "")
}

fn write_pdf_objects_with_trailer(
    directory: &Path,
    name: &str,
    objects: &[String],
    root_object: usize,
    trailer_entries: &str,
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
            "trailer\n<< /Size {} /Root {root_object} 0 R {trailer_entries} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    write_fixture(directory, name, &bytes)
}
