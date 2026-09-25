//! Bounded macOS rendering for exact reviewed artifact bytes.
//!
//! The daemon has already reconstructed and authenticated the saved bytes. Office artifacts use
//! one representative Quick Look thumbnail; PDFs use a fixed JXA program in a separate macOS
//! PDFKit process so the person can request each exact page. Mesh validates the bounded PNG and
//! inert text before removing the private temporary material. No renderer output participates in
//! the approval statement.

use std::fmt;

const MAX_ARTIFACT_BYTES: usize = 32 * 1024 * 1024;
const MAX_PREVIEW_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PDF_PREVIEW_PAGE: usize = 64;
const MAX_PDF_DOCUMENT_PAGES: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArtifactKind {
    Pdf,
    Presentation,
    Document,
    Spreadsheet,
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ArtifactKind {
    pub(crate) fn from_path(path: &str) -> Option<Self> {
        let extension = std::path::Path::new(path)
            .extension()?
            .to_str()?
            .to_ascii_lowercase();
        match extension.as_str() {
            "pdf" => Some(Self::Pdf),
            "pptx" => Some(Self::Presentation),
            "docx" => Some(Self::Document),
            "xlsx" => Some(Self::Spreadsheet),
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "gif" => Some(Self::Gif),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Presentation => "presentation",
            Self::Document => "document",
            Self::Spreadsheet => "spreadsheet",
            Self::Png | Self::Jpeg | Self::Gif | Self::Webp => "image",
        }
    }

    const fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Presentation => "pptx",
            Self::Document => "docx",
            Self::Spreadsheet => "xlsx",
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }

    const fn is_image(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Gif | Self::Webp)
    }

    fn accepts(self, bytes: &[u8]) -> bool {
        match self {
            Self::Pdf => bytes.starts_with(b"%PDF-"),
            Self::Presentation => ooxml_container_has(bytes, b"ppt/presentation.xml"),
            Self::Document => ooxml_container_has(bytes, b"word/document.xml"),
            Self::Spreadsheet => ooxml_container_has(bytes, b"xl/workbook.xml"),
            Self::Png => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            Self::Jpeg => bytes.starts_with(&[0xff, 0xd8, 0xff]),
            Self::Gif => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
            Self::Webp => {
                bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")
            }
        }
    }
}

fn little_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn little_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

#[derive(Clone, Copy, Debug)]
struct ZipCentralEntry<'a> {
    name: &'a [u8],
    uncompressed_size: u32,
}

fn zip_central_entries(bytes: &[u8]) -> Option<Vec<ZipCentralEntry<'_>>> {
    const END_MIN: usize = 22;
    const MAX_COMMENT: usize = u16::MAX as usize;
    const CENTRAL_HEADER: usize = 46;
    if bytes.len() < END_MIN {
        return None;
    }
    let search_start = bytes.len().saturating_sub(END_MIN + MAX_COMMENT);
    let end_offset = bytes[search_start..]
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .map(|offset| search_start + offset)?;
    let comment_len = usize::from(little_u16(bytes, end_offset + 20)?);
    if end_offset.checked_add(END_MIN + comment_len) != Some(bytes.len())
        || little_u16(bytes, end_offset + 4) != Some(0)
        || little_u16(bytes, end_offset + 6) != Some(0)
    {
        return None;
    }
    let entries = usize::from(little_u16(bytes, end_offset + 10)?);
    if entries > 4_096
        || entries == usize::from(u16::MAX)
        || usize::from(little_u16(bytes, end_offset + 8)?) != entries
    {
        return None;
    }
    let central_size = little_u32(bytes, end_offset + 12)? as usize;
    let central_offset = little_u32(bytes, end_offset + 16)? as usize;
    let central_end = central_offset.checked_add(central_size)?;
    if central_end != end_offset || central_end > bytes.len() {
        return None;
    }

    let mut cursor = central_offset;
    let mut result = Vec::with_capacity(entries);
    let mut names = std::collections::HashSet::with_capacity(entries);
    for _ in 0..entries {
        if bytes.get(cursor..cursor + 4) != Some(b"PK\x01\x02")
            || little_u16(bytes, cursor + 34) != Some(0)
        {
            return None;
        }
        let flags = little_u16(bytes, cursor + 8)?;
        let method = little_u16(bytes, cursor + 10)?;
        if flags & 1 != 0 || !matches!(method, 0 | 8) {
            return None;
        }
        let name_len = usize::from(little_u16(bytes, cursor + 28)?);
        let extra_len = usize::from(little_u16(bytes, cursor + 30)?);
        let comment_len = usize::from(little_u16(bytes, cursor + 32)?);
        let name_start = cursor.checked_add(CENTRAL_HEADER)?;
        let name_end = name_start.checked_add(name_len)?;
        let next = name_end.checked_add(extra_len)?.checked_add(comment_len)?;
        if next > central_end {
            return None;
        }
        let name = &bytes[name_start..name_end];
        if name.is_empty()
            || name.starts_with(b"/")
            || name.contains(&b'\\')
            || name.split(|byte| *byte == b'/').any(|part| part == b"..")
            || !names.insert(name)
        {
            return None;
        }
        result.push(ZipCentralEntry {
            name,
            uncompressed_size: little_u32(bytes, cursor + 24)?,
        });
        cursor = next;
    }
    (cursor == central_end).then_some(result)
}

/// Inspect only the bounded ZIP central directory; document payloads remain opaque to Mesh.
///
/// OOXML packages have two family-independent package entries and one exact family root. Requiring
/// those names prevents a generic ZIP renamed to `.pptx`, `.docx`, or `.xlsx` from being presented
/// as that document family. ZIP64 and multi-disk archives are outside the 32 MiB alpha preview and
/// fail closed.
fn ooxml_container_has(bytes: &[u8], family_root: &[u8]) -> bool {
    let Some(entries) = zip_central_entries(bytes) else {
        return false;
    };
    let mut content_types = 0_u8;
    let mut relationships = 0_u8;
    let mut family = 0_u8;
    for entry in entries {
        match entry.name {
            b"[Content_Types].xml" => content_types = content_types.saturating_add(1),
            b"_rels/.rels" => relationships = relationships.saturating_add(1),
            name if name == family_root => family = family.saturating_add(1),
            _ => {}
        }
    }
    content_types == 1 && relationships == 1 && family == 1
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ArtifactPreview {
    pub(crate) kind: ArtifactKind,
    pub(crate) png: Vec<u8>,
    pub(crate) text: Option<ArtifactText>,
    pub(crate) renderer: ArtifactRenderer,
    pub(crate) page_number: Option<usize>,
    pub(crate) page_count: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArtifactRenderer {
    QuickLookThumbnail,
    ImageIoThumbnail,
    PdfKitPage,
}

impl ArtifactRenderer {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::QuickLookThumbnail => "macos-quick-look-thumbnail",
            Self::ImageIoThumbnail => "macos-imageio-thumbnail-v1",
            Self::PdfKitPage => "macos-pdfkit-page-v1",
        }
    }

    pub(crate) const fn scope(self) -> &'static str {
        match self {
            Self::QuickLookThumbnail | Self::ImageIoThumbnail => "representative-preview",
            Self::PdfKitPage => "exact-page-preview",
        }
    }
}

struct PlatformPreview {
    png: Vec<u8>,
    text: Option<ArtifactText>,
    renderer: ArtifactRenderer,
    page_number: Option<usize>,
    page_count: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ArtifactText {
    pub(crate) source: ArtifactTextSource,
    pub(crate) lines: Vec<String>,
    pub(crate) sections: Vec<ArtifactTextSection>,
    pub(crate) truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArtifactTextSource {
    QuickLookVisible,
    PdfPageText,
    PresentationSlides,
    DocumentBlocks,
    SpreadsheetCells,
}

impl ArtifactTextSource {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::QuickLookVisible => "macos-quick-look-visible-text",
            Self::PdfPageText => "macos-pdfkit-page-text-v1",
            Self::PresentationSlides => "mesh-pptx-slide-text-v1",
            Self::DocumentBlocks => "mesh-docx-block-text-v1",
            Self::SpreadsheetCells => "mesh-xlsx-cell-formula-v1",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ArtifactTextSection {
    pub(crate) label: String,
    pub(crate) line_start: usize,
    pub(crate) line_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ArtifactPreviewError {
    Unsupported,
    TooLarge,
    InvalidContainer,
    InvalidPage,
    Unavailable,
}

impl fmt::Display for ArtifactPreviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unsupported => "This file type does not have an artifact preview.",
            Self::TooLarge => "This artifact is too large for the bounded local preview.",
            Self::InvalidContainer => "The saved bytes do not match the document type.",
            Self::InvalidPage => "That page is not present in this saved PDF.",
            Self::Unavailable => "macOS could not render a visual preview for this saved artifact.",
        })
    }
}

pub(crate) fn render(
    path: &str,
    bytes: &[u8],
    page_number: Option<usize>,
) -> Result<ArtifactPreview, ArtifactPreviewError> {
    let kind = ArtifactKind::from_path(path).ok_or(ArtifactPreviewError::Unsupported)?;
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(ArtifactPreviewError::TooLarge);
    }
    if !kind.accepts(bytes) {
        return Err(ArtifactPreviewError::InvalidContainer);
    }
    if page_number.is_some_and(|page| page == 0 || page > MAX_PDF_PREVIEW_PAGE)
        || (kind != ArtifactKind::Pdf && page_number.is_some())
    {
        return Err(ArtifactPreviewError::InvalidPage);
    }
    render_platform(kind, bytes, page_number).map(
        |PlatformPreview {
             png,
             text,
             renderer,
             page_number,
             page_count,
         }| ArtifactPreview {
            kind,
            png,
            text,
            renderer,
            page_number,
            page_count,
        },
    )
}

/// Return the closed document suffix only when the exact bytes match that artifact family.
/// Native-inspection export uses the same bounded container admission as visual rendering without
/// requiring the platform renderer itself to succeed.
pub(crate) fn inspection_extension(path: &str, bytes: &[u8]) -> Option<&'static str> {
    let kind = ArtifactKind::from_path(path)?;
    (bytes.len() <= MAX_ARTIFACT_BYTES && kind.accepts(bytes)).then(|| kind.extension())
}

fn artifact_text_character_is_unsafe(character: char) -> bool {
    (character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

fn decoded_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => {
            char::from_u32(u32::from_str_radix(&entity[2..], 16).ok()?)
        }
        _ if entity.starts_with('#') => char::from_u32(entity[1..].parse().ok()?),
        _ => None,
    }
}

fn decode_html_text(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        let rest = &text[cursor..];
        if rest.starts_with('&') {
            if let Some(end) = rest
                .get(1..)
                .and_then(|tail| tail.find(';'))
                .filter(|end| *end <= 12)
            {
                let entity_end = end + 1;
                if let Some(character) = decoded_entity(&rest[1..entity_end]) {
                    decoded.push(character);
                    cursor += entity_end + 1;
                    continue;
                }
            }
        }
        let character = rest
            .chars()
            .next()
            .expect("cursor remains on a character boundary");
        decoded.push(character);
        cursor += character.len_utf8();
    }
    decoded
}

fn push_artifact_line(
    lines: &mut Vec<String>,
    characters: &mut usize,
    current: &mut String,
) -> bool {
    while current.ends_with([' ', '\t']) {
        current.pop();
    }
    if current.is_empty() {
        return true;
    }
    const MAX_LINES: usize = 512;
    const MAX_CHARACTERS: usize = 128 * 1024;
    if lines.len() == MAX_LINES || characters.saturating_add(current.len()) > MAX_CHARACTERS {
        return false;
    }
    *characters += current.len();
    lines.push(std::mem::take(current));
    true
}

fn tag_has_class(tag: &str, expected: &str) -> bool {
    let Some(class_at) = tag.find("class=") else {
        return false;
    };
    let value = &tag[class_at + "class=".len()..];
    let Some(quote) = value
        .chars()
        .next()
        .filter(|quote| matches!(quote, '\'' | '"'))
    else {
        return false;
    };
    let value = &value[quote.len_utf8()..];
    let Some(end) = value.find(quote) else {
        return false;
    };
    value[..end]
        .split_ascii_whitespace()
        .any(|class| class == expected)
}

fn artifact_section_label(kind: ArtifactKind, number: usize) -> String {
    let family = match kind {
        ArtifactKind::Pdf => "Page",
        ArtifactKind::Presentation => "Slide",
        ArtifactKind::Document => "Document",
        ArtifactKind::Spreadsheet => "Sheet",
        ArtifactKind::Png | ArtifactKind::Jpeg | ArtifactKind::Gif | ArtifactKind::Webp => "Image",
    };
    if kind == ArtifactKind::Document {
        family.to_owned()
    } else {
        format!("{family} {number}")
    }
}

fn artifact_text_sections(
    kind: ArtifactKind,
    line_count: usize,
    mut starts: Vec<(usize, String)>,
) -> Vec<ArtifactTextSection> {
    if starts.is_empty() {
        starts.push((0, artifact_section_label(kind, 1)));
    }
    starts
        .iter()
        .enumerate()
        .filter_map(|(index, (line_start, label))| {
            let line_end = starts
                .get(index + 1)
                .map_or(line_count, |(next_start, _)| *next_start);
            (line_end > *line_start).then(|| ArtifactTextSection {
                label: label.clone(),
                line_start: *line_start,
                line_count: line_end - line_start,
            })
        })
        .collect()
}

