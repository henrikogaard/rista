use crate::note_composer::{parse, ComposeError};
use markdown::mdast::Node;
use std::ops::Range;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

#[derive(Clone)]
struct Destination {
    range: Range<usize>,
    raw: String,
}

type DefinitionEntry<'a> = (Range<usize>, String, &'a Node);
type DefinitionIndex<'a> = std::collections::HashMap<String, Vec<DefinitionEntry<'a>>>;

fn protected_ranges(text: &str) -> Result<Vec<Range<usize>>, ComposeError> {
    let root = parse(text)?;
    let mut ranges = Vec::new();
    fn collect(node: &Node, text: &str, base: usize, out: &mut Vec<Range<usize>>) {
        if matches!(node, Node::Html(_)) {
            return;
        }
        if matches!(
            node,
            Node::Code(_)
                | Node::InlineCode(_)
                | Node::Math(_)
                | Node::InlineMath(_)
                | Node::Yaml(_)
                | Node::Toml(_)
                | Node::MdxjsEsm(_)
                | Node::MdxFlowExpression(_)
                | Node::MdxTextExpression(_)
                | Node::MdxJsxFlowElement(_)
                | Node::MdxJsxTextElement(_)
        ) {
            if let Some(position) = node.position() {
                let start = position.start.offset.min(text.len());
                let end = position.end.offset.min(text.len());
                if start <= end && text.is_char_boundary(start) && text.is_char_boundary(end) {
                    out.push((start + base)..(end + base));
                }
            }
            return;
        }
        for child in node.children().into_iter().flatten() {
            collect(child, text, base, out);
        }
    }
    collect(&root, text, 0, &mut ranges);
    let html_ranges = raw_html_ranges(text);
    let mut segment_start = 0;
    for html_range in &html_ranges {
        if segment_start < html_range.start {
            let segment = &text[segment_start..html_range.start];
            let segment_root = parse(segment)?;
            collect(&segment_root, segment, segment_start, &mut ranges);
        }
        segment_start = segment_start.max(html_range.end);
    }
    if segment_start < text.len() {
        let segment = &text[segment_start..];
        let segment_root = parse(segment)?;
        collect(&segment_root, segment, segment_start, &mut ranges);
    }
    ranges.extend(html_ranges);
    if let Some(range) = crate::properties::frontmatter_span(text) {
        ranges.push(range);
    }
    ranges.sort_by_key(|range| range.start);
    Ok(ranges)
}

fn raw_html_ranges(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let Some(offset) = text[cursor..].find('<').map(|offset| cursor + offset) else {
            break;
        };
        if text[offset..].starts_with("<!--") {
            let end = text[offset + 4..]
                .find("-->")
                .map(|found| offset + 4 + found + 3)
                .unwrap_or(text.len());
            ranges.push(offset..end);
            cursor = end;
            continue;
        }
        let mut tag_end = offset + 1;
        let mut quote = None;
        while tag_end < bytes.len() {
            let byte = bytes[tag_end];
            if let Some(delimiter) = quote {
                if byte == delimiter {
                    quote = None;
                }
            } else if byte == b'\'' || byte == b'"' {
                quote = Some(byte);
            } else if byte == b'>' {
                break;
            }
            tag_end += 1;
        }
        if tag_end >= bytes.len() {
            break;
        }
        let contents = text[offset + 1..tag_end].trim();
        let closing = contents.starts_with('/');
        let name = contents
            .trim_start_matches('/')
            .trim_start()
            .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == ':'))
            .next()
            .unwrap_or_default();
        let valid_tag = !name.is_empty()
            && name.as_bytes()[0].is_ascii_alphabetic()
            && (contents[name.len()..].starts_with(' ')
                || contents[name.len()..].starts_with('/')
                || contents[name.len()..].is_empty());
        if !valid_tag {
            cursor = tag_end + 1;
            continue;
        }
        let range_end = if !closing && !contents.ends_with('/') && !html_void_element(name) {
            matching_html_element_end(text, tag_end + 1, name)
        } else {
            tag_end + 1
        };
        ranges.push(offset..range_end);
        cursor = range_end;
    }
    ranges
}

fn html_void_element(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn matching_html_element_end(text: &str, mut cursor: usize, name: &str) -> usize {
    let bytes = text.as_bytes();
    let name = name.to_ascii_lowercase();
    let mut depth = 1usize;
    while cursor < bytes.len() {
        let Some(offset) = text[cursor..].find('<').map(|found| cursor + found) else {
            break;
        };
        if text[offset..].starts_with("<!--") {
            cursor = text[offset + 4..]
                .find("-->")
                .map(|found| offset + 4 + found + 3)
                .unwrap_or(text.len());
            continue;
        }
        let mut end = offset + 1;
        let mut quote = None;
        while end < bytes.len() {
            let byte = bytes[end];
            if let Some(delimiter) = quote {
                if byte == delimiter {
                    quote = None;
                }
            } else if byte == b'\'' || byte == b'"' {
                quote = Some(byte);
            } else if byte == b'>' {
                break;
            }
            end += 1;
        }
        if end >= bytes.len() {
            break;
        }
        let contents = text[offset + 1..end].trim();
        let closing = contents.starts_with('/');
        let tag_name = contents
            .trim_start_matches('/')
            .trim_start()
            .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == ':'))
            .next()
            .unwrap_or_default();
        if tag_name.eq_ignore_ascii_case(&name) {
            if closing {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return end + 1;
                }
            } else if !contents.ends_with('/') && !html_void_element(tag_name) {
                depth += 1;
            }
        }
        cursor = end + 1;
    }
    text.len()
}

fn matching_label_end(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    for (offset, byte) in bytes.iter().enumerate().take(end).skip(start) {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'[' {
            depth += 1;
        } else if *byte == b']' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(offset);
            }
        }
    }
    None
}

fn destination_range(text: &str, span: Range<usize>, definition: bool) -> Option<Range<usize>> {
    let bytes = text.as_bytes();
    let start = span.start.min(bytes.len());
    let end = span.end.min(bytes.len());
    let label_start = if bytes.get(start) == Some(&b'!') {
        start + 1
    } else {
        start
    };
    if bytes.get(label_start) != Some(&b'[') {
        return None;
    }
    let close = matching_label_end(bytes, label_start, end)?;
    let mut cursor = close + 1;
    while cursor < end && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if definition {
        if bytes.get(cursor) != Some(&b':') {
            return None;
        }
        cursor += 1;
    } else {
        if bytes.get(cursor) != Some(&b'(') {
            return None;
        }
        cursor += 1;
    }
    while cursor < end && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if bytes.get(cursor) == Some(&b'<') {
        let inner = cursor + 1;
        let mut escaped = false;
        cursor = inner;
        while cursor < end {
            if escaped {
                escaped = false;
            } else if bytes[cursor] == b'\\' {
                escaped = true;
            } else if bytes[cursor] == b'>' {
                return Some(inner..cursor);
            }
            cursor += 1;
        }
        return None;
    }

    let destination_start = cursor;
    let mut parens = 0usize;
    let mut escaped = false;
    while cursor < end {
        let byte = bytes[cursor];
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'(' && !definition {
            parens += 1;
        } else if byte == b')' && !definition {
            if parens == 0 {
                break;
            }
            parens -= 1;
        } else if byte.is_ascii_whitespace() {
            break;
        }
        cursor += 1;
    }
    (destination_start < cursor).then_some(destination_start..cursor)
}

