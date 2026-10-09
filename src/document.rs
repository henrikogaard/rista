//! Document: one open note — editor, preview state, autosave, disk sync.

use crate::history;
use crate::preview;
use crate::settings::Settings;
use gpui_kit::component::input::{EditorState, InputEvent, TabSize, TextDecorationCollection};
use gpui_kit::component::text::TextViewState;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

pub type ImageResolver = Rc<dyn Fn(&str) -> Option<PathBuf>>;

pub enum DocumentEvent {
    /// Content changed (dirty flag may have toggled).
    Changed,
    /// Saved to disk.
    Saved,
    /// Caret moved — the status bar's Ln/Col display tracks it.
    Selection,
    SourceScrolled,
}

impl EventEmitter<DocumentEvent> for Document {}

pub struct Document {
    pub path: PathBuf,
    /// The note's title, shared with its `/` completions so template
    /// items expand `{{title}}` — kept in step by `set_path`.
    completion_title: Rc<std::cell::RefCell<String>>,
    /// Image file — the editor stays empty and the workspace renders
    /// the picture itself instead of the source/preview panes.
    pub is_image: bool,
    pub file_preview: Option<crate::file_preview::Preview>,
    pub editor: Entity<EditorState>,
    /// Collapsible-callout fold state, keyed by callout source offset.
    pub callout_folds: preview::CalloutFolds,
    /// ```` ```base ```` embeds in this note's preview — spec-hash → live view.
    pub base_embeds: preview::EmbedViews,
    pub preview: Entity<TextViewState>,
    pub dirty: bool,
    /// The file on disk changed while we hold unsaved edits.
    pub conflict: bool,
    disk_bytes: Option<Vec<u8>>,
    reloading: bool,
    mtime: Option<SystemTime>,
    revision: u64,
    /// Vault root — history snapshots live under `<root>/.rista/`.
    pub vault_root: Option<PathBuf>,
    save_task: Option<Task<()>>,
    preview_task: Option<Task<()>>,
    image_resolver: ImageResolver,
    /// Frontmatter banner (`banner:`/`cover:`), refreshed with the preview.
    pub banner: Option<preview::BannerSpec>,
    /// `cssclasses:`/`cssclass:` frontmatter — per-note styling hook;
    /// `wide` lifts the readable-width cap, `narrow`/`readable` forces it.
    pub css_classes: Vec<String>,
    /// Cached `(words, chars)` refreshed with the preview.
    pub stats: (usize, usize),
    /// Live-preview emphasis layer over the source editor — bold,
    /// italic, dimmed markers. Lazily created on first refresh.
    decorations: Option<TextDecorationCollection>,
    /// Focus mode: only the block under the caret stays lit —
    /// decorations recompute on cursor moves while this is on.
    pub focus_mode: bool,
    /// Last caret position decorations were computed for. `collection.set`
    /// itself notifies the editor, so without this guard refresh → notify
    /// → refresh would spin forever.
    focus_cursor: Option<usize>,
    /// Last caret offset broadcast as `DocumentEvent::Selection` —
    /// dedupes so unchanged cursors don't re-render the workspace.
    status_cursor: Option<usize>,
    source_scroll_offset: Point<Pixels>,
    /// Vault for link-graph lookups (linked mentions). Absent for
    /// documents opened outside a vault.
    vault: Option<Entity<crate::vault::Vault>>,
    /// Notes linking here — refreshed on open and vault changes.
    pub linked_mentions: Vec<PathBuf>,
    /// What this note links at — `[[wiki]]`, `![[embed]]`,
    /// `[label](path)`; refreshed with the preview debounce.
    pub outgoing_links: Vec<PathBuf>,
    /// `[[targets]]` that don't resolve — shown dimmed.
    pub outgoing_unresolved: Vec<String>,
    preview_line_offsets: Vec<usize>,
    preview_blocks: Vec<(usize, String)>,
    preview_locations: Vec<(usize, usize)>,
    mapped_preview: Option<gpui_kit::component::text::RenderedText>,
    /// Whether the preview's linked-mentions footer is expanded.
    pub mentions_open: bool,
    _subscriptions: Vec<Subscription>,
}

pub(crate) struct InitialDocument {
    path: PathBuf,
    content: String,
    disk_bytes: Option<Vec<u8>>,
    mtime: Option<SystemTime>,
    is_image: bool,
    file_preview: Option<crate::file_preview::Preview>,
}