/// Extract visible text from Quick Look's generated HTML without embedding or executing it.
///
/// Only the body is considered; style/script blocks are discarded, common structural tags become
/// line/cell boundaries, unsafe controls and bidi overrides fail closed, and both input and output
/// are bounded. This is a review navigation aid, never an approval input.
fn extract_quick_look_text(kind: ArtifactKind, html: &[u8]) -> Option<ArtifactText> {
    const MAX_HTML_BYTES: usize = 4 * 1024 * 1024;
    if html.len() > MAX_HTML_BYTES {
        return None;
    }
    let html = std::str::from_utf8(html).ok()?;
    let lowercase = html.to_ascii_lowercase();
    let body_tag = lowercase.find("<body")?;
    let body_start = body_tag + lowercase[body_tag..].find('>')? + 1;
    let body_end = body_start + lowercase[body_start..].find("</body>")?;
    let body = &html[body_start..body_end];
    let mut cursor = 0;
    let mut current = String::new();
    let mut lines = Vec::new();
    let mut characters = 0;
    let mut skipping = None::<String>;
    let mut section_starts = Vec::new();
    let mut truncated = false;

    while cursor < body.len() {
        if body[cursor..].starts_with('<') {
            let relative_end = body[cursor..].find('>')?;
            let tag = body[cursor + 1..cursor + relative_end]
                .trim()
                .to_ascii_lowercase();
            cursor += relative_end + 1;
            if let Some(skipped) = skipping.as_deref() {
                if tag
                    .strip_prefix('/')
                    .is_some_and(|closing| closing.trim() == skipped)
                {
                    skipping = None;
                }
                continue;
            }
            let name = tag
                .trim_start_matches('/')
                .split_ascii_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches('/');
            if matches!(name, "style" | "script") && !tag.starts_with('/') {
                skipping = Some(name.to_owned());
                continue;
            }
            let closes = tag.starts_with('/');
            let begins_section = !closes
                && match kind {
                    ArtifactKind::Presentation => name == "div" && tag_has_class(&tag, "slide"),
                    ArtifactKind::Spreadsheet => {
                        name == "table" && tag_has_class(&tag, "worksheet")
                    }
                    ArtifactKind::Document
                    | ArtifactKind::Pdf
                    | ArtifactKind::Png
                    | ArtifactKind::Jpeg
                    | ArtifactKind::Gif
                    | ArtifactKind::Webp => false,
                };
            if begins_section {
                if !push_artifact_line(&mut lines, &mut characters, &mut current) {
                    truncated = true;
                    break;
                }
                const MAX_SECTIONS: usize = 64;
                if section_starts.len() == MAX_SECTIONS {
                    truncated = true;
                    break;
                }
                section_starts.push((
                    lines.len(),
                    artifact_section_label(kind, section_starts.len() + 1),
                ));
            }
            let line_boundary = name == "br"
                || (closes
                    && matches!(
                        name,
                        "p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "tr"
                    ));
            if closes && name == "td" && !current.ends_with('\t') {
                current.push('\t');
            }
            if line_boundary && !push_artifact_line(&mut lines, &mut characters, &mut current) {
                truncated = true;
                break;
            }
            continue;
        }

        let relative_end = body[cursor..].find('<').unwrap_or(body.len() - cursor);
        if skipping.is_some() {
            cursor += relative_end;
            continue;
        }
        let text = decode_html_text(&body[cursor..cursor + relative_end]);
        cursor += relative_end;
        let mut pending_space = false;
        for character in text.chars() {
            if artifact_text_character_is_unsafe(character) {
                return None;
            }
            if character.is_whitespace() {
                pending_space = !current.is_empty() && !current.ends_with([' ', '\t']);
            } else {
                if pending_space {
                    current.push(' ');
                }
                current.push(character);
                pending_space = false;
            }
        }
    }
    if !truncated && !push_artifact_line(&mut lines, &mut characters, &mut current) {
        truncated = true;
    }
    if lines.is_empty() {
        return None;
    }
    let sections = artifact_text_sections(kind, lines.len(), section_starts);
    Some(ArtifactText {
        source: ArtifactTextSource::QuickLookVisible,
        lines,
        sections,
        truncated,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct XmlTag {
    name: String,
    attributes: Vec<(String, String)>,
}

impl XmlTag {
    fn local_name(&self) -> &str {
        self.name
            .rsplit_once(':')
            .map_or(&self.name, |(_, name)| name)
    }

    fn attribute(&self, expected: &str) -> Option<&str> {
        let mut matches = self.attributes.iter().filter_map(|(name, value)| {
            (name
                .rsplit_once(':')
                .map_or(name.as_str(), |(_, name)| name)
                == expected)
                .then_some(value.as_str())
        });
        let value = matches.next()?;
        matches.next().is_none().then_some(value)
    }

    fn qualified_attribute(&self, expected: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find_map(|(name, value)| (name == expected).then_some(value.as_str()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum XmlEvent {
    Start(XmlTag),
    Empty(XmlTag),
    End(String),
    Text(String),
}

fn xml_name_is_safe(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'-' | b'.'))
}

fn decode_xml_text(text: &str) -> Option<String> {
    let mut decoded = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        let rest = &text[cursor..];
        if rest.starts_with('&') {
            let end = rest.get(1..)?.find(';')?;
            if end > 12 {
                return None;
            }
            let entity_end = end + 1;
            let character = decoded_entity(&rest[1..entity_end])?;
            if artifact_text_character_is_unsafe(character) {
                return None;
            }
            decoded.push(character);
            cursor += entity_end + 1;
            continue;
        }
        let character = rest.chars().next()?;
        if artifact_text_character_is_unsafe(character) {
            return None;
        }
        decoded.push(character);
        cursor += character.len_utf8();
    }
    Some(decoded)
}

fn parse_xml_tag(source: &str) -> Option<(XmlTag, bool)> {
    let source = source.trim();
    let (source, empty) = source
        .strip_suffix('/')
        .map_or((source, false), |source| (source.trim_end(), true));
    let name_end = source.find(char::is_whitespace).unwrap_or(source.len());
    let name = source.get(..name_end)?;
    if !xml_name_is_safe(name) {
        return None;
    }
    let mut remaining = source.get(name_end..)?.trim_start();
    let mut attributes = Vec::new();
    let mut names = std::collections::HashSet::new();
    while !remaining.is_empty() {
        let key_end = remaining
            .find(|character: char| character.is_whitespace() || character == '=')
            .unwrap_or(remaining.len());
        let key = remaining.get(..key_end)?;
        // Real Word roots carry a broad, fixed family of namespace declarations. Keep the
        // parser bounded while admitting that ordinary OOXML envelope instead of falling back
        // to less-structured Quick Look text.
        if !xml_name_is_safe(key) || attributes.len() == 64 {
            return None;
        }
        remaining = remaining.get(key_end..)?.trim_start();
        remaining = remaining.strip_prefix('=')?.trim_start();
        let quote = remaining
            .chars()
            .next()
            .filter(|quote| matches!(quote, '\'' | '"'))?;
        remaining = remaining.get(quote.len_utf8()..)?;
        let value_end = remaining.find(quote)?;
        let value = decode_xml_text(remaining.get(..value_end)?)?;
        if value.len() > 2_048 {
            return None;
        }
        if !names.insert(key.to_owned()) {
            return None;
        }
        attributes.push((key.to_owned(), value));
        remaining = remaining.get(value_end + quote.len_utf8()..)?.trim_start();
    }
    Some((
        XmlTag {
            name: name.to_owned(),
            attributes,
        },
        empty,
    ))
}

fn bounded_xml_events(bytes: &[u8]) -> Option<Vec<XmlEvent>> {
    const MAX_XML_BYTES: usize = 4 * 1024 * 1024;
    const MAX_EVENTS: usize = 100_000;
    if bytes.len() > MAX_XML_BYTES {
        return None;
    }
    let source = std::str::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes)).ok()?;
    let mut events = Vec::new();
    let mut stack = Vec::new();
    let mut root_seen = false;
    let mut cursor = 0;
    while cursor < source.len() {
        if events.len() == MAX_EVENTS {
            return None;
        }
        let Some(relative_start) = source[cursor..].find('<') else {
            let text = decode_xml_text(&source[cursor..])?;
            if stack.is_empty() && !text.trim().is_empty() {
                return None;
            }
            if !text.is_empty() {
                events.push(XmlEvent::Text(text));
            }
            cursor = source.len();
            continue;
        };
        let tag_start = cursor + relative_start;
        if tag_start > cursor {
            let text = decode_xml_text(&source[cursor..tag_start])?;
            if stack.is_empty() && !text.trim().is_empty() {
                return None;
            }
            if !text.is_empty() {
                events.push(XmlEvent::Text(text));
            }
        }
        if source[tag_start..].starts_with("<!--") {
            let end = source[tag_start + 4..].find("-->")? + tag_start + 7;
            cursor = end;
            continue;
        }
        if source[tag_start..].starts_with("<?") {
            let end = source[tag_start + 2..].find("?>")? + tag_start + 4;
            cursor = end;
            continue;
        }
        if source[tag_start..].starts_with("<!") {
            return None;
        }
        let mut quote = None;
        let mut tag_end = None;
        for (offset, character) in source[tag_start + 1..].char_indices() {
            if matches!(character, '\'' | '"') {
                quote = if quote == Some(character) {
                    None
                } else if quote.is_none() {
                    Some(character)
                } else {
                    quote
                };
            } else if character == '>' && quote.is_none() {
                tag_end = Some(tag_start + 1 + offset);
                break;
            }
        }
        let tag_end = tag_end?;
        let raw = source[tag_start + 1..tag_end].trim();
        if let Some(name) = raw.strip_prefix('/') {
            let name = name.trim();
            if !xml_name_is_safe(name) || stack.pop().as_deref() != Some(name) {
                return None;
            }
            events.push(XmlEvent::End(name.to_owned()));
        } else {
            let (tag, empty) = parse_xml_tag(raw)?;
            if stack.is_empty() {
                if root_seen {
                    return None;
                }
                root_seen = true;
            }
            if empty {
                events.push(XmlEvent::Empty(tag));
            } else {
                stack.push(tag.name.clone());
                events.push(XmlEvent::Start(tag));
            }
        }
        cursor = tag_end + 1;
    }
    (root_seen && stack.is_empty()).then_some(events)
}

#[cfg(target_os = "macos")]
fn unzip_entry_bounded(
    input: &std::path::Path,
    archive: &[u8],
    name: &str,
    maximum: usize,
) -> Option<Vec<u8>> {
    use std::io::Read as _;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let entry = zip_central_entries(archive)?
        .into_iter()
        .find(|entry| entry.name == name.as_bytes())?;
    let declared = usize::try_from(entry.uncompressed_size).ok()?;
    if declared > maximum {
        return None;
    }
    let mut child = Command::new("/usr/bin/unzip")
        .args(["-p", "--"])
        .arg(input)
        .arg(name)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::with_capacity(declared.min(maximum));
        stdout
            .take(u64::try_from(maximum).ok()?.saturating_add(1))
            .read_to_end(&mut bytes)
            .ok()?;
        Some(bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                break child.wait().ok();
            }
            Err(_) => break None,
        }
    }?;
    let bytes = reader.join().ok()??;
    (status.success() && bytes.len() == declared && bytes.len() <= maximum).then_some(bytes)
}

fn normalized_worksheet_target(target: &str) -> Option<String> {
    if target.contains('\\') || target.contains('%') {
        return None;
    }
    let target = target.strip_prefix('/').unwrap_or(target);
    let target = if target.starts_with("xl/") {
        target.to_owned()
    } else {
        format!("xl/{target}")
    };
    if !target.starts_with("xl/worksheets/")
        || target
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return None;
    }
    Some(target)
}

fn normalized_slide_target(target: &str) -> Option<String> {
    if target.contains('\\') || target.contains('%') {
        return None;
    }
    let target = target.strip_prefix('/').unwrap_or(target);
    let target = if target.starts_with("ppt/") {
        target.to_owned()
    } else {
        format!("ppt/{target}")
    };
    if !target.starts_with("ppt/slides/")
        || target
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return None;
    }
    Some(target)
}

fn presentation_slides(presentation: &[u8], relationships: &[u8]) -> Option<Vec<String>> {
    let mut targets = std::collections::HashMap::new();
    let mut relationship_ids = std::collections::HashSet::new();
    let mut relationship_path = Vec::new();
    for event in bounded_xml_events(relationships)? {
        match event {
            XmlEvent::Start(tag) => {
                let local = tag.local_name().to_owned();
                if relationship_path.is_empty() && local != "Relationships" {
                    return None;
                }
                if local == "Relationship" {
                    if relationship_path.as_slice() != ["Relationships"] {
                        return None;
                    }
                    let id = tag.attribute("Id")?.to_owned();
                    relationship_ids.insert(id).then_some(())?;
                    add_presentation_relationship(&mut targets, &tag)?;
                } else if relationship_path.as_slice() == ["Relationships"] {
                    return None;
                }
                relationship_path.push(local);
            }
            XmlEvent::Empty(tag) => {
                if tag.local_name() != "Relationship"
                    || relationship_path.as_slice() != ["Relationships"]
                {
                    return None;
                }
                let id = tag.attribute("Id")?.to_owned();
                relationship_ids.insert(id).then_some(())?;
                add_presentation_relationship(&mut targets, &tag)?;
            }
            XmlEvent::End(_) => {
                relationship_path.pop()?;
            }
            XmlEvent::Text(text) if !text.trim().is_empty() => return None,
            XmlEvent::Text(_) => {}
        }
    }
    if !relationship_path.is_empty() {
        return None;
    }

    let mut slides = Vec::new();
    let mut paths = std::collections::HashSet::new();
    let mut presentation_path = Vec::new();
    for event in bounded_xml_events(presentation)? {
        match event {
            XmlEvent::Start(tag) => {
                let local = tag.local_name().to_owned();
                if presentation_path.is_empty() && local != "presentation" {
                    return None;
                }
                if local == "sldId" {
                    if presentation_path.as_slice() != ["presentation", "sldIdLst"] {
                        return None;
                    }
                    add_presentation_slide(&mut slides, &mut paths, &targets, &tag)?;
                }
                presentation_path.push(local);
            }
            XmlEvent::Empty(tag) if tag.local_name() == "sldId" => {
                if presentation_path.as_slice() != ["presentation", "sldIdLst"] {
                    return None;
                }
                add_presentation_slide(&mut slides, &mut paths, &targets, &tag)?;
            }
            XmlEvent::Empty(_) => {}
            XmlEvent::End(_) => {
                presentation_path.pop()?;
            }
            XmlEvent::Text(text) if !text.trim().is_empty() => return None,
            XmlEvent::Text(_) => {}
        }
    }
    (presentation_path.is_empty() && !slides.is_empty()).then_some(slides)
}

fn add_presentation_relationship(
    targets: &mut std::collections::HashMap<String, String>,
    tag: &XmlTag,
) -> Option<()> {
    if !tag.attribute("Type")?.ends_with("/slide") {
        return Some(());
    }
    if tag.attribute("TargetMode").is_some() {
        return None;
    }
    let id = tag.attribute("Id")?.to_owned();
    let target = normalized_slide_target(tag.attribute("Target")?)?;
    targets.insert(id, target).is_none().then_some(())
}

fn add_presentation_slide(
    slides: &mut Vec<String>,
    paths: &mut std::collections::HashSet<String>,
    targets: &std::collections::HashMap<String, String>,
    tag: &XmlTag,
) -> Option<()> {
    if slides.len() == 64 {
        return None;
    }
    let target = targets.get(tag.qualified_attribute("r:id")?)?.clone();
    paths.insert(target.clone()).then_some(())?;
    slides.push(target);
    Some(())
}

fn ooxml_boolean(value: Option<&str>, default: bool) -> Option<bool> {
    match value {
        None => Some(default),
        Some("true" | "1") => Some(true),
        Some("false" | "0") => Some(false),
        Some(_) => None,
    }
}

fn presentation_literal_field(value: &str) -> Option<String> {
    if value.len() > 2_048 {
        return None;
    }
    let mut result = String::with_capacity(value.len());
    for character in value.chars() {
        if artifact_text_character_is_unsafe(character) {
            return None;
        }
        match character {
            // Synthetic visibility markers share this text channel. Escape their delimiters and
            // the escape character in literal presentation strings so the encoding remains
            // unambiguous, including inside a marker's user-supplied object name.
            '\\' | '<' | '>' => {
                result.push('\\');
                result.push(character);
            }
            '\t' => result.push_str("\\t"),
            '\r' => result.push_str("\\r"),
            '\n' => result.push_str("\\n"),
            _ => result.push(character),
        }
    }
    Some(result)
}

fn presentation_hidden_object_marker(tag: &XmlTag) -> Option<Option<String>> {
    if !ooxml_boolean(tag.qualified_attribute("hidden"), false)? {
        return Some(None);
    }
    let id = tag.qualified_attribute("id")?;
    let name = presentation_literal_field(tag.qualified_attribute("name")?)?;
    if id.is_empty()
        || id.len() > 10
        || !id.bytes().all(|byte| byte.is_ascii_digit())
        || id.parse::<u32>().is_err()
        || name.len() > 128
    {
        return None;
    }
    let label = if name.is_empty() {
        format!("<object {id} visibility: hidden>")
    } else {
        format!("<object {id} visibility: hidden · {name}>")
    };
    Some(Some(label))
}

fn add_presentation_line(lines: &mut Vec<String>, line: String) -> Option<()> {
    if lines.len() == 512 {
        return None;
    }
    lines.push(line);
    Some(())
}