fn markdown_destinations(text: &str) -> Result<Vec<Destination>, ComposeError> {
    let root = parse(text)?;
    let protected = protected_ranges(text)?;
    let mut out = Vec::new();
    fn collect(node: &Node, text: &str, protected: &[Range<usize>], out: &mut Vec<Destination>) {
        let (position, definition) = match node {
            Node::Link(link) => (link.position.as_ref(), false),
            Node::Image(image) => (image.position.as_ref(), false),
            Node::Definition(definition) => (definition.position.as_ref(), true),
            _ => (None, false),
        };
        if let Some(position) = position {
            let span = position.start.offset..position.end.offset;
            let intersects_protected = protected
                .iter()
                .any(|range| range.start < span.end && span.start < range.end);
            if !intersects_protected {
                if let Some(range) = destination_range(text, span, definition) {
                    out.push(Destination {
                        raw: text[range.clone()].to_string(),
                        range,
                    });
                }
            }
        }
        for child in node.children().into_iter().flatten() {
            collect(child, text, protected, out);
        }
    }
    collect(&root, text, &protected, &mut out);
    // The Markdown parser keeps a definition immediately following a
    // paragraph as paragraph text. Recover that source-level definition
    // without reparsing an extracted fragment; fenced/code ranges remain
    // excluded by the AST protection map.
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        let line_end = line_start + line.len();
        if !protected.iter().any(|range| range.contains(&line_start)) {
            if let Some(range) = destination_range(text, line_start..line_end, true) {
                if !out.iter().any(|destination| destination.range == range) {
                    out.push(Destination {
                        raw: text[range.clone()].to_string(),
                        range,
                    });
                }
            }
        }
        line_start = line_end;
    }
    out.sort_by_key(|destination| destination.range.start);
    Ok(out)
}

fn markdown_unescape(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            result.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            result.push(ch);
        }
    }
    if escaped {
        result.push('\\');
    }
    result
}

fn decode_percent(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = (*bytes.get(index + 1)? as char).to_digit(16)?;
            let low = (*bytes.get(index + 2)? as char).to_digit(16)?;
            decoded.push((high * 16 + low) as u8);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn split_suffix(value: &str) -> (&str, &str) {
    value.split_at(value.find(['?', '#']).unwrap_or(value.len()))
}

fn local_target(value: &str, from_note: &Path, root: &Path) -> Option<PathBuf> {
    let value = markdown_unescape(value.trim());
    if value.starts_with("//")
        || value.starts_with('#')
        || value.starts_with("mailto:")
        || value.starts_with("data:")
        || value.contains("://")
    {
        return None;
    }
    let (path, _) = split_suffix(&value);
    if path.is_empty() {
        return None;
    }
    let decoded = decode_percent(path)?;
    let candidate = if decoded.starts_with('/') {
        root.join(decoded.trim_start_matches('/'))
    } else {
        from_note.parent()?.join(decoded)
    };
    let canonical = candidate.canonicalize().ok()?;
    let canonical_root = root.canonicalize().ok()?;
    (canonical.starts_with(canonical_root) && canonical.is_file()).then_some(canonical)
}

fn relative_url(target: &Path, from_note: &Path, root: &Path) -> Result<String, ComposeError> {
    let root = root.canonicalize().map_err(|_| ComposeError::InvalidPath)?;
    let target = canonical_or_new_vault_path(target, &root)?;
    let from = from_note
        .parent()
        .and_then(|path| path.canonicalize().ok())
        .ok_or(ComposeError::InvalidPath)?;
    let from = from
        .strip_prefix(&root)
        .map_err(|_| ComposeError::InvalidPath)?;
    let to = target
        .strip_prefix(&root)
        .map_err(|_| ComposeError::InvalidPath)?;
    let from_parts = from.components().collect::<Vec<_>>();
    let to_parts = to.components().collect::<Vec<_>>();
    let shared = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = vec!["..".to_string(); from_parts.len().saturating_sub(shared)];
    parts.extend(
        to_parts[shared..]
            .iter()
            .map(|part| part.as_os_str().to_string_lossy().into_owned()),
    );
    Ok(parts
        .iter()
        .map(|part| {
            url::form_urlencoded::byte_serialize(part.as_bytes())
                .collect::<String>()
                .replace('+', "%20")
        })
        .collect::<Vec<_>>()
        .join("/"))
}

fn canonical_or_new_vault_path(target: &Path, root: &Path) -> Result<PathBuf, ComposeError> {
    let target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        root.join(target)
    };
    if let Ok(canonical) = target.canonicalize() {
        return canonical
            .starts_with(root)
            .then_some(canonical)
            .ok_or(ComposeError::InvalidPath);
    }
    let parent = target
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .filter(|parent| parent.starts_with(root))
        .ok_or(ComposeError::InvalidPath)?;
    let target = parent.join(target.file_name().ok_or(ComposeError::InvalidPath)?);
    target
        .starts_with(root)
        .then_some(target)
        .ok_or(ComposeError::InvalidPath)
}

fn apply_edits(text: &str, mut edits: Vec<(Range<usize>, String)>) -> Result<String, ComposeError> {
    edits.sort_by_key(|(range, _)| range.start);
    if edits.windows(2).any(|pair| pair[0].0.end > pair[1].0.start) {
        return Err(ComposeError::InvalidMarkdown);
    }
    let mut result = text.to_string();
    for (range, replacement) in edits.into_iter().rev() {
        if range.end > result.len()
            || !result.is_char_boundary(range.start)
            || !result.is_char_boundary(range.end)
        {
            return Err(ComposeError::InvalidMarkdown);
        }
        result.replace_range(range, &replacement);
    }
    Ok(result)
}

fn wikilink_target(value: &str) -> &str {
    value.split(['#', '|']).next().unwrap_or(value).trim()
}

fn note_key(value: &str) -> String {
    value.nfc().collect::<String>().to_lowercase()
}

#[derive(Default)]
struct IdentityIndex {
    by_stem: std::collections::HashMap<String, Vec<PathBuf>>,
    by_alias: std::collections::HashMap<String, Vec<PathBuf>>,
    by_relative: std::collections::HashMap<String, PathBuf>,
}

fn resolve_wikilink(index: &IdentityIndex, value: &str) -> Result<Option<PathBuf>, ComposeError> {
    let target = wikilink_target(value).trim_start_matches('/');
    if target.is_empty() {
        return Ok(None);
    }
    let needle = note_key(target.trim_end_matches(".md"));
    let matches: Vec<PathBuf> = if target.contains('/') {
        index
            .by_relative
            .get(&needle)
            .cloned()
            .into_iter()
            .collect()
    } else {
        index
            .by_stem
            .get(&needle)
            .into_iter()
            .chain(index.by_alias.get(&needle))
            .flat_map(|paths| paths.iter().cloned())
            .collect()
    };
    let unique = matches
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    match unique.len() {
        0 => Ok(None),
        1 => Ok(unique.into_iter().next()),
        _ => Err(ComposeError::AmbiguousLink),
    }
}

pub struct LinkSnapshot {
    root: PathBuf,
    notes: Vec<PathBuf>,
    texts: std::collections::HashMap<PathBuf, String>,
    identities: IdentityIndex,
}

pub struct ExtractionAnchors {
    headings: std::collections::HashMap<String, Vec<(Range<usize>, bool)>>,
    blocks: std::collections::HashMap<String, Vec<(Range<usize>, bool)>>,
}

