use super::*;
use crate::settings::{Language, WindowSession};
use std::collections::BTreeMap;

#[derive(Debug, PartialEq)]
enum NameError {
    Invalid,
    Duplicate,
}

impl NameError {
    fn message(&self, language: Language) -> &'static str {
        match self {
            Self::Invalid => language.text(
                "Enter a name of 1–80 characters without line breaks.",
                "Skriv et navn på 1–80 tegn uten linjeskift.",
            ),
            Self::Duplicate => language.text(
                "That name is already in use. Choose another name.",
                "Navnet er allerede i bruk. Velg et annet navn.",
            ),
        }
    }
}

fn workspace_name(
    name: &str,
    layouts: &BTreeMap<String, WindowSession>,
    previous: Option<&str>,
) -> Result<String, NameError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(NameError::Invalid);
    }
    if layouts.contains_key(name) && previous != Some(name) {
        return Err(NameError::Duplicate);
    }
    Ok(name.to_owned())
}

fn prepare_workspace(
    mut session: WindowSession,
    mut resolve: impl FnMut(&Path, bool) -> Option<PathBuf>,
) -> (WindowSession, bool) {
    let mut skipped = false;
    let mut resolve = |path: &Path, directory| {
        let resolved = resolve(path, directory);
        skipped |= resolved.is_none();
        resolved
    };
    session.last_vault = session.last_vault.and_then(|path| resolve(&path, true));
    session.active_folder = session.active_folder.and_then(|path| resolve(&path, true));
    session.open_tabs = session
        .open_tabs
        .iter()
        .filter_map(|path| {
            resolve(Path::new(path), false).map(|path| path.to_string_lossy().into_owned())
        })
        .collect();
    session.active_tab = session.active_tab.and_then(|path| {
        resolve(Path::new(&path), false).map(|path| path.to_string_lossy().into_owned())
    });
    session.pinned_tabs = session
        .pinned_tabs
        .iter()
        .filter_map(|path| {
            resolve(Path::new(path), false).map(|path| path.to_string_lossy().into_owned())
        })
        .collect();
    if let Some(snapshot) = &mut session.editor_session {
        snapshot.paths.retain(|_, path| {
            if let Some(resolved) = resolve(path, false) {
                *path = resolved;
                true
            } else {
                false
            }
        });
    }
    (session, skipped)
}

impl Workspace {
    fn sync_workspace_catalog(&mut self, cx: &App) {
        let layouts = cx
            .global::<crate::windows::WindowManager>()
            .settings
            .workspace_layouts
            .clone();
        self.settings_baseline.workspace_layouts = layouts.clone();
        self.settings.workspace_layouts = layouts;
    }