impl Document {
    pub(crate) fn load_initial(path: &Path) -> io::Result<InitialDocument> {
        let is_image = crate::vault::is_image_file(path);
        let mtime = std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok();
        if !std::fs::metadata(path)?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Not a regular file",
            ));
        }
        if is_image || !crate::file_preview::is_document(path) {
            return Ok(InitialDocument {
                path: path.to_path_buf(),
                content: String::new(),
                disk_bytes: None,
                mtime,
                is_image,
                file_preview: if is_image {
                    None
                } else {
                    Some(crate::file_preview::load(path)?)
                },
            });
        }
        let disk_bytes = std::fs::read(path)?;
        let content = String::from_utf8(disk_bytes.clone())
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        Ok(InitialDocument {
            path: path.to_path_buf(),
            content,
            disk_bytes: Some(disk_bytes),
            mtime,
            is_image,
            file_preview: None,
        })
    }

    pub fn open(
        vault_root: Option<PathBuf>,
        settings: &Settings,
        image_resolver: ImageResolver,
        vault: Option<Entity<crate::vault::Vault>>,
        initial: InitialDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let InitialDocument {
            path,
            content,
            disk_bytes,
            mtime,
            is_image,
            file_preview,
        } = initial;

        let completions_vault = vault.clone();
        let completion_title = Rc::new(std::cell::RefCell::new(note_title(&path)));
        let completions_title = completion_title.clone();
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .language("markdown")
                .line_number(settings.show_line_numbers)
                .soft_wrap(settings.soft_wrap)
                .folding(true)
                .tab_size(TabSize {
                    tab_size: settings.tab_size,
                    hard_tabs: false,
                })
                .searchable(true);
            state.lsp_mut().completion_provider = Some(Rc::new(
                crate::slash::VaultCompletions::new(completions_vault, completions_title),
            ));
            state.set_value(content.clone(), window, cx);
            state
        });

        let doc_dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let (preview_text, preview_line_offsets) = preview::preprocess_mapped(
            &content,
            &path,
            vault_root.as_deref(),
            &*image_resolver,
            &preview::MarkColors::from_theme(cx.theme()),
        );
        let preview = cx.new(|cx| TextViewState::markdown(&preview_text, cx));
        let banner = preview::banner_spec(&content, &doc_dir, &*image_resolver);

        let mut this = Self {
            preview_blocks: preview::navigation_blocks(&preview_text),
            preview_locations: Vec::new(),
            mapped_preview: None,
            preview_line_offsets,
            is_image,
            file_preview,
            path,
            completion_title,
            editor,
            callout_folds: preview::CalloutFolds::default(),
            base_embeds: preview::EmbedViews::default(),
            preview,
            dirty: false,
            conflict: false,
            disk_bytes,
            reloading: false,
            mtime,
            revision: 0,
            vault_root,
            save_task: None,
            preview_task: None,
            image_resolver,
            banner,
            stats: word_stats(&content),
            css_classes: crate::properties::frontmatter_cssclasses(&content),
            decorations: None,
            focus_mode: false,
            focus_cursor: None,
            status_cursor: None,
            source_scroll_offset: Point::default(),
            vault,
            linked_mentions: Vec::new(),
            outgoing_links: Vec::new(),
            outgoing_unresolved: Vec::new(),
            mentions_open: false,
            _subscriptions: Vec::new(),
        };

        this.refresh_decorations(cx);
        this.refresh_linked_mentions(cx);
        this.refresh_outgoing(cx);
        this._subscriptions = vec![
            cx.subscribe_in(&this.editor, window, |this, _editor, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.on_edited(window, cx);
                }
            }),
            // gpui-base never clears `completion.trigger_start_offset`, so a
            // later '/' at an earlier offset would be ignored. While the menu
            // is closed, keep the trigger anchor pinned to the cursor instead.
            cx.observe(&this.editor, |this, editor, cx| {
                let offset = editor.read(cx).scroll_offset();
                if this.source_scroll_offset != offset {
                    this.source_scroll_offset = offset;
                    cx.emit(DocumentEvent::SourceScrolled);
                }
                let cursor = editor.update(cx, |editor, cx| {
                    let menu = editor.completion_menu_state();
                    let cursor = editor.cursor();
                    if menu.open || menu.trigger_start_offset == Some(cursor) {
                        return cursor;
                    }
                    editor.present_completion_items(cursor, "", vec![], cx);
                    cursor
                });
                // Focus mode follows the caret — arrow keys notify too.
                // Skipping unchanged positions breaks the set→notify loop.
                if this.focus_mode && this.focus_cursor != Some(cursor) {
                    this.focus_cursor = Some(cursor);
                    this.refresh_decorations(cx);
                }
                if this.status_cursor != Some(cursor) {
                    this.status_cursor = Some(cursor);
                    cx.emit(DocumentEvent::Selection);
                }
            }),
            cx.observe_global::<gpui_kit::component::Theme>(|this, cx| {
                this.refresh_decorations(cx);
                cx.notify();
            }),
        ];

        this
    }

    pub fn title(&self) -> String {
        note_title(&self.path)
    }

    /// Point the document at a renamed/moved/saved-as file.
    pub fn set_path(&mut self, path: PathBuf) {
        *self.completion_title.borrow_mut() = note_title(&path);
        self.path = path;
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string()
    }

    fn on_edited(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.reloading || self.is_read_only() {
            return;
        }
        if !self.dirty
            && self
                .disk_bytes
                .as_ref()
                .is_some_and(|bytes| bytes.as_slice() == self.editor.read(cx).value().as_bytes())
        {
            return;
        }
        self.dirty = true;
        self.revision += 1;
        let revision = self.revision;
        cx.emit(DocumentEvent::Changed);

        // Preview refresh — short debounce so typing bursts parse once.
        self.preview_task = Some(cx.spawn(async move |this: WeakEntity<Document>, cx| {
            smol::Timer::after(std::time::Duration::from_millis(150)).await;
            let _ = this.update(&mut *cx, |this, cx| {
                if this.revision == revision {
                    this.sync_preview(cx);
                }
            });
        }));

        // Autosave — quiet, after the pause.
        self.save_task = Some(cx.spawn(async move |this: WeakEntity<Document>, cx| {
            smol::Timer::after(std::time::Duration::from_millis(800)).await;
            let _ = this.update(&mut *cx, |this, cx| {
                if this.revision == revision && this.dirty && !this.conflict {
                    let _ = this.save(cx);
                }
            });
        }));
        cx.notify();
    }

    fn sync_preview(&mut self, cx: &mut Context<Self>) {
        let (raw, resolver, doc_dir) = {
            let raw = self.editor.read(cx).value();
            (raw, self.image_resolver.clone(), self.doc_dir())
        };
        let (text, offsets) = preview::preprocess_mapped(
            &raw,
            &self.path,
            self.vault_root.as_deref(),
            &*resolver,
            &preview::MarkColors::from_theme(cx.theme()),
        );
        self.preview_line_offsets = offsets;
        self.preview_blocks = preview::navigation_blocks(&text);
        self.mapped_preview = None;
        self.banner = preview::banner_spec(&raw, &doc_dir, &*resolver);
        self.stats = word_stats(&raw);
        self.css_classes = crate::properties::frontmatter_cssclasses(&raw);
        self.preview
            .update(cx, |state, cx| state.set_text(&text, cx));
        self.refresh_decorations(cx);
        self.refresh_outgoing(cx);
    }

    /// Re-resolve this note's outgoing links (editor content → vault
    /// paths) — runs on the preview debounce so typing updates the
    /// sidebar pane without scanning on every keystroke.
    fn refresh_outgoing(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let (links, unresolved) = {
            let raw = self.editor.read(cx).value().to_string();
            vault.read(cx).outgoing_from(&raw, &self.doc_dir())
        };
        if self.outgoing_links != links || self.outgoing_unresolved != unresolved {
            self.outgoing_links = links;
            self.outgoing_unresolved = unresolved;
            cx.emit(DocumentEvent::Changed);
        }
    }

    /// Rebuild the live-emphasis decoration layer — markdown markers
    /// dim, inline styling applied. Runs on the same debounce as the
    /// preview sync, so it lands once per typing burst.
    pub fn set_focus_mode(&mut self, on: bool, cx: &mut Context<Self>) {
        self.focus_mode = on;
        self.focus_cursor = None;
        self.refresh_decorations(cx);
    }

    fn refresh_decorations(&mut self, cx: &mut Context<Self>) {
        let focus = self.focus_mode;
        let items = self.editor.update(cx, |state, cx| {
            let text = state.value().to_string();
            let cursor = focus.then(|| state.cursor());
            crate::decorations::markdown_decorations(&text, cx.theme(), cursor)
        });
        match &self.decorations {
            Some(collection) => collection.set(items, cx),
            None => {
                let collection = self.editor.update(cx, |state, cx| {
                    state.create_decorations_collection(items, cx)
                });
                self.decorations = Some(collection);
            }
        }
    }

    fn doc_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default()
    }

    /// Re-resolve embeds/banners after the vault index changed (e.g. an
    /// image appeared that an `![[embed]]` was waiting on). Safe on dirty
    /// docs — only the preview surface is touched.
    pub fn resync_preview(&mut self, cx: &mut Context<Self>) {
        self.sync_preview(cx);
        self.refresh_linked_mentions(cx);
    }

    /// Notes that `[[link]]` here — drives the preview's mentions footer.
    /// Only runs on open/vault events, not the typing debounce: the scan
    /// reads every note and inbound links only change elsewhere.
    fn refresh_linked_mentions(&mut self, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let path = self.path.clone();
        self.linked_mentions = vault.read(cx).backlinks_to(&path);
    }

    /// Move the caret to the start of a 1-based line (outline jump).
    pub fn jump_to_line(&mut self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.reveal_preview_line(line.saturating_sub(1), cx);
        self.editor.update(cx, |editor, cx| {
            let text = editor.value();
            let at = text
                .split_inclusive('\n')
                .take(line.saturating_sub(1))
                .map(|s| s.len())
                .sum::<usize>()
                .min(text.len());
            editor.set_selected_range(at..at, cx);
            editor.focus(window, cx);
        });
    }

    pub fn reveal_preview_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let Some(&source) = self.preview_line_offsets.get(line) else {
            return;
        };
        let rendered = self.preview.read(cx).rendered_text();
        if self.mapped_preview.as_ref() != Some(&rendered) {
            self.preview_locations =
                preview::match_navigation_blocks(&self.preview_blocks, rendered.as_str());
            self.mapped_preview = Some(rendered);
        }
        let Some(&(_, start)) = self
            .preview_locations
            .iter()
            .rev()
            .find(|(offset, _)| *offset <= source)
        else {
            return;
        };
        self.preview.update(cx, |preview, cx| {
            let _ = preview.reveal_range(start..start, cx);
        });
    }

    pub fn follow_source_scroll(&mut self, cx: &mut Context<Self>) {
        let line = self
            .editor
            .read(cx)
            .visible_row_range()
            .map(|range| range.start);
        if let Some(line) = line {
            self.reveal_preview_line(line, cx);
        }
    }

    /// Append `line` at end of buffer (a leading newline is inserted
    /// when the file doesn't end with one). The caret lands after the
    /// inserted line; autosave persists it.
    pub fn append_line(&mut self, line: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value();
            let len = text.len();
            editor.set_selected_range(len..len, cx);
            let prefix = if len == 0 || text.to_string().ends_with('\n') {
                ""
            } else {
                "\n"
            };
            editor.replace(format!("{prefix}{line}\n"), window, cx);
        });
    }

    /// Splice a blank `|  |  |…` row after 1-based source `line` —
    /// the rendered table's "+ New row" affordance.
    pub fn add_table_row(
        &mut self,
        line: usize,
        cols: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let mut start = 0usize;
            for (ix, l) in text.split_inclusive('\n').enumerate() {
                if ix + 1 == line {
                    let off = start + l.trim_end_matches('\n').len();
                    let row = format!("\n|{}", "  |".repeat(cols.max(1)));
                    editor.set_selected_range(off..off, cx);
                    editor.replace(row, window, cx);
                    return;
                }
                start += l.len();
            }
        });
    }

    /// Append one cell to every source line in `start..=end` — the
    /// "+ column" affordance. `start + 1` is the GFM separator row and
    /// gets ` --- `; other rows get a blank cell. Splices bottom-up so
    /// earlier byte offsets stay valid.
    pub fn add_table_col(
        &mut self,
        start: usize,
        end: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let mut spans = Vec::new();
            let mut byte = 0usize;
            for (ix, l) in text.split_inclusive('\n').enumerate() {
                spans.push((ix + 1, byte));
                byte += l.len();
            }
            for (ix, byte_start) in spans.iter().rev() {
                if *ix < start || *ix > end {
                    continue;
                }
                let content = text[*byte_start..]
                    .split('\n')
                    .next()
                    .unwrap_or_default()
                    .trim_end();
                let cell = if *ix == start + 1 { "---" } else { " " };
                let off = byte_start + content.len();
                editor.set_selected_range(off..off, cx);
                editor.replace(format!(" {cell} |"), window, cx);
            }
        });
    }

    /// Align the markdown table around the caret — pads every cell to
    /// its column's width and normalizes the `|---|` separator
    /// (Advanced Tables style). Returns false when the caret isn't in
    /// a table.
    pub fn format_table(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let caret = editor.selected_range().start.min(text.len());
            let Some((start, end, out)) = format_table_md(&text, caret) else {
                return false;
            };
            editor.set_selected_range(start..end, cx);
            editor.replace(out, window, cx);
            true
        })
    }

    /// Write the buffer to disk only if its loaded bytes are still current.
    pub fn save(&mut self, cx: &mut Context<Self>) -> std::io::Result<()> {
        let text = self.editor.read(cx).value();
        let Some(baseline) = self.disk_bytes.as_deref() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "This document cannot be saved as text",
            ));
        };
        if let Err(err) = disk_matches_baseline(&self.path, baseline) {
            self.mark_conflict(cx);
            return Err(err);
        }
        if let Some(root) = &self.vault_root {
            history::snapshot_before_write(root, &self.path, &text);
        }
        match guarded_write(&self.path, baseline, text.as_bytes()) {
            Ok(()) => {
                self.disk_bytes = Some(text.as_bytes().to_vec());
                self.mtime = std::fs::metadata(&self.path)
                    .and_then(|m| m.modified())
                    .ok();
                self.dirty = false;
                self.conflict = false;
                self.revision += 1;
                self.save_task.take();
                self.sync_preview(cx);
                cx.emit(DocumentEvent::Saved);
                cx.notify();
                Ok(())
            }
            Err(err) => {
                self.mark_conflict(cx);
                Err(err)
            }
        }
    }

    pub fn flush_and_save(&mut self, cx: &mut Context<Self>) -> io::Result<()> {
        if !self.is_read_only()
            && !self.dirty
            && self
                .disk_bytes
                .as_deref()
                .is_some_and(|bytes| bytes != self.editor.read(cx).value().as_bytes())
        {
            self.dirty = true;
            cx.emit(DocumentEvent::Changed);
            cx.notify();
        }
        if self.dirty {
            self.save(cx)
        } else {
            Ok(())
        }
    }

    /// Save the current editor text to a new path; repoints the document at it.
    pub fn save_as(&mut self, path: PathBuf, cx: &mut Context<Self>) -> std::io::Result<()> {
        if self.is_read_only() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Read-only files cannot be saved as text",
            ));
        }
        if same_existing_path(&self.path, &path) {
            return self.save(cx);
        }
        let path = canonical_save_path(&path)?;
        let text = self.editor.read(cx).value();
        std::fs::write(&path, text.as_bytes())?;
        self.set_path(path);
        self.disk_bytes = Some(text.as_bytes().to_vec());
        self.mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        self.dirty = false;
        self.conflict = false;
        self.revision += 1;
        self.save_task.take();
        cx.emit(DocumentEvent::Saved);
        cx.notify();
        Ok(())
    }

    pub fn set_location_context(
        &mut self,
        vault_root: Option<PathBuf>,
        vault: Option<Entity<crate::vault::Vault>>,
        image_resolver: ImageResolver,
        cx: &mut Context<Self>,
    ) {
        self.vault_root = vault_root;
        self.vault = vault;
        self.image_resolver = image_resolver;
        if self.vault.is_none() {
            self.linked_mentions.clear();
            self.outgoing_links.clear();
            self.outgoing_unresolved.clear();
        }
        self.sync_preview(cx);
        self.refresh_linked_mentions(cx);
        cx.notify();
    }

    fn mark_conflict(&mut self, cx: &mut Context<Self>) {
        self.save_task.take();
        self.dirty = true;
        if !self.conflict {
            self.conflict = true;
            cx.emit(DocumentEvent::Changed);
            cx.notify();
        }
    }

    /// Load `text` into the editor as a user edit (dirty, syncs preview).
    /// Used to restore a history snapshot.
    pub fn restore_text(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.set_value(text, window, cx);
        });
        self.dirty = true;
        self.revision += 1;
        self.sync_preview(cx);
        cx.emit(DocumentEvent::Changed);
        cx.notify();
    }

    /// Insert an expanded template at the caret. Template frontmatter
    /// merges into the note's (see `templater::merge_into`) and only the
    /// body goes in at the caret; `cursor` (an offset into `expanded`)
    /// marks where the caret lands.
    pub fn insert_template(
        &mut self,
        expanded: &str,
        cursor: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            let doc = editor.value().to_string();
            let (frontmatter, body) = crate::templater::merge_into(&doc, expanded);
            // `body` is a suffix of `expanded`.
            let skipped = expanded.len() - body.len();
            let mut start = editor.cursor();
            if let Some((range, text)) = frontmatter {
                if range.start <= start {
                    start = start + text.len() - range.len();
                }
                editor.set_selected_range(range, cx);
                editor.replace(text, window, cx);
            }
            editor.set_selected_range(start..start, cx);
            editor.insert(body, window, cx);
            if let Some(at) = cursor.and_then(|c| c.checked_sub(skipped)) {
                editor.set_selected_range(start + at..start + at, cx);
            }
        });
    }

    /// Replace the note's YAML frontmatter block. `body` is YAML source
    /// without the `---` delimiters; `None` removes the block entirely.
    /// One undo unit, cursor lands at the end of the block.
    pub fn set_properties(
        &mut self,
        body: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.editor.read(cx).value().to_string();
        let span = crate::properties::frontmatter_span(&text);
        self.editor.update(cx, |editor, cx| match (span, body) {
            (Some(span), Some(body)) => {
                let block = format!("---\n{}\n---\n", body.trim_end_matches('\n'));
                editor.set_selected_range(span, cx);
                editor.replace(block, window, cx);
            }
            (Some(mut span), None) => {
                // Removing everything — also swallow a following blank line
                // so the note doesn't gain a leading empty line.
                if text[span.end..].starts_with('\n') {
                    span.end += 1;
                }
                editor.set_selected_range(span, cx);
                editor.replace("", window, cx);
            }
            (None, Some(body)) => {
                let block = format!("---\n{}\n---\n\n", body.trim_end_matches('\n'));
                editor.set_selected_range(0..0, cx);
                editor.replace(block, window, cx);
            }
            (None, None) => {}
        });
    }

    /// Append `prop` to the `order:` list of a `.base` spec's
    /// `view_ix`-th view — the column chooser writes through here.
    /// Returns false when the spec can't be located/edited.
    pub fn add_base_column(
        &mut self,
        view_ix: usize,
        prop: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) = crate::bases::splice_order(&text, view_ix, prop) else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Insert `prop` into the `view_ix`-th view's `order:` next to
    /// `anchor` (before when `after` is false, after otherwise) — the
    /// header "Insert column left/right" write path. Falls back to
    /// appending when the anchor isn't listed.
    pub fn insert_base_column(
        &mut self,
        view_ix: usize,
        prop: &str,
        anchor: &str,
        after: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) =
            crate::bases::splice_order_at(&text, view_ix, prop, anchor, after)
        else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Set/delete `entry: value` inside the `view_ix`-th view's
    /// `{map_key}:` nested mapping — the header "Summarize…" write path
    /// (`summaries:`). `None` deletes the entry, dropping the map key
    /// when it was the last one. Returns false when the view or map
    /// can't be located.
    pub fn set_base_view_map_entry(
        &mut self,
        view_ix: usize,
        map_key: &str,
        entry_key: &str,
        value: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) =
            crate::bases::splice_view_map_entry(&text, view_ix, map_key, entry_key, value)
        else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Set/delete `properties: {prop: {displayName: v}}` at spec root —
    /// the header "Rename column" write path. `None` removes the
    /// displayName (header falls back to the prop name). Returns false
    /// when the spec can't be edited.
    pub fn set_base_display_name(
        &mut self,
        prop: &str,
        value: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) = crate::bases::splice_root_display_name(&text, prop, value)
        else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Set/delete `formulas: {name: 'expr'}` at spec root — the
    /// formula-column "Edit formula" write path. `None` removes the
    /// formula (and `formulas:` when it was the last). Returns false
    /// when the spec can't be edited.
    pub fn set_base_formula(
        &mut self,
        name: &str,
        expr: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let value = expr.map(|e| format!("'{}'", crate::bases::yaml_squote(e)));
        let Some((start, end, insert)) =
            crate::bases::splice_root_map_entry(&text, "formulas", name, value.as_deref())
        else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Drop `prop` from the `order:` list of a `.base` spec's
    /// `view_ix`-th view — the header "Hide column" write path.
    /// `current_cols` is the displayed column set, needed when the
    /// spec has no `order:` to remove from. Returns false when the
    /// prop isn't listed or the view can't be located.
    pub fn remove_base_column(
        &mut self,
        view_ix: usize,
        prop: &str,
        current_cols: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) =
            crate::bases::drop_order(&text, view_ix, prop, current_cols)
        else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Rewrite the `view_ix`-th view's `order:` with `cols` — the
    /// header drag-reorder write path. Returns false when the view
    /// can't be located.
    pub fn reorder_base_columns(
        &mut self,
        view_ix: usize,
        cols: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) = crate::bases::reorder_order(&text, view_ix, cols) else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Append a `{type: kind, name:}` item to the spec's `views:` list —
    /// the "+ view" tab writes through here. Returns false when the
    /// spec can't be edited.
    pub fn add_base_view(
        &mut self,
        name: &str,
        kind: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) = crate::bases::splice_view(&text, name, kind) else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Rewrite the `view_ix`-th view's `name:` — the tab "Rename view"
    /// write path. Returns false when the view can't be located.
    pub fn rename_base_view(
        &mut self,
        view_ix: usize,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) = crate::bases::splice_name(&text, view_ix, name) else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Set or clear a scalar key on the `view_ix`-th view item —
    /// `group_by:` from the tab "Group by…" menu. Returns false when
    /// the view (or a key to clear) can't be located.
    pub fn set_base_view_key(
        &mut self,
        view_ix: usize,
        key: &str,
        value: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) = crate::bases::splice_view_key(&text, view_ix, key, value)
        else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Append a filter expression to the `view_ix`-th view's `filters:`
    /// — the tab "Filter by…" write path. `op` is one of the picker
    /// ids (`is`, `is not`, `contains`, `does not contain`, `>` `<`
    /// `>=` `<=`, `is empty`, `is not empty`); the empty operators
    /// ignore `value`. Returns false when the view can't be located.
    pub fn add_base_view_filter(
        &mut self,
        view_ix: usize,
        prop: &str,
        op: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Numbers and booleans write unquoted so they compare against
        // typed frontmatter — everything else is a string literal.
        let bare = value.parse::<f64>().is_ok() || value == "true" || value == "false";
        let lit = if bare {
            value.to_string()
        } else {
            format!("\"{value}\"")
        };
        let expr = match op {
            "is" => format!("{prop} == {lit}"),
            "is not" => format!("{prop} != {lit}"),
            "contains" => format!("{prop}.contains({lit})"),
            "does not contain" => format!("!{prop}.contains({lit})"),
            ">" | "<" | ">=" | "<=" => format!("{prop} {op} {lit}"),
            "is empty" => format!("{prop}.isEmpty()"),
            "is not empty" => format!("!{prop}.isEmpty()"),
            _ => format!("{prop} == {lit}"),
        };
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) = crate::bases::splice_view_filter(&text, view_ix, &expr)
        else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Remove the `term_ix`-th expression item under the `view_ix`-th
    /// view's `filters:` — the "Remove filter…" write path. Returns
    /// false when the view or term can't be located.
    pub fn remove_base_view_filter(
        &mut self,
        view_ix: usize,
        term_ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) =
            crate::bases::splice_view_filter_remove(&text, view_ix, term_ix)
        else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Delete the `view_ix`-th view item from the spec — the tab
    /// "Delete view" write path. Returns false when the view can't
    /// be located.
    pub fn remove_base_view(
        &mut self,
        view_ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) = crate::bases::drop_view(&text, view_ix) else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Copy the `view_ix`-th view item right after itself under
    /// `name` — the tab "Duplicate view" write path. Returns false
    /// when the view can't be located.
    pub fn duplicate_base_view(
        &mut self,
        view_ix: usize,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) = crate::bases::duplicate_view(&text, view_ix, name) else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Move the `from`-th view item to position `to` in the spec —
    /// the tab drag-reorder write path. Returns false when the views
    /// can't be located.
    pub fn reorder_base_views(
        &mut self,
        from: usize,
        to: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = self.editor.read(cx).value().to_string();
        let Some((start, end, insert)) = crate::bases::reorder_views(&text, from, to) else {
            return false;
        };
        self.editor.update(cx, |editor, cx| {
            editor.set_selected_range(start..end, cx);
            editor.replace(&insert, window, cx);
        });
        true
    }

    /// Flip the task-list marker (`[ ]`/`[x]`) on `line` (1-based) —
    /// the preview's interactive checkbox writes back into source.
    /// Line numbers survive `preprocess` rewriting; byte offsets don't.
    /// The found `[x]` still has to follow a list bullet before we splice.
    /// Returns false when the line carries no task marker.
    pub fn toggle_task(
        &mut self,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let mut start = 0usize;
            for (n, l) in text.split_inclusive('\n').enumerate() {
                if n + 1 != line {
                    start += l.len();
                    continue;
                }
                let seg = &text[start..start + l.len()];
                let Some(rel) = seg.find('[') else {
                    return false;
                };
                let at = start + rel;
                let next = match (text.as_bytes().get(at + 1), text.as_bytes().get(at + 2)) {
                    (Some(b' '), Some(b']')) => "x",
                    (Some(b'x') | Some(b'X'), Some(b']')) => " ",
                    _ => return false,
                };
                // `- [ ]` / `* [ ]` / `+ [ ]` / `1. [ ]` — the `[` must
                // follow a list bullet on its line.
                let prefix = seg[..rel].trim_end();
                let bullet = prefix.ends_with('-')
                    || prefix.ends_with('*')
                    || prefix.ends_with('+')
                    || prefix
                        .rsplit(' ')
                        .next()
                        .and_then(|t| t.strip_suffix('.'))
                        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
                if !bullet {
                    return false;
                }
                editor.set_selected_range(at + 1..at + 2, cx);
                editor.replace(next.to_string(), window, cx);
                return true;
            }
            false
        })
    }

    /// `toggle_task` for the caret's line — the ⌘⏎ / palette entry point
    /// used while editing source.
    pub fn toggle_task_at_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let line = self.editor.update(cx, |editor, _cx| {
            let text = editor.value();
            let cursor = editor.cursor().min(text.len());
            text[..cursor].bytes().filter(|b| *b == b'\n').count() + 1
        });
        self.toggle_task(line, window, cx)
    }

    /// The raw text of the `col`-th pipe cell on `line` (1-based) —
    /// the preview's editable-table dialog prefills from this.
    pub fn table_cell_text(
        &self,
        line: usize,
        col: usize,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.editor.update(cx, |editor, _cx| {
            let text = editor.value().to_string();
            let line_text = text.split('\n').nth(line.saturating_sub(1))?;
            pipe_segments(line_text)
                .get(col)
                .map(|seg| line_text[seg.clone()].trim().to_string())
        })
    }

    /// Replace the `col`-th pipe cell on `line` (1-based) — the preview's
    /// table editor writes back into source. The pipes stay put; the
    /// cell's content becomes ` {text} `. Returns false when the line
    /// has no such cell.
    pub fn set_table_cell(
        &mut self,
        line: usize,
        col: usize,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let mut start = 0usize;
            for (n, l) in text.split_inclusive('\n').enumerate() {
                if n + 1 != line {
                    start += l.len();
                    continue;
                }
                let line_end = l.strip_suffix('\n').map(|_| l.len() - 1).unwrap_or(l.len());
                let Some(seg) = pipe_segments(&l[..line_end]).get(col).cloned() else {
                    return false;
                };
                // An unescaped `|` would break the row's cells apart.
                let escaped = new_text.trim().replace('|', "\\|");
                editor.set_selected_range(start + seg.start..start + seg.end, cx);
                editor.replace(format!(" {escaped} "), window, cx);
                return true;
            }
            false
        })
    }

    /// The link covering the caret — `[[wikilink]]`, `![[embed]]`,
    /// `[label](url)`, or a bare URL token — if there is one. Links are
    /// scanned on the caret's line only; markdown links never span lines.
    pub fn link_at_cursor(&mut self, cx: &mut Context<Self>) -> Option<LinkTarget> {
        self.editor.update(cx, |editor, _cx| {
            let text = editor.value().to_string();
            let cursor = editor.cursor().min(text.len());
            let line_start = text[..cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let line_end = text[cursor..]
                .find('\n')
                .map(|i| cursor + i)
                .unwrap_or(text.len());
            let line = &text[line_start..line_end];
            let rel = cursor - line_start;

            // `[[target]]` and `![[embed]]`
            let mut at = 0;
            while let Some(open) = line[at..].find("[[") {
                let open = at + open;
                let Some(close) = line[open + 2..].find("]]") else {
                    break;
                };
                let close = open + 2 + close;
                // The bang in `![[` counts as inside the span.
                let head = if open > 0 && line.as_bytes()[open - 1] == b'!' {
                    open - 1
                } else {
                    open
                };
                if head <= rel && rel <= close + 2 {
                    return Some(LinkTarget::Note(line[open + 2..close].to_string()));
                }
                at = close + 2;
            }

            // `[label](target)` — vault-relative `.md` paths count as
            // note links, `http(s)` as URLs.
            let mut at = 0;
            while let Some(mid) = line[at..].find("](") {
                let mid = at + mid;
                let Some(close) = line[mid + 2..].find(')') else {
                    break;
                };
                let close = mid + 2 + close;
                let Some(open) = line[..mid].rfind('[') else {
                    break;
                };
                if open <= rel && rel <= close {
                    let target = line[mid + 2..close].trim_start_matches('<');
                    let target = target.trim_end_matches('>').trim_start_matches("./");
                    return Some(
                        if target.starts_with("http://") || target.starts_with("https://") {
                            LinkTarget::Url(target.to_string())
                        } else {
                            LinkTarget::Note(target.to_string())
                        },
                    );
                }
                at = close + 1;
            }

            // Bare URL token under the caret.
            let start = line[..rel]
                .rfind(|c: char| c.is_whitespace() || c == '<')
                .map(|i| i + 1)
                .unwrap_or(0);
            let end = line[rel..]
                .find(|c: char| c.is_whitespace() || c == '>')
                .map(|i| rel + i)
                .unwrap_or(line.len());
            let tok = &line[start..end];
            if tok.starts_with("http://") || tok.starts_with("https://") {
                return Some(LinkTarget::Url(tok.to_string()));
            }
            None
        })
    }

    /// Called when the watcher noticed a filesystem change under this path.
    pub fn check_external(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_preview.is_some() {
            let mtime = std::fs::metadata(&self.path)
                .and_then(|m| m.modified())
                .ok();
            if mtime != self.mtime {
                self.file_preview = Some(
                    crate::file_preview::load(&self.path)
                        .unwrap_or(crate::file_preview::Preview::Binary),
                );
                self.mtime = mtime;
                cx.notify();
            }
            return;
        }
        let Some(baseline) = self.disk_bytes.as_deref() else {
            return;
        };
        let current = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.mark_conflict(cx);
                return;
            }
        };
        if current == baseline {
            if self.conflict {
                self.conflict = false;
                cx.emit(DocumentEvent::Changed);
                cx.notify();
            }
            return;
        }
        if self.dirty || self.editor.read(cx).value().as_bytes() != baseline {
            self.mark_conflict(cx);
            return;
        }
        if self.reload(window, cx).is_err() {
            self.mark_conflict(cx);
        }
    }

    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) -> io::Result<()> {
        if self.is_read_only() {
            return Ok(());
        }
        let bytes = std::fs::read(&self.path)?;
        let content = String::from_utf8(bytes.clone())
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        self.save_task.take();
        self.revision += 1;
        self.reloading = true;
        self.editor.update(cx, |editor, cx| {
            editor.set_value(content, window, cx);
        });
        self.reloading = false;
        self.disk_bytes = Some(bytes);
        self.mtime = std::fs::metadata(&self.path)
            .and_then(|meta| meta.modified())
            .ok();
        self.sync_preview(cx);
        self.dirty = false;
        self.conflict = false;
        cx.emit(DocumentEvent::Changed);
        cx.notify();
        Ok(())
    }

    pub fn is_read_only(&self) -> bool {
        self.is_image || self.file_preview.is_some()
    }

    /// Move the lines covered by the selection up or down by one line,
    /// rich (⌥↑/⌥↓). Selection is expanded to whole lines and
    /// follows the moved block.
    pub fn move_block(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let sel = editor.selected_range();
            let line_start = |i: usize| {
                text[..i.min(text.len())]
                    .rfind('\n')
                    .map(|j| j + 1)
                    .unwrap_or(0)
            };
            let line_end = |i: usize| {
                text[i.min(text.len())..]
                    .find('\n')
                    .map(|j| i.min(text.len()) + j + 1)
                    .unwrap_or(text.len())
            };
            fn strip(s: &str) -> &str {
                s.strip_suffix('\n').unwrap_or(s)
            }

            let start = line_start(sel.start);
            // A selection ending exactly on a line start doesn't include that line.
            let end_anchor = if sel.end > sel.start && text[..sel.end].ends_with('\n') {
                sel.end - 1
            } else {
                sel.end
            };
            let end = line_end(end_anchor);
            let block = &text[start..end];

            let (span, replacement, delta, negate) = if down {
                if end >= text.len() {
                    return;
                }
                let next_end = line_end(end);
                let next = &text[end..next_end];
                (
                    start..next_end,
                    format!("{}\n{}", strip(next), block),
                    next_end - end,
                    false,
                )
            } else {
                if start == 0 {
                    return;
                }
                let prev_start = line_start(start - 1);
                let prev = &text[prev_start..start];
                (
                    prev_start..end,
                    format!("{}\n{}", strip(block), prev),
                    start - prev_start,
                    true,
                )
            };

            editor.set_selected_range(span, cx);
            editor.replace(replacement, window, cx);
            let (ns, ne) = if negate {
                (sel.start - delta, sel.end - delta)
            } else {
                (sel.start + delta, sel.end + delta)
            };
            editor.set_selected_range(ns..ne, cx);
        });
    }

    /// the list continuation: Enter on a list/quote line
    /// inserts `\n` + the same marker (tasks get a fresh `- [ ] `,
    /// ordered lists increment); Enter on a marker-only line just
    /// strips the marker, ending the list. Returns false outside
    /// lists so the default newline proceeds.
    pub fn continue_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            if !sel.is_empty() {
                return false;
            }
            let text = editor.value().to_string();
            let caret = sel.start.min(text.len());
            let ls = text[..caret].rfind('\n').map(|j| j + 1).unwrap_or(0);
            let le = text[caret..]
                .find('\n')
                .map(|j| caret + j)
                .unwrap_or(text.len());
            let line = &text[ls..le];
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];

            // Table row — Enter appends an empty row with the same
            // column count and lands the caret in its first cell
            //. Separator lines get a normal newline.
            let is_table = trimmed.starts_with('|')
                && trimmed.ends_with('|')
                && trimmed[1..].contains('|')
                && !trimmed
                    .trim_matches('|')
                    .trim()
                    .chars()
                    .all(|c| matches!(c, '-' | ':' | ' '));
            if is_table {
                let cols = trimmed.matches('|').count().saturating_sub(1);
                let row = format!("|{}", "  |".repeat(cols));
                editor.set_selected_range(le..le, cx);
                editor.replace(format!("\n{indent}{row}"), window, cx);
                let at = le + 1 + indent.len() + 2;
                editor.set_selected_range(at..at, cx);
                return true;
            }

            let marker: Option<String> = (|| {
                for b in ["-", "*", "+"] {
                    if let Some(rest) = trimmed.strip_prefix(&format!("{b} ")) {
                        for boxed in ["[ ] ", "[x] ", "[X] "] {
                            if let Some(item) = rest.strip_prefix(boxed) {
                                return Some(if item.trim().is_empty() {
                                    String::new()
                                } else {
                                    format!("{b} [ ] ")
                                });
                            }
                        }
                        return Some(if rest.trim().is_empty() {
                            String::new()
                        } else {
                            format!("{b} ")
                        });
                    }
                }
                if let Some(rest) = trimmed.strip_prefix("> ") {
                    return Some(if rest.trim().is_empty() {
                        String::new()
                    } else {
                        "> ".to_string()
                    });
                }
                let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    let after = &trimmed[digits.len()..];
                    for sep in [". ", ") "] {
                        if let Some(item) = after.strip_prefix(sep) {
                            let n: u64 = digits.parse().unwrap_or(0);
                            return Some(if item.trim().is_empty() {
                                String::new()
                            } else {
                                format!("{}{sep}", n + 1)
                            });
                        }
                    }
                }
                None
            })();

            let Some(marker) = marker else {
                return false;
            };
            if marker.is_empty() {
                // Marker-only line — remove it, leaving a bare line.
                editor.set_selected_range(ls..le, cx);
                editor.replace("", window, cx);
                editor.set_selected_range(ls..ls, cx);
            } else {
                editor.set_selected_range(caret..caret, cx);
                editor.replace(format!("\n{indent}{marker}"), window, cx);
            }
            true
        })
    }

    /// Insert `text` at byte `at` as one undo step, keeping the
    /// selection where it was.
    pub fn insert_at(
        &mut self,
        at: usize,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            let shift = |b: usize| if b > at { b + text.len() } else { b };
            let (start, end) = (shift(sel.start), shift(sel.end));
            editor.set_selected_range(at..at, cx);
            editor.insert(text.to_string(), window, cx);
            editor.set_selected_range(start..end, cx);
        });
    }

    /// "Copy link to block" — the `^id` of the block at the caret,
    /// adding a fresh one (one undo step, caret kept) when the block
    /// has none. `None` on blank lines, headings, and frontmatter.
    pub fn ensure_block_id(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let cursor = editor.cursor().min(text.len());
            let (id, insert) = block_anchor(&text, cursor, &new_block_id(&text))?;
            if let Some((at, ins)) = insert {
                let sel = editor.selected_range();
                let shift = |b: usize| if b > at { b + ins.len() } else { b };
                let (start, end) = (shift(sel.start), shift(sel.end));
                editor.set_selected_range(at..at, cx);
                editor.insert(ins, window, cx);
                editor.set_selected_range(start..end, cx);
            }
            Some(id)
        })
    }

    /// Editor menu "Highlight color": recolor the highlight at the caret,
    /// highlight the selection in that color, or start an empty one.
    pub fn set_highlight_color(
        &mut self,
        emoji: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let (range, new_text, caret) =
                highlight_color_edit(&text, editor.selected_range(), emoji);
            editor.set_selected_range(range, cx);
            editor.replace(new_text, window, cx);
            editor.set_selected_range(caret, cx);
        });
    }

    /// Pasting a URL over a selection wraps the selection in
    /// `[selection](url)` — returns false when nothing is selected so
    /// the caller lets the normal paste through.
    pub fn wrap_selection_in_link(
        &mut self,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            if sel.is_empty() {
                return false;
            }
            let text = editor.value().to_string();
            let label = text[sel.clone()].to_string();
            editor.set_selected_range(sel.clone(), cx);
            editor.replace(format!("[{label}]({url})"), window, cx);
            true
        })
    }

    /// Palette "Insert markdown link" — wraps the selection as the
    /// label (`[sel](url)`). `url` comes from the clipboard; when it
    /// is empty the caret lands inside `()` ready to type the target.
    pub fn insert_link(&mut self, url: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            let text = editor.value().to_string();
            let label = text[sel.clone()].to_string();
            let u = url.unwrap_or("");
            let out = format!("[{label}]({u})");
            editor.set_selected_range(sel.clone(), cx);
            editor.replace(out.clone(), window, cx);
            // Caret inside `()` when no url yet, after `)` when there is.
            let pos = sel.start
                + if u.is_empty() {
                    out.len() - 1
                } else {
                    out.len()
                };
            editor.set_selected_range(pos..pos, cx);
        });
    }

    /// Palette "Insert horizontal rule" — a `---` paragraph at the
    /// caret, separated by blank lines (it starts its own line when
    /// the caret is mid-line).
    pub fn insert_hr(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            let text = editor.value().to_string();
            let nl = if sel.start > 0 && !text[..sel.start].ends_with('\n') {
                "\n"
            } else {
                ""
            };
            let out = format!("{nl}---\n\n");
            editor.set_selected_range(sel.clone(), cx);
            editor.replace(out.clone(), window, cx);
            let pos = sel.start + out.len() - 1;
            editor.set_selected_range(pos..pos, cx);
        });
    }

    /// Insert an empty 2×2 markdown table at the caret — header row,
    /// separator, one body row. The caret lands in the first header
    /// cell so a header name can be typed right away (the "Insert table").
    pub fn insert_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            let text = editor.value().to_string();
            let nl = if sel.start > 0 && !text[..sel.start].ends_with('\n') {
                "\n"
            } else {
                ""
            };
            let out = format!("{nl}|  |  |\n| --- | --- |\n|  |  |\n\n");
            editor.set_selected_range(sel.clone(), cx);
            editor.replace(out.clone(), window, cx);
            let pos = sel.start + nl.len() + 2;
            editor.set_selected_range(pos..pos, cx);
        });
    }

    /// Insert a footnote reference `[^n]` at the selection and append
    /// its `[^n]: ` definition at the end of the document, leaving the
    /// caret after the definition marker so the text can be typed
    /// right away — the "Insert footnote".
    pub fn insert_footnote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let mut n = 1usize;
            while text.contains(&format!("[^{n}]")) {
                n += 1;
            }
            editor.replace(format!("[^{n}]"), window, cx);
            let text = editor.value().to_string();
            let tail = if text.ends_with('\n') {
                format!("\n[^{n}]: ")
            } else {
                format!("\n\n[^{n}]: ")
            };
            let end = text.len();
            editor.set_selected_range(end..end, cx);
            editor.replace(tail.clone(), window, cx);
            editor.set_selected_range(end + tail.len()..end + tail.len(), cx);
        });
    }

    /// Wrap the selected lines in an the reference editor callout: a `> [!kind]`
    /// marker line followed by every line quoted `> `. When the block
    /// already is a callout, the marker and quoting come back off. The
    /// `kind` word stays selected so typing replaces it (`note` →
    /// `warning`, `tip`, …).
    pub fn toggle_callout(&mut self, kind: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            let text = editor.value().to_string();
            let mut ls = text[..sel.start].rfind('\n').map(|j| j + 1).unwrap_or(0);
            let mut le = text[sel.end..]
                .find('\n')
                .map(|j| sel.end + j)
                .unwrap_or(text.len());
            // A caret anywhere inside a quote/callout still targets the
            // whole run — climb `>` lines both ways before looking.
            // (Only when the caret's own line is quoted, so a plain
            // line under a callout isn't absorbed.)
            while ls > 0 && text[ls..le].trim_start().starts_with('>') {
                let pe = ls - 1;
                let ps = text[..pe].rfind('\n').map(|j| j + 1).unwrap_or(0);
                if text[ps..pe].trim_start().starts_with('>') {
                    ls = ps;
                } else {
                    break;
                }
            }
            while le < text.len() {
                let ne = text[le + 1..]
                    .find('\n')
                    .map(|j| le + 1 + j)
                    .unwrap_or(text.len());
                if text[le + 1..ne].trim_start().starts_with('>') {
                    le = ne;
                } else {
                    break;
                }
            }
            let block = &text[ls..le];
            let lines: Vec<&str> = block.split('\n').collect();
            let is_callout = lines
                .first()
                .map(|l| l.trim_start().starts_with("> [!") || l.trim_start().starts_with(">!["))
                .unwrap_or(false);
            let (out, caret) = if is_callout {
                let rest: Vec<&str> = lines
                    .iter()
                    .skip(1)
                    .map(|line| {
                        let t = line.trim_start();
                        t.strip_prefix("> ")
                            .or_else(|| t.strip_prefix('>'))
                            .unwrap_or(t)
                    })
                    .collect();
                (rest.join("\n"), ls..ls)
            } else {
                let mut o = String::with_capacity(block.len() + 32);
                o.push_str("> [!");
                o.push_str(kind);
                o.push(']');
                for line in &lines {
                    o.push('\n');
                    if line.trim().is_empty() {
                        o.push('>');
                    } else {
                        o.push_str("> ");
                        o.push_str(line);
                    }
                }
                (o, ls + 4..ls + 4 + kind.len())
            };
            editor.set_selected_range(ls..le, cx);
            editor.replace(out, window, cx);
            editor.set_selected_range(caret, cx);
        });
    }

    /// Duplicate the line(s) covered by the selection (⌘D). The copy
    /// lands right below and the selection follows it.
    pub fn duplicate_block(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let sel = editor.selected_range();
            let line_start = |i: usize| {
                text[..i.min(text.len())]
                    .rfind('\n')
                    .map(|j| j + 1)
                    .unwrap_or(0)
            };
            let line_end = |i: usize| {
                text[i.min(text.len())..]
                    .find('\n')
                    .map(|j| i.min(text.len()) + j + 1)
                    .unwrap_or(text.len())
            };
            let start = line_start(sel.start);
            let end_anchor = if sel.end > sel.start && text[..sel.end].ends_with('\n') {
                sel.end - 1
            } else {
                sel.end
            };
            let end = line_end(end_anchor);
            let stripped = text[start..end]
                .strip_suffix('\n')
                .unwrap_or(&text[start..end]);
            editor.set_selected_range(start..end, cx);
            editor.replace(format!("{stripped}\n{stripped}"), window, cx);
            let delta = stripped.len() + 1;
            editor.set_selected_range(sel.start + delta..sel.end + delta, cx);
        });
    }

    /// Toggle `%%` around the selection — the ⌘/ comment. With
    /// no selection it toggles the current line's trimmed span.
    pub fn toggle_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let sel = editor.selected_range();
            let (start, end) = if sel.is_empty() {
                let ls = text[..sel.start.min(text.len())]
                    .rfind('\n')
                    .map(|j| j + 1)
                    .unwrap_or(0);
                let le = text[sel.start.min(text.len())..]
                    .find('\n')
                    .map(|j| sel.start.min(text.len()) + j)
                    .unwrap_or(text.len());
                let line = &text[ls..le];
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return;
                }
                let off = line.len() - line.trim_start().len();
                (ls + off, ls + off + trimmed.len())
            } else {
                (sel.start, sel.end)
            };
            // Markers may sit just outside the selection (the selection
            // left inside `%%…%%` after a wrap, or a manual inner select)
            // — unwrap those rather than double-wrapping.
            let (span, inner) = if start >= 2
                && end + 2 <= text.len()
                && &text[start - 2..start] == "%%"
                && &text[end..end + 2] == "%%"
            {
                (start - 2..end + 2, Some(text[start..end].to_string()))
            } else if end - start >= 4
                && text[start..end].starts_with("%%")
                && text[start..end].ends_with("%%")
            {
                (start..end, Some(text[start + 2..end - 2].to_string()))
            } else {
                (start..end, None)
            };
            let (replacement, inner_start, inner_len) = match inner {
                Some(inner) => (inner.clone(), span.start, inner.len()),
                None => (format!("%%{}%%", &text[start..end]), start + 2, end - start),
            };
            editor.set_selected_range(span, cx);
            editor.replace(replacement, window, cx);
            editor.set_selected_range(inner_start..inner_start + inner_len, cx);
        });
    }

    /// Toggle `marker` (`**`, `*`, `~~`, `==`) around the selection —
    /// the inline formatting toggle. With no selection it wraps
    /// the word under the caret; already-wrapped text unwraps.
    pub fn toggle_wrap(&mut self, marker: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.value().to_string();
            let sel = editor.selected_range();
            let word_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
            let (start, end) = if sel.is_empty() {
                let i = sel.start.min(text.len());
                let bytes = text.as_bytes();
                let mut s = i;
                while s > 0 && word_char(bytes[s - 1]) {
                    s -= 1;
                }
                let mut e = i;
                while e < bytes.len() && word_char(bytes[e]) {
                    e += 1;
                }
                if s == e {
                    return;
                }
                (s, e)
            } else {
                (sel.start, sel.end)
            };
            let m = marker.len();
            let (span, inner) = if start >= m
                && end + m <= text.len()
                && &text[start - m..start] == marker
                && &text[end..end + m] == marker
            {
                (start - m..end + m, Some(text[start..end].to_string()))
            } else if end - start >= 2 * m
                && text[start..end].starts_with(marker)
                && text[start..end].ends_with(marker)
            {
                (start..end, Some(text[start + m..end - m].to_string()))
            } else {
                (start..end, None)
            };
            let (replacement, inner_start, inner_len) = match inner {
                Some(inner) => (inner.clone(), span.start, inner.len()),
                None => (
                    format!("{marker}{}{marker}", &text[start..end]),
                    start + m,
                    end - start,
                ),
            };
            editor.set_selected_range(span, cx);
            editor.replace(replacement, window, cx);
            editor.set_selected_range(inner_start..inner_start + inner_len, cx);
        });
    }

    /// Toggle `> ` on every line the selection touches — the "Blockquote" command. Strips it only when every non-empty
    /// line is already quoted.
    pub fn toggle_line_prefix(
        &mut self,
        prefix: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            let text = editor.value().to_string();
            let ls = text[..sel.start.min(text.len())]
                .rfind('\n')
                .map(|j| j + 1)
                .unwrap_or(0);
            let le = text[sel.end.min(text.len())..]
                .find('\n')
                .map(|j| sel.end + j)
                .unwrap_or(text.len());
            let block = &text[ls..le];
            let all_quoted = block
                .lines()
                .filter(|l| !l.trim().is_empty())
                .all(|l| l.trim_start().starts_with(prefix));
            let mut out = String::with_capacity(block.len() + 8);
            for (i, line) in block.split('\n').enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                if all_quoted {
                    let at = line.find(prefix).unwrap_or(line.len());
                    out.push_str(&line[..at]);
                    out.push_str(&line[at + prefix.len()..]);
                } else if line.trim().is_empty() {
                    out.push_str(line);
                } else {
                    out.push_str(prefix);
                    out.push_str(line);
                }
            }
            editor.set_selected_range(ls..le, cx);
            editor.replace(out, window, cx);
            editor.set_selected_range(ls..ls, cx);
        });
    }

    /// Toggle a ``` fence around the selected lines — the palette's
    /// "Code block" command. Unwraps when the selection already sits
    /// between a ``` pair; otherwise wraps whole lines.
    pub fn toggle_fence(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            let text = editor.value().to_string();
            let ls = text[..sel.start.min(text.len())]
                .rfind('\n')
                .map(|j| j + 1)
                .unwrap_or(0);
            let le = text[sel.end.min(text.len())..]
                .find('\n')
                .map(|j| sel.end + j)
                .unwrap_or(text.len());
            let block = &text[ls..le];
            // Wrapped already? `…```\n<block>\n```…`
            let pre = &text[..ls];
            let post = &text[le..];
            if let Some(fence_start) = pre.rfind("```") {
                let fence_line_start = pre[..fence_start].rfind('\n').map(|j| j + 1).unwrap_or(0);
                let off = post.len() - post.trim_start_matches('\n').len();
                let closes = post[off..].starts_with("```");
                let only_fence = pre[fence_line_start..].trim_end_matches('\n') == "```" && closes;
                if only_fence {
                    // Drop "```" + its newline (keep the '\n' ending
                    // the block), then the "```\n" opener line.
                    let close_end = post[off..]
                        .find('\n')
                        .map(|j| le + off + j + 1)
                        .unwrap_or(text.len());
                    editor.set_selected_range(le + off..close_end, cx);
                    editor.replace(String::new(), window, cx);
                    editor.set_selected_range(fence_line_start..ls, cx);
                    editor.replace(String::new(), window, cx);
                    editor.set_selected_range(fence_line_start..fence_line_start, cx);
                    return;
                }
            }
            let wrapped = format!("```\n{block}\n```");
            editor.set_selected_range(ls..le, cx);
            editor.replace(wrapped, window, cx);
            editor.set_selected_range(ls + 4..ls + 4, cx);
        });
    }

    /// Strip the list marker a line starts with (`- `, `* `, `+ `,
    /// `- [x] ` task boxes, `1. `/`1) ` ordered). Returns the rest,
    /// or `None` when the line has no marker.
    fn unlist(line: &str) -> Option<&str> {
        let t = line.trim_start();
        for m in ["- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "+ "] {
            if let Some(rest) = t.strip_prefix(m) {
                return Some(rest);
            }
        }
        let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 {
            let after = &t[digits..];
            for sep in [". ", ") "] {
                if let Some(rest) = after.strip_prefix(sep) {
                    return Some(rest);
                }
            }
        }
        None
    }

    /// Toggle a list marker on every line the selection touches —
    /// the "Toggle bulleted/numbered list/checklist".
    /// `style`: `- `, `- [ ] ` or `1. `; existing markers are
    /// stripped first so list styles convert rather than nest.
    pub fn toggle_list(&mut self, style: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            let text = editor.value().to_string();
            let ls = text[..sel.start.min(text.len())]
                .rfind('\n')
                .map(|j| j + 1)
                .unwrap_or(0);
            let le = text[sel.end.min(text.len())..]
                .find('\n')
                .map(|j| sel.end + j)
                .unwrap_or(text.len());
            let block = &text[ls..le];
            let numbered = style == "1. ";
            let all_marked = block.lines().filter(|l| !l.trim().is_empty()).all(|l| {
                let t = l.trim_start();
                if numbered {
                    let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
                    digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") "))
                } else if style == "- [ ] " {
                    t.starts_with("- [ ] ") || t.starts_with("- [x] ") || t.starts_with("- [X] ")
                } else {
                    t.starts_with(style)
                }
            });
            let mut out = String::with_capacity(block.len() + 16);
            let mut n = 0u64;
            for (i, line) in block.split('\n').enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                if line.trim().is_empty() {
                    out.push_str(line);
                    continue;
                }
                if all_marked {
                    // Remove exactly this style's marker.
                    let t = line.trim_start();
                    let marker_len = if numbered {
                        let digits =
                            t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
                        if digits > 0
                            && (t[digits..].starts_with(". ") || t[digits..].starts_with(") "))
                        {
                            digits + 2
                        } else {
                            0
                        }
                    } else if style == "- [ ] " {
                        ["- [ ] ", "- [x] ", "- [X] "]
                            .iter()
                            .find(|m| t.starts_with(**m))
                            .map(|m| m.len())
                            .unwrap_or(0)
                    } else if t.starts_with(style) {
                        style.len()
                    } else {
                        0
                    };
                    let at = line.find(t).unwrap_or(0);
                    out.push_str(&line[..at]);
                    out.push_str(&line[at + marker_len..]);
                } else {
                    // Convert: strip any existing marker, then add.
                    let indent_len = line.len() - line.trim_start().len();
                    let body = Self::unlist(&line[indent_len..])
                        .unwrap_or(line[indent_len..].trim_start());
                    out.push_str(&line[..indent_len]);
                    if numbered {
                        n += 1;
                        out.push_str(&format!("{n}. "));
                    } else {
                        out.push_str(style);
                    }
                    out.push_str(body);
                }
            }
            editor.set_selected_range(ls..le, cx);
            editor.replace(out, window, cx);
            editor.set_selected_range(ls..ls, cx);
        });
    }

    /// Toggle `#`*`level` + ` ` on every line the selection touches —
    /// the "Heading N". Strips any existing ATX marker first
    /// so `# a` → level 2 becomes `## a`; toggles off (plain text)
    /// when every non-empty line is already that level.
    pub fn toggle_heading(&mut self, level: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            let text = editor.value().to_string();
            let ls = text[..sel.start.min(text.len())]
                .rfind('\n')
                .map(|j| j + 1)
                .unwrap_or(0);
            let le = text[sel.end.min(text.len())..]
                .find('\n')
                .map(|j| sel.end + j)
                .unwrap_or(text.len());
            let block = &text[ls..le];
            let atx = |line: &str| -> (usize, usize) {
                // (heading level, marker length incl. indent) for a line
                // like `   ### text` → (3, 7). (0, 0) when not a heading.
                let t = line.trim_start_matches(' ');
                let hashes = t.len() - t.trim_start_matches('#').len();
                if hashes > 0 && hashes <= 6 && t[hashes..].starts_with(' ') {
                    (hashes, line.len() - t.len() + hashes + 1)
                } else {
                    (0, 0)
                }
            };
            let all_level = block
                .lines()
                .filter(|l| !l.trim().is_empty())
                .all(|l| atx(l).0 == level);
            let prefix = "#".repeat(level) + " ";
            let mut out = String::with_capacity(block.len() + 8);
            for (i, line) in block.split('\n').enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                if line.trim().is_empty() {
                    out.push_str(line);
                    continue;
                }
                let (_, marker_len) = atx(line);
                if all_level {
                    out.push_str(&line[marker_len..]);
                } else {
                    let indent_len = line.len() - line.trim_start_matches(' ').len();
                    out.push_str(&line[..indent_len]);
                    out.push_str(&prefix);
                    out.push_str(&line[marker_len.max(indent_len)..]);
                }
            }
            editor.set_selected_range(ls..le, cx);
            editor.replace(out, window, cx);
            editor.set_selected_range(ls..ls, cx);
        });
    }

    /// ⌘⇧K — delete every line the selection touches, trailing
    /// newline included; the caret lands on the next line (or the
    /// previous one at EOF).
    pub fn delete_lines(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            let text = editor.value().to_string();
            if text.is_empty() {
                return;
            }
            let ls = text[..sel.start.min(text.len())]
                .rfind('\n')
                .map(|j| j + 1)
                .unwrap_or(0);
            let le = text[sel.end.min(text.len())..]
                .find('\n')
                .map(|j| sel.end + j + 1)
                .unwrap_or(text.len());
            // First line has no leading \n — also drop it from the
            // previous line's end so no blank line remains.
            let (drop_start, drop_end, caret) = if le < text.len() {
                (ls, le, ls)
            } else if ls > 0 {
                (ls - 1, le, ls - 1)
            } else {
                (0, le, 0)
            };
            editor.set_selected_range(drop_start..drop_end, cx);
            editor.replace(String::new(), window, cx);
            let len = editor.value().len();
            editor.set_selected_range(caret.min(len)..caret.min(len), cx);
        });
    }

    /// Byte-exact selected text + its range — `None` when the selection
    /// is empty. Used by "extract to new note".
    pub fn selected_text(&self, cx: &App) -> Option<(String, std::ops::Range<usize>)> {
        let editor = self.editor.read(cx);
        let sel = editor.selected_range();
        let text = editor.value().to_string();
        if sel.is_empty() || sel.end > text.len() {
            return None;
        }
        Some((text[sel.clone()].to_string(), sel))
    }

    /// Replace a byte range with new text, leaving the caret at its end.
    pub fn replace_range(
        &mut self,
        range: std::ops::Range<usize>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            let end = range.start + text.len();
            editor.set_selected_range(range, cx);
            editor.replace(text, window, cx);
            editor.set_selected_range(end..end, cx);
        });
    }

    /// Tab/Shift-Tab inside a `|`-table row hops the caret cell to
    /// cell. Only fires when the caret sits on a table
    /// line; the separator row and other text fall through.
    pub fn table_cell_nav(&mut self, backward: bool, cx: &mut Context<Self>) -> bool {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            if !sel.is_empty() {
                return false;
            }
            let text = editor.value().to_string();
            let caret = sel.start.min(text.len());
            let ls = text[..caret].rfind('\n').map(|j| j + 1).unwrap_or(0);
            let le = text[caret..]
                .find('\n')
                .map(|j| caret + j)
                .unwrap_or(text.len());
            let line = &text[ls..le];
            let trimmed = line.trim_start();
            let is_table = trimmed.starts_with('|')
                && trimmed.ends_with('|')
                && trimmed[1..].contains('|')
                && !trimmed
                    .trim_matches('|')
                    .trim()
                    .chars()
                    .all(|c| matches!(c, '-' | ':' | ' '));
            if !is_table {
                return false;
            }
            let bytes = text.as_bytes();
            let next = if backward {
                // Previous `|` strictly before the caret's cell —
                // skip the pipe that opens the current cell.
                let mut scan = caret.min(le);
                while scan > ls && bytes[scan - 1] != b'|' {
                    scan -= 1;
                }
                // `scan` is just after the cell's leading pipe.
                (ls..scan.saturating_sub(1))
                    .rev()
                    .find(|&i| bytes[i] == b'|')
                    .map(|i| i + 1)
            } else {
                (caret..le).find(|&i| bytes[i] == b'|').map(|i| i + 1)
            };
            // Wrap to the adjacent row's first/last cell when this
            // row runs out — the Tab cycle.
            let at = match next {
                Some(at) => at,
                None => {
                    let neighbor = if backward {
                        text[..ls].rfind('\n').map(|j| (j + 1, ls))
                    } else if le < text.len() {
                        Some((le + 1, text.len()))
                    } else {
                        None
                    };
                    let Some((ns, limit)) = neighbor else {
                        return false;
                    };
                    let nle = text[ns..]
                        .find('\n')
                        .map(|j| ns + j)
                        .unwrap_or(limit.min(text.len()));
                    let nline = &text[ns..nle];
                    if !nline.trim_start().starts_with('|') {
                        return false;
                    }
                    if backward {
                        // Last cell of the row above: the `|` before
                        // its trailing pipe.
                        let body = nline.trim_end();
                        if !body.ends_with('|') {
                            return false;
                        }
                        match body[..body.len() - 1].rfind('|').map(|p| ns + p + 1) {
                            Some(at) => at,
                            None => return false,
                        }
                    } else {
                        nline.find('|').map(|i| ns + i + 1).unwrap_or(ns)
                    }
                }
            };
            let mut at = at;
            while at < text.len() && bytes[at] == b' ' {
                at += 1;
            }
            editor.set_selected_range(at..at, cx);
            true
        })
    }

    /// Auto-pair — bound on openers and symmetric chars in the "RistaEditor"
    /// key context. Selection → wrap in the pair and re-select the inner text;
    /// empty caret → insert the pair with the caret inside. Symmetric pairs
    /// typed against their own closer skip over it, doubled markers
    /// (`~~x~~`, `==x==`) close plainly after the same char, and quotes/`~`/
    /// `=`/`%`/`$` don't pair after a letter/digit (apostrophes, `a=b`) —
    /// the rules.
    pub fn insert_pair(&mut self, pair: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.pair_edit(pair, true, window, cx);
    }

    /// Closer key (`)`, `]`, `}`): selection → wrap in the pair; caret against
    /// the closer → step over it; otherwise insert the closer alone.
    pub fn close_pair(&mut self, pair: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        self.pair_edit(pair, false, window, cx);
    }

    /// Backspace with the caret inside an empty pair `( | )`/`" | "` deletes
    /// both chars — the pair-delete. Returns false when the caret
    /// isn't between a pair so the editor's own Backspace runs.
    pub fn delete_pair(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            if sel.start != sel.end {
                return false;
            }
            let text = editor.value().to_string();
            let pos = sel.start;
            let (Some(prev), Some(next)) =
                (text[..pos].chars().next_back(), text[pos..].chars().next())
            else {
                return false;
            };
            let paired = matches!(
                (prev, next),
                ('(', ')') | ('[', ']') | ('{', '}') | ('<', '>')
            ) || (prev == next
                && matches!(prev, '"' | '\'' | '`' | '~' | '=' | '%' | '$' | '*' | '_'));
            if !paired {
                return false;
            }
            editor.set_selected_range(pos - prev.len_utf8()..pos + next.len_utf8(), cx);
            editor.replace("", window, cx);
            true
        })
    }

    fn pair_edit(
        &mut self,
        pair: &'static str,
        opener: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let half = pair.len() / 2;
        let (open, close) = (&pair[..half], &pair[half..]);
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            let text = editor.value().to_string();
            if sel.start != sel.end {
                let inner = text[sel.clone()].to_string();
                editor.replace(format!("{open}{inner}{close}"), window, cx);
                let s = sel.start + open.len();
                editor.set_selected_range(s..s + inner.len(), cx);
                return;
            }
            let pos = sel.start.min(text.len());
            let symmetric = open == close;
            let next_is_close = text[pos..].starts_with(close);
            if next_is_close && (!opener || symmetric) {
                editor.set_selected_range(pos + close.len()..pos + close.len(), cx);
                return;
            }
            if !opener {
                editor.replace(close.to_string(), window, cx);
                return;
            }
            let prev = text[..pos].chars().next_back();
            // Doubled-marker close: `~~`/`==`/`%%`/`$$` typed after the same
            // char completes the closing marker instead of nesting a pair.
            let doubled = symmetric && prev == open.chars().next();
            // Word guard — quotes and markdown pair chars don't pair after a
            // letter/digit (apostrophes, `a=b`), the rule.
            let word_guard = prev.is_some_and(|c| c.is_alphanumeric())
                && matches!(open, "'" | "\"" | "~" | "=" | "%" | "$");
            if doubled || word_guard {
                editor.replace(open.to_string(), window, cx);
                return;
            }
            editor.replace(pair.to_string(), window, cx);
            let s = pos + open.len();
            editor.set_selected_range(s..s, cx);
        });
    }

    /// Tab/Shift-Tab on list lines — the reference editor indents list items two
    /// spaces instead of inserting a tab. Every list line the
    /// selection touches shifts together; non-list selections fall
    /// through to the editor's default behavior.
    pub fn indent_selection(
        &mut self,
        outdent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.editor.update(cx, |editor, cx| {
            let sel = editor.selected_range();
            let text = editor.value().to_string();
            let mut edits: Vec<(std::ops::Range<usize>, String, isize)> = vec![];
            let mut ls = text[..sel.start].rfind('\n').map(|j| j + 1).unwrap_or(0);
            while ls <= sel.end.min(text.len()) {
                let le = text[ls..].find('\n').map(|j| ls + j).unwrap_or(text.len());
                let line = &text[ls..le];
                let trimmed = line.trim_start();
                let marker = ["- ", "* ", "+ ", "> "]
                    .iter()
                    .any(|m| trimmed.starts_with(m))
                    || {
                        let digits = trimmed.len()
                            - trimmed
                                .trim_start_matches(|c: char| c.is_ascii_digit())
                                .len();
                        digits > 0
                            && (trimmed[digits..].starts_with(". ")
                                || trimmed[digits..].starts_with(") "))
                    };
                if marker {
                    if outdent {
                        let take = line
                            .chars()
                            .take_while(|c| c.is_whitespace())
                            .take(2)
                            .map(|c| c.len_utf8())
                            .sum::<usize>();
                        if take > 0 {
                            edits.push((ls..ls + take, String::new(), -(take as isize)));
                        }
                    } else {
                        edits.push((ls..ls, "  ".to_string(), 2));
                    }
                }
                if le >= text.len() {
                    break;
                }
                ls = le + 1;
            }
            if edits.is_empty() {
                return false;
            }
            let (mut s, mut e) = (sel.start as isize, sel.end as isize);
            for (range, _, delta) in &edits {
                for point in [&mut s, &mut e] {
                    if range.start <= *point as usize {
                        *point = (*point + delta).max(range.start as isize);
                    }
                }
            }
            for (range, text, _) in edits.into_iter().rev() {
                editor.set_selected_range(range, cx);
                editor.replace(text, window, cx);
            }
            editor.set_selected_range(s.max(0) as usize..e.max(0) as usize, cx);
            true
        })
    }

    /// Splice a set of byte-range replacements into the source text —
    /// link-safe rename retargets wikilinks this way. Ranges must be
    /// sorted by start and non-overlapping; applied right-to-left so
    /// earlier offsets stay valid.
    pub fn apply_text_edits(
        &mut self,
        edits: crate::vault::TextEdits,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            for (range, text) in edits.into_iter().rev() {
                editor.set_selected_range(range, cx);
                editor.replace(text, window, cx);
            }
        });
    }

    /// Apply live-editing settings onto this editor.
    pub fn apply_settings(
        &mut self,
        settings: &Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            editor.set_soft_wrap(settings.soft_wrap, window, cx);
            editor.set_line_number(settings.show_line_numbers, window, cx);
        });
        cx.notify();
    }
}

