//! Document: one open note — editor, preview state, autosave, disk sync.

use crate::history;
use crate::preview;
use crate::settings::Settings;
use gpui_kit::component::input::{EditorState, InputEvent, TabSize, TextDecorationCollection};
use gpui_kit::component::text::TextViewState;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use std::path::PathBuf;
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
}

impl EventEmitter<DocumentEvent> for Document {}

pub struct Document {
    pub path: PathBuf,
    /// Image file — the editor stays empty and the workspace renders
    /// the picture itself instead of the source/preview panes.
    pub is_image: bool,
    pub editor: Entity<EditorState>,
    /// Collapsible-callout fold state, keyed by callout source offset.
    pub callout_folds: preview::CalloutFolds,
    /// ```` ```base ```` embeds in this note's preview — spec-hash → live view.
    pub base_embeds: preview::EmbedViews,
    pub preview: Entity<TextViewState>,
    pub dirty: bool,
    /// The file on disk changed while we hold unsaved edits.
    pub conflict: bool,
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
    /// Whether the preview's linked-mentions footer is expanded.
    pub mentions_open: bool,
    _subscriptions: Vec<Subscription>,
}

impl Document {
    pub fn open(
        path: PathBuf,
        vault_root: Option<PathBuf>,
        settings: &Settings,
        image_resolver: ImageResolver,
        vault: Option<Entity<crate::vault::Vault>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

        let completions_vault = vault.clone();
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
                crate::slash::VaultCompletions::new(completions_vault),
            ));
            state.set_value(content.clone(), window, cx);
            state
        });

        let doc_dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let preview_text =
            preview::preprocess(&content, &path, vault_root.as_deref(), &*image_resolver);
        let preview = cx.new(|cx| TextViewState::markdown(&preview_text, cx));
        let banner = preview::banner_spec(&content, &doc_dir, &*image_resolver);

        let mut this = Self {
            is_image: crate::vault::is_image_file(&path),
            path,
            editor,
            callout_folds: preview::CalloutFolds::default(),
            base_embeds: preview::EmbedViews::default(),
            preview,
            dirty: false,
            conflict: false,
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
        ];

        this
    }

    pub fn title(&self) -> String {
        self.path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string()
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string()
    }

    fn on_edited(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
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
                if this.revision == revision && this.dirty {
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
        let text = preview::preprocess(&raw, &self.path, self.vault_root.as_deref(), &*resolver);
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

    /// Write the buffer to disk. Returns the io result for callers that care.
    pub fn save(&mut self, cx: &mut Context<Self>) -> std::io::Result<()> {
        let text = self.editor.read(cx).value();
        if let Some(root) = &self.vault_root {
            history::snapshot_before_write(root, &self.path, &text);
        }
        match std::fs::write(&self.path, text.as_bytes()) {
            Ok(()) => {
                self.mtime = std::fs::metadata(&self.path)
                    .and_then(|m| m.modified())
                    .ok();
                self.dirty = false;
                self.conflict = false;
                cx.emit(DocumentEvent::Saved);
                cx.notify();
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// Save the current editor text to a new path; repoints the document at it.
    pub fn save_as(&mut self, path: PathBuf, cx: &mut Context<Self>) -> std::io::Result<()> {
        let text = self.editor.read(cx).value();
        std::fs::write(&path, text.as_bytes())?;
        self.path = path;
        self.mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        self.dirty = false;
        cx.emit(DocumentEvent::Saved);
        cx.notify();
        Ok(())
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

    /// Insert a template's expanded text at the cursor. `{{date}}`,
    /// `{{time}}`, `{{title}}` expand; `{{cursor}}` marks where the
    /// caret lands after insertion.
    pub fn insert_template(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.title();
        let now = history::epoch();
        let expanded = text
            .replace("{{date}}", &history::format_date(now))
            .replace("{{time}}", &history::format_time(now))
            .replace("{{title}}", &title);
        let cursor_at = expanded.find("{{cursor}}");
        let expanded = expanded.replace("{{cursor}}", "");
        self.editor.update(cx, |editor, cx| {
            let start = editor.cursor();
            editor.insert(expanded, window, cx);
            if let Some(at) = cursor_at {
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
        let Ok(meta) = std::fs::metadata(&self.path) else {
            return;
        };
        let Ok(mtime) = meta.modified() else {
            return;
        };
        if Some(mtime) == self.mtime {
            return;
        }
        if self.dirty {
            // Our write may be in flight; only flag when the mtime clearly
            // diverges from what we last wrote.
            self.conflict = true;
            cx.notify();
            return;
        }
        self.reload(window, cx);
    }

    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok(content) = std::fs::read_to_string(&self.path) {
            self.mtime = std::fs::metadata(&self.path)
                .and_then(|m| m.modified())
                .ok();
            self.editor.update(cx, |editor, cx| {
                editor.set_value(content, window, cx);
            });
            self.sync_preview(cx);
            self.dirty = false;
            self.conflict = false;
            cx.notify();
        }
    }

    /// Move the lines covered by the selection up or down by one line,
    /// Notion-style (⌥↑/⌥↓). Selection is expanded to whole lines and
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

    /// Obsidian's list continuation: Enter on a list/quote line
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
            // (Obsidian parity). Separator lines get a normal newline.
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
    /// cell so a header name can be typed right away (Obsidian's
    /// "Insert table").
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
    /// right away — Obsidian's "Insert footnote".
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

    /// Wrap the selected lines in an Obsidian callout: a `> [!kind]`
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

    /// Toggle `%%` around the selection — Obsidian's ⌘/ comment. With
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
    /// Obsidian's inline formatting toggle. With no selection it wraps
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

    /// Toggle `> ` on every line the selection touches — Obsidian's
    /// "Blockquote" command. Strips it only when every non-empty
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
    /// Obsidian's "Toggle bulleted/numbered list/checklist".
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
    /// Obsidian's "Heading N". Strips any existing ATX marker first
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
    /// cell (Obsidian). Only fires when the caret sits on a table
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
            // row runs out — Obsidian's Tab cycle.
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

    /// Tab/Shift-Tab on list lines — Obsidian indents list items two
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

pub fn word_stats(text: &str) -> (usize, usize) {
    (text.split_whitespace().count(), text.chars().count())
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