fn presentation_slide_text(xml: &[u8], number: usize) -> Option<(String, Vec<String>)> {
    let mut path = Vec::new();
    let mut slide_name = None::<String>;
    let mut slide_shown = None::<bool>;
    let mut paragraph = None::<String>;
    let mut in_text = false;
    let mut in_line_break = false;
    let mut in_tab = false;
    let mut lines = Vec::new();
    for event in bounded_xml_events(xml)? {
        match event {
            XmlEvent::Start(tag) => {
                let local = tag.local_name().to_owned();
                if path.is_empty() {
                    if local != "sld" {
                        return None;
                    }
                    slide_shown = Some(ooxml_boolean(tag.attribute("show"), true)?);
                }
                // DrawingML tabs are empty-content separators. A paired spelling is valid XML,
                // but accepting children would make unsupported tab payload disappear from the
                // comparison. Fail closed instead of flattening it into ordinary paragraph text.
                if in_tab {
                    return None;
                }
                if local == "cSld" {
                    if path.as_slice() != ["sld"] || slide_name.is_some() {
                        return None;
                    }
                    slide_name = Some(tag.attribute("name").unwrap_or("").to_owned());
                } else if local == "cNvPr" && path.iter().any(|part| part == "cSld") {
                    if let Some(marker) = presentation_hidden_object_marker(&tag)? {
                        add_presentation_line(&mut lines, marker)?;
                    }
                } else if local == "p" && path.iter().any(|part| part == "cSld") {
                    if paragraph.is_some() {
                        return None;
                    }
                    paragraph = Some(String::new());
                } else if local == "br" && paragraph.is_some() {
                    if in_text || in_line_break || in_tab {
                        return None;
                    }
                    paragraph.as_mut()?.push('\n');
                    in_line_break = true;
                } else if local == "tab" && paragraph.is_some() {
                    if in_text || in_line_break || in_tab {
                        return None;
                    }
                    paragraph.as_mut()?.push('\t');
                    in_tab = true;
                } else if local == "t" && paragraph.is_some() {
                    if in_text || in_line_break || in_tab {
                        return None;
                    }
                    in_text = true;
                } else if local == "t" && path.iter().any(|part| part == "cSld") {
                    return None;
                }
                path.push(local);
            }
            XmlEvent::Empty(tag)
                if matches!(tag.local_name(), "br" | "tab") && paragraph.is_some() =>
            {
                if in_text || in_line_break || in_tab {
                    return None;
                }
                paragraph
                    .as_mut()?
                    .push(if tag.local_name() == "br" { '\n' } else { '\t' });
            }
            XmlEvent::Empty(tag) if tag.local_name() == "cSld" => {
                if path.as_slice() != ["sld"] || slide_name.is_some() {
                    return None;
                }
                slide_name = Some(tag.attribute("name").unwrap_or("").to_owned());
            }
            XmlEvent::Empty(tag)
                if tag.local_name() == "cNvPr" && path.iter().any(|part| part == "cSld") =>
            {
                if let Some(marker) = presentation_hidden_object_marker(&tag)? {
                    add_presentation_line(&mut lines, marker)?;
                }
            }
            XmlEvent::Empty(_) if in_tab => return None,
            XmlEvent::Empty(_) => {}
            XmlEvent::Text(text) if in_text => paragraph.as_mut()?.push_str(&text),
            XmlEvent::Text(_) if in_tab => return None,
            XmlEvent::End(name) => {
                let local = name
                    .rsplit_once(':')
                    .map_or(name.as_str(), |(_, name)| name);
                if local == "br" && paragraph.is_some() {
                    if !in_line_break || in_text || in_tab {
                        return None;
                    }
                    in_line_break = false;
                } else if local == "tab" && paragraph.is_some() {
                    if !in_tab || in_text || in_line_break {
                        return None;
                    }
                    in_tab = false;
                } else if local == "t" && paragraph.is_some() {
                    if !in_text {
                        return None;
                    }
                    in_text = false;
                } else if local == "p" && paragraph.is_some() {
                    if in_text || in_line_break || in_tab {
                        return None;
                    }
                    let value = paragraph.take()?;
                    let value = presentation_literal_field(value.trim_matches(' '))?;
                    if !value.is_empty() {
                        add_presentation_line(&mut lines, value)?;
                    }
                }
                path.pop()?;
            }
            XmlEvent::Text(_) => {}
        }
    }
    if !path.is_empty() || paragraph.is_some() || in_text || in_line_break || in_tab {
        return None;
    }
    let slide_shown = slide_shown?;
    let default = format!("Slide {number}");
    let name = slide_name?;
    let label = if name.is_empty() || name == default {
        default
    } else {
        let name = spreadsheet_field(&name)?;
        if name.len() > 64 {
            return None;
        }
        format!("{default} · {name}")
    };
    if lines.is_empty() {
        add_presentation_line(&mut lines, "<no slide text>".to_owned())?;
    }
    if !slide_shown {
        if lines.len() == 512 {
            return None;
        }
        lines.insert(0, "<slide visibility: hidden>".to_owned());
    }
    Some((label, lines))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpreadsheetSheetVisibility {
    Visible,
    Hidden,
    VeryHidden,
}

impl SpreadsheetSheetVisibility {
    fn from_attribute(value: Option<&str>) -> Option<Self> {
        match value {
            None | Some("visible") => Some(Self::Visible),
            Some("hidden") => Some(Self::Hidden),
            Some("veryHidden") => Some(Self::VeryHidden),
            Some(_) => None,
        }
    }

    const fn review_marker(self) -> Option<&'static str> {
        match self {
            Self::Visible => None,
            Self::Hidden => Some("<worksheet visibility: hidden>"),
            Self::VeryHidden => Some("<worksheet visibility: very hidden>"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SpreadsheetSheet {
    name: String,
    path: String,
    visibility: SpreadsheetSheetVisibility,
}

fn spreadsheet_sheets(workbook: &[u8], relationships: &[u8]) -> Option<Vec<SpreadsheetSheet>> {
    let mut targets = std::collections::HashMap::new();
    let mut relationship_ids = std::collections::HashSet::new();
    let mut relationship_path = Vec::new();
    for event in bounded_xml_events(relationships)? {
        match event {
            XmlEvent::Start(tag) => {
                let local = tag.local_name().to_owned();
                if relationship_path.is_empty() && local != "Relationships" {
                    return None;
                }
                if local == "Relationship" {
                    if relationship_path.as_slice() != ["Relationships"] {
                        return None;
                    }
                    let id = tag.attribute("Id")?.to_owned();
                    relationship_ids.insert(id).then_some(())?;
                    add_spreadsheet_relationship(&mut targets, &tag)?;
                } else if relationship_path.as_slice() == ["Relationships"] {
                    return None;
                }
                relationship_path.push(local);
            }
            XmlEvent::Empty(tag) => {
                if tag.local_name() != "Relationship"
                    || relationship_path.as_slice() != ["Relationships"]
                {
                    return None;
                }
                let id = tag.attribute("Id")?.to_owned();
                relationship_ids.insert(id).then_some(())?;
                add_spreadsheet_relationship(&mut targets, &tag)?;
            }
            XmlEvent::End(_) => {
                relationship_path.pop()?;
            }
            XmlEvent::Text(text) if !text.trim().is_empty() => return None,
            XmlEvent::Text(_) => {}
        }
    }
    if !relationship_path.is_empty() {
        return None;
    }
    let mut sheets = Vec::new();
    let mut names = std::collections::HashSet::new();
    let mut paths = std::collections::HashSet::new();
    let mut workbook_path = Vec::new();
    for event in bounded_xml_events(workbook)? {
        match event {
            XmlEvent::Start(tag) => {
                let local = tag.local_name().to_owned();
                if workbook_path.is_empty() && local != "workbook" {
                    return None;
                }
                if local == "sheet" {
                    if workbook_path.as_slice() != ["workbook", "sheets"] {
                        return None;
                    }
                    add_spreadsheet_sheet(&mut sheets, &mut names, &mut paths, &targets, &tag)?;
                }
                workbook_path.push(local);
            }
            XmlEvent::Empty(tag) if tag.local_name() == "sheet" => {
                if workbook_path.as_slice() != ["workbook", "sheets"] {
                    return None;
                }
                add_spreadsheet_sheet(&mut sheets, &mut names, &mut paths, &targets, &tag)?;
            }
            XmlEvent::Empty(_) => {}
            XmlEvent::End(_) => {
                workbook_path.pop()?;
            }
            XmlEvent::Text(text) if !text.trim().is_empty() => return None,
            XmlEvent::Text(_) => {}
        }
    }
    (workbook_path.is_empty() && !sheets.is_empty()).then_some(sheets)
}

fn add_spreadsheet_relationship(
    targets: &mut std::collections::HashMap<String, String>,
    tag: &XmlTag,
) -> Option<()> {
    if !tag.attribute("Type")?.ends_with("/worksheet") {
        return Some(());
    }
    if tag.attribute("TargetMode").is_some() {
        return None;
    }
    let id = tag.attribute("Id")?.to_owned();
    let target = normalized_worksheet_target(tag.attribute("Target")?)?;
    targets.insert(id, target).is_none().then_some(())
}

fn add_spreadsheet_sheet(
    sheets: &mut Vec<SpreadsheetSheet>,
    names: &mut std::collections::HashSet<String>,
    paths: &mut std::collections::HashSet<String>,
    targets: &std::collections::HashMap<String, String>,
    tag: &XmlTag,
) -> Option<()> {
    let name = tag.attribute("name")?.to_owned();
    let target = targets.get(tag.qualified_attribute("r:id")?)?.clone();
    let visibility = SpreadsheetSheetVisibility::from_attribute(tag.attribute("state"))?;
    if sheets.len() == 64
        || name.is_empty()
        || name.len() > 80
        || name.chars().any(artifact_text_character_is_unsafe)
        || !names.insert(name.clone())
        || !paths.insert(target.clone())
    {
        return None;
    }
    sheets.push(SpreadsheetSheet {
        name,
        path: target,
        visibility,
    });
    Some(())
}

fn spreadsheet_shared_strings(xml: Option<&[u8]>) -> Option<Vec<String>> {
    let Some(xml) = xml else {
        return Some(Vec::new());
    };
    let mut strings = Vec::new();
    let mut current = None::<String>;
    let mut in_text = false;
    let mut characters = 0_usize;
    let mut path = Vec::new();
    for event in bounded_xml_events(xml)? {
        match event {
            XmlEvent::Start(tag) => {
                let local = tag.local_name().to_owned();
                if path.is_empty() && local != "sst" {
                    return None;
                }
                if local == "si" {
                    if path.as_slice() != ["sst"] || current.is_some() {
                        return None;
                    }
                    current = Some(String::new());
                } else if local == "t" && current.is_some() {
                    if in_text {
                        return None;
                    }
                    in_text = true;
                }
                path.push(local);
            }
            XmlEvent::Text(text) if in_text => current.as_mut()?.push_str(&text),
            XmlEvent::Text(text) if !text.trim().is_empty() && current.is_none() => return None,
            XmlEvent::End(name) => {
                let local = name
                    .rsplit_once(':')
                    .map_or(name.as_str(), |(_, name)| name);
                if local == "t" && current.is_some() {
                    in_text = false;
                } else if local == "si" {
                    if in_text || path.as_slice() != ["sst", "si"] {
                        return None;
                    }
                    let value = current.take()?;
                    characters = characters.checked_add(value.len())?;
                    if strings.len() == 4_096 || value.len() > 2_048 || characters > 128 * 1024 {
                        return None;
                    }
                    strings.push(value);
                }
                path.pop()?;
            }
            XmlEvent::Empty(tag) if tag.local_name() == "si" => {
                if path.as_slice() != ["sst"] || strings.len() == 4_096 {
                    return None;
                }
                strings.push(String::new());
            }
            XmlEvent::Empty(_) | XmlEvent::Text(_) => {}
        }
    }
    (path.is_empty() && current.is_none() && !in_text).then_some(strings)
}

fn spreadsheet_field(value: &str) -> Option<String> {
    if value.len() > 2_048 {
        return None;
    }
    let mut result = String::with_capacity(value.len());
    for character in value.chars() {
        if artifact_text_character_is_unsafe(character) {
            return None;
        }
        match character {
            // Control whitespace is rendered as a printable escape inside the tab-delimited
            // review channel. Escape literal backslashes first so a real tab/newline cannot
            // compare equal to user-authored `\t`/`\n` text.
            '\\' => result.push_str("\\\\"),
            '\t' => result.push_str("\\t"),
            '\r' => result.push_str("\\r"),
            '\n' => result.push_str("\\n"),
            _ => result.push(character),
        }
    }
    Some(result)
}

fn spreadsheet_cell_coordinates(reference: &str) -> Option<(u32, u32)> {
    let columns = reference.bytes().take_while(u8::is_ascii_uppercase).count();
    let row = &reference[columns..];
    if !(1..=3).contains(&columns)
        || row.is_empty()
        || row.len() > 7
        || row.starts_with('0')
        || !row.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let column = reference[..columns]
        .bytes()
        .fold(0_u32, |value, byte| value * 26 + u32::from(byte - b'A' + 1));
    let row = row.parse::<u32>().ok()?;
    (column <= 16_384 && row <= 1_048_576).then_some((column, row))
}

fn spreadsheet_cell_reference_is_valid(reference: &str) -> bool {
    spreadsheet_cell_coordinates(reference).is_some()
}

fn spreadsheet_merge_count(tag: &XmlTag) -> Option<Option<usize>> {
    let has_count = tag.attributes.iter().any(|(name, _)| {
        name.rsplit_once(':')
            .map_or(name.as_str(), |(_, name)| name)
            == "count"
    });
    if !has_count {
        return Some(None);
    }
    let value = tag.attribute("count")?;
    if value.is_empty()
        || value.len() > 4
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let count = value.parse::<usize>().ok()?;
    (count <= 4_096).then_some(Some(count))
}

fn spreadsheet_merged_range(tag: &XmlTag) -> Option<String> {
    let reference = tag.attribute("ref")?;
    let (start, end) = reference.split_once(':')?;
    if end.contains(':') {
        return None;
    }
    let (start_column, start_row) = spreadsheet_cell_coordinates(start)?;
    let (end_column, end_row) = spreadsheet_cell_coordinates(end)?;
    if start_column > end_column || start_row > end_row {
        return None;
    }
    Some(reference.to_owned())
}

fn spreadsheet_row_visibility_marker(
    tag: &XmlTag,
    rows_hidden_by_default: bool,
) -> Option<Option<String>> {
    let hidden = ooxml_boolean(tag.attribute("hidden"), false)?;
    // SpreadsheetML uses zeroHeight as an optimization: rows are hidden unless a row
    // record makes that exact row visible again. Preserve both sides of that exception.
    if !hidden && !rows_hidden_by_default {
        return Some(None);
    }
    let row = tag.attribute("r")?;
    if row.is_empty()
        || row.len() > 7
        || row.starts_with('0')
        || !row.bytes().all(|byte| byte.is_ascii_digit())
        || !row.parse::<u32>().is_ok_and(|row| row <= 1_048_576)
    {
        return None;
    }
    Some(Some(format!(
        "<row {row} visibility: {}>",
        if hidden { "hidden" } else { "visible" }
    )))
}

fn spreadsheet_rows_hidden_by_default(tag: &XmlTag) -> Option<bool> {
    ooxml_boolean(tag.attribute("zeroHeight"), false)
}

#[derive(Default)]
struct SpreadsheetCell {
    reference: String,
    kind: String,
    formula: Option<String>,
    formula_attributes: Vec<(String, String)>,
    value: Option<String>,
    inline: String,
    inline_string_container_seen: bool,
}

fn spreadsheet_cell_line(cell: SpreadsheetCell, shared: &[String]) -> Option<Option<String>> {
    let stored_value_is_present =
        cell.value.is_some() || (cell.kind == "inlineStr" && cell.inline_string_container_seen);
    let cached_result_is_present = stored_value_is_present;
    let raw_value = cell.value.as_deref().unwrap_or("");
    let value = match cell.kind.as_str() {
        "s" => shared.get(raw_value.parse::<usize>().ok()?)?.clone(),
        "inlineStr" => cell.inline,
        "b" => match raw_value {
            "0" => "FALSE".to_owned(),
            "1" => "TRUE".to_owned(),
            _ => return None,
        },
        "e" => format!("error {raw_value}"),
        "n" | "str" | "d" | "" => raw_value.to_owned(),
        _ => return None,
    };
    if let Some(formula) = cell.formula {
        let formula = spreadsheet_field(&formula)?;
        let mut fields = vec![
            cell.reference,
            "formula".to_owned(),
            "expression".to_owned(),
        ];
        if formula.is_empty() {
            fields.push("empty".to_owned());
        } else {
            fields.push("present".to_owned());
            fields.push(formula);
        }
        fields.push("attributes".to_owned());
        fields.push(cell.formula_attributes.len().to_string());
        for (name, value) in cell.formula_attributes {
            fields.push(spreadsheet_field(&name)?);
            fields.push(spreadsheet_field(&value)?);
        }
        fields.push("result".to_owned());
        if cached_result_is_present {
            fields.push("cached".to_owned());
            fields.push(
                match cell.kind.as_str() {
                    "s" | "inlineStr" | "str" => "text",
                    "b" => "boolean",
                    "e" => "error",
                    "d" => "date",
                    "n" | "" => "number",
                    _ => return None,
                }
                .to_owned(),
            );
            fields.push(spreadsheet_field(&value)?);
        } else {
            fields.push("missing".to_owned());
        }
        return Some(Some(fields.join("\t")));
    }
    let label = match cell.kind.as_str() {
        "s" | "inlineStr" | "str" => "text",
        "b" => "boolean",
        "e" => "error",
        "d" => "date",
        "n" | "" => "number",
        _ => return None,
    };
    if value.is_empty() {
        return if stored_value_is_present {
            // Field boundaries are tabs and literal tabs in cell text are escaped, so this
            // structural suffix cannot collide with a user-authored non-empty value. Cells with
            // only formatting remain outside this content channel.
            Some(Some(format!("{}\t{}\tvalue\tempty", cell.reference, label)))
        } else {
            Some(None)
        };
    }
    Some(Some(format!(
        "{}\t{}\t{}",
        cell.reference,
        label,
        spreadsheet_field(&value)?
    )))
}

fn spreadsheet_cells(xml: &[u8], shared: &[String], maximum: usize) -> Option<(Vec<String>, bool)> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Capture {
        Formula,
        Value,
        Inline,
    }
    let mut cell = None::<SpreadsheetCell>;
    let mut capture = None::<Capture>;
    let mut lines = Vec::new();
    let mut truncated = false;
    let mut path = Vec::new();
    let mut merge_cells_seen = false;
    let mut merge_count = None::<usize>;
    let mut merged_ranges = std::collections::BTreeSet::new();
    let mut paired_merge_cell = false;
    let mut sheet_format_seen = false;
    let mut paired_sheet_format = false;
    let mut rows_hidden_by_default = false;
    let mut sheet_data_seen = false;
    for event in bounded_xml_events(xml)? {
        match event {
            XmlEvent::Start(tag) => {
                let local = tag.local_name().to_owned();
                if paired_merge_cell || paired_sheet_format {
                    return None;
                }
                if path.is_empty() && local != "worksheet" {
                    return None;
                }
                if local == "sheetFormatPr" {
                    if path.as_slice() != ["worksheet"]
                        || sheet_format_seen
                        || sheet_data_seen
                        || cell.is_some()
                    {
                        return None;
                    }
                    sheet_format_seen = true;
                    rows_hidden_by_default = spreadsheet_rows_hidden_by_default(&tag)?;
                    if rows_hidden_by_default {
                        if lines.len() < maximum {
                            lines.push("<default row visibility: hidden>".to_owned());
                        } else {
                            truncated = true;
                        }
                    }
                    paired_sheet_format = true;
                } else if local == "sheetData" {
                    if path.as_slice() != ["worksheet"] || sheet_data_seen {
                        return None;
                    }
                    sheet_data_seen = true;
                } else if local == "mergeCells" {
                    if path.as_slice() != ["worksheet"] || merge_cells_seen || cell.is_some() {
                        return None;
                    }
                    merge_cells_seen = true;
                    merge_count = spreadsheet_merge_count(&tag)?;
                } else if local == "mergeCell" {
                    if path.as_slice() != ["worksheet", "mergeCells"]
                        || merged_ranges.len() == 4_096
                    {
                        return None;
                    }
                    let range = spreadsheet_merged_range(&tag)?;
                    merged_ranges.insert(range).then_some(())?;
                    paired_merge_cell = true;
                } else if path.last().map(String::as_str) == Some("mergeCells") {
                    return None;
                } else if local == "row" {
                    if path.as_slice() != ["worksheet", "sheetData"] {
                        return None;
                    }
                    if let Some(marker) =
                        spreadsheet_row_visibility_marker(&tag, rows_hidden_by_default)?
                    {
                        if lines.len() < maximum {
                            lines.push(marker);
                        } else {
                            truncated = true;
                        }
                    }
                } else if local == "c" {
                    if path.as_slice() != ["worksheet", "sheetData", "row"] || cell.is_some() {
                        return None;
                    }
                    cell = Some(new_spreadsheet_cell(&tag)?);
                } else if local == "f" && cell.is_some() {
                    if path.last().map(String::as_str) != Some("c") {
                        return None;
                    }
                    if capture.is_some() {
                        return None;
                    }
                    begin_spreadsheet_formula(cell.as_mut()?, tag)?;
                    capture = Some(Capture::Formula);
                } else if local == "v" && cell.is_some() {
                    if path.last().map(String::as_str) != Some("c")
                        || capture.is_some()
                        || cell.as_ref()?.value.is_some()
                    {
                        return None;
                    }
                    cell.as_mut()?.value = Some(String::new());
                    capture = Some(Capture::Value);
                } else if local == "is" && cell.is_some() {
                    let current = cell.as_mut()?;
                    if path.last().map(String::as_str) != Some("c")
                        || current.kind != "inlineStr"
                        || current.inline_string_container_seen
                        || capture.is_some()
                    {
                        return None;
                    }
                    current.inline_string_container_seen = true;
                } else if local == "t" && cell.is_some() {
                    let inline_parent = path.last().map(String::as_str) == Some("is")
                        || (path.last().map(String::as_str) == Some("r")
                            && path.iter().rev().nth(1).map(String::as_str) == Some("is"));
                    if !inline_parent || capture.is_some() {
                        return None;
                    }
                    capture = Some(Capture::Inline);
                } else if capture.is_some() {
                    return None;
                }
                path.push(local);
            }
            XmlEvent::Empty(tag) if tag.local_name() == "sheetFormatPr" => {
                if paired_sheet_format
                    || path.as_slice() != ["worksheet"]
                    || sheet_format_seen
                    || sheet_data_seen
                    || cell.is_some()
                {
                    return None;
                }
                sheet_format_seen = true;
                rows_hidden_by_default = spreadsheet_rows_hidden_by_default(&tag)?;
                if rows_hidden_by_default {
                    if lines.len() < maximum {
                        lines.push("<default row visibility: hidden>".to_owned());
                    } else {
                        truncated = true;
                    }
                }
            }
            XmlEvent::Empty(tag) if tag.local_name() == "sheetData" => {
                if path.as_slice() != ["worksheet"] || sheet_data_seen {
                    return None;
                }
                sheet_data_seen = true;
            }
            XmlEvent::Empty(tag) if tag.local_name() == "mergeCells" => return None,
            XmlEvent::Empty(tag) if tag.local_name() == "mergeCell" => {
                if paired_merge_cell
                    || path.as_slice() != ["worksheet", "mergeCells"]
                    || merged_ranges.len() == 4_096
                {
                    return None;
                }
                let range = spreadsheet_merged_range(&tag)?;
                merged_ranges.insert(range).then_some(())?;
            }
            XmlEvent::Empty(tag) if tag.local_name() == "row" => {
                if path.as_slice() != ["worksheet", "sheetData"] {
                    return None;
                }
                if let Some(marker) =
                    spreadsheet_row_visibility_marker(&tag, rows_hidden_by_default)?
                {
                    if lines.len() < maximum {
                        lines.push(marker);
                    } else {
                        truncated = true;
                    }
                }
            }
            XmlEvent::Empty(tag) if tag.local_name() == "c" => {
                if path.as_slice() != ["worksheet", "sheetData", "row"] || cell.is_some() {
                    return None;
                }
                new_spreadsheet_cell(&tag)?;
            }
            XmlEvent::Empty(tag) if tag.local_name() == "f" && cell.is_some() => {
                if path.last().map(String::as_str) != Some("c") {
                    return None;
                }
                if capture.is_some() {
                    return None;
                }
                begin_spreadsheet_formula(cell.as_mut()?, tag)?;
            }
            XmlEvent::Empty(tag) if tag.local_name() == "v" && cell.is_some() => {
                if path.last().map(String::as_str) != Some("c")
                    || capture.is_some()
                    || cell.as_ref()?.value.is_some()
                {
                    return None;
                }
                cell.as_mut()?.value = Some(String::new());
            }
            XmlEvent::Empty(tag) if tag.local_name() == "is" && cell.is_some() => {
                let current = cell.as_mut()?;
                if path.last().map(String::as_str) != Some("c")
                    || current.kind != "inlineStr"
                    || current.inline_string_container_seen
                    || capture.is_some()
                {
                    return None;
                }
                current.inline_string_container_seen = true;
            }
            XmlEvent::Empty(tag) if tag.local_name() == "t" && cell.is_some() => {
                let inline_parent = path.last().map(String::as_str) == Some("is")
                    || (path.last().map(String::as_str) == Some("r")
                        && path.iter().rev().nth(1).map(String::as_str) == Some("is"));
                if !inline_parent || capture.is_some() {
                    return None;
                }
            }
            XmlEvent::Empty(_)
                if capture.is_some()
                    || paired_merge_cell
                    || paired_sheet_format
                    || path.last().map(String::as_str) == Some("mergeCells") =>
            {
                return None;
            }
            XmlEvent::Empty(_) => {}
            XmlEvent::Text(_) if paired_merge_cell || paired_sheet_format => return None,
            XmlEvent::Text(text) if cell.is_some() => match capture {
                Some(Capture::Formula) => cell.as_mut()?.formula.as_mut()?.push_str(&text),
                Some(Capture::Value) => cell.as_mut()?.value.as_mut()?.push_str(&text),
                Some(Capture::Inline) => cell.as_mut()?.inline.push_str(&text),
                None if !text.trim().is_empty() => return None,
                None => {}
            },
            XmlEvent::End(name) => {
                let local = name
                    .rsplit_once(':')
                    .map_or(name.as_str(), |(_, name)| name);
                if paired_sheet_format {
                    if local != "sheetFormatPr"
                        || path.last().map(String::as_str) != Some("sheetFormatPr")
                    {
                        return None;
                    }
                    paired_sheet_format = false;
                } else if paired_merge_cell {
                    if local != "mergeCell" || path.last().map(String::as_str) != Some("mergeCell")
                    {
                        return None;
                    }
                    paired_merge_cell = false;
                } else if local == "mergeCells" {
                    if path.as_slice() != ["worksheet", "mergeCells"]
                        || merged_ranges.is_empty()
                        || merge_count.is_some_and(|count| count != merged_ranges.len())
                    {
                        return None;
                    }
                } else if matches!(local, "f" | "v" | "t") && cell.is_some() {
                    let expected = match local {
                        "f" => Capture::Formula,
                        "v" => Capture::Value,
                        "t" => Capture::Inline,
                        _ => unreachable!(),
                    };
                    if capture != Some(expected) {
                        return None;
                    }
                    capture = None;
                } else if local == "c" {
                    if capture.is_some() || path.last().map(String::as_str) != Some("c") {
                        return None;
                    }
                    if let Some(line) = spreadsheet_cell_line(cell.take()?, shared)? {
                        if lines.len() < maximum {
                            lines.push(line);
                        } else {
                            truncated = true;
                        }
                    }
                }
                path.pop()?;
            }
            XmlEvent::Text(text) if !text.trim().is_empty() => return None,
            _ => {}
        }
    }
    if !path.is_empty()
        || cell.is_some()
        || capture.is_some()
        || paired_merge_cell
        || paired_sheet_format
    {
        return None;
    }
    for range in merged_ranges {
        if lines.len() < maximum {
            lines.push(format!("<merged cells: {range}>"));
        } else {
            truncated = true;
        }
    }
    Some((lines, truncated))
}

fn spreadsheet_section_lines(
    mut cells: Vec<String>,
    visibility: SpreadsheetSheetVisibility,
) -> Vec<String> {
    if cells.is_empty() {
        cells.push("<no cells or formulas>".to_owned());
    }
    if let Some(marker) = visibility.review_marker() {
        cells.insert(0, marker.to_owned());
    }
    cells
}

fn new_spreadsheet_cell(tag: &XmlTag) -> Option<SpreadsheetCell> {
    let reference = tag.attribute("r")?.to_owned();
    if !spreadsheet_cell_reference_is_valid(&reference) {
        return None;
    }
    Some(SpreadsheetCell {
        reference,
        kind: tag.attribute("t").unwrap_or("").to_owned(),
        ..SpreadsheetCell::default()
    })
}

fn begin_spreadsheet_formula(current: &mut SpreadsheetCell, tag: XmlTag) -> Option<()> {
    if current.formula.is_some() {
        return None;
    }
    current.formula = Some(String::new());
    current.formula_attributes = tag
        .attributes
        .into_iter()
        .map(|(name, value)| {
            (
                name.rsplit_once(':')
                    .map_or(name.as_str(), |(_, name)| name)
                    .to_owned(),
                value,
            )
        })
        .collect();
    current.formula_attributes.sort();
    Some(())
}

fn document_style_is_heading(style: &str) -> bool {
    let style = style.to_ascii_lowercase();
    style == "title" || style == "subtitle" || style.starts_with("heading")
}

fn push_document_text(target: &mut String, text: &str) {
    // Hidden-run markers share the human-readable extracted-text channel with document content.
    // Escape their delimiters (and the escape character itself) in every literal run so visible
    // text can never impersonate a visibility marker or collide with a hidden version.
    for character in text.chars() {
        if matches!(character, '\\' | '<' | '>') {
            target.push('\\');
        }
        target.push(character);
    }
}

fn xml_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\r' | '\n')
}

fn document_text_preserves_space(tag: &XmlTag) -> Option<bool> {
    match tag.qualified_attribute("xml:space") {
        None | Some("default") => Some(false),
        Some("preserve") => Some(true),
        Some(_) => None,
    }
}

fn document_review_line(value: &str) -> Option<String> {
    fn push_boundary(target: &mut String, character: char) {
        target.push_str(match character {
            ' ' => "\\s",
            '\t' => "\\t",
            '\r' => "\\r",
            '\n' => "\\n",
            _ => unreachable!(),
        });
    }

    let Some(first_content) = value.find(|character| !xml_whitespace(character)) else {
        let mut line = String::with_capacity(value.len().saturating_mul(2));
        for character in value.chars() {
            push_boundary(&mut line, character);
        }
        return Some(line);
    };
    let last_content = value
        .char_indices()
        .rev()
        .find(|(_, character)| !xml_whitespace(*character))
        .map(|(index, character)| index + character.len_utf8())?;
    let mut line = String::with_capacity(value.len().saturating_add(first_content));
    for character in value[..first_content].chars() {
        push_boundary(&mut line, character);
    }
    line.push_str(&value[first_content..last_content]);
    for character in value[last_content..].chars() {
        push_boundary(&mut line, character);
    }
    Some(line)
}

fn push_document_break(
    paragraph: &mut String,
    run_hidden: bool,
    hidden_marker_open: &mut bool,
    kind: &str,
) {
    if run_hidden && !*hidden_marker_open {
        paragraph.push_str("<hidden text: ");
        *hidden_marker_open = true;
    }
    // Keep structural controls human-readable and injective inside the extracted review line.
    // Literal backslashes from `<w:t>` are already doubled by `push_document_text`, so visible
    // text such as `\\n` cannot impersonate a line break and a preserved literal tab cannot
    // impersonate `<w:tab/>`. Consecutive breaks remain review-significant rather than collapsing.
    paragraph.push_str(if kind == "tab" { "\\t" } else { "\\n" });
}

fn document_blocks(xml: &[u8]) -> Option<ArtifactText> {
    let mut path = Vec::new();
    let mut paragraph = None::<String>;
    let mut paragraph_style = None::<String>;
    let mut run_hidden = None::<bool>;
    let mut run_visibility_seen = false;
    let mut run_content_seen = false;
    let mut hidden_marker_open = false;
    let mut in_text = false;
    let mut text_preserves_space = false;
    let mut text_value = String::new();
    let mut paired_break = None::<String>;
    let mut lines = Vec::new();
    let mut section_starts = Vec::new();
    let mut characters = 0_usize;
    let mut truncated = false;
    let mut heading_number = 0_usize;
    for event in bounded_xml_events(xml)? {
        match event {
            XmlEvent::Start(tag) => {
                let local = tag.local_name().to_owned();
                if paired_break.is_some() {
                    return None;
                }
                if path.is_empty() && local != "document" {
                    return None;
                }
                if local == "p" {
                    if paragraph.is_some() || !path.iter().any(|part| part == "body") {
                        return None;
                    }
                    paragraph = Some(String::new());
                    paragraph_style = None;
                } else if local == "r" && paragraph.is_some() {
                    if run_hidden.is_some() || in_text {
                        return None;
                    }
                    run_hidden = Some(false);
                    run_visibility_seen = false;
                    run_content_seen = false;
                    hidden_marker_open = false;
                } else if local == "vanish" && paragraph.is_some() {
                    // `w:vanish` is a leaf on/off property. A paired or misplaced form is not a
                    // visibility fact this bounded extractor can interpret safely.
                    return None;
                } else if local == "t" && paragraph.is_some() {
                    if in_text
                        || run_hidden.is_none()
                        || path.last().map(String::as_str) != Some("r")
                    {
                        return None;
                    }
                    in_text = true;
                    text_preserves_space = document_text_preserves_space(&tag)?;
                    text_value.clear();
                } else if matches!(local.as_str(), "tab" | "br" | "cr") && paragraph.is_some() {
                    // Empty OOXML elements may use either `<w:br/>` or the equivalent paired
                    // `<w:br></w:br>` serialization. Record the separator now and require the
                    // matching close without admitting nested or text content into this parser.
                    if in_text || run_hidden.is_none() {
                        return None;
                    }
                    run_content_seen = true;
                    push_document_break(
                        paragraph.as_mut()?,
                        run_hidden == Some(true),
                        &mut hidden_marker_open,
                        &local,
                    );
                    paired_break = Some(local.clone());
                }
                path.push(local);
            }
            XmlEvent::Empty(_) if paired_break.is_some() => return None,
            XmlEvent::Empty(tag) if tag.local_name() == "pStyle" && paragraph.is_some() => {
                if path.last().map(String::as_str) != Some("pPr") || paragraph_style.is_some() {
                    return None;
                }
                paragraph_style = Some(tag.attribute("val")?.to_owned());
            }
            XmlEvent::Empty(tag) if tag.local_name() == "vanish" => {
                if paragraph.is_none()
                    || run_hidden.is_none()
                    || run_visibility_seen
                    || run_content_seen
                    || path.len() < 2
                    || path[path.len() - 2] != "r"
                    || path[path.len() - 1] != "rPr"
                {
                    return None;
                }
                run_hidden = Some(ooxml_boolean(tag.attribute("val"), true)?);
                run_visibility_seen = true;
            }
            XmlEvent::Empty(tag)
                if matches!(tag.local_name(), "tab" | "br" | "cr") && paragraph.is_some() =>
            {
                run_hidden?;
                run_content_seen = true;
                push_document_break(
                    paragraph.as_mut()?,
                    run_hidden == Some(true),
                    &mut hidden_marker_open,
                    tag.local_name(),
                );
            }
            XmlEvent::Empty(_) => {}
            XmlEvent::Text(_) if paired_break.is_some() => return None,
            XmlEvent::Text(text) if in_text => text_value.push_str(&text),
            XmlEvent::Text(_) => {}
            XmlEvent::End(name) => {
                let local = name
                    .rsplit_once(':')
                    .map_or(name.as_str(), |(_, name)| name);
                if let Some(expected) = paired_break.as_deref() {
                    if local != expected || path.last().map(String::as_str) != Some(expected) {
                        return None;
                    }
                    paired_break = None;
                } else if local == "t" && paragraph.is_some() {
                    if !in_text {
                        return None;
                    }
                    let value = if text_preserves_space {
                        text_value.as_str()
                    } else {
                        text_value.trim_matches(xml_whitespace)
                    };
                    if !value.is_empty() {
                        run_content_seen = true;
                        let paragraph = paragraph.as_mut()?;
                        if run_hidden == Some(true) && !hidden_marker_open {
                            paragraph.push_str("<hidden text: ");
                            hidden_marker_open = true;
                        }
                        push_document_text(paragraph, value);
                    }
                    in_text = false;
                    text_preserves_space = false;
                    text_value.clear();
                } else if local == "r" && paragraph.is_some() {
                    if in_text
                        || run_hidden.is_none()
                        || path.last().map(String::as_str) != Some("r")
                    {
                        return None;
                    }
                    if hidden_marker_open {
                        paragraph.as_mut()?.push('>');
                    }
                    run_hidden = None;
                    run_visibility_seen = false;
                    run_content_seen = false;
                    hidden_marker_open = false;
                } else if local == "p" {
                    if in_text
                        || run_hidden.is_some()
                        || path.last().map(String::as_str) != Some("p")
                    {
                        return None;
                    }
                    let text = document_review_line(&paragraph.take()?)?;
                    let heading = paragraph_style
                        .take()
                        .as_deref()
                        .is_some_and(document_style_is_heading);
                    if !text.is_empty() {
                        if text.len() > 8 * 1024
                            || lines.len() == 512
                            || characters.saturating_add(text.len()) > 128 * 1024
                        {
                            truncated = true;
                        } else {
                            if heading {
                                if section_starts.is_empty() && !lines.is_empty() {
                                    section_starts.push((0, "Document opening".to_owned()));
                                }
                                if section_starts.len() == 64 {
                                    truncated = true;
                                } else {
                                    heading_number += 1;
                                    let mut label = text.chars().take(56).collect::<String>();
                                    if label.chars().count() < text.chars().count() {
                                        label.push('…');
                                    }
                                    section_starts.push((
                                        lines.len(),
                                        format!("Section {heading_number} · {label}"),
                                    ));
                                }
                            }
                            if !truncated {
                                characters += text.len();
                                lines.push(text);
                            }
                        }
                    }
                }
                path.pop()?;
            }
        }
        if truncated {
            break;
        }
    }
    if lines.is_empty()
        || in_text
        || paragraph.is_some()
        || run_hidden.is_some()
        || paired_break.is_some()
        || !text_value.is_empty()
        || (!truncated && !path.is_empty())
    {
        return None;
    }
    let sections = artifact_text_sections(ArtifactKind::Document, lines.len(), section_starts);
    Some(ArtifactText {
        source: ArtifactTextSource::DocumentBlocks,
        lines,
        sections,
        truncated,
    })
}

#[cfg(target_os = "macos")]
fn extract_document_text(input: &std::path::Path, archive: &[u8]) -> Option<ArtifactText> {
    let document = unzip_entry_bounded(input, archive, "word/document.xml", 4 * 1024 * 1024)?;
    document_blocks(&document)
}

#[cfg(target_os = "macos")]
fn extract_presentation_text(input: &std::path::Path, archive: &[u8]) -> Option<ArtifactText> {
    let presentation = unzip_entry_bounded(input, archive, "ppt/presentation.xml", 256 * 1024)?;
    let relationships = unzip_entry_bounded(
        input,
        archive,
        "ppt/_rels/presentation.xml.rels",
        256 * 1024,
    )?;
    let slides = presentation_slides(&presentation, &relationships)?;
    let mut lines = Vec::new();
    let mut sections = Vec::new();
    let mut characters = 0_usize;
    let mut truncated = false;
    for (index, path) in slides.into_iter().enumerate() {
        if lines.len() == 512 {
            truncated = true;
            break;
        }
        let xml = unzip_entry_bounded(input, archive, &path, 4 * 1024 * 1024)?;
        let (label, slide_lines) = presentation_slide_text(&xml, index + 1)?;
        let start = lines.len();
        for line in slide_lines {
            if lines.len() == 512 || characters.saturating_add(line.len()) > 128 * 1024 {
                truncated = true;
                break;
            }
            characters += line.len();
            lines.push(line);
        }
        if lines.len() > start {
            sections.push(ArtifactTextSection {
                label,
                line_start: start,
                line_count: lines.len() - start,
            });
        }
        if truncated {
            break;
        }
    }
    if lines.is_empty() || sections.is_empty() {
        return None;
    }
    Some(ArtifactText {
        source: ArtifactTextSource::PresentationSlides,
        lines,
        sections,
        truncated,
    })
}

#[cfg(target_os = "macos")]
fn extract_spreadsheet_text(input: &std::path::Path, archive: &[u8]) -> Option<ArtifactText> {
    let workbook = unzip_entry_bounded(input, archive, "xl/workbook.xml", 256 * 1024)?;
    let relationships =
        unzip_entry_bounded(input, archive, "xl/_rels/workbook.xml.rels", 256 * 1024)?;
    let sheets = spreadsheet_sheets(&workbook, &relationships)?;
    let shared_xml = if zip_central_entries(archive)?
        .iter()
        .any(|entry| entry.name == b"xl/sharedStrings.xml")
    {
        Some(unzip_entry_bounded(
            input,
            archive,
            "xl/sharedStrings.xml",
            1024 * 1024,
        )?)
    } else {
        None
    };
    let shared = spreadsheet_shared_strings(shared_xml.as_deref())?;
    let mut lines = Vec::new();
    let mut sections = Vec::new();
    let mut characters = 0_usize;
    let mut truncated = false;
    for sheet in sheets {
        let xml = unzip_entry_bounded(input, archive, &sheet.path, 4 * 1024 * 1024)?;
        let remaining = 512_usize.saturating_sub(lines.len());
        if remaining == 0 {
            truncated = true;
            break;
        }
        let (cells, sheet_truncated) = spreadsheet_cells(&xml, &shared, remaining)?;
        let start = lines.len();
        for line in spreadsheet_section_lines(cells, sheet.visibility) {
            if lines.len() == 512 || characters.saturating_add(line.len()) > 128 * 1024 {
                truncated = true;
                break;
            }
            characters += line.len();
            lines.push(line);
        }
        if lines.len() > start {
            sections.push(ArtifactTextSection {
                label: sheet.name,
                line_start: start,
                line_count: lines.len() - start,
            });
        }
        truncated |= sheet_truncated;
        if truncated {
            break;
        }
    }
    if lines.is_empty() || sections.is_empty() {
        return None;
    }
    Some(ArtifactText {
        source: ArtifactTextSource::SpreadsheetCells,
        lines,
        sections,
        truncated,
    })
}

#[cfg(target_os = "macos")]
fn quick_look_succeeds(input: &std::path::Path, root: &std::path::Path, thumbnail: bool) -> bool {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut command = Command::new("/usr/bin/qlmanage");
    if thumbnail {
        command.args(["-t", "-s", "1200", "-o"]);
    } else {
        command.args(["-p", "-o"]);
    }
    let Ok(mut child) = command
        .arg(root)
        .arg(input)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Err(_) => return false,
        }
    }
}

