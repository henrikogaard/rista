//! Document: one open note — editor, preview state, autosave, disk sync.

use crate::history;
use crate::preview;
use crate::settings::Settings;
use gpui_kit::component::input::{EditorState, InputEvent, TabSize};
use gpui_kit::component::text::TextViewState;
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
    _subscriptions: Vec<Subscription>,
}

impl Document {
    pub fn open(
        path: PathBuf,
        vault_root: Option<PathBuf>,
        settings: &Settings,
        image_resolver: ImageResolver,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

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
            state.lsp_mut().completion_provider = Some(Rc::new(crate::slash::SlashCommands));
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
            _subscriptions: Vec::new(),
        };

        this._subscriptions = vec![
            cx.subscribe_in(&this.editor, window, |this, _editor, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.on_edited(window, cx);
                }
            }),
            // gpui-base never clears `completion.trigger_start_offset`, so a
            // later '/' at an earlier offset would be ignored. While the menu
            // is closed, keep the trigger anchor pinned to the cursor instead.
            cx.observe(&this.editor, |_this, editor, cx| {
                editor.update(cx, |editor, cx| {
                    let menu = editor.completion_menu_state();
                    let cursor = editor.cursor();
                    if menu.open || menu.trigger_start_offset == Some(cursor) {
                        return;
                    }
                    editor.present_completion_items(cursor, "", vec![], cx);
                });
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
    }

    fn doc_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default()
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