/// What `link_at_cursor` resolved — a vault note or an external URL.
pub enum LinkTarget {
    /// `[[target]]` / `![[target]]` / `[label](path.md)` — still the
    /// raw link text; `Vault::resolve_wikilink` maps it to a path.
    Note(String),
    /// `http(s)` destination — opened in the system browser.
    Url(String),
}

/// Byte offsets of the `==` highlight delimiters in `line`, in order —
/// inline code and `===` runs don't count, as for the highlight wash.
pub(crate) fn highlight_delimiters(line: &str) -> Vec<usize> {
    let bytes = line.as_bytes();
    let mut marks = Vec::new();
    let (mut i, mut in_code) = (0, false);
    while i < bytes.len() {
        if bytes[i] == b'`' {
            in_code = !in_code;
        } else if !in_code
            && bytes[i] == b'='
            && bytes.get(i + 1) == Some(&b'=')
            && bytes.get(i + 2) != Some(&b'=')
            && (i == 0 || bytes[i - 1] != b'=')
        {
            marks.push(i);
            i += 2;
            continue;
        }
        i += 1;
    }
    marks
}

/// The edit behind `set_highlight_color`: `(range to replace, new
/// text, selection after)`. A selection becomes `==<emoji>sel==`; a
/// caret inside `==…==` on its line swaps the leading color emoji;
/// anywhere else inserts `==<emoji>==` with the caret inside.
fn highlight_color_edit(
    text: &str,
    sel: std::ops::Range<usize>,
    emoji: &str,
) -> (std::ops::Range<usize>, String, std::ops::Range<usize>) {
    let sel = sel.start.min(text.len())..sel.end.min(text.len());
    if !sel.is_empty() {
        let inner = &text[sel.clone()];
        let start = sel.start + 2 + emoji.len();
        return (
            sel.clone(),
            format!("=={emoji}{inner}=="),
            start..start + inner.len(),
        );
    }
    let at = sel.start;
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    let line = &text[line_start..line_end];
    // `==…==` spans on the line, paired left to right.
    let marks: Vec<usize> = highlight_delimiters(line)
        .into_iter()
        .map(|i| line_start + i)
        .collect();
    for &[open, close] in marks.as_chunks::<2>().0 {
        if (open + 2..=close).contains(&at) {
            let body = open + 2;
            let old = crate::preview::HIGHLIGHT_COLORS
                .iter()
                .find(|(e, _)| text[body..close].starts_with(e))
                .map_or(0, |(e, _)| e.len());
            let shift = |p: usize| {
                if p > body {
                    p + emoji.len() - old.min(p - body)
                } else {
                    p
                }
            };
            let caret = shift(at).max(body + emoji.len());
            return (body..body + old, emoji.to_string(), caret..caret);
        }
    }
    let caret = at + 2 + emoji.len();
    (at..at, format!("=={emoji}=="), caret..caret)
}