impl ExtractionAnchors {
    pub fn new(source: &str, section: Range<usize>) -> Result<Self, ComposeError> {
        let root = parse(source)?;
        let mut headings = std::collections::HashMap::<String, Vec<(Range<usize>, bool)>>::new();
        fn collect_headings(
            node: &Node,
            section: &Range<usize>,
            out: &mut std::collections::HashMap<String, Vec<(Range<usize>, bool)>>,
        ) -> Result<(), ComposeError> {
            if let Node::Heading(_) = node {
                if let Some(position) = node.position() {
                    let range = position.start.offset..position.end.offset;
                    let intersects = range.start < section.end && section.start < range.end;
                    let moved = range.start >= section.start && range.end <= section.end;
                    if intersects && !moved {
                        return Err(ComposeError::UnsupportedLink);
                    }
                    let title = node.to_string();
                    out.entry(note_key(title.trim()))
                        .or_default()
                        .push((range, moved));
                }
            }
            for child in node.children().into_iter().flatten() {
                collect_headings(child, section, out)?;
            }
            Ok(())
        }
        collect_headings(&root, &section, &mut headings)?;

        let lines = source.split_inclusive('\n').collect::<Vec<_>>();
        let mut offsets = Vec::with_capacity(lines.len());
        let mut offset = 0;
        for line in &lines {
            let start = offset;
            offset += line.len();
            offsets.push(start..offset);
        }
        let mut blocks = std::collections::HashMap::<String, Vec<(Range<usize>, bool)>>::new();
        for candidate in crate::document::block_candidates(source) {
            let Some(id) = candidate.id else {
                continue;
            };
            let Some((first, last)) = crate::preview::block_lines(&lines, &id) else {
                continue;
            };
            let Some(start) = offsets.get(first).map(|range| range.start) else {
                continue;
            };
            let Some(end) = offsets.get(last).map(|range| range.end) else {
                continue;
            };
            let range = start..end;
            let intersects = range.start < section.end && section.start < range.end;
            let moved = range.start >= section.start && range.end <= section.end;
            if intersects && !moved {
                return Err(ComposeError::UnsupportedLink);
            }
            blocks.entry(id).or_default().push((range, moved));
        }
        Ok(Self { headings, blocks })
    }

    fn moved(&self, fragment: &str) -> Result<Option<bool>, ComposeError> {
        let fragment = fragment
            .strip_prefix('#')
            .unwrap_or(fragment)
            .split(['|', '?'])
            .next()
            .unwrap_or_default();
        if fragment.is_empty() {
            return Ok(None);
        }
        let decoded = decode_percent(fragment).ok_or(ComposeError::UnsupportedLink)?;
        let matches = if let Some(id) = decoded.strip_prefix('^') {
            self.blocks.get(id)
        } else {
            self.headings.get(&note_key(decoded.trim()))
        };
        match matches {
            Some(matches) if matches.len() == 1 => Ok(Some(matches[0].1)),
            Some(matches) if matches.len() > 1 => Err(ComposeError::AmbiguousLink),
            _ => Ok(None),
        }
    }
}

impl LinkSnapshot {
    pub fn new(root: &Path, notes: &[PathBuf]) -> Result<Self, ComposeError> {
        let root = root.canonicalize().map_err(|_| ComposeError::InvalidPath)?;
        let mut index = IdentityIndex::default();
        let mut paths = Vec::new();
        let mut texts = std::collections::HashMap::new();
        let mut total = 0usize;
        for note in notes
            .iter()
            .filter(|path| crate::note_composer::is_composer_reference_document(path))
        {
            let canonical = note
                .canonicalize()
                .map_err(|_| ComposeError::UnreadableNote)?;
            if !canonical.starts_with(&root) {
                return Err(ComposeError::InvalidPath);
            }
            if std::fs::metadata(&canonical)
                .map_err(|_| ComposeError::UnreadableNote)?
                .len()
                > 2 * 1024 * 1024
            {
                return Err(ComposeError::FileTooLarge);
            }
            let bytes = std::fs::read(&canonical).map_err(|_| ComposeError::UnreadableNote)?;
            total = total.saturating_add(bytes.len());
            if bytes.len() > 2 * 1024 * 1024 || total > 16 * 1024 * 1024 {
                return Err(ComposeError::FileTooLarge);
            }
            let text = String::from_utf8(bytes).map_err(|_| ComposeError::UnreadableNote)?;
            if note.extension().is_some_and(|extension| extension == "md") {
                if let Some(stem) = canonical.file_stem().and_then(|stem| stem.to_str()) {
                    index
                        .by_stem
                        .entry(note_key(stem))
                        .or_default()
                        .push(canonical.clone());
                }
                let relative = canonical
                    .strip_prefix(&root)
                    .map_err(|_| ComposeError::InvalidPath)?
                    .to_string_lossy()
                    .replace('\\', "/");
                index.by_relative.insert(
                    note_key(relative.trim_end_matches(".md")),
                    canonical.clone(),
                );
                for alias in crate::properties::frontmatter_aliases(&text) {
                    index
                        .by_alias
                        .entry(note_key(&alias))
                        .or_default()
                        .push(canonical.clone());
                }
            }
            paths.push(canonical.clone());
            texts.insert(canonical.clone(), text);
        }
        Ok(Self {
            root,
            notes: paths,
            texts,
            identities: index,
        })
    }

    pub fn notes(&self) -> &[PathBuf] {
        &self.notes
    }

    pub fn text(&self, note: &Path) -> Option<&str> {
        let note = note.canonicalize().ok()?;
        self.texts.get(&note).map(String::as_str)
    }

    pub fn rewrite_inbound(
        &self,
        text: &str,
        referring_note: &Path,
        old_target: &Path,
        new_target: &Path,
    ) -> Result<String, ComposeError> {
        if is_canvas_path(referring_note) {
            self.rewrite_canvas(
                text,
                referring_note,
                CanvasRewrite::Inbound {
                    old_target,
                    new_target,
                },
            )
        } else {
            self.rewrite_inbound_markdown(text, referring_note, old_target, new_target)
        }
    }

    fn rewrite_inbound_markdown(
        &self,
        text: &str,
        referring_note: &Path,
        old_target: &Path,
        new_target: &Path,
    ) -> Result<String, ComposeError> {
        rewrite_inbound_with_index(
            text,
            referring_note,
            &self.root,
            &self.identities,
            old_target,
            new_target,
        )
    }

    pub fn rewrite_extraction_links(
        &self,
        text: &str,
        referring_note: &Path,
        source_note: &Path,
        destination: &Path,
        anchors: &ExtractionAnchors,
        moved_body: bool,
    ) -> Result<String, ComposeError> {
        if is_canvas_path(referring_note) {
            return self.rewrite_canvas(
                text,
                referring_note,
                CanvasRewrite::Extraction {
                    source_note,
                    destination,
                    anchors,
                },
            );
        }
        self.rewrite_extraction_markdown(
            text,
            referring_note,
            source_note,
            destination,
            anchors,
            moved_body,
        )
    }