    pub(super) fn show_workspace_name(
        &mut self,
        previous: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refocus(window, cx);
        let language = self.settings.language;
        let title = if previous.is_some() {
            language.text("Rename workspace layout", "Gi arbeidsoppsett nytt navn")
        } else {
            language.text("Save workspace layout", "Lagre arbeidsoppsett")
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        if let Some(name) = &previous {
            input.update(cx, |input, cx| input.set_value(name.clone(), window, cx));
        }
        let error = Rc::new(RefCell::new(None::<SharedString>));
        let view = cx.entity();
        let focus_input = input.clone();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            dialog
                .title(title)
                .w(rems(26.))
                .button_props(
                    gpui_kit::component::dialog::DialogButtonProps::default()
                        .show_cancel(true)
                        .cancel_text(language.text("Cancel", "Avbryt"))
                        .ok_text(if previous.is_some() {
                            language.text("Rename", "Gi nytt navn")
                        } else {
                            language.text("Save", "Lagre")
                        }),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(language.text("Name", "Navn"))
                        .child(Input::new(&input))
                        .when_some(error.borrow().clone(), |column, error| {
                            column.child(div().text_sm().text_color(cx.theme().danger).child(error))
                        }),
                )
                .on_ok({
                    let view = view.clone();
                    let input = input.clone();
                    let previous = previous.clone();
                    let error = error.clone();
                    move |_, window, cx| {
                        let raw = input.read(cx).value().to_string();
                        view.update(cx, |this, cx| {
                            this.sync_workspace_catalog(cx);
                            let name = match workspace_name(
                                &raw,
                                &this.settings.workspace_layouts,
                                previous.as_deref(),
                            ) {
                                Ok(name) => name,
                                Err(reason) => {
                                    *error.borrow_mut() = Some(reason.message(language).into());
                                    window.refresh();
                                    cx.notify();
                                    return false;
                                }
                            };
                            let session = if let Some(previous) = &previous {
                                let Some(session) =
                                    this.settings.workspace_layouts.remove(previous)
                                else {
                                    *error.borrow_mut() = Some(
                                        language
                                            .text(
                                                "This layout no longer exists.",
                                                "Oppsettet finnes ikke lenger.",
                                            )
                                            .into(),
                                    );
                                    window.refresh();
                                    cx.notify();
                                    return false;
                                };
                                session
                            } else {
                                this.persist_tabs(cx);
                                WindowSession::from_settings(0, &this.settings)
                            };
                            this.settings.workspace_layouts.insert(name, session);
                            this.persist_settings(cx);
                            this.note_status(
                                language.text("Workspace layout saved", "Arbeidsoppsett lagret"),
                                cx,
                            );
                            true
                        })
                    }
                })
                .on_close({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.focus_active_pane_after_mount(window, cx)
                        })
                    }
                })
        });
        window.defer(cx, move |window, cx| {
            focus_input.update(cx, |input, cx| input.focus(window, cx))
        });
    }

    pub(super) fn show_workspace_picker(
        &mut self,
        command: PaletteCmd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refocus(window, cx);
        let language = self.settings.language;
        let names = cx
            .global::<crate::windows::WindowManager>()
            .settings
            .workspace_layouts
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let manage = command == PaletteCmd::LoadWorkspace;
        if names.is_empty() && !manage {
            self.note_status(
                language.text("No saved workspace layouts", "Ingen lagrede arbeidsoppsett"),
                cx,
            );
            return;
        }
        let items = names
            .iter()
            .map(|name| {
                CommandItem::new()
                    .label(name.clone())
                    .icon(assets::IconName::AppWindow)
            })
            .collect::<Vec<_>>();
        let commands = [
            PaletteCmd::SaveWorkspace,
            PaletteCmd::RenameWorkspace,
            PaletteCmd::DeleteWorkspace,
        ];
        let management = commands
            .iter()
            .map(|command| {
                CommandItem::new()
                    .label(match language {
                        Language::English => command.label(),
                        Language::Norwegian => command.label_norwegian(),
                    })
                    .icon(command.spec().0)
            })
            .collect::<Vec<_>>();
        let state = cx.new(|cx| CommandState::new(window, cx));
        let focus_state = state.clone();
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, window, _| {
            dialog
                .w(window.rem_size() * 30.)
                .title(match language {
                    Language::English => command.label(),
                    Language::Norwegian => command.label_norwegian(),
                })
                .overlay_closable(true)
                .child(
                    Command::new(&state)
                        .placeholder(language.text("Find a layout…", "Finn et oppsett…"))
                        .searchable(true)
                        .filterable(true)
                        .max_h(rems(24.))
                        .group(
                            CommandGroup::new()
                                .label(language.text("Saved layouts", "Lagrede oppsett"))
                                .items(items.clone()),
                        )
                        .when(manage, |picker| {
                            picker.group(
                                CommandGroup::new()
                                    .label(language.text("Manage layouts", "Administrer oppsett"))
                                    .items(management.clone()),
                            )
                        })
                        .on_confirm({
                            let view = view.clone();
                            let names = names.clone();
                            move |index, window, cx| {
                                let name = (index.section == 0)
                                    .then(|| names.get(index.row).cloned())
                                    .flatten();
                                let next = (index.section == 1 && manage)
                                    .then(|| commands.get(index.row).copied())
                                    .flatten();
                                window.close_dialog(cx);
                                // The next dialog must open after the previous modal restores focus.
                                let view = view.clone();
                                window.defer(cx, move |window, cx| {
                                    view.update(cx, |this, cx| {
                                        if let Some(next) = next {
                                            this.run_command(next, window, cx);
                                        } else if let Some(name) = name {
                                            match command {
                                                PaletteCmd::RenameWorkspace => {
                                                    this.show_workspace_name(Some(name), window, cx)
                                                }
                                                PaletteCmd::DeleteWorkspace => {
                                                    this.confirm_delete_workspace(name, window, cx)
                                                }
                                                _ => this.load_workspace(&name, window, cx),
                                            }
                                        }
                                    })
                                });
                            }
                        })
                        .on_cancel(|window, cx| window.close_dialog(cx)),
                )
                .on_close({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.focus_active_pane_after_mount(window, cx)
                        })
                    }
                })
        });
        window.defer(cx, move |window, cx| {
            focus_state.update(cx, |state, cx| state.focus(window, cx))
        });
    }

    fn confirm_delete_workspace(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self.settings.language;
        let title: SharedString = match language {
            Language::English => format!("Delete “{name}”?"),
            Language::Norwegian => format!("Slett «{name}»?"),
        }
        .into();
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            dialog
                .title(title.clone())
                .description(language.text(
                    "Only the saved layout is removed. Your notes and open windows are unchanged.",
                    "Bare det lagrede oppsettet fjernes. Notater og åpne vinduer beholdes.",
                ))
                .button_props(
                    gpui_kit::component::dialog::DialogButtonProps::default()
                        .show_cancel(true)
                        .cancel_text(language.text("Cancel", "Avbryt"))
                        .ok_text(language.text("Delete", "Slett")),
                )
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok({
                    let view = view.clone();
                    let name = name.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.sync_workspace_catalog(cx);
                            this.settings.workspace_layouts.remove(&name);
                            this.persist_settings(cx);
                            this.note_status(
                                language.text("Workspace layout deleted", "Arbeidsoppsett slettet"),
                                cx,
                            );
                        });
                        true
                    }
                })
                .on_close({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.focus_active_pane_after_mount(window, cx)
                        })
                    }
                })
        });
    }

    fn load_workspace(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = cx
            .global::<crate::windows::WindowManager>()
            .settings
            .workspace_layouts
            .get(name)
            .cloned()
        else {
            self.note_status(
                self.tr(
                    "This layout no longer exists.",
                    "Oppsettet finnes ikke lenger.",
                ),
                cx,
            );
            return;
        };
        if self.save_all(cx).is_err() {
            self.note_status(
                self.tr(
                    "Could not save before switching layouts. Your changes are still open.",
                    "Kunne ikke lagre før bytte av oppsett. Endringene dine er fortsatt åpne.",
                ),
                cx,
            );
            self.focus_active_pane_after_mount(window, cx);
            return;
        }
        let (mut session, mut skipped) = prepare_workspace(session, |path, directory| {
            path.canonicalize().ok().filter(|path| {
                if directory {
                    path.is_dir()
                } else {
                    path.is_file()
                }
            })
        });
        if session.editor_session.is_none() {
            let paths = session
                .open_tabs
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>();
            let mode = match session.view_mode {
                ViewMode::Source => PaneMode::Source,
                ViewMode::Split => PaneMode::Split,
                ViewMode::Preview => PaneMode::Preview,
            };
            session.editor_session = Some(
                EditorLayout::migrate_legacy(
                    &paths,
                    session.active_tab.as_deref().map(Path::new),
                    mode,
                )
                .0
                .snapshot(),
            );
        }
        let previous = self.settings.clone();
        self.restoring_session = true;
        crate::windows::apply_session(&mut self.settings, &session);
        if self.vault.read(cx).root != session.last_vault {
            let switched = match &session.last_vault {
                Some(root) => self.open_vault_at(root.clone(), window, cx),
                None => self.close_vault(window, cx),
            };
            if !switched {
                self.settings = previous;
                self.restoring_session = false;
                self.note_status(
                    self.tr(
                        "Could not load the layout. Current tabs remain open.",
                        "Kunne ikke åpne oppsettet. Gjeldende faner er fortsatt åpne.",
                    ),
                    cx,
                );
                return;
            }
            self.graph_dock = None;
        }
        self.folder = None;
        self.graph = None;
        self.peek = None;
        self.clear_file_menu();
        self.pending_editor_focus = None;
        self.nav_stack.clear();
        self.nav_pos = 0;
        self.nav_suppress = false;
        if let Some(root) = &session.last_vault {
            let expanded = session
                .expanded_folders
                .get(&root.to_string_lossy().to_string())
                .cloned()
                .unwrap_or_default();
            self.vault
                .update(cx, |vault, cx| vault.restore_expanded_folders(expanded, cx));
        }
        let paths = session
            .editor_session
            .as_ref()
            .map(|snapshot| snapshot.paths.values().cloned().collect::<Vec<_>>())
            .unwrap_or_else(|| session.open_tabs.iter().map(PathBuf::from).collect());
        let mut needed = BTreeMap::<PathBuf, usize>::new();
        for path in &paths {
            let count = needed.entry(path.clone()).or_default();
            *count += 1;
            if self
                .docs
                .iter()
                .filter(|doc| doc.entity.read(cx).path == *path)
                .count()
                < *count
            {
                self.open_document_new_tab(path.clone(), window, cx);
            }
        }
        self.restore_editor_session(
            session.editor_session.clone(),
            session.active_tab.as_deref().map(Path::new),
            match session.view_mode {
                ViewMode::Source => PaneMode::Source,
                ViewMode::Split => PaneMode::Split,
                ViewMode::Preview => PaneMode::Preview,
            },
            cx,
        );
        skipped |= self.docs.len() < paths.len();
        for doc in &mut self.docs {
            doc.preview = false;
        }
        if let Some(folder) = &session.active_folder {
            self.open_folder_page(folder.clone(), window, cx);
        }
        // Opening tabs updates their active facade; retain the saved local chrome.
        self.settings.sidebar_collapsed = session.sidebar_collapsed;
        self.settings.sidebar_width = session.sidebar_width;
        self.settings.dock_width = session.dock_width;
        self.settings.dock_heights = session.dock_heights;
        self.layout_epoch = self.layout_epoch.wrapping_add(1);
        self.restoring_session = false;
        self.persist_tabs(cx);
        self.focus_active_pane_after_mount(window, cx);
        self.note_status(
            if skipped {
                self.tr(
                    "Layout loaded. Unavailable files or folders were skipped.",
                    "Oppsett åpnet. Utilgjengelige filer eller mapper ble hoppet over.",
                )
            } else {
                self.tr("Workspace layout loaded", "Arbeidsoppsett åpnet")
            },
            cx,
        );
    }

    pub(super) fn saved_dock_height(&self, name: &str) -> Option<Pixels> {
        self.settings
            .dock_heights
            .get(name)
            .copied()
            .filter(|height| height.is_finite() && *height > 0.)
            .map(px)
    }
}