fn note_title(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string()
}

pub fn word_stats(text: &str) -> (usize, usize) {
    (text.split_whitespace().count(), text.chars().count())
}

/// The `id` of a line's trailing ` ^id` block marker (or of a lone
/// `^id` line) — the same convention block-ref completion reads.
fn trailing_block_id(line: &str) -> Option<&str> {
    let t = line.trim_end();
    let pos = t.rfind('^')?;
    let id = &t[pos + 1..];
    let valid = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        && (pos == 0 || t.as_bytes()[pos - 1] == b' ');
    valid.then_some(id)
}

/// A random six-character block id not yet used in `text`.
fn new_block_id(text: &str) -> String {
    use std::hash::BuildHasher;
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let state = std::collections::hash_map::RandomState::new();
    for salt in 0u64.. {
        let mut n = state.hash_one((std::time::SystemTime::now(), salt));
        let id: String = (0..6)
            .map(|_| {
                let c = CHARS[(n % CHARS.len() as u64) as usize] as char;
                n /= CHARS.len() as u64;
                c
            })
            .collect();
        if !text.contains(&format!("^{id}")) {
            return id;
        }
    }
    unreachable!()
}

#[derive(Clone, Copy, PartialEq)]
enum BlockKind {
    Blank,
    Heading,
    Anchor,
    List,
    Quote,
    Table,
    /// Fenced code; the payload is the opening fence's line index.
    Code(usize),
    Para,
}