#[cfg(target_os = "macos")]
fn render_image_io_thumbnail(input: &std::path::Path, output: &std::path::Path) -> Option<Vec<u8>> {
    use std::fs;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    // `sips` is the fixed macOS ImageIO front end. Unlike Quick Look it decodes raster bytes
    // directly, does not depend on a Finder/Quick Look service being available, and `-Z` bounds
    // the representative image to 1,200 pixels on its longest edge.
    let mut child = Command::new("/usr/bin/sips")
        .args(["-s", "format", "png", "-Z", "1200"])
        .arg(input)
        .arg("--out")
        .arg(output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(12);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    };
    if !status.success() {
        return None;
    }
    let metadata = fs::symlink_metadata(output).ok()?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_PREVIEW_BYTES
    {
        return None;
    }
    let png = fs::read(output).ok()?;
    png.starts_with(b"\x89PNG\r\n\x1a\n").then_some(png)
}

#[cfg(target_os = "macos")]
const PDF_PAGE_RENDERER: &str = r#"ObjC.import('Foundation');
ObjC.import('AppKit');
ObjC.import('PDFKit');

function run(argv) {
  if (argv.length !== 3) throw new Error('expected input, output prefix, and page number');
  const input = $(argv[0]);
  const output = $(argv[1]);
  const requested = Number(argv[2]);
  if (!Number.isSafeInteger(requested) || requested < 1) throw new Error('invalid page');
  const document = $.PDFDocument.alloc.initWithURL($.NSURL.fileURLWithPath(input));
  if (!document || document.isLocked) throw new Error('invalid or locked PDF');
  const count = Number(document.pageCount);
  if (!Number.isSafeInteger(count) || count < 1 || count > 1000000
      || requested > 64 || requested > count) {
    throw new Error('page outside bounded document');
  }
  const page = document.pageAtIndex(requested - 1);
  if (!page) throw new Error('missing page');
  const image = page.thumbnailOfSizeForBox($.NSMakeSize(1200, 1200), $.kPDFDisplayBoxMediaBox);
  if (!image) throw new Error('render failed');
  const bitmap = $.NSBitmapImageRep.imageRepWithData(image.TIFFRepresentation);
  if (!bitmap) throw new Error('bitmap failed');
  const png = bitmap.representationUsingTypeProperties($.NSBitmapImageFileTypePNG, $({}));
  if (!png || !png.writeToFileAtomically(output.stringByAppendingString('.png'), true)) {
    throw new Error('write PNG failed');
  }
  const rawText = page.string || $('');
  const truncated = Number(rawText.length) > 32768;
  const text = truncated ? rawText.substringToIndex(32768) : rawText;
  if (!text.dataUsingEncoding($.NSUTF8StringEncoding)
      .writeToFileAtomically(output.stringByAppendingString('.txt'), true)) {
    throw new Error('write text failed');
  }
  return `${count}\t${truncated ? 1 : 0}`;
}
"#;