#[cfg(test)]
mod tests {
    use super::{prepare_workspace, workspace_name, NameError};
    use crate::editor_layout::{EditorLayout, PaneMode};
    use crate::settings::{Settings, WindowSession};
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    #[test]
    fn names_are_trimmed_and_never_overwrite_another_layout() {
        let layouts = BTreeMap::from([("Writing".into(), WindowSession::default())]);
        assert_eq!(
            workspace_name("  Reading  ", &layouts, None).unwrap(),
            "Reading"
        );
        assert_eq!(
            workspace_name("Writing", &layouts, None),
            Err(NameError::Duplicate)
        );
        assert_eq!(
            workspace_name("Writing", &layouts, Some("Writing")).unwrap(),
            "Writing"
        );
        assert_eq!(
            workspace_name("\n", &layouts, None),
            Err(NameError::Invalid)
        );
        assert_eq!(
            workspace_name("a\nb", &layouts, None),
            Err(NameError::Invalid)
        );
        assert_eq!(
            workspace_name(&"a".repeat(81), &layouts, None),
            Err(NameError::Invalid)
        );
    }

    #[test]
    fn missing_paths_are_skipped_without_mutating_the_saved_layout() {
        let session = WindowSession {
            last_vault: Some(PathBuf::from("missing-vault")),
            open_tabs: vec!["note.md".into(), "missing.md".into()],
            active_tab: Some("note.md".into()),
            pinned_tabs: vec!["note.md".into()],
            editor_session: Some(
                EditorLayout::migrate_legacy(
                    &[PathBuf::from("note.md"), PathBuf::from("missing.md")],
                    None,
                    PaneMode::Preview,
                )
                .0
                .snapshot(),
            ),
            ..Default::default()
        };
        let (prepared, skipped) = prepare_workspace(session.clone(), |path, _| {
            (path == Path::new("note.md")).then(|| PathBuf::from("/canonical/note.md"))
        });
        assert!(skipped);
        assert!(prepared.last_vault.is_none());
        assert_eq!(prepared.open_tabs, ["/canonical/note.md"]);
        assert_eq!(prepared.active_tab.as_deref(), Some("/canonical/note.md"));
        assert_eq!(prepared.pinned_tabs, ["/canonical/note.md"]);
        assert_eq!(prepared.editor_session.unwrap().paths.len(), 1);
        assert_eq!(session.open_tabs.len(), 2);
    }