/// The block around byte `cursor` and where its id lives — the
/// "copy link to block" anchor. Paragraphs and list items carry a
/// trailing ` ^id`; quotes, tables, and code fences get a lone `^id`
/// line after the block, set off by blank lines. Returns the id the
/// block already has, or `new_id` plus the `(offset, text)` insert
/// that adds it. `None` on blank lines, headings, and frontmatter.
fn block_anchor(
    text: &str,
    cursor: usize,
    new_id: &str,
) -> Option<(String, Option<(usize, String)>)> {
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut off = 0;
    for l in text.split_inclusive('\n') {
        lines.push((off, l.trim_end_matches('\n').trim_end_matches('\r')));
        off += l.len();
    }
    if text.is_empty() || text.ends_with('\n') {
        lines.push((off, ""));
    }
    let cur = lines.iter().rposition(|(start, _)| *start <= cursor)?;

    // Frontmatter is not a block.
    let mut first = 0;
    if lines[0].1 == "---" {
        if let Some(close) = lines.iter().skip(1).position(|(_, l)| *l == "---") {
            first = close + 2;
        }
    }
    if cur < first {
        return None;
    }

    let mut kinds = vec![BlockKind::Blank; lines.len()];
    let mut ix = first;
    while ix < lines.len() {
        let s = lines[ix].1.trim_start();
        if s.starts_with("```") || s.starts_with("~~~") {
            let fence = &s[..3];
            let open = ix;
            kinds[ix] = BlockKind::Code(open);
            ix += 1;
            while ix < lines.len() {
                kinds[ix] = BlockKind::Code(open);
                ix += 1;
                if lines[ix - 1].1.trim_start().starts_with(fence) {
                    break;
                }
            }
            continue;
        }
        let b = s.as_bytes();
        let hashes = s.chars().take_while(|&c| c == '#').count();
        let digits = s.chars().take_while(char::is_ascii_digit).count();
        kinds[ix] = if s.is_empty() {
            BlockKind::Blank
        } else if (1..=6).contains(&hashes) && matches!(b.get(hashes), None | Some(b' ')) {
            BlockKind::Heading
        } else if s.starts_with('^')
            && trailing_block_id(s).is_some_and(|id| id.len() + 1 == s.trim_end().len())
        {
            BlockKind::Anchor
        } else if s.starts_with('>') {
            BlockKind::Quote
        } else if s.starts_with('|') {
            BlockKind::Table
        } else if (matches!(b[0], b'-' | b'*' | b'+') && matches!(b.get(1), None | Some(b' ')))
            || (digits > 0
                && matches!(b.get(digits), Some(b'.' | b')'))
                && matches!(b.get(digits + 1), None | Some(b' ')))
        {
            BlockKind::List
        } else {
            BlockKind::Para
        };
        ix += 1;
    }

    let kind = kinds[cur];
    let mut last = cur;
    match kind {
        BlockKind::Blank | BlockKind::Heading => return None,
        BlockKind::Anchor => {
            return trailing_block_id(lines[cur].1).map(|id| (id.to_string(), None))
        }
        BlockKind::List => {}
        BlockKind::Para => {
            while kinds.get(last + 1) == Some(&BlockKind::Para) {
                last += 1;
            }
        }
        BlockKind::Quote | BlockKind::Table | BlockKind::Code(_) => {
            while kinds.get(last + 1) == Some(&kind) {
                last += 1;
            }
        }
    }

    let (start, line) = lines[last];
    if matches!(kind, BlockKind::List | BlockKind::Para) {
        if let Some(id) = trailing_block_id(line) {
            return Some((id.to_string(), None));
        }
        let at = start + line.trim_end().len();
        return Some((new_id.to_string(), Some((at, format!(" ^{new_id}")))));
    }

    // Structured block: an existing lone `^id` line right after it,
    // or after one blank line, already names it.
    let next = |n: usize| kinds.get(last + n).copied();
    let anchor_at = match (next(1), next(2)) {
        (Some(BlockKind::Anchor), _) => Some(last + 1),
        (Some(BlockKind::Blank), Some(BlockKind::Anchor)) => Some(last + 2),
        _ => None,
    };
    if let Some(a) = anchor_at {
        return trailing_block_id(lines[a].1).map(|id| (id.to_string(), None));
    }
    let tail = if matches!(next(1), None | Some(BlockKind::Blank)) {
        ""
    } else {
        "\n"
    };
    Some((
        new_id.to_string(),
        Some((start + line.len(), format!("\n\n^{new_id}{tail}"))),
    ))
}