#[cfg(target_os = "macos")]
fn render_pdf_page(
    input: &std::path::Path,
    root: &std::path::Path,
    requested: usize,
) -> Option<(Vec<u8>, ArtifactText, usize)> {
    use std::fs::{self, OpenOptions};
    use std::io::{Read as _, Write as _};
    use std::os::unix::fs::OpenOptionsExt as _;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    if requested == 0 || requested > MAX_PDF_PREVIEW_PAGE {
        return None;
    }
    let script = root.join("pdf-page-renderer.js");
    let mut script_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&script)
        .ok()?;
    script_file.write_all(PDF_PAGE_RENDERER.as_bytes()).ok()?;
    script_file.sync_all().ok()?;
    drop(script_file);

    let output = root.join("page");
    let mut child = Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript"])
        .arg(&script)
        .arg(input)
        .arg(&output)
        .arg(requested.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(12);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    };
    if !status.success() {
        return None;
    }
    let mut report = String::new();
    child
        .stdout
        .take()?
        .take(64)
        .read_to_string(&mut report)
        .ok()?;
    let (count, helper_truncated) = report.trim().split_once('\t')?;
    let page_count = count.parse::<usize>().ok()?;
    if !(1..=MAX_PDF_DOCUMENT_PAGES).contains(&page_count)
        || requested > MAX_PDF_PREVIEW_PAGE
        || requested > page_count
    {
        return None;
    }
    let helper_truncated = match helper_truncated {
        "0" => false,
        "1" => true,
        _ => return None,
    };

    let png_path = output.with_extension("png");
    let text_path = output.with_extension("txt");
    let png_metadata = fs::symlink_metadata(&png_path).ok()?;
    let text_metadata = fs::symlink_metadata(&text_path).ok()?;
    if !png_metadata.is_file()
        || png_metadata.file_type().is_symlink()
        || png_metadata.len() > MAX_PREVIEW_BYTES
        || !text_metadata.is_file()
        || text_metadata.file_type().is_symlink()
        || text_metadata.len() > 128 * 1024
    {
        return None;
    }
    let png = fs::read(png_path).ok()?;
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let raw_text = fs::read_to_string(text_path).ok()?;
    let mut lines = Vec::new();
    let mut characters = 0_usize;
    let mut truncated = helper_truncated;
    for raw_line in raw_text.lines() {
        let mut line = String::new();
        let mut pending_space = false;
        for character in raw_line.chars() {
            if artifact_text_character_is_unsafe(character) {
                return None;
            }
            if character.is_whitespace() {
                pending_space = !line.is_empty();
            } else {
                if pending_space {
                    line.push(' ');
                }
                line.push(character);
                pending_space = false;
            }
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if lines.len() == 512 || characters.saturating_add(line.len()) > 128 * 1024 {
            truncated = true;
            break;
        }
        characters += line.len();
        lines.push(line.to_owned());
    }
    if lines.is_empty() {
        lines.push("<no page text>".to_owned());
    }
    let line_count = lines.len();
    Some((
        png,
        ArtifactText {
            source: ArtifactTextSource::PdfPageText,
            lines,
            sections: vec![ArtifactTextSection {
                label: format!("Page {requested}"),
                line_start: 0,
                line_count,
            }],
            truncated,
        },
        page_count,
    ))
}

#[cfg(target_os = "macos")]
fn render_platform(
    kind: ArtifactKind,
    bytes: &[u8],
    page_number: Option<usize>,
) -> Result<PlatformPreview, ArtifactPreviewError> {
    use std::fs::{self, OpenOptions};
    use std::io::Write as _;
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "mesh-artifact-preview-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).map_err(|_| ArtifactPreviewError::Unavailable)?;
    let cleanup = || {
        let _ = fs::remove_dir_all(&root);
    };
    if fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).is_err() {
        cleanup();
        return Err(ArtifactPreviewError::Unavailable);
    }
    let input = root.join(format!("review.{}", kind.extension()));
    let mut file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&input)
    {
        Ok(file) => file,
        Err(_) => {
            cleanup();
            return Err(ArtifactPreviewError::Unavailable);
        }
    };
    if file.write_all(bytes).is_err() || file.sync_all().is_err() {
        cleanup();
        return Err(ArtifactPreviewError::Unavailable);
    }
    drop(file);

    if kind == ArtifactKind::Pdf {
        let requested = page_number.unwrap_or(1);
        let Some((png, text, page_count)) = render_pdf_page(&input, &root, requested) else {
            cleanup();
            return Err(if requested == 0 || requested > MAX_PDF_PREVIEW_PAGE {
                ArtifactPreviewError::InvalidPage
            } else {
                ArtifactPreviewError::Unavailable
            });
        };
        cleanup();
        return Ok(PlatformPreview {
            png,
            text: Some(text),
            renderer: ArtifactRenderer::PdfKitPage,
            page_number: Some(requested),
            page_count: Some(page_count),
        });
    }
    if page_number.is_some() {
        cleanup();
        return Err(ArtifactPreviewError::InvalidPage);
    }

    if kind.is_image() {
        let output = root.join("image-preview.png");
        let rendered = render_image_io_thumbnail(&input, &output);
        cleanup();
        return rendered
            .map(|png| PlatformPreview {
                png,
                text: None,
                renderer: ArtifactRenderer::ImageIoThumbnail,
                page_number: None,
                page_count: None,
            })
            .ok_or(ArtifactPreviewError::Unavailable);
    }

    if !quick_look_succeeds(&input, &root, true) {
        cleanup();
        return Err(ArtifactPreviewError::Unavailable);
    }

    let output = root.join(format!("review.{}.png", kind.extension()));
    let metadata = match fs::symlink_metadata(&output) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= MAX_PREVIEW_BYTES =>
        {
            metadata
        }
        _ => {
            cleanup();
            return Err(ArtifactPreviewError::Unavailable);
        }
    };
    let _ = metadata;
    let png = fs::read(&output).map_err(|_| ArtifactPreviewError::Unavailable);
    let semantic_text = match kind {
        ArtifactKind::Presentation => extract_presentation_text(&input, bytes),
        ArtifactKind::Document => extract_document_text(&input, bytes),
        ArtifactKind::Spreadsheet => extract_spreadsheet_text(&input, bytes),
        ArtifactKind::Pdf
        | ArtifactKind::Png
        | ArtifactKind::Jpeg
        | ArtifactKind::Gif
        | ArtifactKind::Webp => None,
    };
    let text = if kind.is_image() {
        None
    } else {
        semantic_text.or_else(|| {
            if quick_look_succeeds(&input, &root, false) {
                let preview = root.join(format!(
                    "review.{}.qlpreview/Preview.html",
                    kind.extension()
                ));
                match fs::symlink_metadata(&preview) {
                    Ok(metadata)
                        if metadata.is_file()
                            && !metadata.file_type().is_symlink()
                            && metadata.len() <= 4 * 1024 * 1024 =>
                    {
                        fs::read(preview)
                            .ok()
                            .and_then(|html| extract_quick_look_text(kind, &html))
                    }
                    _ => None,
                }
            } else {
                None
            }
        })
    };
    cleanup();
    let png = png?;
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(ArtifactPreviewError::Unavailable);
    }
    Ok(PlatformPreview {
        png,
        text,
        renderer: ArtifactRenderer::QuickLookThumbnail,
        page_number: None,
        page_count: None,
    })
}

#[cfg(not(target_os = "macos"))]
fn render_platform(
    _kind: ArtifactKind,
    _bytes: &[u8],
    _page_number: Option<usize>,
) -> Result<PlatformPreview, ArtifactPreviewError> {
    Err(ArtifactPreviewError::Unavailable)
}

pub(crate) fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        encoded.push(ALPHABET[((value >> 18) & 63) as usize] as char);
        encoded.push(ALPHABET[((value >> 12) & 63) as usize] as char);
        encoded.push(if chunk.len() > 1 {
            ALPHABET[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            ALPHABET[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_zip(entries: &[&str]) -> Vec<u8> {
        let mut archive = Vec::new();
        let mut offsets = Vec::new();
        for name in entries {
            offsets.push(archive.len());
            archive.extend_from_slice(b"PK\x03\x04");
            archive.extend_from_slice(&20_u16.to_le_bytes());
            archive.extend_from_slice(&[0; 22]);
            archive.extend_from_slice(&(name.len() as u16).to_le_bytes());
            archive.extend_from_slice(&0_u16.to_le_bytes());
            archive.extend_from_slice(name.as_bytes());
        }
        let central_offset = archive.len();
        for (name, offset) in entries.iter().zip(offsets) {
            archive.extend_from_slice(b"PK\x01\x02");
            archive.extend_from_slice(&20_u16.to_le_bytes());
            archive.extend_from_slice(&20_u16.to_le_bytes());
            archive.extend_from_slice(&[0; 20]);
            archive.extend_from_slice(&(name.len() as u16).to_le_bytes());
            archive.extend_from_slice(&0_u16.to_le_bytes());
            archive.extend_from_slice(&0_u16.to_le_bytes());
            archive.extend_from_slice(&0_u16.to_le_bytes());
            archive.extend_from_slice(&0_u16.to_le_bytes());
            archive.extend_from_slice(&0_u32.to_le_bytes());
            archive.extend_from_slice(&(offset as u32).to_le_bytes());
            archive.extend_from_slice(name.as_bytes());
        }
        let central_size = archive.len() - central_offset;
        archive.extend_from_slice(b"PK\x05\x06");
        archive.extend_from_slice(&0_u16.to_le_bytes());
        archive.extend_from_slice(&0_u16.to_le_bytes());
        archive.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        archive.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        archive.extend_from_slice(&(central_size as u32).to_le_bytes());
        archive.extend_from_slice(&(central_offset as u32).to_le_bytes());
        archive.extend_from_slice(&0_u16.to_le_bytes());
        archive
    }

    fn pdf_with_pages(page_count: usize) -> Vec<u8> {
        assert!((1..=128).contains(&page_count));
        let font_id = page_count + 3;
        let kids = (0..page_count)
            .map(|index| format!("{} 0 R", index + 3))
            .collect::<Vec<_>>()
            .join(" ");
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            format!("<< /Type /Pages /Kids [{kids}] /Count {page_count} >>").into_bytes(),
        ];
        for index in 0..page_count {
            let content_id = page_count + 4 + index;
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 144] /Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {content_id} 0 R >>"
                )
                .into_bytes(),
            );
        }
        objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());
        for index in 0..page_count {
            let stream = format!(
                "BT /F1 24 Tf 40 80 Td (Mesh alpha page {}) Tj ET\n",
                index + 1
            );
            objects.push(
                [
                    format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes(),
                    stream.into_bytes(),
                    b"endstream".to_vec(),
                ]
                .concat(),
            );
        }
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    fn one_page_pdf() -> Vec<u8> {
        pdf_with_pages(1)
    }

    #[test]
    fn word_blocks_preserve_heading_paragraph_and_table_order_without_executing_fields() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="word"><w:body>
  <w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>People plan</w:t></w:r></w:p>
  <w:p><w:r><w:t>Approve</w:t><w:tab/><w:t>three hires</w:t></w:r></w:p>
  <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Owner</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Finance</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
  <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p>
  <w:p><w:r><w:instrText>DO NOT EXECUTE</w:instrText><w:t>Cash runway</w:t></w:r></w:p>