    #[test]
    fn catalog_roundtrip_and_concurrent_updates_preserve_local_state() {
        let baseline = Settings::default();
        let mut first = baseline.clone();
        first.sidebar_width = Some(340.);
        first.dock_width = Some(420.);
        first.dock_heights.insert("graph".into(), 250.);
        first
            .workspace_layouts
            .insert("Writing".into(), WindowSession::from_settings(0, &first));
        let mut second = baseline.clone();
        second
            .workspace_layouts
            .insert("Reading".into(), WindowSession::default());
        let (merged, _) = crate::windows::merge_shared_settings(&baseline, &second, &first);
        let decoded: Settings =
            serde_json::from_str(&serde_json::to_string(&merged).unwrap()).unwrap();
        assert_eq!(decoded.workspace_layouts.len(), 2);
        assert_eq!(decoded.workspace_layouts["Writing"].dock_width, Some(420.));
        assert_eq!(
            decoded.workspace_layouts["Writing"].dock_heights["graph"],
            250.
        );
        assert_eq!(decoded.sidebar_width, Some(340.));
        let local = crate::windows::with_local_settings(decoded.clone(), &baseline);
        assert_eq!(local.dock_width, None);
        assert_eq!(local.workspace_layouts.len(), 2);
        let mut changed = decoded.clone();
        changed.workspace_layouts.remove("Reading");
        let (merged, _) = crate::windows::merge_shared_settings(&decoded, &changed, &decoded);
        assert!(merged.workspace_layouts.contains_key("Writing"));
        assert!(!merged.workspace_layouts.contains_key("Reading"));
    }
}