/// A linkable block in a note, for `[[note#^` completion.
pub(crate) struct BlockCandidate {
    /// The block's existing `^id`, if it has one.
    pub id: Option<String>,
    /// Its first line, trimmed — the completion label and the source
    /// of `block_hash_id`.
    pub first_line: String,
    /// Byte offset of that first line.
    pub at: usize,
}

/// Every block `block_anchor` can name, in order.
pub(crate) fn block_candidates(text: &str) -> Vec<BlockCandidate> {
    let mut out: Vec<BlockCandidate> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let first_line = line.trim();
        if first_line.is_empty() {
            continue;
        }
        let Some((id, insert)) = block_anchor(text, start, "") else {
            continue;
        };
        // Lines of one block share its insertion point (or its id).
        let key = match &insert {
            Some((offset, _)) => format!("@{offset}"),
            None => format!("^{id}"),
        };
        if seen.insert(key) {
            out.push(BlockCandidate {
                id: insert.is_none().then_some(id),
                first_line: first_line.to_string(),
                at: start,
            });
        }
    }
    out
}

/// A stable six-character id for a block, from its first line — what
/// `[[note#^` completion links before the target has the id, so a
/// later save can find the block again and add it (FNV-1a, base 36).
pub(crate) fn block_hash_id(first_line: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in first_line.trim().bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    (0..6)
        .map(|_| {
            let c = CHARS[(hash % 36) as usize] as char;
            hash /= 36;
            c
        })
        .collect()
}

