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
}

impl EventEmitter<DocumentEvent> for Document {}

pub struct Document {
    pub path: PathBuf,
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
    /// Vault for link-graph lookups (linked mentions). Absent for
    /// documents opened outside a vault.
    vault: Option<Entity<crate::vault::Vault>>,
    /// Notes linking here — refreshed on open and vault changes.
    pub linked_mentions: Vec<PathBuf>,
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
        let preview_text = preview::preprocess(&content, &doc_dir, &*image_resolver);
        let preview = cx.new(|cx| TextViewState::markdown(&preview_text, cx));
        let banner = preview::banner_spec(&content, &doc_dir, &*image_resolver);

        let mut this = Self {
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
            decorations: None,
            focus_mode: false,
            focus_cursor: None,
            vault,
            linked_mentions: Vec::new(),
            mentions_open: false,
            _subscriptions: Vec::new(),
        };

        this.refresh_decorations(cx);
        this.refresh_linked_mentions(cx);
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
        let text = preview::preprocess(&raw, &doc_dir, &*resolver);
        self.banner = preview::banner_spec(&raw, &doc_dir, &*resolver);
        self.stats = word_stats(&raw);
        self.preview
            .update(cx, |state, cx| state.set_text(&text, cx));
        self.refresh_decorations(cx);
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

pub fn word_stats(text: &str) -> (usize, usize) {
    (text.split_whitespace().count(), text.chars().count())
}
