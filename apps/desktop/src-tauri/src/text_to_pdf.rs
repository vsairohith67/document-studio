use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::contracts::{OperationError, OperationStage};

pub const TEXT_TO_PDF_OPERATION_ID: &str = "text.to-pdf";
pub const TEXT_TO_PDF_VERSION: &str = "1.0.0";
pub const TXT_MAX_RAW_BYTES: usize = 8_388_608;
pub const TXT_MAX_LOGICAL_LINES: usize = 100_000;
pub const TXT_MAX_LINE_BYTES: usize = 65_536;
pub const TXT_MAX_HTML_BYTES: usize = 42_991_616;
pub const TXT_MAX_CSS_BYTES: usize = 65_536;
pub const TXT_APPROVED_FONT_BYTES: usize = 1_101_032;
pub const TXT_MAX_SERVED_BYTES: usize = 44_158_184;
pub const TXT_MAX_PDF_BYTES: u64 = 536_870_912;
pub const TXT_MAX_PAGES: u64 = 4_096;
pub const TXT_MAX_SHAPING_RUN: usize = 32;

pub const RENDER_ORIGIN: &str = "https://txt-renderer.document-studio.invalid/1/";
pub const DOCUMENT_URL: &str = "https://txt-renderer.document-studio.invalid/1/document.html";
pub const CSS_URL: &str = "https://txt-renderer.document-studio.invalid/1/document.css";
pub const NOTO_SANS_URL: &str =
    "https://txt-renderer.document-studio.invalid/1/fonts/noto-sans-regular.ttf";
pub const NOTO_DEVANAGARI_URL: &str =
    "https://txt-renderer.document-studio.invalid/1/fonts/noto-sans-devanagari-regular.ttf";
pub const NOTO_TELUGU_URL: &str =
    "https://txt-renderer.document-studio.invalid/1/fonts/noto-sans-telugu-regular.ttf";

pub const NOTO_SANS_BYTES: &[u8] = include_bytes!("../resources/fonts/g04e1/NotoSans-Regular.ttf");
pub const NOTO_DEVANAGARI_BYTES: &[u8] =
    include_bytes!("../resources/fonts/g04e1/NotoSansDevanagari-Regular.ttf");
pub const NOTO_TELUGU_BYTES: &[u8] =
    include_bytes!("../resources/fonts/g04e1/NotoSansTelugu-Regular.ttf");
pub const FONT_MANIFEST_JSON: &str = include_str!("../resources/fonts/g04e1/font-manifest.json");

pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'none'; connect-src 'none'; img-src 'none'; media-src 'none'; object-src 'none'; frame-src 'none'; child-src 'none'; form-action 'none'; base-uri 'none'; style-src 'self'; font-src 'self'; worker-src 'none'; manifest-src 'none'; frame-ancestors 'none';";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextPageSize {
    A4,
    Letter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextOrientation {
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextToPdfSettings {
    pub page_size: TextPageSize,
    pub orientation: TextOrientation,
}

impl TextToPdfSettings {
    pub fn paper_inches(self) -> (f64, f64) {
        let portrait = match self.page_size {
            TextPageSize::A4 => (8.267_716_535_4, 11.692_913_385_8),
            TextPageSize::Letter => (8.5, 11.0),
        };
        match self.orientation {
            TextOrientation::Portrait => portrait,
            TextOrientation::Landscape => (portrait.1, portrait.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedText {
    pub text: String,
    pub logical_lines: usize,
    pub used_scripts: BTreeSet<AdmittedScript>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdmittedScript {
    LatinCommon,
    Devanagari,
    Telugu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontEvidence {
    pub family: String,
    pub style: String,
    pub full_name: String,
    pub version: String,
    pub postscript_name: String,
    pub copyright: String,
    pub weight: u16,
    pub table_tags: Vec<String>,
    pub cmap_formats: Vec<u16>,
    pub cmap_code_points: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ApprovedFont<'a> {
    pub bytes: &'a [u8],
    pub expected_bytes: usize,
    pub expected_sha256: &'static str,
    pub expected_family: &'static str,
    pub expected_full_name: &'static str,
    pub expected_postscript_name: &'static str,
    pub expected_version: &'static str,
}

pub const APPROVED_FONTS: [ApprovedFont<'static>; 3] = [
    ApprovedFont {
        bytes: NOTO_SANS_BYTES,
        expected_bytes: 621_572,
        expected_sha256: "478c558ea716033cd60c03438f628dfa75694dcf6b5f6d505a2f05fd2b4f3823",
        expected_family: "Noto Sans",
        expected_full_name: "Noto Sans Regular",
        expected_postscript_name: "NotoSans-Regular",
        expected_version: "Version 2.015; ttfautohint (v1.8.4.7-5d5b)",
    },
    ApprovedFont {
        bytes: NOTO_DEVANAGARI_BYTES,
        expected_bytes: 244_284,
        expected_sha256: "306b53ecfb182a504dd8a7446093c316387d2fd8dc350d0792ed1753fe0996cd",
        expected_family: "Noto Sans Devanagari",
        expected_full_name: "Noto Sans Devanagari Regular",
        expected_postscript_name: "NotoSansDevanagari-Regular",
        expected_version: "Version 2.006; ttfautohint (v1.8.4.7-5d5b)",
    },
    ApprovedFont {
        bytes: NOTO_TELUGU_BYTES,
        expected_bytes: 235_176,
        expected_sha256: "b274780b69d1d23fe84b55e809a152cb2ac5306d33864b1f87622f6971871aae",
        expected_family: "Noto Sans Telugu",
        expected_full_name: "Noto Sans Telugu Regular",
        expected_postscript_name: "NotoSansTelugu-Regular",
        expected_version: "Version 2.005; ttfautohint (v1.8.4.7-5d5b)",
    },
];

pub fn preflight_text(raw: &[u8]) -> Result<NormalizedText, OperationError> {
    if raw.len() > TXT_MAX_RAW_BYTES {
        return Err(text_error(
            "TXT_SIZE_LIMIT",
            "The TXT file is too large",
            "TXT input is limited to 8,388,608 bytes.",
        ));
    }
    validate_approved_fonts()?;
    let parsed_fonts = [
        Sfnt::parse(NOTO_SANS_BYTES).map_err(|_| font_error())?,
        Sfnt::parse(NOTO_DEVANAGARI_BYTES).map_err(|_| font_error())?,
        Sfnt::parse(NOTO_TELUGU_BYTES).map_err(|_| font_error())?,
    ];
    if starts_with_unsupported_bom(raw) {
        return Err(text_error(
            "TXT_UNSUPPORTED_BOM",
            "The text encoding is not supported",
            "Use strict UTF-8. UTF-16 and UTF-32 byte-order marks are rejected.",
        ));
    }
    let raw = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(raw);
    let decoded = std::str::from_utf8(raw).map_err(|_| {
        text_error(
            "TXT_INVALID_UTF8",
            "The TXT file is not valid UTF-8",
            "Save the document as strict UTF-8 and try again.",
        )
    })?;
    let normalized = decoded.replace("\r\n", "\n").replace('\r', "\n");
    let logical_lines = normalized.split('\n').count();
    if logical_lines > TXT_MAX_LOGICAL_LINES {
        return Err(text_error(
            "TXT_LINE_COUNT_LIMIT",
            "The TXT file has too many lines",
            "TXT conversion supports at most 100,000 logical lines.",
        ));
    }
    if normalized
        .split('\n')
        .any(|line| line.len() > TXT_MAX_LINE_BYTES)
    {
        return Err(text_error(
            "TXT_LINE_BYTES_LIMIT",
            "A TXT line is too long",
            "Each normalized logical line is limited to 65,536 UTF-8 bytes.",
        ));
    }

    let chars = normalized.chars().collect::<Vec<_>>();
    let mut shaping_run = 0_usize;
    let mut used_scripts = BTreeSet::new();
    let mut cmap_cache = BTreeMap::<(usize, u32), bool>::new();
    for (index, character) in chars.iter().copied().enumerate() {
        let code = u32::from(character);
        if character == '\n' || character == '\t' {
            shaping_run = 0;
            continue;
        }
        if character == '\u{feff}' {
            return Err(text_error(
                "TXT_UNSUPPORTED_BOM",
                "An embedded byte-order mark was rejected",
                "A UTF-8 byte-order mark is allowed only once at byte zero.",
            ));
        }
        if is_rejected_control(code) {
            return Err(text_error(
                "TXT_CONTROL_CHARACTER",
                "The TXT file contains a control character",
                "Only TAB and normalized line-feed controls are accepted.",
            ));
        }
        if is_bidi_control(code) {
            return Err(text_error(
                "TXT_BIDI_CONTROL",
                "The TXT file contains a bidirectional formatting control",
                "Bidirectional embedding, override, isolate, and mark controls are rejected.",
            ));
        }
        if is_noncharacter(code) {
            return Err(text_error(
                "TXT_NONCHARACTER",
                "The TXT file contains a Unicode noncharacter",
                "Remove the noncharacter and try again.",
            ));
        }
        let script = if matches!(code, 0x200c | 0x200d) {
            validate_joiner(&chars, index)?
        } else {
            admitted_script(code).ok_or_else(|| unsupported_unicode(code))?
        };
        let font_index = font_index(script);
        let is_covered = if let Some(covered) = cmap_cache.get(&(font_index, code)) {
            *covered
        } else {
            let covered = parsed_fonts[font_index]
                .cmap_contains(code)
                .map_err(|_| font_error())?;
            cmap_cache.insert((font_index, code), covered);
            covered
        };
        if !is_covered {
            return Err(unsupported_unicode(code));
        }
        if !character.is_whitespace() {
            used_scripts.insert(script);
        }
        if is_combining_or_default_ignorable(code) {
            shaping_run = shaping_run.checked_add(1).ok_or_else(shaping_error)?;
            if shaping_run > TXT_MAX_SHAPING_RUN {
                return Err(shaping_error());
            }
        } else {
            shaping_run = 0;
        }
    }
    Ok(NormalizedText {
        text: normalized,
        logical_lines,
        used_scripts,
    })
}

pub fn canonical_html(text: &str) -> Result<Vec<u8>, OperationError> {
    let escaped_capacity = text.len().checked_mul(5).ok_or_else(response_size_error)?;
    if escaped_capacity > TXT_MAX_HTML_BYTES {
        return Err(response_size_error());
    }
    let mut escaped = String::new();
    escaped
        .try_reserve(escaped_capacity.min(TXT_MAX_HTML_BYTES))
        .map_err(|_| response_size_error())?;
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(character),
        }
    }
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"color-scheme\" content=\"only light\"><meta http-equiv=\"Content-Security-Policy\" content=\"{CONTENT_SECURITY_POLICY}\"><link rel=\"stylesheet\" href=\"document.css\"></head><body><pre>{escaped}</pre></body></html>"
    );
    if html.len() > TXT_MAX_HTML_BYTES || html.len() > u32::MAX as usize {
        return Err(response_size_error());
    }
    Ok(html.into_bytes())
}

pub fn canonical_css() -> Result<Vec<u8>, OperationError> {
    let css = concat!(
        "@font-face{font-family:'DocumentStudioText';src:url('fonts/noto-sans-devanagari-regular.ttf') format('truetype');font-style:normal;font-weight:400;font-display:block;unicode-range:U+0900-097F,U+A8E0-A8FF,U+200C-200D;}",
        "@font-face{font-family:'DocumentStudioText';src:url('fonts/noto-sans-telugu-regular.ttf') format('truetype');font-style:normal;font-weight:400;font-display:block;unicode-range:U+0C00-0C7F,U+200C-200D;}",
        "@font-face{font-family:'DocumentStudioText';src:url('fonts/noto-sans-regular.ttf') format('truetype');font-style:normal;font-weight:400;font-display:block;unicode-range:U+0020-007E,U+00A0-024F,U+0300-036F,U+2000-206F,U+20A0-21FF;}",
        "@page{margin:0.5in;}html,body{margin:0;padding:0;background:#fff;color:#000;}body{font-family:'DocumentStudioText';font-weight:400;font-style:normal;font-size:11pt;line-height:1.45;font-synthesis:none;}pre{margin:0;white-space:pre-wrap;overflow-wrap:anywhere;tab-size:4;font:inherit;}"
    );
    if css.len() > TXT_MAX_CSS_BYTES || css.len() > u32::MAX as usize {
        return Err(response_size_error());
    }
    Ok(css.as_bytes().to_vec())
}

pub fn validate_approved_fonts() -> Result<Vec<FontEvidence>, OperationError> {
    if APPROVED_FONTS
        .iter()
        .try_fold(0_usize, |total, font| total.checked_add(font.bytes.len()))
        != Some(TXT_APPROVED_FONT_BYTES)
    {
        return Err(font_error());
    }
    APPROVED_FONTS
        .iter()
        .map(|approved| validate_font(*approved))
        .collect()
}

fn validate_font(approved: ApprovedFont<'_>) -> Result<FontEvidence, OperationError> {
    if approved.bytes.len() != approved.expected_bytes
        || sha256_hex(approved.bytes) != approved.expected_sha256
    {
        return Err(font_error());
    }
    let font = Sfnt::parse(approved.bytes).map_err(|_| font_error())?;
    let table_tags = font.table_tags();
    for required in [
        "OS/2", "cmap", "glyf", "head", "hhea", "hmtx", "loca", "maxp", "name", "post",
    ] {
        if !table_tags.iter().any(|tag| tag == required) {
            return Err(font_error());
        }
    }
    if table_tags.iter().any(|tag| tag == "fvar") {
        return Err(font_error());
    }
    let family = font.name(1).map_err(|_| font_error())?;
    let style = font.name(2).map_err(|_| font_error())?;
    let full_name = font.name(4).map_err(|_| font_error())?;
    let version = font.name(5).map_err(|_| font_error())?;
    let postscript_name = font.name(6).map_err(|_| font_error())?;
    let copyright = font.name(0).map_err(|_| font_error())?;
    let weight = font.weight().map_err(|_| font_error())?;
    if family != approved.expected_family
        || style != "Regular"
        || full_name != approved.expected_full_name
        || version != approved.expected_version
        || postscript_name != approved.expected_postscript_name
        || copyright.is_empty()
        || weight != 400
        || font.is_bold_or_italic().map_err(|_| font_error())?
    {
        return Err(font_error());
    }
    let (cmap_formats, cmap_code_points) = font.cmap_evidence().map_err(|_| font_error())?;
    Ok(FontEvidence {
        family,
        style,
        full_name,
        version,
        postscript_name,
        copyright,
        weight,
        table_tags,
        cmap_formats,
        cmap_code_points,
    })
}

pub fn total_response_bytes(html: usize, css: usize) -> Result<usize, OperationError> {
    let total = html
        .checked_add(css)
        .and_then(|value| value.checked_add(TXT_APPROVED_FONT_BYTES))
        .ok_or_else(response_size_error)?;
    if total > TXT_MAX_SERVED_BYTES || total > u32::MAX as usize {
        return Err(response_size_error());
    }
    Ok(total)
}

fn starts_with_unsupported_bom(raw: &[u8]) -> bool {
    raw.starts_with(&[0xff, 0xfe, 0x00, 0x00])
        || raw.starts_with(&[0x00, 0x00, 0xfe, 0xff])
        || raw.starts_with(&[0xff, 0xfe])
        || raw.starts_with(&[0xfe, 0xff])
}

fn is_rejected_control(code: u32) -> bool {
    matches!(code, 0x0000..=0x0008 | 0x000b..=0x001f | 0x007f..=0x009f)
}

fn is_bidi_control(code: u32) -> bool {
    matches!(code, 0x061c | 0x200e | 0x200f | 0x202a..=0x202e | 0x2066..=0x2069)
}

fn is_noncharacter(code: u32) -> bool {
    matches!(code, 0xfdd0..=0xfdef) || code & 0xffff == 0xfffe || code & 0xffff == 0xffff
}

fn admitted_script(code: u32) -> Option<AdmittedScript> {
    if matches!(code, 0x2028 | 0x2029) {
        // Unicode mandatory line/paragraph separators are not normalized TXT
        // logical lines. Reject them before WebView2 so they cannot bypass the
        // LF-owned line-count and per-line byte limits during layout.
        None
    } else if matches!(code, 0x0900..=0x097f | 0xa8e0..=0xa8ff) {
        Some(AdmittedScript::Devanagari)
    } else if matches!(code, 0x0c00..=0x0c7f) {
        Some(AdmittedScript::Telugu)
    } else if matches!(
        code,
        0x0020..=0x007e
            | 0x00a0..=0x024f
            | 0x0300..=0x036f
            | 0x2000..=0x206f
            | 0x20a0..=0x21ff
    ) {
        Some(AdmittedScript::LatinCommon)
    } else {
        None
    }
}

fn font_index(script: AdmittedScript) -> usize {
    match script {
        AdmittedScript::LatinCommon => 0,
        AdmittedScript::Devanagari => 1,
        AdmittedScript::Telugu => 2,
    }
}

fn is_combining_or_default_ignorable(code: u32) -> bool {
    matches!(
        code,
        0x0300..=0x036f
            | 0x0900..=0x0903
            | 0x093a..=0x094f
            | 0x0951..=0x0957
            | 0x0962..=0x0963
            | 0x0c00..=0x0c04
            | 0x0c3c..=0x0c56
            | 0x0c62..=0x0c63
            | 0x200c..=0x200d
    )
}

fn validate_joiner(chars: &[char], index: usize) -> Result<AdmittedScript, OperationError> {
    if index < 2 || index + 1 >= chars.len() {
        return Err(shaping_error());
    }
    let virama = u32::from(chars[index - 1]);
    let script = match virama {
        0x094d => AdmittedScript::Devanagari,
        0x0c4d => AdmittedScript::Telugu,
        _ => return Err(shaping_error()),
    };
    if !has_shaping_base_before_virama(chars, index, script)
        || !is_shaping_base(u32::from(chars[index + 1]), script)
    {
        return Err(shaping_error());
    }
    let mut start = index;
    while start > 0 && index - start < 16 && joiner_cluster_member(chars[start - 1], script) {
        start -= 1;
    }
    let mut end = index + 1;
    while end < chars.len() && end - index <= 16 && joiner_cluster_member(chars[end], script) {
        end += 1;
    }
    if index - start >= 16 || end - index > 16 {
        return Err(shaping_error());
    }
    if chars[start..end]
        .iter()
        .filter(|character| matches!(u32::from(**character), 0x200c | 0x200d))
        .count()
        != 1
    {
        return Err(shaping_error());
    }
    Ok(script)
}

fn joiner_cluster_member(character: char, script: AdmittedScript) -> bool {
    let code = u32::from(character);
    is_shaping_base(code, script)
        || matches!(code, 0x200c | 0x200d)
        || is_combining_or_default_ignorable(code)
}

fn has_shaping_base_before_virama(
    chars: &[char],
    joiner_index: usize,
    script: AdmittedScript,
) -> bool {
    let before_virama = u32::from(chars[joiner_index - 2]);
    if is_shaping_base(before_virama, script) {
        return true;
    }
    let nukta = match script {
        AdmittedScript::Devanagari => 0x093c,
        AdmittedScript::Telugu => 0x0c3c,
        AdmittedScript::LatinCommon => return false,
    };
    before_virama == nukta
        && joiner_index >= 3
        && is_shaping_base(u32::from(chars[joiner_index - 3]), script)
}

fn is_shaping_base(code: u32, script: AdmittedScript) -> bool {
    match script {
        AdmittedScript::Devanagari => {
            matches!(code, 0x0915..=0x0939 | 0x0958..=0x095f | 0x0978..=0x097f)
        }
        AdmittedScript::Telugu => matches!(code, 0x0c15..=0x0c39 | 0x0c58..=0x0c5a),
        AdmittedScript::LatinCommon => false,
    }
}

#[derive(Clone, Copy)]
struct Table<'a> {
    tag: [u8; 4],
    bytes: &'a [u8],
}

struct Sfnt<'a> {
    tables: Vec<Table<'a>>,
}

impl<'a> Sfnt<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, ()> {
        if read_u32(bytes, 0)? != 0x0001_0000 {
            return Err(());
        }
        let count = usize::from(read_u16(bytes, 4)?);
        let directory_end = 12_usize
            .checked_add(count.checked_mul(16).ok_or(())?)
            .ok_or(())?;
        if directory_end > bytes.len() {
            return Err(());
        }
        let mut tables = Vec::new();
        tables.try_reserve_exact(count).map_err(|_| ())?;
        for index in 0..count {
            let record = 12 + index * 16;
            let tag = bytes[record..record + 4].try_into().map_err(|_| ())?;
            let offset = usize::try_from(read_u32(bytes, record + 8)?).map_err(|_| ())?;
            let length = usize::try_from(read_u32(bytes, record + 12)?).map_err(|_| ())?;
            let end = offset.checked_add(length).ok_or(())?;
            if end > bytes.len() {
                return Err(());
            }
            tables.push(Table {
                tag,
                bytes: &bytes[offset..end],
            });
        }
        Ok(Self { tables })
    }

    fn table(&self, tag: &[u8; 4]) -> Result<&'a [u8], ()> {
        self.tables
            .iter()
            .find(|table| &table.tag == tag)
            .map(|table| table.bytes)
            .ok_or(())
    }

    fn table_tags(&self) -> Vec<String> {
        self.tables
            .iter()
            .map(|table| String::from_utf8_lossy(&table.tag).into_owned())
            .collect()
    }

    fn name(&self, name_id: u16) -> Result<String, ()> {
        let table = self.table(b"name")?;
        let count = usize::from(read_u16(table, 2)?);
        let storage = usize::from(read_u16(table, 4)?);
        let records_end = 6_usize
            .checked_add(count.checked_mul(12).ok_or(())?)
            .ok_or(())?;
        if records_end > table.len() || storage > table.len() {
            return Err(());
        }
        let mut candidate = None;
        for index in 0..count {
            let offset = 6 + index * 12;
            let platform = read_u16(table, offset)?;
            let encoding = read_u16(table, offset + 2)?;
            let language = read_u16(table, offset + 4)?;
            if read_u16(table, offset + 6)? != name_id {
                continue;
            }
            let length = usize::from(read_u16(table, offset + 8)?);
            let relative = usize::from(read_u16(table, offset + 10)?);
            let start = storage.checked_add(relative).ok_or(())?;
            let end = start.checked_add(length).ok_or(())?;
            if end > table.len() {
                return Err(());
            }
            let value = if platform == 3 && matches!(encoding, 1 | 10) && length % 2 == 0 {
                let utf16 = table[start..end]
                    .chunks_exact(2)
                    .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                    .collect::<Vec<_>>();
                String::from_utf16(&utf16).map_err(|_| ())?
            } else if platform == 1 {
                String::from_utf8(table[start..end].to_vec()).map_err(|_| ())?
            } else {
                continue;
            };
            if platform == 3 && language == 0x0409 {
                return Ok(value);
            }
            candidate.get_or_insert(value);
        }
        candidate.ok_or(())
    }

    fn weight(&self) -> Result<u16, ()> {
        read_u16(self.table(b"OS/2")?, 4)
    }

    fn is_bold_or_italic(&self) -> Result<bool, ()> {
        let head_style = read_u16(self.table(b"head")?, 44)?;
        let os2 = self.table(b"OS/2")?;
        let selection = if os2.len() >= 64 {
            read_u16(os2, 62)?
        } else {
            0
        };
        Ok(head_style & 0x0003 != 0 || selection & 0x0021 != 0)
    }

    fn cmap_records(&self) -> Result<Vec<(u16, &'a [u8])>, ()> {
        let cmap = self.table(b"cmap")?;
        if read_u16(cmap, 0)? != 0 {
            return Err(());
        }
        let count = usize::from(read_u16(cmap, 2)?);
        let records_end = 4_usize
            .checked_add(count.checked_mul(8).ok_or(())?)
            .ok_or(())?;
        if records_end > cmap.len() {
            return Err(());
        }
        let mut records = Vec::new();
        for index in 0..count {
            let record = 4 + index * 8;
            let platform = read_u16(cmap, record)?;
            let encoding = read_u16(cmap, record + 2)?;
            if platform != 3 || !matches!(encoding, 1 | 10) {
                continue;
            }
            let offset = usize::try_from(read_u32(cmap, record + 4)?).map_err(|_| ())?;
            let format = read_u16(cmap, offset)?;
            let length = match format {
                4 => usize::from(read_u16(cmap, offset + 2)?),
                12 => usize::try_from(read_u32(cmap, offset + 4)?).map_err(|_| ())?,
                _ => continue,
            };
            let end = offset.checked_add(length).ok_or(())?;
            if end > cmap.len() {
                return Err(());
            }
            records.push((format, &cmap[offset..end]));
        }
        if records.is_empty() {
            return Err(());
        }
        Ok(records)
    }

    fn cmap_contains(&self, code: u32) -> Result<bool, ()> {
        for (format, table) in self.cmap_records()? {
            let glyph = match format {
                4 if code <= u32::from(u16::MAX) => cmap4_glyph(table, code as u16)?,
                12 => cmap12_glyph(table, code)?,
                _ => 0,
            };
            if glyph != 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn cmap_evidence(&self) -> Result<(Vec<u16>, usize), ()> {
        let records = self.cmap_records()?;
        let mut formats = records.iter().map(|record| record.0).collect::<Vec<_>>();
        formats.sort_unstable();
        formats.dedup();
        let count = if let Some((_, table)) = records.iter().find(|record| record.0 == 12) {
            cmap12_code_point_count(table)?
        } else if let Some((_, table)) = records.iter().find(|record| record.0 == 4) {
            (0..=u32::from(u16::MAX)).try_fold(0_usize, |total, code| {
                if cmap4_glyph(table, code as u16)? == 0 {
                    Ok(total)
                } else {
                    total.checked_add(1).ok_or(())
                }
            })?
        } else {
            return Err(());
        };
        Ok((formats, count))
    }
}

fn cmap12_code_point_count(table: &[u8]) -> Result<usize, ()> {
    let groups = usize::try_from(read_u32(table, 12)?).map_err(|_| ())?;
    if 16_usize
        .checked_add(groups.checked_mul(12).ok_or(())?)
        .ok_or(())?
        > table.len()
    {
        return Err(());
    }
    (0..groups).try_fold(0_usize, |total, index| {
        let offset = 16 + index * 12;
        let start = read_u32(table, offset)?;
        let end = read_u32(table, offset + 4)?;
        if start > end || end > 0x10ffff {
            return Err(());
        }
        let length = usize::try_from(end - start + 1).map_err(|_| ())?;
        total.checked_add(length).ok_or(())
    })
}

fn cmap4_glyph(table: &[u8], code: u16) -> Result<u16, ()> {
    let segment_count = usize::from(read_u16(table, 6)? / 2);
    let end_codes = 14;
    let start_codes = end_codes + segment_count * 2 + 2;
    let deltas = start_codes + segment_count * 2;
    let offsets = deltas + segment_count * 2;
    if offsets.checked_add(segment_count * 2).ok_or(())? > table.len() {
        return Err(());
    }
    for index in 0..segment_count {
        let end = read_u16(table, end_codes + index * 2)?;
        if code > end {
            continue;
        }
        let start = read_u16(table, start_codes + index * 2)?;
        if code < start {
            return Ok(0);
        }
        let delta = read_u16(table, deltas + index * 2)?;
        let range_offset = read_u16(table, offsets + index * 2)?;
        if range_offset == 0 {
            return Ok(code.wrapping_add(delta));
        }
        let address = offsets
            .checked_add(index * 2)
            .and_then(|value| value.checked_add(usize::from(range_offset)))
            .and_then(|value| value.checked_add(usize::from(code - start) * 2))
            .ok_or(())?;
        let glyph = read_u16(table, address)?;
        return Ok(if glyph == 0 {
            0
        } else {
            glyph.wrapping_add(delta)
        });
    }
    Ok(0)
}

fn cmap12_glyph(table: &[u8], code: u32) -> Result<u16, ()> {
    let groups = usize::try_from(read_u32(table, 12)?).map_err(|_| ())?;
    if 16_usize
        .checked_add(groups.checked_mul(12).ok_or(())?)
        .ok_or(())?
        > table.len()
    {
        return Err(());
    }
    for index in 0..groups {
        let offset = 16 + index * 12;
        let start = read_u32(table, offset)?;
        let end = read_u32(table, offset + 4)?;
        if code < start {
            return Ok(0);
        }
        if code <= end {
            let glyph = read_u32(table, offset + 8)?
                .checked_add(code - start)
                .ok_or(())?;
            return u16::try_from(glyph).map_err(|_| ());
        }
    }
    Ok(0)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ()> {
    let end = offset.checked_add(2).ok_or(())?;
    let value: [u8; 2] = bytes
        .get(offset..end)
        .ok_or(())?
        .try_into()
        .map_err(|_| ())?;
    Ok(u16::from_be_bytes(value))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ()> {
    let end = offset.checked_add(4).ok_or(())?;
    let value: [u8; 4] = bytes
        .get(offset..end)
        .ok_or(())?
        .try_into()
        .map_err(|_| ())?;
    Ok(u32::from_be_bytes(value))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn unsupported_unicode(_code: u32) -> OperationError {
    text_error(
        "TXT_UNSUPPORTED_UNICODE",
        "The TXT file contains unsupported Unicode",
        "TXT-to-PDF V1 supports admitted English, Devanagari, Telugu, and bounded common punctuation only.",
    )
}

fn shaping_error() -> OperationError {
    text_error(
        "TXT_SHAPING_COMPLEXITY_LIMIT",
        "The TXT shaping sequence is too complex",
        "Use bounded Devanagari or Telugu shaping clusters with no more than 32 consecutive marks.",
    )
}

fn response_size_error() -> OperationError {
    text_error(
        "TXT_RESPONSE_SIZE_LIMIT",
        "The private renderer response is too large",
        "The bounded in-memory TXT rendering response limit was exceeded.",
    )
}

fn font_error() -> OperationError {
    text_error(
        "TXT_FONT_INTEGRITY_FAILED",
        "The approved TXT fonts failed verification",
        "The packaged static Regular font set is missing, altered, or incompatible.",
    )
}

fn text_error(code: &str, title: &str, detail: &str) -> OperationError {
    OperationError::safe(code, title, detail, OperationStage::Preflight, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact_sized_text(size: usize) -> Vec<u8> {
        let mut value = Vec::with_capacity(size);
        while size - value.len() > 1_025 {
            value.extend(std::iter::repeat_n(b'a', 1_024));
            value.push(b'\n');
        }
        value.extend(std::iter::repeat_n(b'a', size - value.len()));
        value
    }

    #[test]
    fn packaged_fonts_are_exact_static_regular_faces() {
        let evidence = validate_approved_fonts().expect("approved fonts");
        assert_eq!(evidence.len(), 3);
        assert_eq!(evidence[0].postscript_name, "NotoSans-Regular");
        assert_eq!(evidence[1].postscript_name, "NotoSansDevanagari-Regular");
        assert_eq!(evidence[2].postscript_name, "NotoSansTelugu-Regular");
        assert!(evidence.iter().all(|font| font.weight == 400));
    }

    #[test]
    fn packaged_font_manifest_has_only_three_ttf_faces_and_exact_ofl_materials() {
        let manifest: serde_json::Value = serde_json::from_str(FONT_MANIFEST_JSON).unwrap();
        let fonts = manifest["fonts"].as_array().unwrap();
        assert_eq!(fonts.len(), 3);
        assert!(fonts.iter().all(|font| {
            font["weight"] == 400
                && font["style"] == "Regular"
                && font["spdxLicense"] == "OFL-1.1"
                && font["copyright"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
                && font["source"]["commit"]
                    .as_str()
                    .is_some_and(|value| value.len() == 40)
                && font["source"]["archiveSha256"]
                    .as_str()
                    .is_some_and(|value| value.len() == 64)
                && font["openTypeTables"]
                    .as_array()
                    .is_some_and(|tables| tables.iter().all(|table| table != "fvar"))
        }));

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("fonts")
            .join("g04e1");
        let mut files = BTreeSet::new();
        for entry in std::fs::read_dir(&root).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                for license in std::fs::read_dir(entry.path()).unwrap() {
                    let license = license.unwrap();
                    files.insert(format!(
                        "licenses/{}",
                        license.file_name().to_string_lossy()
                    ));
                    let notice = std::fs::read_to_string(license.path()).unwrap();
                    assert!(notice.contains("SIL OPEN FONT LICENSE Version 1.1"));
                }
            } else {
                files.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
        assert_eq!(
            files,
            BTreeSet::from([
                "NotoSans-Regular.ttf".to_owned(),
                "NotoSansDevanagari-Regular.ttf".to_owned(),
                "NotoSansTelugu-Regular.ttf".to_owned(),
                "font-manifest.json".to_owned(),
                "licenses/NotoSans-OFL-1.1.txt".to_owned(),
                "licenses/NotoSansDevanagari-OFL-1.1.txt".to_owned(),
                "licenses/NotoSansTelugu-OFL-1.1.txt".to_owned(),
            ])
        );
    }

    #[test]
    fn normalization_and_line_boundaries_are_exact() {
        let empty = preflight_text(b"").expect("empty input");
        assert_eq!(empty.logical_lines, 1);
        let terminal = preflight_text(b"a\r\nb\rc\n").expect("line endings");
        assert_eq!(terminal.text, "a\nb\nc\n");
        assert_eq!(terminal.logical_lines, 4);
        assert_eq!(
            preflight_text(&vec![b'a'; TXT_MAX_RAW_BYTES])
                .unwrap_err()
                .code,
            "TXT_LINE_BYTES_LIMIT"
        );
        assert_eq!(
            preflight_text(&vec![b'a'; TXT_MAX_RAW_BYTES + 1])
                .unwrap_err()
                .code,
            "TXT_SIZE_LIMIT"
        );
    }

    #[test]
    fn inclusive_raw_line_count_and_line_byte_limits_are_exact() {
        let exact = exact_sized_text(TXT_MAX_RAW_BYTES);
        assert_eq!(exact.len(), TXT_MAX_RAW_BYTES);
        assert!(preflight_text(&exact).is_ok());
        assert_eq!(
            preflight_text(&exact_sized_text(TXT_MAX_RAW_BYTES + 1))
                .unwrap_err()
                .code,
            "TXT_SIZE_LIMIT"
        );

        let hundred_thousand = "x\n".repeat(99_999);
        assert_eq!(
            preflight_text(hundred_thousand.as_bytes())
                .unwrap()
                .logical_lines,
            100_000
        );
        assert_eq!(
            preflight_text("x\n".repeat(100_000).as_bytes())
                .unwrap_err()
                .code,
            "TXT_LINE_COUNT_LIMIT"
        );
        assert!(preflight_text(&vec![b'a'; 65_536]).is_ok());
        assert_eq!(
            preflight_text(&vec![b'a'; 65_537]).unwrap_err().code,
            "TXT_LINE_BYTES_LIMIT"
        );
    }

    #[test]
    fn unicode_mandatory_breaks_cannot_bypass_logical_line_limits() {
        for separator in ['\u{2028}', '\u{2029}'] {
            let error = preflight_text(separator.to_string().as_bytes()).unwrap_err();
            assert_eq!(error.code, "TXT_UNSUPPORTED_UNICODE");
        }
    }

    #[test]
    fn utf8_bom_and_every_line_ending_policy_are_exact() {
        assert_eq!(preflight_text(b"\xef\xbb\xbftext").unwrap().text, "text");
        for unsupported in [
            &[0xff, 0xfe][..],
            &[0xfe, 0xff][..],
            &[0xff, 0xfe, 0x00, 0x00][..],
            &[0x00, 0x00, 0xfe, 0xff][..],
        ] {
            assert_eq!(
                preflight_text(unsupported).unwrap_err().code,
                "TXT_UNSUPPORTED_BOM"
            );
        }
        for malformed in [
            &[0x80][..],
            &[0xc0, 0xaf][..],
            &[0xe2, 0x82][..],
            &[0xf4, 0x90, 0x80, 0x80][..],
        ] {
            assert_eq!(
                preflight_text(malformed).unwrap_err().code,
                "TXT_INVALID_UTF8"
            );
        }
        assert_eq!(preflight_text(b"a\r\nb").unwrap().text, "a\nb");
        assert_eq!(preflight_text(b"a\rb").unwrap().text, "a\nb");
        assert_eq!(preflight_text(b"a\nb").unwrap().text, "a\nb");
        assert_eq!(preflight_text(b"a\n").unwrap().logical_lines, 2);
    }

    #[test]
    fn rejected_control_bidi_and_noncharacter_classes_are_complete() {
        for code in (0x0000..=0x001f)
            .filter(|code| !matches!(code, 0x0009 | 0x000a | 0x000d))
            .chain(0x007f..=0x009f)
        {
            assert!(is_rejected_control(code), "U+{code:04X}");
        }
        for code in [
            0x061c, 0x200e, 0x200f, 0x202a, 0x202b, 0x202c, 0x202d, 0x202e, 0x2066, 0x2067, 0x2068,
            0x2069,
        ] {
            assert!(is_bidi_control(code), "U+{code:04X}");
            let text = format!("a{}b", char::from_u32(code).unwrap());
            assert_eq!(
                preflight_text(text.as_bytes()).unwrap_err().code,
                "TXT_BIDI_CONTROL"
            );
        }
        for plane in 0..=16 {
            assert!(is_noncharacter((plane << 16) | 0xfffe));
            assert!(is_noncharacter((plane << 16) | 0xffff));
        }
        assert!(is_noncharacter(0xfdd0));
        assert!(is_noncharacter(0xfdef));
    }

    #[test]
    fn shaping_run_and_joiner_cluster_bounds_are_exact() {
        let combining_32 = format!("a{}", "\u{0301}".repeat(32));
        let combining_33 = format!("a{}", "\u{0301}".repeat(33));
        assert!(preflight_text(combining_32.as_bytes()).is_ok());
        assert_eq!(
            preflight_text(combining_33.as_bytes()).unwrap_err().code,
            "TXT_SHAPING_COMPLEXITY_LIMIT"
        );
        for invalid in [
            "\u{200d}क",
            "क\u{200d}ष",
            "क्\u{200d}A",
            "క్\u{200c}ష\u{200d}క",
            "१्\u{200d}क",
            "क्\u{200d}१",
            "౧్\u{200d}క",
            "క్\u{200d}౧",
        ] {
            assert_eq!(
                preflight_text(invalid.as_bytes()).unwrap_err().code,
                "TXT_SHAPING_COMPLEXITY_LIMIT"
            );
        }
    }

    #[test]
    fn unsupported_scripts_and_response_caps_fail_closed() {
        for text in ["emoji 😀", "Greek Ω", "Arabic مرحبا", "CJK 文"] {
            assert_eq!(
                preflight_text(text.as_bytes()).unwrap_err().code,
                "TXT_UNSUPPORTED_UNICODE"
            );
        }
        assert_eq!(
            total_response_bytes(TXT_MAX_SERVED_BYTES, 1)
                .unwrap_err()
                .code,
            "TXT_RESPONSE_SIZE_LIMIT"
        );
    }

    #[test]
    fn bom_control_bidi_noncharacter_and_unsupported_text_fail_typed() {
        assert_eq!(
            preflight_text(&[0xff, 0xfe, 0, 0]).unwrap_err().code,
            "TXT_UNSUPPORTED_BOM"
        );
        assert_eq!(
            preflight_text(&[0xff]).unwrap_err().code,
            "TXT_INVALID_UTF8"
        );
        assert_eq!(
            preflight_text(b"a\0b").unwrap_err().code,
            "TXT_CONTROL_CHARACTER"
        );
        assert_eq!(
            preflight_text("a\u{202e}b".as_bytes()).unwrap_err().code,
            "TXT_BIDI_CONTROL"
        );
        assert_eq!(
            preflight_text("a\u{fdd0}b".as_bytes()).unwrap_err().code,
            "TXT_NONCHARACTER"
        );
        assert_eq!(
            preflight_text("emoji \u{1f600}".as_bytes())
                .unwrap_err()
                .code,
            "TXT_UNSUPPORTED_UNICODE"
        );
        assert_eq!(
            preflight_text("a\u{feff}b".as_bytes()).unwrap_err().code,
            "TXT_UNSUPPORTED_BOM"
        );
    }

    #[test]
    fn english_hindi_telugu_and_bounded_joiners_are_admitted() {
        let value =
            preflight_text("English हिन्दी తెలుగు क्\u{200d}ष क़्\u{200d}ष క్\u{200c}ష".as_bytes())
                .expect("admitted scripts");
        assert!(value.used_scripts.contains(&AdmittedScript::LatinCommon));
        assert!(value.used_scripts.contains(&AdmittedScript::Devanagari));
        assert!(value.used_scripts.contains(&AdmittedScript::Telugu));
        assert_eq!(
            preflight_text("a\u{200d}b".as_bytes()).unwrap_err().code,
            "TXT_SHAPING_COMPLEXITY_LIMIT"
        );
    }

    #[test]
    fn escaping_is_literal_and_wrapper_has_no_script() {
        let html = String::from_utf8(
            canonical_html("<script>&\" event= URL CSS SVG {{x}}").expect("html"),
        )
        .expect("utf8");
        assert!(html.contains("&lt;script&gt;&amp;\" event= URL CSS SVG {{x}}"));
        assert!(!html.contains("<script>"));
        assert_eq!(html.matches("<pre>").count(), 1);
        assert!(!html.contains("ExecuteScript"));
        let css = String::from_utf8(canonical_css().expect("css")).expect("utf8");
        assert!(css.contains("font-synthesis:none"));
        assert!(!css.contains("sans-serif"));
        assert!(total_response_bytes(html.len(), css.len()).is_ok());
    }
}