    fn rewrite_extraction_markdown(
        &self,
        text: &str,
        referring_note: &Path,
        source_note: &Path,
        destination: &Path,
        anchors: &ExtractionAnchors,
        moved_body: bool,
    ) -> Result<String, ComposeError> {
        let source = source_note
            .canonicalize()
            .map_err(|_| ComposeError::InvalidPath)?;
        let destination = canonical_or_new_vault_path(destination, &self.root)?;
        let referring = canonical_or_new_vault_path(referring_note, &self.root)?;
        let mut edits = Vec::new();

        for destination_link in markdown_destinations(text)? {
            let raw = destination_link.raw.trim();
            let (_, suffix) = split_suffix(raw);
            let state = if suffix.starts_with('#') {
                anchors.moved(suffix)?
            } else {
                None
            };
            let target = if raw.starts_with('#') {
                None
            } else {
                local_target(raw, &referring, &self.root)
            };
            let replacement_target = match (target.as_deref(), raw.starts_with('#'), state) {
                (Some(target), _, Some(true)) if target == source => Some(&destination),
                (_, true, Some(true)) if referring == source => Some(&destination),
                (_, true, Some(false)) if moved_body && referring == destination => Some(&source),
                _ => None,
            };
            if let Some(target) = replacement_target {
                edits.push((
                    destination_link.range,
                    format!("{}{suffix}", relative_url(target, &referring, &self.root)?),
                ));
            }
        }

        let protected = protected_ranges(text)?;
        let mut cursor = 0;
        while cursor + 1 < text.len() {
            let Some(found) = text[cursor..].find("[[").map(|offset| cursor + offset) else {
                break;
            };
            let mut slashes = 0;
            let mut preceding = found;
            while preceding > 0 && text.as_bytes()[preceding - 1] == b'\\' {
                slashes += 1;
                preceding -= 1;
            }
            if slashes % 2 == 1 {
                cursor = found + 2;
                continue;
            }
            if let Some(range) = protected.iter().find(|range| range.contains(&found)) {
                cursor = range.end.max(found + 2);
                continue;
            }
            let Some(end) = text[found + 2..]
                .find("]]")
                .map(|offset| found + 2 + offset)
            else {
                break;
            };
            let raw = &text[found + 2..end];
            let note = wikilink_target(raw);
            let (_, suffix) = split_suffix(raw);
            let state = if suffix.starts_with('#') {
                anchors.moved(suffix)?
            } else {
                None
            };
            let target = if note.is_empty() {
                Some(referring.clone())
            } else {
                resolve_wikilink(&self.identities, raw)?
            };
            let replacement_target = match (target.as_deref(), note.is_empty(), state) {
                (Some(target), _, Some(true)) if target == source => Some(&destination),
                (_, true, Some(true)) if referring == source => Some(&destination),
                (_, true, Some(false)) if moved_body && referring == destination => Some(&source),
                _ => None,
            };
            if let Some(target) = replacement_target {
                let relative = target
                    .strip_prefix(&self.root)
                    .map_err(|_| ComposeError::InvalidPath)?
                    .to_string_lossy()
                    .replace('\\', "/");
                let relative = relative.strip_suffix(".md").unwrap_or(&relative);
                edits.push(((found + 2)..end, format!("{relative}{suffix}")));
            }
            cursor = end + 2;
        }
        apply_edits(text, edits)
    }

    fn rewrite_canvas(
        &self,
        text: &str,
        referring_note: &Path,
        rewrite: CanvasRewrite<'_>,
    ) -> Result<String, ComposeError> {
        let (source_note, destination, anchors) = match &rewrite {
            CanvasRewrite::Inbound {
                old_target,
                new_target,
            } => (old_target, new_target, None),
            CanvasRewrite::Extraction {
                source_note,
                destination,
                anchors,
            } => (source_note, destination, Some(*anchors)),
        };
        let source_note = source_note
            .canonicalize()
            .map_err(|_| ComposeError::InvalidPath)?;
        if !source_note.starts_with(&self.root) {
            return Err(ComposeError::InvalidPath);
        }
        let destination = canonical_or_new_vault_path(destination, &self.root)?;
        let destination_relative = destination
            .strip_prefix(&self.root)
            .map_err(|_| ComposeError::InvalidPath)?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let mut canvas =
            crate::canvas::CanvasDocument::parse(text).map_err(|_| ComposeError::InvalidCanvas)?;
        let mut edits = Vec::new();

        for node in canvas.nodes() {
            let id = node["id"].as_str().ok_or(ComposeError::InvalidCanvas)?;
            match node["type"].as_str() {
                Some("text") => {
                    let Some(body) = node["text"].as_str() else {
                        continue;
                    };
                    let rewritten = match &rewrite {
                        CanvasRewrite::Inbound {
                            old_target,
                            new_target,
                        } => self.rewrite_inbound_markdown(
                            body,
                            referring_note,
                            old_target,
                            new_target,
                        )?,
                        CanvasRewrite::Extraction {
                            source_note,
                            destination,
                            anchors,
                        } => self.rewrite_extraction_markdown(
                            body,
                            referring_note,
                            source_note,
                            destination,
                            anchors,
                            false,
                        )?,
                    };
                    if rewritten != body {
                        edits.push((id.to_string(), "text", rewritten));
                    }
                }
                Some("file") => {
                    let Some(file) = node["file"].as_str() else {
                        continue;
                    };
                    let subpath = node["subpath"].as_str();
                    if let Some(rewritten) = self.rewrite_canvas_file_reference(
                        file,
                        subpath,
                        &source_note,
                        &destination_relative,
                        anchors,
                    )? {
                        edits.push((id.to_string(), "file", rewritten));
                    }
                }
                Some("group") if anchors.is_none() => {
                    let Some(background) = node["background"].as_str() else {
                        continue;
                    };
                    if let Some(rewritten) = self.rewrite_canvas_file_path(
                        background,
                        &source_note,
                        &destination_relative,
                    ) {
                        edits.push((id.to_string(), "background", rewritten));
                    }
                }
                _ => {}
            }
        }

        for (id, field, value) in edits {
            canvas
                .set_node_field(&id, field, serde_json::Value::String(value))
                .map_err(|_| ComposeError::InvalidCanvas)?;
        }
        canvas.serialize().map_err(|_| ComposeError::InvalidCanvas)
    }

    fn rewrite_canvas_file_reference(
        &self,
        file: &str,
        subpath: Option<&str>,
        source_note: &Path,
        destination: &str,
        anchors: Option<&ExtractionAnchors>,
    ) -> Result<Option<String>, ComposeError> {
        let Some((resolved, suffix)) = self.resolve_canvas_file_path(file) else {
            return Ok(None);
        };
        if resolved != source_note {
            return Ok(None);
        }
        let inline_anchor = canvas_anchor(suffix);
        let separate_anchor = subpath.and_then(canvas_anchor);
        if let (Some(inline), Some(separate)) = (inline_anchor, separate_anchor) {
            if !same_canvas_anchor(inline, separate) {
                if let Some(anchors) = anchors {
                    let inline_moved = anchors.moved(inline)?;
                    let separate_moved = anchors.moved(separate)?;
                    if inline_moved == Some(true) || separate_moved == Some(true) {
                        return Err(ComposeError::AmbiguousLink);
                    }
                    return Ok(None);
                }
                return Err(ComposeError::AmbiguousLink);
            }
        }

        if let Some(anchors) = anchors {
            let moved = match (inline_anchor, separate_anchor) {
                (Some(inline), _) => anchors.moved(inline)? == Some(true),
                (None, Some(separate)) => anchors.moved(separate)? == Some(true),
                (None, None) => false,
            };
            if !moved {
                return Ok(None);
            }
        }
        Ok(Some(format!("{destination}{suffix}")))
    }

    fn rewrite_canvas_file_path(
        &self,
        file: &str,
        source_note: &Path,
        destination: &str,
    ) -> Option<String> {
        let (resolved, suffix) = self.resolve_canvas_file_path(file)?;
        (resolved == source_note).then(|| format!("{destination}{suffix}"))
    }