/// The edit that gives `text` the block id `id` — `None` when some
/// block already carries it or no block hashes to it.
pub(crate) fn linked_block_id_edit(text: &str, id: &str) -> Option<(usize, String)> {
    if text.lines().any(|l| trailing_block_id(l) == Some(id)) {
        return None;
    }
    let block = block_candidates(text)
        .into_iter()
        .find(|b| b.id.is_none() && block_hash_id(&b.first_line) == id)?;
    block_anchor(text, block.at, id)?.1
}

/// Byte ranges between the pipes of a table-row line — `| a | b |`
/// yields the ` a ` and ` b ` segments. `\|` doesn't split; rows
/// without leading/trailing pipes still yield their edge segments.
fn pipe_segments(line: &str) -> Vec<std::ops::Range<usize>> {
    let bytes = line.as_bytes();
    let mut pipes = vec![];
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'|' && (i == 0 || bytes[i - 1] != b'\\') {
            pipes.push(i);
        }
    }
    let mut segs = vec![];
    if pipes.is_empty() {
        return segs;
    }
    let first = pipes[0];
    if !line[..first].trim().is_empty() {
        segs.push(0..first);
    }
    for w in pipes.windows(2) {
        segs.push(w[0] + 1..w[1]);
    }
    let last = *pipes.last().unwrap_or(&0);
    if !line[last + 1..].trim().is_empty() {
        segs.push(last + 1..line.len());
    }
    segs
}