</w:body></w:document>"#;
        let text = document_blocks(xml).expect("bounded Word structure");
        assert_eq!(text.source, ArtifactTextSource::DocumentBlocks);
        assert_eq!(
            text.lines,
            [
                "People plan",
                r"Approve\tthree hires",
                "Owner",
                "Finance",
                "Risks",
                "Cash runway",
            ]
        );
        assert_eq!(
            text.sections
                .iter()
                .map(|section| section.label.as_str())
                .collect::<Vec<_>>(),
            ["Section 1 · People plan", "Section 2 · Risks"]
        );
        assert!(!text
            .lines
            .iter()
            .any(|line| line.contains("DO NOT EXECUTE")));
    }

    #[test]
    fn word_blocks_preserve_equivalent_empty_and_paired_break_elements() {
        let document = |separator: &str| {
            format!(
                r#"<w:document xmlns:w="word"><w:body><w:p><w:r><w:t>Approve</w:t>{separator}<w:t>three hires</w:t></w:r></w:p></w:body></w:document>"#,
            )
        };
        let absent = document_blocks(document("").as_bytes()).expect("Word text without break");
        let empty_break =
            document_blocks(document("<w:br/>").as_bytes()).expect("self-closing Word break");
        let paired_break =
            document_blocks(document("<w:br></w:br>").as_bytes()).expect("paired Word break");
        let empty_tab =
            document_blocks(document("<w:tab/>").as_bytes()).expect("self-closing Word tab");
        let paired_tab =
            document_blocks(document("<w:tab></w:tab>").as_bytes()).expect("paired Word tab");
        let empty_carriage_return = document_blocks(document("<w:cr/>").as_bytes())
            .expect("self-closing Word carriage return");
        let paired_carriage_return = document_blocks(document("<w:cr></w:cr>").as_bytes())
            .expect("paired Word carriage return");
        let double_break =
            document_blocks(document("<w:br/><w:br/>").as_bytes()).expect("two Word line breaks");
        let literal_space =
            document_blocks(document(r#"<w:t xml:space="preserve"> </w:t>"#).as_bytes())
                .expect("literal Word space");
        let literal_tab =
            document_blocks(document("<w:t xml:space=\"preserve\">&#9;</w:t>").as_bytes())
                .expect("literal Word tab");
        let literal_break_marker = document_blocks(document(r"<w:t>\n</w:t>").as_bytes())
            .expect("literal break marker text");

        assert_eq!(absent.lines, ["Approvethree hires"]);
        assert_eq!(empty_break.lines, [r"Approve\nthree hires"]);
        assert_eq!(paired_break, empty_break);
        assert_eq!(empty_tab.lines, [r"Approve\tthree hires"]);
        assert_eq!(paired_tab, empty_tab);
        assert_eq!(empty_carriage_return, empty_break);
        assert_eq!(paired_carriage_return, empty_break);
        assert_eq!(double_break.lines, [r"Approve\n\nthree hires"]);
        assert_ne!(
            empty_break, literal_space,
            "a line break must not compare as a literal space"
        );
        assert_ne!(
            empty_break, literal_break_marker,
            "literal marker text must not forge a line break"
        );
        assert_ne!(
            empty_tab, literal_tab,
            "a tab element must not collide with literal tab text"
        );
        assert_ne!(
            paired_break, absent,
            "a visible break must remain review-significant"
        );
        assert!(document_blocks(document("<w:br>forged</w:br>").as_bytes()).is_none());
        assert!(document_blocks(document("<w:br><w:t>forged</w:t></w:br>").as_bytes()).is_none());
    }

    #[test]
    fn word_text_space_semantics_are_visible_in_review_and_canonical_across_runs() {
        let document = |text: &str| {
            document_blocks(
                format!(
                    r#"<w:document xmlns:w="word"><w:body><w:p><w:r>{text}</w:r></w:p></w:body></w:document>"#,
                )
                .as_bytes(),
            )
            .expect("bounded Word text")
        };

        let plain = document("<w:t>Compensation adjustment</w:t>");
        let preserved = document(r#"<w:t xml:space="preserve"> Compensation adjustment </w:t>"#);
        assert_ne!(
            plain, preserved,
            "xml:space preserve makes boundary whitespace visible content"
        );
        assert_eq!(
            preserved.lines,
            [r"\sCompensation adjustment\s"],
            "significant boundary spaces remain legible in a text diff"
        );

        assert_eq!(
            plain,
            document("<w:t> Compensation adjustment </w:t>"),
            "unmarked run-boundary whitespace is not significant"
        );
        assert_eq!(
            plain,
            document(r#"<w:t xml:space="default"> Compensation adjustment </w:t>"#),
            "explicit default and omitted xml:space have the same semantics"
        );
        assert_eq!(
            document("<w:t>Compensation </w:t><w:t> adjustment</w:t>"),
            document("<w:t>Compensationadjustment</w:t>"),
            "default-space trimming is applied to each text element"
        );
        assert_eq!(
            document(r#"<w:t xml:space="preserve">Compensation </w:t><w:t>adjustment</w:t>"#),
            plain,
            "equivalent visible text remains equal across run serialization boundaries"
        );
        assert_ne!(
            preserved,
            document(r"<w:t>\sCompensation adjustment\s</w:t>"),
            "literal backslash escapes cannot impersonate preserved boundary spaces"
        );
        assert!(document_blocks(
            br#"<w:document xmlns:w="word"><w:body><w:p><w:r><w:t xml:space="collapse">Compensation adjustment</w:t></w:r></w:p></w:body></w:document>"#,
        )
        .is_none());
    }

    #[test]
    fn word_blocks_preserve_document_opening_before_the_first_heading() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="word"><w:body>
  <w:p><w:r><w:t>Introductory context</w:t></w:r></w:p>
  <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>People plan</w:t></w:r></w:p>
  <w:p><w:r><w:t>Approve three hires</w:t></w:r></w:p>
</w:body></w:document>"#;
        let text = document_blocks(xml).expect("bounded Word opening and heading");
        assert_eq!(
            text.lines,
            ["Introductory context", "People plan", "Approve three hires"]
        );
        assert_eq!(
            text.sections
                .iter()
                .map(|section| section.label.as_str())
                .collect::<Vec<_>>(),
            ["Document opening", "Section 1 · People plan"]
        );
    }

    #[test]
    fn word_hidden_run_visibility_is_review_significant() {
        let document = |vanish: &str| {
            format!(
                r#"<w:document xmlns:w="word"><w:body><w:p><w:r><w:rPr>{vanish}</w:rPr><w:t>Compensation adjustment</w:t></w:r></w:p></w:body></w:document>"#,
            )
        };
        let shown = document_blocks(document("").as_bytes()).expect("shown Word run");
        let shown_false = document_blocks(document(r#"<w:vanish w:val="0"/>"#).as_bytes())
            .expect("explicitly shown Word run");
        let hidden = document_blocks(document("<w:vanish/>").as_bytes()).expect("hidden Word run");
        let hidden_true = document_blocks(document(r#"<w:vanish w:val="true"/>"#).as_bytes())
            .expect("word-form hidden Word run");

        assert_eq!(shown, shown_false);
        assert_ne!(shown, hidden, "a hidden run must not compare as visible");
        assert_eq!(hidden, hidden_true);
        assert_eq!(hidden.lines, ["<hidden text: Compensation adjustment>"]);
        let visible_marker_text = document_blocks(
            document("<w:vanish w:val=\"0\"/>")
                .replace(
                    "Compensation adjustment",
                    "&lt;hidden text: Compensation adjustment&gt;",
                )
                .as_bytes(),
        )
        .expect("visible text resembling the marker");
        assert_eq!(
            visible_marker_text.lines,
            [r"\<hidden text: Compensation adjustment\>"]
        );
        assert_ne!(
            visible_marker_text, hidden,
            "visible text must not forge a hidden marker"
        );
        assert!(
            document_blocks(document(r#"<w:vanish w:val="sometimes"/>"#).as_bytes(),).is_none()
        );
        assert!(document_blocks(document("<w:vanish></w:vanish>").as_bytes(),).is_none());
    }

    #[test]
    fn word_heading_label_bound_covers_native_astral_truncation() {
        let heading = "📊".repeat(57);
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="word"><w:body>
  <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>{heading}</w:t></w:r></w:p>
</w:body></w:document>"#
        );
        let text = document_blocks(xml.as_bytes()).expect("bounded astral Word heading");
        let expected = format!("Section 1 · {}…", "📊".repeat(56));
        assert_eq!(text.sections[0].label, expected);
        let utf16_units = text.sections[0].label.encode_utf16().count();
        assert!(utf16_units <= 128);
        assert!(utf16_units > 80);
    }

    #[test]
    fn malformed_or_unbounded_word_blocks_fail_closed() {
        let malformed = br#"<w:document><w:body><w:p><w:r><w:t>open</w:p></w:body></w:document>"#;
        assert_eq!(document_blocks(malformed), None);
        let nested = br#"<w:document><w:body><w:p><w:p><w:r><w:t>nested</w:t></w:r></w:p></w:p></w:body></w:document>"#;
        assert_eq!(document_blocks(nested), None);
    }

    #[test]
    fn closed_artifact_families_and_container_signatures_are_exact() {
        assert_eq!(
            ArtifactKind::from_path("reports/Board.PDF"),
            Some(ArtifactKind::Pdf)
        );
        assert_eq!(
            ArtifactKind::from_path("people/plan.pptx"),
            Some(ArtifactKind::Presentation)
        );
        assert_eq!(
            ArtifactKind::from_path("finance/budget.xlsx"),
            Some(ArtifactKind::Spreadsheet)
        );
        assert_eq!(
            ArtifactKind::from_path("people/policy.docx"),
            Some(ArtifactKind::Document)
        );
        assert_eq!(
            ArtifactKind::from_path("assets/hero.PNG"),
            Some(ArtifactKind::Png)
        );
        assert_eq!(
            ArtifactKind::from_path("assets/photo.jpeg"),
            Some(ArtifactKind::Jpeg)
        );
        assert_eq!(ArtifactKind::from_path("people/policy.doc"), None);
        assert!(ArtifactKind::Pdf.accepts(b"%PDF-1.7\n"));
        assert!(!ArtifactKind::Pdf.accepts(b"PK\x03\x04"));
        assert!(ArtifactKind::Png.accepts(b"\x89PNG\r\n\x1a\nexact saved bytes"));
        assert!(!ArtifactKind::Png.accepts(b"spoofed image bytes"));
        assert!(ArtifactKind::Jpeg.accepts(b"\xff\xd8\xffexact saved bytes"));
        assert!(ArtifactKind::Gif.accepts(b"GIF89aexact saved bytes"));
        assert!(ArtifactKind::Webp.accepts(b"RIFF0000WEBPexact saved bytes"));
        let presentation =
            empty_zip(&["[Content_Types].xml", "_rels/.rels", "ppt/presentation.xml"]);
        assert!(ArtifactKind::Presentation.accepts(&presentation));
        assert!(!ArtifactKind::Document.accepts(&presentation));
        assert!(!ArtifactKind::Spreadsheet.accepts(&presentation));
        assert!(!ArtifactKind::Presentation.accepts(&empty_zip(&["arbitrary.txt"])));
        assert!(!ArtifactKind::Presentation.accepts(&empty_zip(&[
            "[Content_Types].xml",
            "_rels/.rels",
            "ppt/presentation.xml",
            "ppt/presentation.xml",
        ])));
        assert!(!ArtifactKind::Presentation.accepts(b"%PDF-1.7\n"));
    }

    #[test]
    fn base64_encoding_is_stable_without_a_new_runtime_dependency() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_imageio_renders_a_bounded_real_png_without_quick_look() {
        let rendered = render(
            "assets/mesh-proof.png",
            include_bytes!("icons/icon.png"),
            None,
        )
        .expect("macOS ImageIO renders the exact PNG bytes");
        assert_eq!(rendered.kind, ArtifactKind::Png);
        assert_eq!(rendered.renderer, ArtifactRenderer::ImageIoThumbnail);
        assert_eq!(rendered.page_number, None);
        assert_eq!(rendered.page_count, None);
        assert_eq!(rendered.text, None);
        assert!(rendered.png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(rendered.png.len() as u64 <= MAX_PREVIEW_BYTES);
    }

    #[test]
    fn invalid_artifact_bytes_refuse_before_platform_rendering() {
        assert_eq!(
            render("report.pdf", b"not a pdf", None),
            Err(ArtifactPreviewError::InvalidContainer)
        );
        assert_eq!(
            render("slides.pptx", b"not a zip", None),
            Err(ArtifactPreviewError::InvalidContainer)
        );
        assert_eq!(
            render("archive.zip", b"PK\x03\x04", None),
            Err(ArtifactPreviewError::Unsupported)
        );
        assert_eq!(
            render("report.pdf", &one_page_pdf(), Some(65)),
            Err(ArtifactPreviewError::InvalidPage)
        );
    }

    #[test]
    fn quick_look_html_is_reduced_to_bounded_plain_review_text() {
        let html = br#"<html><head><style>secret-css</style></head><body>
            <style>.in-body { color: red }</style>
            <p>Hiring &amp; retention</p>
            <table><tr><td>Revenue</td><td>$120,000</td></tr></table>
            <script>never-render-this()</script>
            <div>North&#x20;America</div>
        </body></html>"#;
        let extracted =
            extract_quick_look_text(ArtifactKind::Document, html).expect("bounded visible text");
        assert_eq!(
            extracted.lines,
            ["Hiring & retention", "Revenue\t$120,000", "North America"]
        );
        assert_eq!(extracted.source, ArtifactTextSource::QuickLookVisible);
        assert_eq!(
            extracted.sections,
            [ArtifactTextSection {
                label: "Document".to_owned(),
                line_start: 0,
                line_count: 3,
            }]
        );
        assert!(!extracted.truncated);
        assert!(extract_quick_look_text(
            ArtifactKind::Document,
            b"<html><body><p>safe\xe2\x80\xaetext</p></body></html>"
        )
        .is_none());
        assert!(extract_quick_look_text(
            ArtifactKind::Document,
            b"<html><head></head>missing body</html>"
        )
        .is_none());
    }

    #[test]
    fn spreadsheet_xml_exposes_exact_cells_formulas_and_sheet_names() {
        let workbook = br#"<?xml version="1.0"?><x:workbook xmlns:x="urn:x" xmlns:r="urn:r"><x:sheets><x:sheet name="Finance &amp; Ops" sheetId="1" r:id="sheet-one"/></x:sheets></x:workbook>"#;
        let relationships = br#"<?xml version="1.0"?><Relationships><Relationship Id="sheet-one" Type="urn/worksheet" Target="/xl/worksheets/sheet1.xml"/></Relationships>"#;
        assert_eq!(
            spreadsheet_sheets(workbook, relationships),
            Some(vec![SpreadsheetSheet {
                name: "Finance & Ops".to_owned(),
                path: "xl/worksheets/sheet1.xml".to_owned(),
                visibility: SpreadsheetSheetVisibility::Visible,
            }])
        );

        let shared = spreadsheet_shared_strings(Some(
            br#"<sst><si><t>Revenue</t></si><si><r><t>North </t></r><r><t>America</t></r></si></sst>"#,
        ))
        .expect("bounded shared strings");
        assert_eq!(shared, ["Revenue", "North America"]);
        let (cells, truncated) = spreadsheet_cells(
            br#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1"><f>SUM(B2:B4)</f><v>380000</v></c><c r="C1" t="inlineStr"><is><t>North&#x20;America</t></is></c><c r="D1" t="b"><v>1</v></c></row></sheetData></worksheet>"#,
            &shared,
            512,
        )
        .expect("bounded worksheet cells");
        assert!(!truncated);
        assert_eq!(
            cells,
            [
                "A1\ttext\tRevenue",
                "B1\tformula\texpression\tpresent\tSUM(B2:B4)\tattributes\t0\tresult\tcached\tnumber\t380000",
                "C1\ttext\tNorth America",
                "D1\tboolean\tTRUE",
            ]
        );
        let (bounded, truncated) = spreadsheet_cells(
            b"<worksheet><sheetData><row><c r='A1'><v>1</v></c><c r='A2'><v>2</v></c></row></sheetData></worksheet>",
            &[],
            1,
        )
        .expect("bounded cell projection");
        assert_eq!(bounded, ["A1\tnumber\t1"]);
        assert!(truncated);
    }

    #[test]
    fn spreadsheet_control_whitespace_and_literal_escapes_do_not_compare_equal() {
        let extracted = |shared: &str| {
            let shared = spreadsheet_shared_strings(Some(
                format!(r#"<sst><si><t>{shared}</t></si></sst>"#).as_bytes(),
            ))
            .expect("bounded shared string");
            spreadsheet_cells(
                b"<worksheet><sheetData><row r='1'><c r='A1' t='s'><v>0</v></c></row></sheetData></worksheet>",
                &shared,
                512,
            )
            .expect("bounded worksheet")
            .0
        };

        let control_whitespace = extracted("Forecast&#9;Q1&#13;Q2&#10;Q3");
        let literal_escapes = extracted(r"Forecast\tQ1\rQ2\nQ3");

        assert_ne!(
            control_whitespace, literal_escapes,
            "real control whitespace and visible backslash escapes are different cell text"
        );
    }

    #[test]
    fn spreadsheet_explicit_empty_text_cell_does_not_compare_as_absent() {
        let extracted = |cell: &str| {
            spreadsheet_cells(
                format!("<worksheet><sheetData><row r='1'>{cell}</row></sheetData></worksheet>")
                    .as_bytes(),
                &[],
                512,
            )
            .expect("bounded worksheet")
            .0
        };

        let absent = extracted("");
        let self_closing = extracted("<c r='A1' t='str'><v/></c>");
        let paired = extracted("<c r='A1' t='str'><v></v></c>");
        let inline_empty_self_closing = extracted("<c r='A1' t='inlineStr'><is/></c>");
        let inline_empty_paired = extracted("<c r='A1' t='inlineStr'><is></is></c>");
        let inline_self_closing = extracted("<c r='A1' t='inlineStr'><is><t/></is></c>");
        let inline_paired = extracted("<c r='A1' t='inlineStr'><is><t></t></is></c>");
        let inline_without_container = extracted("<c r='A1' t='inlineStr'/>");
        let formatting_only = extracted("<c r='A1' s='1'/>");

        assert_ne!(
            absent, self_closing,
            "an explicit empty stored text cell has an exact coordinate and must not compare as absent"
        );
        assert_eq!(
            self_closing, paired,
            "equivalent explicit-empty SpreadsheetML forms must compare equally"
        );
        assert_eq!(self_closing, inline_self_closing);
        assert_eq!(inline_self_closing, inline_empty_self_closing);
        assert_eq!(inline_empty_self_closing, inline_empty_paired);
        assert_eq!(inline_self_closing, inline_paired);
        assert_eq!(self_closing, ["A1\ttext\tvalue\tempty"]);
        assert_eq!(
            absent, inline_without_container,
            "an inlineStr cell without its stored inline-string container remains absent"
        );
        assert_eq!(
            absent, formatting_only,
            "formatting-only empty cells remain in the disclosed exact-copy boundary"
        );

        let invalid = |cell: &str| {
            spreadsheet_cells(
                format!("<worksheet><sheetData><row r='1'>{cell}</row></sheetData></worksheet>")
                    .as_bytes(),
                &[],
                512,
            )
        };
        assert!(invalid("<c r='A1' t='inlineStr'><is/><is/></c>").is_none());
        assert!(invalid("<c r='A1' t='inlineStr'><is></is><is></is></c>").is_none());
        assert!(invalid("<c r='A1' t='inlineStr'><ext><is/></ext></c>").is_none());
        assert!(invalid("<c r='A1' t='inlineStr'><ext><is></is></ext></c>").is_none());
        assert!(invalid("<c r='A1' t='str'><is/></c>").is_none());
        assert!(invalid("<c r='A1' t='str'><is></is></c>").is_none());
    }

    #[test]
    fn spreadsheet_formula_structure_cannot_collide_with_literal_sentinels_or_escapes() {
        let extracted = |cell: &str| {
            spreadsheet_cells(
                format!("<worksheet><sheetData><row r='1'>{cell}</row></sheetData></worksheet>")
                    .as_bytes(),
                &[],
                512,
            )
            .expect("bounded worksheet")
            .0
        };

        let missing_result = extracted("<c r='A1'><f>SUM(B1:B2)</f></c>");
        let literal_missing_marker =
            extracted("<c r='A1' t='str'><f>SUM(B1:B2)</f><v>&lt;not cached&gt;</v></c>");
        assert_ne!(
            missing_result, literal_missing_marker,
            "a missing cached result is structural, not a forgeable text sentinel"
        );
        assert_eq!(
            missing_result,
            ["A1\tformula\texpression\tpresent\tSUM(B1:B2)\tattributes\t0\tresult\tmissing"]
        );
        let self_closing_empty_result = extracted("<c r='A1' t='str'><f>SUM(B1:B2)</f><v/></c>");
        let paired_empty_result = extracted("<c r='A1' t='str'><f>SUM(B1:B2)</f><v></v></c>");
        assert_ne!(
            missing_result, self_closing_empty_result,
            "an explicit empty cached result must not compare as an absent result"
        );
        assert_eq!(
            self_closing_empty_result, paired_empty_result,
            "equivalent empty cached-result XML forms must compare equally"
        );

        let empty_expression = extracted("<c r='A1'><f/></c>");
        let literal_expression_marker =
            extracted("<c r='A1'><f>&lt;expression stored by another cell&gt;</f></c>");
        assert_ne!(
            empty_expression, literal_expression_marker,
            "an empty formula expression is structural, not a forgeable text sentinel"
        );

        let control_delimiters = extracted("<c r='A1'><f ref='A1&#9;B1'>SUM(&#9;A1)</f></c>");
        let literal_delimiters = extracted(r"<c r='A1'><f ref='A1\tB1'>SUM(\tA1)</f></c>");
        assert_ne!(
            control_delimiters, literal_delimiters,
            "formula expressions and attributes must preserve literal backslashes"
        );
    }

    #[test]
    fn spreadsheet_sheet_visibility_is_review_significant() {
        let relationships = br#"<Relationships><Relationship Id="sheet-one" Type="urn/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let workbook = |state: &str| {
            format!(
                r#"<workbook><sheets><sheet name="Forecast" sheetId="1" r:id="sheet-one"{state}/></sheets></workbook>"#,
            )
        };
        let visible =
            spreadsheet_sheets(workbook("").as_bytes(), relationships).expect("visible worksheet");
        let hidden = spreadsheet_sheets(workbook(r#" state="hidden""#).as_bytes(), relationships)
            .expect("hidden worksheet");
        let very_hidden =
            spreadsheet_sheets(workbook(r#" state="veryHidden""#).as_bytes(), relationships)
                .expect("very-hidden worksheet");

        assert_ne!(
            visible, hidden,
            "a hidden worksheet must not compare as visible"
        );
        assert_ne!(
            hidden, very_hidden,
            "very-hidden must remain distinguishable"
        );
        assert_eq!(visible[0].visibility, SpreadsheetSheetVisibility::Visible);
        assert_eq!(hidden[0].visibility, SpreadsheetSheetVisibility::Hidden);
        assert_eq!(
            very_hidden[0].visibility,
            SpreadsheetSheetVisibility::VeryHidden,
        );
        assert!(
            spreadsheet_sheets(workbook(r#" state="unknown""#).as_bytes(), relationships,)
                .is_none()
        );
    }

    #[test]
    fn spreadsheet_row_visibility_is_review_significant() {
        let worksheet = |hidden: &str| {
            format!(
                r#"<worksheet><sheetData><row r="2"{hidden}><c r="A2"><v>42000</v></c></row></sheetData></worksheet>"#,
            )
        };
        let (shown, shown_truncated) =
            spreadsheet_cells(worksheet("").as_bytes(), &[], 512).expect("shown row");
        let (hidden, hidden_truncated) =
            spreadsheet_cells(worksheet(r#" hidden="1""#).as_bytes(), &[], 512)
                .expect("hidden row");
        let (hidden_word, _) =
            spreadsheet_cells(worksheet(r#" hidden="true""#).as_bytes(), &[], 512)
                .expect("word-form hidden row");
        let (shown_digit, _) = spreadsheet_cells(worksheet(r#" hidden="0""#).as_bytes(), &[], 512)
            .expect("digit-form shown row");

        assert!(!shown_truncated && !hidden_truncated);
        assert_ne!(shown, hidden, "a hidden row must not compare as shown");
        assert_eq!(shown, shown_digit);
        assert_eq!(hidden, hidden_word);
        assert_eq!(hidden, ["<row 2 visibility: hidden>", "A2\tnumber\t42000"]);
        let (bounded, bounded_truncated) =
            spreadsheet_cells(worksheet(r#" hidden="1""#).as_bytes(), &[], 1)
                .expect("bounded hidden row");
        assert_eq!(bounded, ["<row 2 visibility: hidden>"]);
        assert!(bounded_truncated);
        assert!(
            spreadsheet_cells(worksheet(r#" hidden="sometimes""#).as_bytes(), &[], 512,).is_none()
        );
        assert!(spreadsheet_cells(
            b"<worksheet><sheetData><row hidden='1'/></sheetData></worksheet>",
            &[],
            512,
        )
        .is_none());
    }

    #[test]
    fn spreadsheet_default_row_visibility_is_review_significant_and_fail_closed() {
        let extracted = |format_properties: &str, maximum: usize| {
            spreadsheet_cells(
                format!(
                    "<worksheet>{format_properties}<sheetData><row r='2'><c r='A2'><v>42000</v></c></row></sheetData></worksheet>"
                )
                .as_bytes(),
                &[],
                maximum,
            )
        };

        let shown = extracted("<sheetFormatPr defaultRowHeight='15' zeroHeight='0'/>", 512)
            .expect("rows shown by default");
        let shown_word = extracted(
            "<sheetFormatPr defaultRowHeight='15' zeroHeight='false'></sheetFormatPr>",
            512,
        )
        .expect("paired word-form shown default");
        let format_only = extracted("<sheetFormatPr defaultRowHeight='15'/>", 512)
            .expect("ordinary format properties without default hiding");
        let hidden = extracted("<sheetFormatPr defaultRowHeight='15' zeroHeight='1'/>", 512)
            .expect("rows hidden by default");
        let hidden_word = extracted(
            "<sheetFormatPr defaultRowHeight='15' zeroHeight='true'></sheetFormatPr>",
            512,
        )
        .expect("paired word-form default visibility");
        let absent = extracted("", 512).expect("default format properties omitted");

        assert_eq!(shown, shown_word);
        assert_eq!(shown, format_only);
        assert_eq!(shown, absent);
        assert_ne!(
            shown, hidden,
            "default-hidden rows must not compare as shown"
        );
        assert_eq!(hidden, hidden_word);
        assert_eq!(
            hidden.0,
            [
                "<default row visibility: hidden>",
                "<row 2 visibility: visible>",
                "A2\tnumber\t42000"
            ]
        );

        let hidden_row = spreadsheet_cells(
            b"<worksheet><sheetFormatPr defaultRowHeight='15' zeroHeight='1'/><sheetData><row r='2' hidden='1'><c r='A2'><v>42000</v></c></row></sheetData></worksheet>",
            &[],
            512,
        )
        .expect("explicit hidden row under default-hidden worksheet");
        assert_eq!(
            hidden_row.0,
            [
                "<default row visibility: hidden>",
                "<row 2 visibility: hidden>",
                "A2\tnumber\t42000"
            ]
        );
        assert_ne!(hidden, hidden_row);

        let empty_visible_row = spreadsheet_cells(
            b"<worksheet><sheetFormatPr defaultRowHeight='15' zeroHeight='1'/><sheetData><row r='2'/></sheetData></worksheet>",
            &[],
            512,
        )
        .expect("empty explicit row remains a visible exception");
        assert_eq!(
            empty_visible_row.0,
            [
                "<default row visibility: hidden>",
                "<row 2 visibility: visible>"
            ]
        );

        let bounded = extracted("<sheetFormatPr defaultRowHeight='15' zeroHeight='1'/>", 1)
            .expect("default-row visibility shares the worksheet line bound");
        assert_eq!(bounded.0, ["<default row visibility: hidden>"]);
        assert!(bounded.1);

        assert!(extracted(
            "<sheetFormatPr defaultRowHeight='15' zeroHeight='sometimes'/>",
            512,
        )
        .is_none());
        assert!(extracted(
            "<sheetFormatPr defaultRowHeight='15' zeroHeight='1'/><sheetFormatPr defaultRowHeight='15' zeroHeight='1'/>",
            512,
        )
        .is_none());
        assert!(spreadsheet_cells(
            b"<worksheet><sheetData/><sheetFormatPr defaultRowHeight='15' zeroHeight='1'/></worksheet>",
            &[],
            512,
        )
        .is_none());
        assert!(spreadsheet_cells(
            b"<worksheet><sheetData><sheetFormatPr defaultRowHeight='15' zeroHeight='1'/></sheetData></worksheet>",
            &[],
            512,
        )
        .is_none());
    }

    #[test]
    fn spreadsheet_merged_ranges_are_review_significant_and_fail_closed() {
        let extracted = |merged: &str| {
            spreadsheet_cells(
                format!(
                    "<worksheet><sheetData><row r='1'><c r='A1' t='inlineStr'><is><t>Quarter</t></is></c><c r='B1'><v>1</v></c><c r='C1'><v>2</v></c></row></sheetData>{merged}</worksheet>"
                )
                .as_bytes(),
                &[],
                512,
            )
        };

        let two_columns = extracted("<mergeCells count='1'><mergeCell ref='A1:B1'/></mergeCells>")
            .expect("bounded two-column merge");
        let three_columns =
            extracted("<mergeCells count='1'><mergeCell ref='A1:C1'></mergeCell></mergeCells>")
                .expect("bounded three-column merge");
        assert_ne!(two_columns, three_columns);
        assert_eq!(two_columns.0.last().unwrap(), "<merged cells: A1:B1>");
        assert_eq!(three_columns.0.last().unwrap(), "<merged cells: A1:C1>");
        assert_eq!(
            extracted("<mergeCells><mergeCell ref='C2:D2'/><mergeCell ref='A1:B1'/></mergeCells>")
                .expect("merge declaration order is not workbook structure"),
            extracted("<mergeCells><mergeCell ref='A1:B1'/><mergeCell ref='C2:D2'/></mergeCells>")
                .expect("same canonical merge set")
        );

        let bounded_xml = "<worksheet><sheetData><row r='1'><c r='A1'><v>1</v></c></row></sheetData><mergeCells><mergeCell ref='A1:B1'/></mergeCells></worksheet>";
        let (bounded, truncated) = spreadsheet_cells(bounded_xml.as_bytes(), &[], 1)
            .expect("the merged-range projection has the same line bound as cell content");
        assert_eq!(bounded, ["A1\tnumber\t1"]);
        assert!(truncated);

        assert!(extracted("<mergeCells/>").is_none());
        assert!(extracted("<mergeCells></mergeCells>").is_none());
        assert!(extracted(
            "<mergeCells><mergeCell ref='A1:B1'/><mergeCell ref='A1:B1'/></mergeCells>"
        )
        .is_none());
        assert!(extracted("<mergeCells><mergeCell ref='A1:B1'/></mergeCells><mergeCells><mergeCell ref='B2:C2'/></mergeCells>").is_none());
        assert!(extracted("<mergeCells count='2'><mergeCell ref='A1:B1'/></mergeCells>").is_none());
        assert!(extracted("<mergeCells><mergeCell ref='B1:A1'/></mergeCells>").is_none());
        assert!(extracted("<mergeCells><mergeCell ref='A1'/></mergeCells>").is_none());
        assert!(
            extracted("<mergeCells><wrapper><mergeCell ref='A1:B1'/></wrapper></mergeCells>")
                .is_none()
        );
        assert!(
            extracted("<mergeCells><mergeCell ref='A1:B1'>text</mergeCell></mergeCells>").is_none()
        );
        assert!(extracted("<mergeCell ref='A1:B1'/>").is_none());
    }

    #[test]
    fn empty_worksheet_remains_an_exact_named_content_section() {
        assert_eq!(
            spreadsheet_section_lines(Vec::new(), SpreadsheetSheetVisibility::Visible),
            ["<no cells or formulas>"]
        );
        assert_eq!(
            spreadsheet_section_lines(
                vec!["A1\tnumber\t1".to_owned()],
                SpreadsheetSheetVisibility::Visible,
            ),
            ["A1\tnumber\t1"]
        );
        assert_eq!(
            spreadsheet_section_lines(Vec::new(), SpreadsheetSheetVisibility::Hidden),
            ["<worksheet visibility: hidden>", "<no cells or formulas>"]
        );
        assert_eq!(
            spreadsheet_section_lines(
                vec!["A1\tnumber\t1".to_owned()],
                SpreadsheetSheetVisibility::VeryHidden,
            ),
            ["<worksheet visibility: very hidden>", "A1\tnumber\t1"]
        );
    }

    #[test]
    fn presentation_xml_exposes_exact_slide_order_names_and_paragraphs() {
        let presentation = br#"<p:presentation><p:sldIdLst><p:sldId r:id='first'/><p:sldId r:id='second'/></p:sldIdLst></p:presentation>"#;
        let relationships = br#"<Relationships><Relationship Id='first' Type='urn/slide' Target='slides/slide2.xml'/><Relationship Id='second' Type='urn/slide' Target='slides/slide1.xml'/></Relationships>"#;
        assert_eq!(
            presentation_slides(presentation, relationships),
            Some(vec![
                "ppt/slides/slide2.xml".to_owned(),
                "ppt/slides/slide1.xml".to_owned(),
            ])
        );
        assert_eq!(
            presentation_slide_text(
                br#"<p:sld><p:cSld name='Hiring plan'><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>People </a:t></a:r><a:r><a:t>plan</a:t></a:r></a:p><a:p><a:r><a:t>Finance &amp; HR</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
                2,
            ),
            Some((
                "Slide 2 · Hiring plan".to_owned(),
                vec!["People plan".to_owned(), "Finance & HR".to_owned()],
            ))
        );
        assert_eq!(
            presentation_slide_text(b"<p:sld><p:cSld name='Slide 3'/></p:sld>", 3),
            Some(("Slide 3".to_owned(), vec!["<no slide text>".to_owned()])),
        );
    }

    #[test]
    fn presentation_line_breaks_and_tabs_remain_review_significant() {
        let slide_text = |middle: &str| {
            let xml = format!(
                "<p:sld><p:cSld><a:p><a:r><a:t>less</a:t></a:r>{middle}<a:r><a:t>more</a:t></a:r></a:p></p:cSld></p:sld>"
            );
            presentation_slide_text(xml.as_bytes(), 1).expect("bounded slide text")
        };

        assert_eq!(slide_text("<a:br/>").1, [r"less\nmore"]);
        assert_eq!(
            slide_text("<a:br><a:rPr lang='en-US'/></a:br>").1,
            [r"less\nmore"]
        );
        assert_eq!(slide_text("<a:tab/>").1, [r"less\tmore"]);
        assert_eq!(
            slide_text("<a:tab></a:tab>").1,
            slide_text("<a:tab/>").1,
            "paired and empty DrawingML tabs are the same presentation content",
        );
        for unsupported_tab in ["<a:tab>hidden</a:tab>", "<a:tab><a:rPr/></a:tab>"] {
            let xml = format!(
                "<p:sld><p:cSld><a:p><a:r><a:t>less</a:t></a:r>{unsupported_tab}<a:r><a:t>more</a:t></a:r></a:p></p:cSld></p:sld>"
            );
            assert_eq!(
                presentation_slide_text(xml.as_bytes(), 1),
                None,
                "unsupported tab content must fail closed",
            );
        }
        assert_eq!(slide_text("<a:br/><a:tab/>").1, [r"less\n\tmore"]);

        let edge_breaks = presentation_slide_text(
            b"<p:sld><p:cSld><a:p><a:br/><a:r><a:t>center</a:t></a:r><a:br/></a:p></p:cSld></p:sld>",
            1,
        )
        .expect("bounded slide text with edge breaks");
        assert_eq!(edge_breaks.1, [r"\ncenter\n"]);

        let literal_escapes = presentation_slide_text(
            br"<p:sld><p:cSld><a:p><a:r><a:t>less\n\tmore</a:t></a:r></a:p></p:cSld></p:sld>",
            1,
        )
        .expect("literal escapes");
        assert_eq!(literal_escapes.1, [r"less\\n\\tmore"]);
        assert_ne!(slide_text("<a:br/><a:tab/>").1, literal_escapes.1);

        let at_bound = format!(
            "<p:sld><p:cSld><a:p><a:r><a:t>{}</a:t></a:r><a:br/></a:p></p:cSld></p:sld>",
            "x".repeat(2_047)
        );
        let over_bound = format!(
            "<p:sld><p:cSld><a:p><a:r><a:t>{}</a:t></a:r><a:br/></a:p></p:cSld></p:sld>",
            "x".repeat(2_048)
        );
        assert!(presentation_slide_text(at_bound.as_bytes(), 1).is_some());
        assert!(presentation_slide_text(over_bound.as_bytes(), 1).is_none());
    }

    #[test]
    fn presentation_slide_visibility_is_review_significant() {
        let shown = presentation_slide_text(
            b"<p:sld><p:cSld><a:p><a:r><a:t>Forecast</a:t></a:r></a:p></p:cSld></p:sld>",
            1,
        )
        .expect("shown slide");
        let hidden = presentation_slide_text(
            b"<p:sld show='0'><p:cSld><a:p><a:r><a:t>Forecast</a:t></a:r></a:p></p:cSld></p:sld>",
            1,
        )
        .expect("hidden slide");
        let hidden_word = presentation_slide_text(
            b"<p:sld show='false'><p:cSld><a:p><a:r><a:t>Forecast</a:t></a:r></a:p></p:cSld></p:sld>",
            1,
        )
        .expect("word-form hidden slide");
        let shown_digit = presentation_slide_text(
            b"<p:sld show='1'><p:cSld><a:p><a:r><a:t>Forecast</a:t></a:r></a:p></p:cSld></p:sld>",
            1,
        )
        .expect("digit-form shown slide");

        assert_ne!(shown, hidden, "a hidden slide must not compare as shown");
        assert_eq!(shown, shown_digit);
        assert_eq!(hidden, hidden_word);
        assert_eq!(hidden.1, ["<slide visibility: hidden>", "Forecast"]);
        assert!(presentation_slide_text(
            b"<p:sld show='sometimes'><p:cSld name='Slide 1'/></p:sld>",
            1,
        )
        .is_none());
    }

    #[test]
    fn presentation_hidden_object_visibility_is_review_significant() {
        let shown = presentation_slide_text(
            br#"<p:sld><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id='2' name='Forecast box'/></p:nvSpPr><p:txBody><a:p><a:r><a:t>Forecast</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            1,
        )
        .expect("shown object");
        let hidden = presentation_slide_text(
            br#"<p:sld><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id='2' name='Forecast box' hidden='1'/></p:nvSpPr><p:txBody><a:p><a:r><a:t>Forecast</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            1,
        )
        .expect("hidden object");
        let hidden_word = presentation_slide_text(
            br#"<p:sld><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id='2' name='Forecast box' hidden='true'/></p:nvSpPr><p:txBody><a:p><a:r><a:t>Forecast</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            1,
        )
        .expect("word-form hidden object");
        let shown_digit = presentation_slide_text(
            br#"<p:sld><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id='2' name='Forecast box' hidden='0'/></p:nvSpPr><p:txBody><a:p><a:r><a:t>Forecast</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            1,
        )
        .expect("digit-form shown object");

        assert_ne!(shown, hidden, "a hidden object must not compare as shown");
        assert_eq!(shown, shown_digit);
        assert_eq!(hidden, hidden_word);
        assert_eq!(
            hidden.1,
            ["<object 2 visibility: hidden · Forecast box>", "Forecast"]
        );
        assert!(presentation_slide_text(
            br#"<p:sld><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id='2' name='Forecast box' hidden='sometimes'/></p:nvSpPr></p:sp></p:spTree></p:cSld></p:sld>"#,
            1,
        )
        .is_none());
        assert!(presentation_slide_text(
            br#"<p:sld><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr name='Forecast box' hidden='1'/></p:nvSpPr></p:sp></p:spTree></p:cSld></p:sld>"#,
            1,
        )
        .is_none());
    }

    #[test]
    fn presentation_literal_text_cannot_impersonate_a_visibility_marker() {
        let marker = "<object 2 visibility: hidden · Forecast box>";
        let visible_marker_text = presentation_slide_text(
            br#"<p:sld><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id='3' name='Visible label'/></p:nvSpPr><p:txBody><a:p><a:r><a:t>&lt;object 2 visibility: hidden &#183; Forecast box&gt;</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            1,
        )
        .expect("visible marker-like slide text");
        let hidden_object = presentation_slide_text(
            br#"<p:sld><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id='2' name='Forecast box' hidden='1'/></p:nvSpPr></p:sp></p:spTree></p:cSld></p:sld>"#,
            1,
        )
        .expect("hidden presentation object marker");
        let escaped_literal = presentation_slide_text(
            br#"<p:sld><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>\&lt;object 2 visibility: hidden &#183; Forecast box\&gt;</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            1,
        )
        .expect("literal backslashes and marker delimiters");

        assert_eq!(hidden_object.1, [marker]);
        assert_eq!(
            visible_marker_text.1,
            [r"\<object 2 visibility: hidden · Forecast box\>"]
        );
        assert_eq!(
            escaped_literal.1,
            [r"\\\<object 2 visibility: hidden · Forecast box\\\>"]
        );
        assert_ne!(
            visible_marker_text, hidden_object,
            "visible slide text must not forge a hidden-object marker"
        );
    }

    #[test]
    fn presentation_object_names_use_injective_marker_encoding() {
        let hidden_object = |name: &str| {
            presentation_slide_text(
                format!(
                    "<p:sld><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id='2' name='{name}' hidden='1'/></p:nvSpPr></p:sp></p:spTree></p:cSld></p:sld>"
                )
                .as_bytes(),
                1,
            )
        };
        let control_whitespace = hidden_object("Forecast&#9;Q1&#13;Q2&#10;Q3")
            .expect("control whitespace in an object name");
        let literal_escapes = hidden_object(r"Forecast\tQ1\rQ2\nQ3")
            .expect("literal slash escapes in an object name");
        let delimiters = hidden_object(r"Forecast &lt;box&gt; \path")
            .expect("marker delimiters in an object name");

        assert_eq!(
            control_whitespace.1,
            [r"<object 2 visibility: hidden · Forecast\tQ1\rQ2\nQ3>"]
        );
        assert_eq!(
            literal_escapes.1,
            [r"<object 2 visibility: hidden · Forecast\\tQ1\\rQ2\\nQ3>"]
        );
        assert_ne!(control_whitespace, literal_escapes);
        assert_eq!(
            delimiters.1,
            [r"<object 2 visibility: hidden · Forecast \<box\> \\path>"]
        );
        assert!(
            hidden_object(&"\\".repeat(65)).is_none(),
            "the 128-byte name bound applies after escaping"
        );
    }

    #[test]
    fn spreadsheet_semantics_fail_closed_on_active_or_ambiguous_xml() {
        assert!(bounded_xml_events(
            b"<!DOCTYPE x [<!ENTITY e SYSTEM 'file:///etc/passwd'>]><x>&e;</x>"
        )
        .is_none());
        let namespaced = bounded_xml_events(b"<x id='first' p:id='second'/>")
            .expect("namespace-qualified attributes are distinct XML names");
        let XmlEvent::Empty(tag) = &namespaced[0] else {
            panic!("one empty namespaced element");
        };
        assert!(
            tag.attribute("id").is_none(),
            "ambiguous local lookup refused"
        );
        assert_eq!(tag.qualified_attribute("p:id"), Some("second"));
        assert!(bounded_xml_events(b"<x id='first' id='second'/>").is_none());
        assert!(
            spreadsheet_cells(b"<worksheet><c r='XFE1'><v>1</v></c></worksheet>", &[], 512)
                .is_none()
        );
        assert!(spreadsheet_cells(
            b"<worksheet><c r='A1048577'><v>1</v></c></worksheet>",
            &[],
            512
        )
        .is_none());
        assert!(spreadsheet_sheets(
            b"<workbook><sheet name='Unsafe' r:id='external'/></workbook>",
            b"<Relationships><Relationship Id='external' Type='urn/worksheet' TargetMode='External' Target='https://example.test/sheet.xml'/></Relationships>"
        )
        .is_none());
        assert!(spreadsheet_cells(
            b"<worksheet><extension><c r='A1'><v>forged</v></c></extension></worksheet>",
            &[],
            512,
        )
        .is_none());
        assert!(spreadsheet_shared_strings(
            Some(b"<extension><si><t>forged</t></si></extension>",)
        )
        .is_none());
        assert!(spreadsheet_sheets(
            b"<workbook><extension><sheet name='Forged' r:id='sheet-one'/></extension></workbook>",
            b"<Relationships><Relationship Id='sheet-one' Type='urn/worksheet' Target='worksheets/sheet1.xml'/></Relationships>",
        )
        .is_none());
        assert!(presentation_slides(
            b"<p:presentation><p:sldIdLst><p:sldId r:id='external'/></p:sldIdLst></p:presentation>",
            b"<Relationships><Relationship Id='external' Type='urn/slide' TargetMode='External' Target='https://example.test/slide.xml'/></Relationships>",
        )
        .is_none());
        assert!(presentation_slides(
            b"<p:presentation><p:extension><p:sldId r:id='first'/></p:extension></p:presentation>",
            b"<Relationships><Relationship Id='first' Type='urn/slide' Target='slides/slide1.xml'/></Relationships>",
        )
        .is_none());
        assert!(presentation_slides(
            b"<p:presentation><p:sldIdLst><p:sldId r:id='same'/></p:sldIdLst></p:presentation>",
            b"<Relationships><Relationship Id='same' Type='urn/theme' Target='theme/theme1.xml'/><Relationship Id='same' Type='urn/slide' Target='slides/slide1.xml'/></Relationships>",
        )
        .is_none());
        assert!(presentation_slide_text(
            b"<p:sld><p:cSld><a:t>forged outside paragraph</a:t></p:cSld></p:sld>",
            1,
        )
        .is_none());
    }

    #[test]
    fn quick_look_office_structure_is_preserved_as_bounded_sections() {
        let slides = extract_quick_look_text(
            ArtifactKind::Presentation,
            br#"<html><body><div class="slide"><p>Summary</p></div><div class="slide"><p>Hiring</p></div></body></html>"#,
        )
        .expect("slide text");
        assert_eq!(
            slides.sections,
            [
                ArtifactTextSection {
                    label: "Slide 1".to_owned(),
                    line_start: 0,
                    line_count: 1,
                },
                ArtifactTextSection {
                    label: "Slide 2".to_owned(),
                    line_start: 1,
                    line_count: 1,
                }
            ]
        );

        let sheets = extract_quick_look_text(
            ArtifactKind::Spreadsheet,
            br#"<html><body><table class="worksheet"><tr><td>Revenue</td><td>120</td></tr></table><table class="worksheet"><tr><td>Headcount</td><td>5</td></tr></table></body></html>"#,
        )
        .expect("worksheet text");
        assert_eq!(sheets.lines, ["Revenue\t120", "Headcount\t5"]);
        assert_eq!(
            sheets.sections,
            [
                ArtifactTextSection {
                    label: "Sheet 1".to_owned(),
                    line_start: 0,
                    line_count: 1,
                },
                ArtifactTextSection {
                    label: "Sheet 2".to_owned(),
                    line_start: 1,
                    line_count: 1,
                }
            ]
        );
    }

    #[cfg(target_os = "macos")]
    fn renderer_fixture(variable: &str, name: &str) -> std::path::PathBuf {
        std::env::var_os(variable).map_or_else(
            || {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../tests/fixtures/artifact-renderers")
                    .join(name)
            },
            std::path::PathBuf::from,
        )
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit macOS PDFKit process integration proof"]
    fn macos_pdfkit_renders_a_real_pdf_page_to_bounded_png_and_text() {
        let rendered = render("finance/board-pack.pdf", &one_page_pdf(), Some(1))
            .expect("separate macOS PDFKit process renders the exact PDF page");
        assert_eq!(rendered.kind, ArtifactKind::Pdf);
        assert_eq!(rendered.renderer, ArtifactRenderer::PdfKitPage);
        assert_eq!(rendered.page_number, Some(1));
        assert_eq!(rendered.page_count, Some(1));
        assert!(rendered.png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(rendered.png.len() as u64 <= MAX_PREVIEW_BYTES);
        let text = rendered.text.expect("page-aware inert text");
        assert_eq!(text.source, ArtifactTextSource::PdfPageText);
        assert_eq!(text.sections[0].label, "Page 1");
        assert!(text.lines.iter().any(|line| line == "Mesh alpha page 1"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit large-document PDFKit process integration proof"]
    fn macos_pdfkit_previews_the_bounded_prefix_of_a_large_document() {
        let bytes = pdf_with_pages(65);
        for requested in [1, MAX_PDF_PREVIEW_PAGE] {
            let rendered = render("finance/long-report.pdf", &bytes, Some(requested))
                .expect("render one page inside the bounded preview prefix");
            assert_eq!(rendered.page_number, Some(requested));
            assert_eq!(rendered.page_count, Some(65));
            assert!(rendered.png.starts_with(b"\x89PNG\r\n\x1a\n"));
            assert!(rendered
                .text
                .expect("bounded page text")
                .lines
                .iter()
                .any(|line| line == &format!("Mesh alpha page {requested}")));
        }
        assert_eq!(
            render("finance/long-report.pdf", &bytes, Some(65)),
            Err(ArtifactPreviewError::InvalidPage)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit generated multi-page PDF and macOS PDFKit process integration proof"]
    fn macos_pdfkit_preserves_real_page_order_and_finance_hr_text() {
        let fixture = renderer_fixture("MESH_TEST_PDF", "review.pdf");
        let bytes = std::fs::read(fixture).expect("read generated PDF fixture");
        for (page, expected) in [
            (1, "Board summary"),
            (2, "Finance and HR review required"),
            (3, "Cash runway: 18 months"),
        ] {
            let rendered = render("finance/board-pack.pdf", &bytes, Some(page))
                .expect("render exact requested PDF page");
            assert_eq!(rendered.renderer, ArtifactRenderer::PdfKitPage);
            assert_eq!(rendered.page_number, Some(page));
            assert_eq!(rendered.page_count, Some(3));
            assert!(rendered.png.starts_with(b"\x89PNG\r\n\x1a\n"));
            let text = rendered.text.expect("page-specific text");
            assert_eq!(text.source, ArtifactTextSource::PdfPageText);
            assert_eq!(text.sections[0].label, format!("Page {page}"));
            assert!(text.lines.iter().any(|line| line == expected));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit generated Office fixture and macOS Quick Look integration proof"]
    fn macos_quick_look_renders_real_ooxml_families() {
        let fixtures = [
            (
                "MESH_TEST_PPTX",
                "finance/board-pack.pptx",
                ArtifactKind::Presentation,
                "Mesh Alpha Review",
                "Slide 1",
            ),
            (
                "MESH_TEST_DOCX",
                "people/plan.docx",
                ArtifactKind::Document,
                "People plan",
                "Section 1 · People plan",
            ),
            (
                "MESH_TEST_XLSX",
                "finance/budget.xlsx",
                ArtifactKind::Spreadsheet,
                "Revenue",
                "Budget",
            ),
        ];
        for (variable, path, kind, expected_text, expected_section) in fixtures {
            let fixture_name = match kind {
                ArtifactKind::Presentation => "review.pptx",
                ArtifactKind::Document => "review.docx",
                ArtifactKind::Spreadsheet => "review.xlsx",
                _ => unreachable!("the fixture list contains only Office documents"),
            };
            let fixture = renderer_fixture(variable, fixture_name);
            let bytes = std::fs::read(&fixture).expect("read generated Office fixture");
            if kind == ArtifactKind::Presentation {
                let presentation =
                    unzip_entry_bounded(&fixture, &bytes, "ppt/presentation.xml", 256 * 1024)
                        .expect("bounded real presentation root");
                let relationships = unzip_entry_bounded(
                    &fixture,
                    &bytes,
                    "ppt/_rels/presentation.xml.rels",
                    256 * 1024,
                )
                .expect("bounded real presentation relationships");
                let slides = presentation_slides(&presentation, &relationships)
                    .expect("ordered real presentation slide parts");
                for (index, slide) in slides.iter().enumerate() {
                    let xml = unzip_entry_bounded(&fixture, &bytes, slide, 4 * 1024 * 1024)
                        .expect("bounded real slide XML");
                    presentation_slide_text(&xml, index + 1)
                        .expect("bounded real slide name and paragraph text");
                }
            }
            let rendered = render(path, &bytes, None)
                .unwrap_or_else(|error| panic!("Quick Look renders {path}: {error:?}"));
            assert_eq!(rendered.kind, kind);
            assert!(rendered.png.starts_with(b"\x89PNG\r\n\x1a\n"));
            assert!(rendered.png.len() as u64 <= MAX_PREVIEW_BYTES);
            let text = rendered.text.expect("Quick Look visible-text extraction");
            assert!(
                text.lines.iter().any(|line| line.contains(expected_text)),
                "missing {expected_text:?} in {:?}",
                text.lines
            );
            assert_eq!(text.sections[0].label, expected_section);
            if kind == ArtifactKind::Spreadsheet {
                assert_eq!(text.source, ArtifactTextSource::SpreadsheetCells);
                assert!(text
                    .lines
                    .iter()
                    .any(|line| line.starts_with("B2\tnumber\t120000")));
                assert!(text
                    .lines
                    .iter()
                    .any(|line| line == "D2\tformula\texpression\tpresent\tB2-C2\tattributes\t0\tresult\tcached\tnumber\t35000"));
            } else if kind == ArtifactKind::Presentation {
                assert_eq!(text.source, ArtifactTextSource::PresentationSlides);
                assert_eq!(
                    text.sections
                        .iter()
                        .map(|section| section.label.as_str())
                        .collect::<Vec<_>>(),
                    ["Slide 1", "Slide 2"]
                );
                assert!(text
                    .lines
                    .iter()
                    .any(|line| line == "People and finance decision"));
                assert!(text.lines.iter().any(|line| {
                    line == "Approve three hires after the revised cash-flow review."
                }));
                assert!(!text.lines.iter().any(|line| line.contains("Speaker notes")));
            } else if kind == ArtifactKind::Document {
                assert_eq!(text.source, ArtifactTextSource::DocumentBlocks);
            } else {
                assert_eq!(text.source, ArtifactTextSource::QuickLookVisible);
            }
        }
    }
}