    fn resolve_canvas_file_path<'a>(&self, file: &'a str) -> Option<(PathBuf, &'a str)> {
        let split = file.find(['#', '^']).unwrap_or(file.len());
        let (path, suffix) = file.split_at(split);
        if path.is_empty() || Path::new(path).is_absolute() {
            return None;
        }
        let candidate = self.root.join(path).canonicalize().ok()?;
        (candidate.starts_with(&self.root) && candidate.is_file()).then_some((candidate, suffix))
    }
}

enum CanvasRewrite<'a> {
    Inbound {
        old_target: &'a Path,
        new_target: &'a Path,
    },
    Extraction {
        source_note: &'a Path,
        destination: &'a Path,
        anchors: &'a ExtractionAnchors,
    },
}

fn is_canvas_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "canvas")
}

fn canvas_anchor(value: &str) -> Option<&str> {
    let anchor = value
        .strip_prefix('#')
        .unwrap_or(value)
        .split(['|', '?'])
        .next()
        .unwrap_or_default();
    (!anchor.is_empty()).then_some(anchor)
}

fn same_canvas_anchor(left: &str, right: &str) -> bool {
    let left = decode_percent(left).unwrap_or_else(|| left.to_string());
    let right = decode_percent(right).unwrap_or_else(|| right.to_string());
    match (left.strip_prefix('^'), right.strip_prefix('^')) {
        (Some(left), Some(right)) => left == right,
        (None, None) => note_key(left.trim()) == note_key(right.trim()),
        _ => false,
    }
}

pub fn rebase_markdown_links(
    text: &str,
    from_note: &Path,
    to_note: &Path,
    root: &Path,
) -> Result<String, ComposeError> {
    let mut edits = Vec::new();
    for destination in markdown_destinations(text)? {
        let raw = destination.raw.trim();
        if raw.starts_with('#')
            || raw.starts_with("//")
            || raw.contains("://")
            || raw.starts_with("mailto:")
            || raw.starts_with("data:")
        {
            continue;
        }
        let Some(target) = local_target(raw, from_note, root) else {
            return Err(ComposeError::InvalidPath);
        };
        let (_, suffix) = split_suffix(&destination.raw);
        edits.push((
            destination.range,
            format!("{}{suffix}", relative_url(&target, to_note, root)?),
        ));
    }
    apply_edits(text, edits)
}

#[cfg(test)]
fn rewrite_inbound_links(
    text: &str,
    referring_note: &Path,
    root: &Path,
    notes: &[PathBuf],
    old_target: &Path,
    new_target: &Path,
) -> Result<String, ComposeError> {
    let snapshot = LinkSnapshot::new(root, notes)?;
    snapshot.rewrite_inbound(text, referring_note, old_target, new_target)
}

fn rewrite_inbound_with_index(
    text: &str,
    referring_note: &Path,
    root: &Path,
    index: &IdentityIndex,
    old_target: &Path,
    new_target: &Path,
) -> Result<String, ComposeError> {
    let canonical_root = root.canonicalize().map_err(|_| ComposeError::InvalidPath)?;
    let old_target = old_target
        .canonicalize()
        .map_err(|_| ComposeError::InvalidPath)?;
    if !old_target.starts_with(&canonical_root) {
        return Err(ComposeError::InvalidPath);
    }
    let new_target = canonical_or_new_vault_path(new_target, &canonical_root)?;
    let mut edits = Vec::new();
    for destination in markdown_destinations(text)? {
        if local_target(&destination.raw, referring_note, root).as_deref() != Some(&old_target) {
            continue;
        }
        let (_, suffix) = split_suffix(&destination.raw);
        edits.push((
            destination.range,
            format!(
                "{}{suffix}",
                relative_url(&new_target, referring_note, root)?
            ),
        ));
    }

    let protected = protected_ranges(text)?;
    let mut cursor = 0;
    while cursor + 1 < text.len() {
        let Some(found) = text[cursor..].find("[[").map(|offset| cursor + offset) else {
            break;
        };
        let mut slash_count = 0;
        let mut preceding = found;
        while preceding > 0 && text.as_bytes()[preceding - 1] == b'\\' {
            slash_count += 1;
            preceding -= 1;
        }
        if slash_count % 2 == 1 {
            cursor = found + 2;
            continue;
        }
        if let Some(range) = protected.iter().find(|range| range.contains(&found)) {
            cursor = range.end.max(found + 2);
            continue;
        }
        let Some(end) = text[found + 2..]
            .find("]]")
            .map(|offset| found + 2 + offset)
        else {
            break;
        };
        let raw = &text[found + 2..end];
        if resolve_wikilink(index, raw)?.as_deref() == Some(&old_target) {
            let suffix = raw
                .find(['#', '|'])
                .map(|at| &raw[at..])
                .unwrap_or_default();
            let relative = new_target
                .strip_prefix(&canonical_root)
                .map_err(|_| ComposeError::InvalidPath)?
                .to_string_lossy()
                .replace('\\', "/");
            let relative = relative.strip_suffix(".md").unwrap_or(&relative);
            edits.push(((found + 2)..end, format!("{relative}{suffix}")));
        }
        cursor = end + 2;
    }
    apply_edits(text, edits)
}

