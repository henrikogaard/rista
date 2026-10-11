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

fn protected_ranges(text: &str) -> Result<Vec<Range<usize>>, ComposeError> {
    let root = parse(text)?;
    let mut ranges = Vec::new();
    fn collect(node: &Node, text: &str, out: &mut Vec<Range<usize>>) {
        if matches!(
            node,
            Node::Code(_)
                | Node::InlineCode(_)
                | Node::Math(_)
                | Node::InlineMath(_)
                | Node::Html(_)
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
                    out.push(start..end);
                }
            }
            return;
        }
        for child in node.children().into_iter().flatten() {
            collect(child, text, out);
        }
    }
    collect(&root, text, &mut ranges);
    ranges.extend(raw_html_ranges(text));
    if let Some(range) = crate::properties::frontmatter_span(text) {
        ranges.push(range);
    }
    ranges.sort_by_key(|range| range.start);
    Ok(ranges)
}

fn raw_html_ranges(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let lower = text.to_ascii_lowercase();
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
        let mut range_end = tag_end + 1;
        if !closing && !contents.ends_with('/') {
            let close = format!("</{}", name.to_ascii_lowercase());
            if let Some(close_start) = lower[range_end..]
                .find(&close)
                .map(|found| range_end + found)
            {
                let close_end = text[close_start..]
                    .find('>')
                    .map(|found| close_start + found + 1)
                    .unwrap_or(text.len());
                range_end = close_end;
            } else {
                range_end = text.len();
            }
        }
        ranges.push(offset..range_end);
        cursor = range_end;
    }
    ranges
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
    let mut out = Vec::new();
    fn collect(node: &Node, text: &str, out: &mut Vec<Destination>) {
        let (position, definition) = match node {
            Node::Link(link) => (link.position.as_ref(), false),
            Node::Image(image) => (image.position.as_ref(), false),
            Node::Definition(definition) => (definition.position.as_ref(), true),
            _ => (None, false),
        };
        if let Some(position) = position {
            if let Some(range) =
                destination_range(text, position.start.offset..position.end.offset, definition)
            {
                out.push(Destination {
                    raw: text[range.clone()].to_string(),
                    range,
                });
            }
        }
        for child in node.children().into_iter().flatten() {
            collect(child, text, out);
        }
    }
    collect(&root, text, &mut out);
    // The Markdown parser keeps a definition immediately following a
    // paragraph as paragraph text. Recover that source-level definition
    // without reparsing an extracted fragment; fenced/code ranges remain
    // excluded by the AST protection map.
    let protected = protected_ranges(text)?;
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
    let target = target
        .canonicalize()
        .map_err(|_| ComposeError::InvalidPath)?;
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

impl LinkSnapshot {
    pub fn new(root: &Path, notes: &[PathBuf]) -> Result<Self, ComposeError> {
        let root = root.canonicalize().map_err(|_| ComposeError::InvalidPath)?;
        let mut index = IdentityIndex::default();
        let mut paths = Vec::new();
        let mut texts = std::collections::HashMap::new();
        let mut total = 0usize;
        for note in notes
            .iter()
            .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
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
        rewrite_inbound_with_index(
            text,
            referring_note,
            &self.root,
            &self.identities,
            old_target,
            new_target,
        )
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
    let old_target = old_target
        .canonicalize()
        .map_err(|_| ComposeError::InvalidPath)?;
    let new_target = new_target
        .canonicalize()
        .map_err(|_| ComposeError::InvalidPath)?;
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
            let canonical_root = root.canonicalize().map_err(|_| ComposeError::InvalidPath)?;
            let relative = new_target
                .strip_prefix(canonical_root)
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
) -> Result<String, ComposeError> {
    let source_root = parse(source)?;
    let mut references = std::collections::HashSet::new();
    fn collect_references(
        node: &Node,
        range: Option<&Range<usize>>,
        references: &mut std::collections::HashSet<String>,
    ) {
        let in_range = range.is_none_or(|range| {
            node.position().is_some_and(|position| {
                position.start.offset < range.end && position.end.offset > range.start
            })
        });
        if !in_range {
            return;
        }
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
            collect_references(child, range, references);
        }
    }
    // Walk the original tree, rather than reparsing the selected bytes. A
    // reference whose definition is outside the selection is otherwise
    // represented as plain text by the fragment parser.
    collect_references(&source_root, Some(&section), &mut references);

    let mut definition_nodes = Vec::new();
    fn collect_definitions<'a>(node: &'a Node, definitions: &mut Vec<(String, &'a Node)>) {
        let identifier = match node {
            Node::Definition(definition) => Some(definition.identifier.to_lowercase()),
            Node::FootnoteDefinition(definition) => {
                Some(format!("footnote:{}", definition.identifier.to_lowercase()))
            }
            _ => None,
        };
        if let Some(identifier) = identifier {
            definitions.push((identifier, node));
        }
        for child in node.children().into_iter().flatten() {
            collect_definitions(child, definitions);
        }
    }
    collect_definitions(&source_root, &mut definition_nodes);
    let available = definition_nodes
        .iter()
        .map(|(identifier, _)| identifier.as_str())
        .collect::<std::collections::HashSet<_>>();
    if references
        .iter()
        .any(|identifier| !available.contains(identifier.as_str()))
    {
        return Err(ComposeError::AmbiguousLink);
    }

    // Footnote bodies may themselves contain references. Resolve those
    // anchors transitively before copying definitions to the extracted note.
    let mut changed = true;
    while changed {
        changed = false;
        for (identifier, node) in &definition_nodes {
            if references.contains(identifier) {
                let before = references.len();
                collect_references(node, None, &mut references);
                changed |= before != references.len();
            }
        }
    }
    let mut definitions = Vec::new();
    let mut seen = std::collections::HashMap::<String, String>::new();
    for (identifier, node) in definition_nodes {
        let Some(position) = node.position() else {
            continue;
        };
        let start = position.start.offset;
        let end = position.end.offset;
        if !references.contains(&identifier)
            || (start >= section.start && end <= section.end)
            || start > end
            || end > source.len()
            || !source.is_char_boundary(start)
            || !source.is_char_boundary(end)
        {
            continue;
        }
        let definition = source[start..end].to_string();
        if let Some(existing) = seen.get(&identifier) {
            if existing != &definition {
                return Err(ComposeError::AmbiguousLink);
            }
        } else {
            seen.insert(identifier, definition.clone());
            definitions.push(definition);
        }
    }
    if definitions.is_empty() {
        return Ok(body.to_string());
    }
    let mut result = body.trim_end().to_string();
    for definition in definitions {
        result.push_str("\n\n");
        result.push_str(&definition);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rista-composer-{id}"));
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
        let body = extract_reference_definitions(source, range.clone(), &source[range]).unwrap();
        assert!(body.contains("[ref]: https://example.invalid"));
        assert!(body.contains("[^note]: Preserved note."));
    }
}