/// `(byte_start, byte_end, replacement)` realigning the pipe table
/// covering `caret`, or None when the caret isn't on a table row.
/// Cells pad to their column's widest content; `:` alignment
/// markers on the separator row are preserved.
fn format_table_md(src: &str, caret: usize) -> Option<(usize, usize, String)> {
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let mut offs = Vec::with_capacity(lines.len());
    let mut at = 0usize;
    for l in &lines {
        offs.push(at);
        at += l.len();
    }
    let is_row = |l: &str| {
        let t = l.trim_end_matches('\n').trim_start();
        t.starts_with('|') && t.matches('|').count() >= 2
    };
    let is_sep = |l: &str| {
        let t = l.trim_end_matches('\n');
        is_row(t)
            && t.chars()
                .filter(|&c| c != '|' && c != ' ')
                .all(|c| c == '-' || c == ':')
    };
    let ln = (0..lines.len())
        .find(|i| caret >= offs[*i] && caret < offs[*i] + lines[*i].len())
        .unwrap_or_else(|| lines.len().saturating_sub(1));
    if lines.is_empty() || !is_row(lines[ln]) {
        return None;
    }
    let mut s = ln;
    while s > 0 && is_row(lines[s - 1]) {
        s -= 1;
    }
    let mut e = ln;
    while e + 1 < lines.len() && is_row(lines[e + 1]) {
        e += 1;
    }
    if e - s + 1 < 2 || !is_sep(lines[s + 1]) {
        return None;
    }
    fn text_line(l: &str) -> &str {
        l.trim_end_matches('\n')
    }
    let cells_of = |l: &str| {
        pipe_segments(text_line(l))
            .into_iter()
            .map(|r| text_line(l)[r].trim().to_string())
            .collect::<Vec<String>>()
    };
    let cols = (s..=e)
        .filter(|i| !is_sep(lines[*i]))
        .map(|i| cells_of(lines[i]).len())
        .max()?;
    if cols == 0 {
        return None;
    }
    let mut w = vec![0usize; cols];
    for l in &lines[s..=e] {
        if is_sep(l) {
            continue;
        }
        for (i2, c) in cells_of(l).iter().enumerate() {
            w[i2] = w[i2].max(c.chars().count());
        }
    }
    let mut out = String::new();
    for l in &lines[s..=e] {
        if is_sep(l) {
            let marks = cells_of(l);
            out.push('|');
            for (i2, width) in w.iter().enumerate() {
                let m = marks.get(i2).map(|s| s.as_str()).unwrap_or("---");
                let l = m.starts_with(':');
                let r = m.ends_with(':');
                let dashes = (*width).max(3).saturating_sub(l as usize + r as usize);
                out.push(' ');
                if l {
                    out.push(':');
                }
                out.push_str(&"-".repeat(dashes));
                if r {
                    out.push(':');
                }
                out.push_str(" |");
            }
        } else {
            let cs = cells_of(l);
            out.push('|');
            for (i2, width) in w.iter().enumerate() {
                let c = cs.get(i2).map(|s| s.as_str()).unwrap_or_default();
                out.push(' ');
                out.push_str(c);
                out.push_str(&" ".repeat(width.saturating_sub(c.chars().count())));
                out.push_str(" |");
            }
        }
        out.push('\n');
    }
    if !lines[e].ends_with('\n') {
        out.pop();
    }
    Some((offs[s], offs[e] + lines[e].len(), out))
}

fn disk_matches_baseline(path: &Path, baseline: &[u8]) -> io::Result<()> {
    let current = std::fs::read(path)?;
    if current == baseline {
        Ok(())
    } else {
        Err(io::Error::other("File changed on disk"))
    }
}

fn guarded_write(path: &Path, baseline: &[u8], content: &[u8]) -> io::Result<()> {
    disk_matches_baseline(path, baseline)?;
    std::fs::write(path, content)
}

fn same_existing_path(source: &Path, target: &Path) -> bool {
    source == target
        || source
            .canonicalize()
            .ok()
            .zip(target.canonicalize().ok())
            .is_some_and(|(source, target)| source == target)
}

fn canonical_save_path(path: &Path) -> io::Result<PathBuf> {
    if path.exists() {
        return path.canonicalize();
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Save destination has no file name",
        )
    })?;
    Ok(parent.canonicalize()?.join(file_name))
}

#[cfg(test)]
mod tests {
    use super::{
        block_anchor, block_candidates, block_hash_id, format_table_md, guarded_write,
        highlight_color_edit, linked_block_id_edit, new_block_id,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_path() -> PathBuf {
        static NEXT_PATH: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "rista-save-test-{}-{}",
            std::process::id(),
            NEXT_PATH.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn other_files_never_load_into_a_writable_text_buffer() {
        let path = test_path().with_extension("txt");
        std::fs::write(&path, b"Read-only text").unwrap();
        let initial = super::Document::load_initial(&path).unwrap();
        assert!(initial.disk_bytes.is_none());
        assert!(initial.content.is_empty());
        assert_eq!(
            initial.file_preview,
            Some(crate::file_preview::Preview::Text("Read-only text".into()))
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"Read-only text");
        std::fs::write(&path, [0, 255, 0]).unwrap();
        assert_eq!(
            super::Document::load_initial(&path).unwrap().file_preview,
            Some(crate::file_preview::Preview::Binary)
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn guarded_save_accepts_unchanged_bytes() {
        let path = test_path();
        std::fs::write(&path, b"before").unwrap();

        guarded_write(&path, b"before", b"after").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"after");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn guarded_save_rejects_same_length_change_without_watcher() {
        let path = test_path();
        std::fs::write(&path, b"before").unwrap();
        std::fs::write(&path, b"change").unwrap();

        assert!(guarded_write(&path, b"before", b"local!").is_err());

        assert_eq!(std::fs::read(&path).unwrap(), b"change");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn guarded_save_rejects_deletion() {
        let path = test_path();
        std::fs::write(&path, b"before").unwrap();
        std::fs::remove_file(&path).unwrap();

        assert!(guarded_write(&path, b"before", b"local").is_err());

        assert!(!path.exists());
    }

    #[test]
    fn guarded_save_rejects_read_errors_without_writing() {
        let path = test_path();
        std::fs::create_dir(&path).unwrap();

        assert!(guarded_write(&path, b"before", b"local").is_err());

        assert!(path.is_dir());
        std::fs::remove_dir(path).unwrap();
    }

    #[test]
    fn formats_ragged_table() {
        let src = "# Format test\n\n| Name | Status | Age |\n|---|---|---|\n| alpha |done |5|\n| beta-longer | todo | 42 |\n\nafter\n";
        let caret = src.find("alpha").unwrap();
        let (s, e, out) = format_table_md(src, caret).expect("table at caret");
        assert_eq!(
            &src[s..e],
            "| Name | Status | Age |\n|---|---|---|\n| alpha |done |5|\n| beta-longer | todo | 42 |\n"
        );
        println!("{}", out);
    }

    /// Apply `block_anchor` at the `@` in `src` (removed first).
    fn anchor(src: &str) -> (String, String) {
        let cursor = src.find('@').unwrap();
        let text = src.replacen('@', "", 1);
        let (id, insert) = block_anchor(&text, cursor, "new1").unwrap();
        let mut out = text.clone();
        if let Some((at, ins)) = insert {
            out.insert_str(at, &ins);
        }
        (id, out)
    }

    #[test]
    fn block_id_goes_at_the_end_of_a_paragraph() {
        assert_eq!(
            anchor("one\ntw@o\nthree\n\nnext\n"),
            ("new1".into(), "one\ntwo\nthree ^new1\n\nnext\n".into())
        );
        assert_eq!(anchor("solo@"), ("new1".into(), "solo ^new1".into()));
    }

    #[test]
    fn block_id_goes_on_the_list_item_line() {
        assert_eq!(
            anchor("- a\n- b@\n- c\n"),
            ("new1".into(), "- a\n- b ^new1\n- c\n".into())
        );
        assert_eq!(
            anchor("1. fi@rst\n2. second"),
            ("new1".into(), "1. first ^new1\n2. second".into())
        );
    }

    #[test]
    fn existing_block_ids_are_reused() {
        assert_eq!(
            anchor("para@ here ^abc\n"),
            ("abc".into(), "para here ^abc\n".into())
        );
        assert_eq!(
            anchor("> q@uote\n\n^q1\n"),
            ("q1".into(), "> quote\n\n^q1\n".into())
        );
    }

    #[test]
    fn structured_blocks_get_a_lone_id_line() {
        assert_eq!(
            anchor("> a@\n> b\nafter\n"),
            ("new1".into(), "> a\n> b\n\n^new1\n\nafter\n".into())
        );
        assert_eq!(
            anchor("| a |\n|---|\n| 1@ |\n"),
            ("new1".into(), "| a |\n|---|\n| 1 |\n\n^new1\n".into())
        );
        assert_eq!(
            anchor("```\nco@de\n```"),
            ("new1".into(), "```\ncode\n```\n\n^new1".into())
        );
    }

    #[test]
    fn headings_blanks_and_frontmatter_have_no_block() {
        assert!(block_anchor("# Title\n", 3, "x").is_none());
        assert!(block_anchor("a\n\nb", 2, "x").is_none());
        assert!(block_anchor("---\ntag: x\n---\nbody", 6, "x").is_none());
        assert!(block_anchor("---\ntag: x\n---\nbody", 16, "x").is_some());
    }

    #[test]
    fn new_block_ids_are_short_and_unused() {
        let id = new_block_id("");
        assert_eq!(id.len(), 6);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
        assert!(!new_block_id(&format!("x ^{id}")).eq(&id));
    }

    /// Apply `highlight_color_edit` at `@` (a `[`…`]` pair marks a selection).
    fn recolor(src: &str, emoji: &str) -> String {
        let (sel, text) = match (src.find('['), src.find(']')) {
            (Some(a), Some(b)) => (a..b - 1, src.replacen('[', "", 1).replacen(']', "", 1)),
            _ => {
                let at = src.find('@').unwrap();
                (at..at, src.replacen('@', "", 1))
            }
        };
        let (range, new, caret) = highlight_color_edit(&text, sel, emoji);
        let mut out = text.clone();
        out.replace_range(range, &new);
        out.insert(caret.end, '|');
        if caret.start != caret.end {
            out.insert(caret.start, '|');
        }
        out
    }

    #[test]
    fn highlight_colors_wrap_recolor_and_insert() {
        assert_eq!(recolor("a [word] b", "🔴"), "a ==🔴|word|== b");
        assert_eq!(recolor("x ==hi@gh== y", "🔵"), "x ==🔵hi|gh== y");
        assert_eq!(recolor("==🔴r@ed==", "🟢"), "==🟢r|ed==");
        assert_eq!(recolor("==🔴@red==", ""), "==|red==");
        assert_eq!(recolor("==a== b@ ==c==", "🟡"), "==a== b==🟡|== ==c==");
        assert_eq!(recolor("`a==b` ==🔴re@al==", "🟢"), "`a==b` ==🟢re|al==");
    }

    #[test]
    fn block_candidates_list_each_block_once() {
        let text = "# H\n\npara one\npara two\n\n- a\n- b ^bid\n\n> q1\n> q2\n";
        let blocks: Vec<(Option<String>, String)> = block_candidates(text)
            .into_iter()
            .map(|b| (b.id, b.first_line))
            .collect();
        assert_eq!(
            blocks,
            [
                (None, "para one".to_string()),
                (None, "- a".to_string()),
                (Some("bid".to_string()), "- b ^bid".to_string()),
                (None, "> q1".to_string()),
            ]
        );
    }

    #[test]
    fn linked_block_ids_are_added_once_to_the_hashed_block() {
        let text = "intro\n\n- first\n- second\n";
        let id = block_hash_id("- second");
        assert_eq!(block_hash_id("  - second "), id);
        let (at, ins) = linked_block_id_edit(text, &id).unwrap();
        let mut out = text.to_string();
        out.insert_str(at, &ins);
        assert_eq!(out, format!("intro\n\n- first\n- second ^{id}\n"));
        assert_eq!(linked_block_id_edit(&out, &id), None);
        assert_eq!(linked_block_id_edit(text, "zzzzzz"), None);
    }
}