pub fn extract_reference_definitions(
    source: &str,
    section: Range<usize>,
    body: &str,
) -> Result<(String, String), ComposeError> {
    let source_root = parse(source)?;
    let mut definition_nodes = Vec::new();
    fn collect_definitions<'a>(
        node: &'a Node,
        section: &Range<usize>,
        definitions: &mut Vec<(String, Range<usize>, &'a Node)>,
    ) -> Result<(), ComposeError> {
        let identifier = match node {
            Node::Definition(definition) => Some(definition.identifier.to_lowercase()),
            Node::FootnoteDefinition(definition) => {
                Some(format!("footnote:{}", definition.identifier.to_lowercase()))
            }
            _ => None,
        };
        if let Some(identifier) = identifier {
            let position = node.position().ok_or(ComposeError::InvalidMarkdown)?;
            let range = position.start.offset..position.end.offset;
            if range.start < section.end
                && section.start < range.end
                && !(range.start >= section.start && range.end <= section.end)
            {
                return Err(ComposeError::UnsupportedLink);
            }
            definitions.push((identifier, range, node));
        }
        for child in node.children().into_iter().flatten() {
            collect_definitions(child, section, definitions)?;
        }
        Ok(())
    }
    collect_definitions(&source_root, &section, &mut definition_nodes)?;

    let mut moved_references = std::collections::HashSet::new();
    let mut source_references = std::collections::HashSet::new();
    fn collect_references(
        node: &Node,
        section: &Range<usize>,
        moved: &mut std::collections::HashSet<String>,
        remaining: &mut std::collections::HashSet<String>,
    ) -> Result<(), ComposeError> {
        let identifier = match node {
            Node::LinkReference(reference) => Some(reference.identifier.to_lowercase()),
            Node::ImageReference(reference) => Some(reference.identifier.to_lowercase()),
            Node::FootnoteReference(reference) => {
                Some(format!("footnote:{}", reference.identifier.to_lowercase()))
            }
            _ => None,
        };
        if let Some(identifier) = identifier {
            let position = node.position().ok_or(ComposeError::InvalidMarkdown)?;
            let start = position.start.offset;
            let end = position.end.offset;
            let intersects = start < section.end && section.start < end;
            let inside = start >= section.start && end <= section.end;
            if intersects && !inside {
                return Err(ComposeError::UnsupportedLink);
            }
            if inside {
                moved.insert(identifier);
            } else {
                remaining.insert(identifier);
            }
        }
        for child in node.children().into_iter().flatten() {
            collect_references(child, section, moved, remaining)?;
        }
        Ok(())
    }
    // Use the original tree: selected references can point to definitions
    // outside the fragment and would otherwise parse as ordinary text.
    collect_references(
        &source_root,
        &section,
        &mut moved_references,
        &mut source_references,
    )?;

    let definition_text = |range: &Range<usize>| -> Result<String, ComposeError> {
        if range.start > range.end
            || range.end > source.len()
            || !source.is_char_boundary(range.start)
            || !source.is_char_boundary(range.end)
        {
            return Err(ComposeError::InvalidMarkdown);
        }
        Ok(source[range.clone()].to_string())
    };
    let mut definitions_by_id = DefinitionIndex::new();
    for (identifier, range, node) in &definition_nodes {
        definitions_by_id
            .entry(identifier.clone())
            .or_default()
            .push((range.clone(), definition_text(range)?, *node));
    }
    fn close_references(
        references: &mut std::collections::HashSet<String>,
        definitions: &DefinitionIndex<'_>,
    ) -> Result<(), ComposeError> {
        let mut pending = references.iter().cloned().collect::<Vec<_>>();
        while let Some(identifier) = pending.pop() {
            let matches = definitions
                .get(&identifier)
                .ok_or(ComposeError::AmbiguousLink)?;
            if matches.iter().any(|(_, text, _)| text != &matches[0].1) {
                return Err(ComposeError::AmbiguousLink);
            }
            let mut nested = std::collections::HashSet::new();
            for (_, _, node) in matches {
                collect_definition_references(node, &mut nested)?;
            }
            for nested_identifier in nested {
                if references.insert(nested_identifier.clone()) {
                    pending.push(nested_identifier);
                }
            }
        }
        Ok(())
    }
    fn collect_definition_references(
        node: &Node,
        references: &mut std::collections::HashSet<String>,
    ) -> Result<(), ComposeError> {
        match node {
            Node::LinkReference(reference) => {
                references.insert(reference.identifier.to_lowercase());
            }
            Node::ImageReference(reference) => {
                references.insert(reference.identifier.to_lowercase());
            }
            Node::FootnoteReference(reference) => {
                references.insert(format!("footnote:{}", reference.identifier.to_lowercase()));
            }
            _ => {}
        }
        for child in node.children().into_iter().flatten() {
            collect_definition_references(child, references)?;
        }
        Ok(())
    }

    close_references(&mut moved_references, &definitions_by_id)?;
    close_references(&mut source_references, &definitions_by_id)?;

    let mut moved_definitions = Vec::new();
    let mut source_definitions = Vec::new();
    for (identifier, definitions) in &definitions_by_id {
        let Some((_, text, _)) = definitions.first() else {
            continue;
        };
        if moved_references.contains(identifier)
            && definitions
                .iter()
                .all(|(range, _, _)| range.start < section.start || range.end > section.end)
        {
            moved_definitions.push((definitions[0].0.start, text.clone()));
        }
        if source_references.contains(identifier)
            && definitions
                .iter()
                .all(|(range, _, _)| range.start >= section.start && range.end <= section.end)
        {
            source_definitions.push((definitions[0].0.start, text.clone()));
        }
    }
    moved_definitions.sort_by_key(|(offset, _)| *offset);
    source_definitions.sort_by_key(|(offset, _)| *offset);
    let append = |mut base: String, definitions: &[(usize, String)]| {
        for (_, definition) in definitions {
            if !base.ends_with("\n\n") {
                if base.ends_with('\n') {
                    base.push('\n');
                } else {
                    base.push_str("\n\n");
                }
            }
            base.push_str(definition);
        }
        base
    };
    Ok((
        append(body.to_string(), &moved_definitions),
        source_definitions
            .into_iter()
            .map(|(_, definition)| definition)
            .collect::<Vec<_>>()
            .join("\n\n"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(0);

    fn test_root() -> PathBuf {
        let id = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("rista-composer-{}-{id}", std::process::id()));
        fs::create_dir_all(root.join("notes/sub")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("notes/old.md"),
            "---\naliases: [Old Alias]\n---\n# Old\n",
        )
        .unwrap();
        fs::write(root.join("notes/new name.md"), "# New\n").unwrap();
        fs::write(root.join("assets/é image.png"), b"image").unwrap();
        root
    }

    #[test]
    fn rebases_relative_destinations_and_preserves_suffixes() {
        let root = test_root();
        let from = root.join("notes/old.md");
        let to = root.join("notes/sub/new.md");
        let text =
            "[a](../assets/%C3%A9%20image.png#view \"title\")\n![x](<../assets/é image.png>)\n";
        let result = rebase_markdown_links(text, &from, &to, &root).unwrap();
        assert!(result.contains("../../assets/%C3%A9%20image.png#view \"title\""));
        assert!(result.contains("<../../assets/%C3%A9%20image.png>"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rewrites_only_resolved_links_outside_code_and_html() {
        let root = test_root();
        let old = root.join("notes/old.md");
        let new = root.join("notes/new name.md");
        let other = root.join("notes/sub/other.md");
        fs::write(&other, "# Other\n").unwrap();
        assert_eq!(
            local_target("../old.md", &other, &root),
            old.canonicalize().ok()
        );
        assert!(relative_url(&new, &other, &root).is_ok());
        let notes = vec![old.clone(), new.clone(), other.clone()];
        let text = concat!(
            "[[Old Alias#Part|label]]\n",
            "[source](../old.md#Part \"title\")\n",
            "[ref]: ../old.md#Part \"title\"\n",
            "`[[old]] [x](old.md)`\n",
            "```md\n[[old]] [x](old.md)\n```\n",
            "<span>[[old]]</span>\n"
        );
        let result = rewrite_inbound_links(&text, &other, &root, &notes, &old, &new).unwrap();
        assert!(result.contains("[[notes/new name#Part|label]]"));
        assert!(result.contains("[source](../new%20name.md#Part \"title\")"));
        assert!(result.contains("[ref]: ../new%20name.md#Part \"title\""));
        assert!(result.contains("`[[old]] [x](old.md)`"));
        assert!(result.contains("```md\n[[old]] [x](old.md)\n```"));
        assert!(result.contains("<span>[[old]]</span>"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn copies_out_of_section_reference_definitions() {
        let source = "# A\n\nSee [the note][ref] and footnote[^note].\n\n# B\n\n[ref]: https://example.invalid\n\n[^note]: Preserved note.\n";
        let range = 0.."# A\n\nSee [the note][ref] and footnote[^note].\n\n".len();
        let (body, source_definitions) =
            extract_reference_definitions(source, range.clone(), &source[range]).unwrap();
        assert!(body.contains("[ref]: https://example.invalid"));
        assert!(body.contains("[^note]: Preserved note."));
        assert!(source_definitions.is_empty());
    }

    #[test]
    fn preserves_definitions_still_used_by_source_after_extraction() {
        let source = "# Kept\n\nStill [linked][ref].\n\n# Moved\n\nMoved [linked][ref].\n\n[ref]: /same \"title\"\n";
        let start = source.find("# Moved").unwrap();
        let range = start..source.len();
        let (body, source_definitions) =
            extract_reference_definitions(source, range.clone(), &source[range]).unwrap();
        assert!(body.contains("[ref]: /same \"title\""));
        assert!(source_definitions.contains("[ref]: /same \"title\""));
    }

    #[test]
    fn rejects_reference_definitions_with_conflicting_titles() {
        let source = "See [the note][ref].\n\n[ref]: /same \"first title\"\n\n[ref]: /same \"second title\"\n";
        let range = 0.."See [the note][ref].\n\n".len();

        assert_eq!(
            extract_reference_definitions(source, range.clone(), &source[range]).unwrap_err(),
            ComposeError::AmbiguousLink
        );
    }

    #[test]
    fn refuses_to_rebase_a_relative_resource_outside_the_vault() {
        let root = test_root();
        let from = root.join("notes/old.md");
        let to = root.join("notes/sub/new.md");
        let test_name = root.file_name().unwrap().to_string_lossy();
        let outside = root
            .parent()
            .unwrap()
            .join(format!("{test_name}-outside-resource.bin"));
        fs::write(&outside, b"outside").unwrap();

        assert_eq!(
            rebase_markdown_links("[resource](../../outside-resource.bin)", &from, &to, &root,)
                .unwrap_err(),
            ComposeError::InvalidPath
        );

        let _ = fs::remove_file(outside);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn extraction_rewrites_only_moved_anchors_and_supports_new_targets() {
        let root = test_root();
        let source_path = root.join("notes/old.md");
        let other = root.join("notes/sub/other.md");
        let source = "# Stay\n\n[[#Move]]\n\n# Move\n\n[[#Move]] [[#Stay]]\n";
        fs::write(&source_path, source).unwrap();
        fs::write(
            &other,
            "[[Old#Move]] [[Old#Stay]] [moved](../old.md#Move)\n",
        )
        .unwrap();
        let notes = vec![source_path.clone(), other.clone()];
        let snapshot = LinkSnapshot::new(&root, &notes).unwrap();
        let start = source.find("# Move").unwrap();
        let range = start..source.len();
        let anchors = ExtractionAnchors::new(source, range.clone()).unwrap();
        let destination = root.join("notes/sub/moved.md");
        let incoming = snapshot
            .rewrite_extraction_links(
                snapshot.text(&other).unwrap(),
                &other,
                &source_path,
                &destination,
                &anchors,
                false,
            )
            .unwrap();
        assert!(incoming.contains("[[notes/sub/moved#Move]]"));
        assert!(incoming.contains("[[Old#Stay]]"));
        assert!(incoming.contains("moved.md#Move"));

        let body = &source[range];
        let moved = snapshot
            .rewrite_extraction_links(
                body,
                &destination,
                &source_path,
                &destination,
                &anchors,
                true,
            )
            .unwrap();
        assert!(moved.contains("[[notes/old#Stay]]"));
        assert!(moved.contains("[[#Move]]"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn composer_rewrites_canvas_file_and_text_references_losslessly() {
        let root = test_root();
        fs::create_dir_all(root.join("boards/nested")).unwrap();
        fs::create_dir_all(root.join("archive")).unwrap();
        let old = root.join("notes/Årsnotat.md");
        let new = root.join("archive/新しい ノート.md");
        let same_name = root.join("notes/sub/Årsnotat.md");
        let collision = root.join("notes/Årsnotat.canvas");
        let board = root.join("boards/nested/board.canvas");
        let unaffected = root.join("boards/unaffected.canvas");
        let base = root.join("boards/view.base");
        let image = root.join("assets/cover.png");
        fs::write(&old, "# Heading\n").unwrap();
        fs::write(&same_name, "# Different note\n").unwrap();
        fs::write(&collision, r#"{"nodes":[],"edges":[]}"#).unwrap();
        let original = concat!(
            "{\n",
            "  \"futureRoot\": {\"number\": 12345678901234567890.000500, \"text\": \"[[Årsnotat]]\"},\n",
            "  \"nodes\": [\n",
            "    {\"id\":\"text\",\"type\":\"text\",\"x\":0,\"y\":0,\"width\":300,\"height\":120,",
            "\"text\":\"[[Årsnotat#Heading|label]] [nested](../../notes/%C3%85rsnotat.md#Heading)\\n`[[Årsnotat]]`\\n[external](https://example.test/path)\",",
            "\"future\":{\"number\":1.2300,\"text\":\"[[Årsnotat]]\"}},\n",
            "    {\"id\":\"inline\",\"type\":\"file\",\"x\":0,\"y\":0,\"width\":200,\"height\":100,",
            "\"file\":\"notes/Årsnotat.md#Heading\",\"subpath\":\"#Heading\",\"future\":\"keep\"},\n",
            "    {\"id\":\"separate\",\"type\":\"file\",\"x\":0,\"y\":0,\"width\":200,\"height\":100,",
            "\"file\":\"notes/Årsnotat.md\",\"subpath\":\"^block-1\"},\n",
            "    {\"id\":\"group\",\"type\":\"group\",\"x\":0,\"y\":0,\"width\":200,\"height\":100,",
            "\"background\":\"notes/Årsnotat.md\",\"label\":\"[[Årsnotat]]\"},\n",
            "    {\"id\":\"link\",\"type\":\"link\",\"x\":0,\"y\":0,\"width\":200,\"height\":100,",
            "\"url\":\"https://example.test/[[Årsnotat]]\",\"future\":\"[[Årsnotat]]\"},\n",
            "    {\"id\":\"future\",\"type\":\"future\",\"x\":0,\"y\":0,\"width\":200,\"height\":100,",
            "\"file\":\"notes/Årsnotat.md\",\"text\":\"[[Årsnotat]]\"},\n",
            "    {\"id\":\"same-name\",\"type\":\"file\",\"x\":0,\"y\":0,\"width\":200,\"height\":100,",
            "\"file\":\"notes/sub/Årsnotat.md\"}\n",
            "  ],\n",
            "  \"edges\": [{\"id\":\"edge\",\"fromNode\":\"text\",\"toNode\":\"inline\",",
            "\"futureNumber\":9.9900,\"label\":\"[[Årsnotat]]\"}],\n",
            "  \"futureNumber\":9007199254740993.000500,\n",
            "  \"rootLast\":\"keep\"\n",
            "}\n"
        );
        let untouched = "{\n  \"meta\" : \"same bytes\", \"nodes\" : [], \"edges\" : []\n}\n";
        fs::write(&board, original).unwrap();
        fs::write(&unaffected, untouched).unwrap();
        fs::write(&base, "{}").unwrap();
        fs::write(&image, b"image").unwrap();

        let snapshot = LinkSnapshot::new(
            &root,
            &[
                old.clone(),
                collision,
                board.clone(),
                unaffected.clone(),
                base.clone(),
                image.clone(),
            ],
        )
        .unwrap();
        assert!(snapshot.notes().contains(&board.canonicalize().unwrap()));
        assert!(snapshot.text(&board).is_some());
        assert!(!snapshot.notes().contains(&base.canonicalize().unwrap()));
        assert!(!snapshot.notes().contains(&image.canonicalize().unwrap()));

        let rewritten = snapshot
            .rewrite_inbound(original, &board, &old, &new)
            .unwrap();
        let changed: serde_json::Value = serde_json::from_str(&rewritten).unwrap();
        let nodes = changed["nodes"].as_array().unwrap();
        let text = nodes[0]["text"].as_str().unwrap();
        assert!(text.contains("[[archive/新しい ノート#Heading|label]]"));
        assert!(
            text.contains("../../archive/%E6%96%B0%E3%81%97%E3%81%84%20%E3%83%8E%E3%83%BC%E3%83%88.md#Heading"),
            "{text}"
        );
        assert!(text.contains("`[[Årsnotat]]`"));
        assert!(text.contains("https://example.test/path"));
        assert_eq!(nodes[1]["file"], "archive/新しい ノート.md#Heading");
        assert_eq!(nodes[1]["subpath"], "#Heading");
        assert_eq!(nodes[1]["future"], "keep");
        assert_eq!(nodes[2]["file"], "archive/新しい ノート.md");
        assert_eq!(nodes[2]["subpath"], "^block-1");
        assert_eq!(nodes[3]["background"], "archive/新しい ノート.md");
        assert_eq!(nodes[3]["label"], "[[Årsnotat]]");
        assert_eq!(nodes[4]["url"], "https://example.test/[[Årsnotat]]");
        assert_eq!(nodes[4]["future"], "[[Årsnotat]]");
        assert_eq!(nodes[5]["file"], "notes/Årsnotat.md");
        assert_eq!(nodes[5]["text"], "[[Årsnotat]]");
        assert_eq!(nodes[6]["file"], "notes/sub/Årsnotat.md");
        assert_eq!(changed["edges"][0]["label"], "[[Årsnotat]]");
        assert_eq!(changed["rootLast"], "keep");
        assert!(rewritten.contains("12345678901234567890.000500"));
        assert!(rewritten.contains("1.2300"));
        assert!(rewritten.contains("9.9900"));
        assert!(
            rewritten.find("\"id\":\"text\"").unwrap()
                < rewritten.find("\"id\":\"inline\"").unwrap()
        );
        assert!(rewritten.find("\"futureRoot\"").unwrap() < rewritten.find("\"nodes\"").unwrap());
        assert_eq!(
            snapshot
                .rewrite_inbound(untouched, &unaffected, &old, &new)
                .unwrap(),
            untouched
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn extraction_retargets_canvas_file_and_text_anchors_only_when_moved() {
        let root = test_root();
        fs::create_dir_all(root.join("boards")).unwrap();
        let source_path = root.join("notes/Årsnotat.md");
        let destination = root.join("notes/new note.md");
        let board_path = root.join("boards/board.canvas");
        let source = "# Stay\n\nstaying text ^stay-id\n\n# Move\n\nmoved text ^move-id\n";
        fs::write(&source_path, source).unwrap();
        let board = serde_json::json!({
            "nodes": [
                {"id":"heading","type":"file","x":0,"y":0,"width":200,"height":100,
                 "file":"notes/Årsnotat.md#Move"},
                {"id":"block","type":"file","x":0,"y":0,"width":200,"height":100,
                 "file":"notes/Årsnotat.md#^move-id"},
                {"id":"subpath","type":"file","x":0,"y":0,"width":200,"height":100,
                 "file":"notes/Årsnotat.md","subpath":"Move"},
                {"id":"whole","type":"file","x":0,"y":0,"width":200,"height":100,
                 "file":"notes/Årsnotat.md"},
                {"id":"staying","type":"file","x":0,"y":0,"width":200,"height":100,
                 "file":"notes/Årsnotat.md#Stay"},
                {"id":"unknown","type":"file","x":0,"y":0,"width":200,"height":100,
                 "file":"notes/Årsnotat.md#Unknown"},
                {"id":"group","type":"group","x":0,"y":0,"width":200,"height":100,
                 "background":"notes/Årsnotat.md"},
                {"id":"text","type":"text","x":0,"y":0,"width":300,"height":100,
                 "text":"[[Årsnotat#Move]] [[#Move]]"}
            ],
            "edges": []
        })
        .to_string();
        fs::write(&board_path, &board).unwrap();
        let snapshot =
            LinkSnapshot::new(&root, &[source_path.clone(), board_path.clone()]).unwrap();
        let start = source.find("# Move").unwrap();
        let anchors = ExtractionAnchors::new(source, start..source.len()).unwrap();
        let rewritten = snapshot
            .rewrite_extraction_links(
                &board,
                &board_path,
                &source_path,
                &destination,
                &anchors,
                false,
            )
            .unwrap();
        let changed: serde_json::Value = serde_json::from_str(&rewritten).unwrap();
        let nodes = changed["nodes"].as_array().unwrap();
        assert_eq!(nodes[0]["file"], "notes/new note.md#Move");
        assert_eq!(nodes[1]["file"], "notes/new note.md#^move-id");
        assert_eq!(nodes[2]["file"], "notes/new note.md");
        assert_eq!(nodes[2]["subpath"], "Move");
        assert_eq!(nodes[3]["file"], "notes/Årsnotat.md");
        assert_eq!(nodes[4]["file"], "notes/Årsnotat.md#Stay");
        assert_eq!(nodes[5]["file"], "notes/Årsnotat.md#Unknown");
        assert_eq!(nodes[6]["background"], "notes/Årsnotat.md");
        assert_eq!(nodes[7]["text"], "[[notes/new note#Move]] [[#Move]]");

        let conflict = serde_json::json!({
            "nodes": [{
                "id":"conflict","type":"file","x":0,"y":0,"width":200,"height":100,
                "file":"notes/Årsnotat.md#Move","subpath":"Stay"
            }],
            "edges": []
        })
        .to_string();
        assert_eq!(
            snapshot
                .rewrite_extraction_links(
                    &conflict,
                    &board_path,
                    &source_path,
                    &destination,
                    &anchors,
                    false,
                )
                .unwrap_err(),
            ComposeError::AmbiguousLink
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_canvas_fails_with_localized_safe_error() {
        let root = test_root();
        let old = root.join("notes/old.md");
        let new = root.join("notes/new name.md");
        let bad = root.join("notes/bad.canvas");
        fs::write(&bad, "{ not valid Canvas").unwrap();
        let snapshot = LinkSnapshot::new(&root, &[old.clone(), bad.clone()]).unwrap();
        assert_eq!(
            snapshot
                .rewrite_inbound(snapshot.text(&bad).unwrap(), &bad, &old, &new)
                .unwrap_err(),
            ComposeError::InvalidCanvas
        );
        assert_eq!(
            ComposeError::InvalidCanvas.message(false),
            "The board contains invalid Canvas data."
        );
        assert_eq!(
            ComposeError::InvalidCanvas.message(true),
            "Tavlen inneholder ugyldige Canvas-data."
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn respects_escaped_wikilinks_math_and_html_void_elements() {
        let root = test_root();
        let old = root.join("notes/old.md");
        let new = root.join("notes/new name.md");
        let other = root.join("notes/sub/other.md");
        fs::write(&other, "# Other\n").unwrap();
        let notes = vec![old.clone(), new.clone(), other.clone()];
        let text = concat!(
            "<br> [[Old]]\n",
            "<div>[[Old]] [x](../old.md)</div>\n",
            "\\[[Old]]\n",
            "$[[Old]]$\n",
            "[[Old]]\n"
        );
        let result = rewrite_inbound_links(&text, &other, &root, &notes, &old, &new).unwrap();
        assert!(result.starts_with("<br> [[notes/new name]]"));
        assert!(result.contains("<div>[[Old]] [x](../old.md)</div>"));
        assert!(result.contains("\\[[Old]]"));
        assert!(result.contains("$[[Old]]$"));
        assert!(result.ends_with("[[notes/new name]]\n"));
        let _ = fs::remove_dir_all(root);
    }
}
