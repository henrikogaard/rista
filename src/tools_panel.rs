use super::*;
use crate::extensions::{self, Launcher, Localized, WorkingDirectory};

pub(super) struct TerminalTab {
    pub id: usize,
    pub terminal: Entity<crate::terminal::Terminal>,
    pub name: String,
    pub cwd: PathBuf,
}

impl Workspace {
    pub(super) fn on_toggle_terminal(
        &mut self,
        _: &ToggleTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.active_terminal.and_then(|ix| self.terminals.get(ix)) {
            self.terminal_visible = !self.terminal_visible;
            if self.terminal_visible {
                tab.terminal.focus_handle(cx).focus(window, cx);
            } else {
                self.focus_handle.focus(window, cx);
            }
            cx.notify();
        } else {
            self.confirm_tool(self.shell_launcher(), window, cx);
        }
    }

    fn shell_launcher(&self) -> Launcher {
        Launcher {
            id: "shell".into(),
            name: Localized {
                en: "Shell".into(),
                nb: "Skall".into(),
            },
            program: if !self.settings.terminal_shell.trim().is_empty() {
                self.settings.terminal_shell.trim().to_string()
            } else if cfg!(windows) {
                "powershell.exe".into()
            } else {
                std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
            },
            args: if cfg!(windows) {
                vec!["-NoLogo".into()]
            } else {
                vec!["-l".into(), "-i".into()]
            },
            working_directory: WorkingDirectory::Folder,
        }
    }

    pub(super) fn tool_launchers(
        &self,
        cx: &App,
    ) -> (Vec<Launcher>, Vec<crate::extensions::RegistryError>) {
        let root = self.vault.read(cx).root.as_deref();
        let registry = extensions::Registry::load(&extensions::user_dir(), root);
        let mut tools = vec![self.shell_launcher()];
        tools.extend(extensions::bundled_launchers());
        tools.extend(registry.plugins.into_iter().flat_map(|p| p.commands));
        (tools, registry.errors)
    }

    pub(super) fn on_open_tools(
        &mut self,
        _: &OpenTools,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (tools, errors) = self.tool_launchers(cx);
        let view = cx.entity();
        let language = self.settings.language;
        window.open_dialog(cx, move |dialog, _, cx| {
            let mut content = v_flex().gap_3()
                .child(div().text_sm().text_color(cx.theme().muted_foreground).child(language.text(
                    "Run installed command-line tools in a native terminal. Nothing runs until you confirm. JSON extensions reload each time this panel opens.",
                    "Kjør installerte kommandolinjeverktøy i en innebygd terminal. Ingenting kjører før du bekrefter. JSON-utvidelser lastes på nytt når panelet åpnes.")))
                .child(h_flex().gap_2()
                    .child(Button::new("plugins-folder").ghost().small().icon(assets::IconName::FolderOpen)
                        .label(language.text("Open extensions folder", "Åpne utvidelsesmappen"))
                        .on_click({ let view = view.clone(); move |_, _, cx| {
                            let dir = extensions::user_dir();
                            if std::fs::create_dir_all(&dir).is_ok() { cx.reveal_path(&dir); }
                            else { view.update(cx, |this, cx| this.note_status(this.tr("Could not open extensions folder", "Kunne ikke åpne utvidelsesmappen"), cx)); }
                        }}))
                    .child(Button::new("plugins-reload").ghost().small().icon(assets::IconName::RefreshCw)
                        .label(language.text("Reload", "Last på nytt"))
                        .on_click({ let view = view.clone(); move |_, window, cx| { window.close_dialog(cx); view.update(cx, |this, cx| this.on_open_tools(&OpenTools, window, cx)); }})));
            for (ix, tool) in tools.iter().enumerate() {
                content = content.child(h_flex().w_full().gap_3().items_center()
                    .child(Icon::new(assets::IconName::Terminal).size_4().text_color(cx.theme().muted_foreground))
                    .child(v_flex().flex_1().min_w_0().child(div().text_sm().child(tool.name.text(language).to_string()))
                        .child(div().text_xs().truncate().text_color(cx.theme().muted_foreground).child(tool.program.clone())))
                    .child(Button::new(("launch-tool", ix)).ghost().small().label(language.text("Launch…", "Start…"))
                        .on_click({ let view = view.clone(); let tool = tool.clone(); move |_, window, cx| {
                            window.close_dialog(cx); view.update(cx, |this, cx| this.confirm_tool(tool.clone(), window, cx));
                        }})));
            }
            if !errors.is_empty() {
                content = content.child(div().text_sm().text_color(cx.theme().warning).child(format!("{} ({})",
                    language.text("Some extensions could not be loaded", "Noen utvidelser kunne ikke lastes"), errors.len())));
                for error in errors.iter().take(8) {
                    content = content.child(div().text_xs().child(error.text(language)));
                }
            }
            dialog.title(language.text("Tools & extensions", "Verktøy og utvidelser")).w(px(600.))
                .child(div().id("tools-scroll").max_h(px(470.)).overflow_y_scroll().child(content))
        });
    }

    pub(super) fn confirm_tool(
        &mut self,
        tool: Launcher,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_tool_in(tool, None, window, cx);
    }

    pub(super) fn open_terminal_here(
        &mut self,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_tool_in(self.shell_launcher(), Some(folder), window, cx);
    }

    fn confirm_tool_in(
        &mut self,
        tool: Launcher,
        folder: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            self.note_status(
                self.tr(
                    "Open a vault before launching a tool",
                    "Åpne et hvelv før du starter et verktøy",
                ),
                cx,
            );
            return;
        };
        let folder = folder.unwrap_or_else(|| {
            self.folder
                .as_ref()
                .map(|p| p.path.clone())
                .or_else(|| {
                    self.active_doc()
                        .and_then(|d| d.read(cx).path.parent().map(Path::to_path_buf))
                })
                .filter(|p| p.starts_with(&root))
                .unwrap_or_else(|| root.clone())
        });
        let cwd = match tool.working_directory {
            WorkingDirectory::Vault => root.clone(),
            WorkingDirectory::Folder => folder.clone(),
        };
        let Ok(cwd) = cwd.canonicalize() else {
            self.note_status(
                self.tr(
                    "Working folder is unavailable",
                    "Arbeidsmappen er utilgjengelig",
                ),
                cx,
            );
            return;
        };
        if !cwd.starts_with(&root) {
            self.note_status(
                self.tr(
                    "Working folder must be inside the vault",
                    "Arbeidsmappen må være i hvelvet",
                ),
                cx,
            );
            return;
        }
        let file = self.active_doc().map(|d| d.read(cx).path.clone());
        let args = match expand_tool_args(&tool.args, &root, &folder, file.as_deref()) {
            Some(args) => args,
            None => {
                self.note_status(
                    self.tr(
                        "This tool requires an open note",
                        "Dette verktøyet krever et åpent notat",
                    ),
                    cx,
                );
                return;
            }
        };
        let program = tool.program.clone();
        let title = tool.name.text(self.settings.language).to_string();
        let language = self.settings.language;
        let view = cx.entity();
        let command = serde_json::to_string(
            &std::iter::once(program.clone())
                .chain(args.iter().cloned())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let cwd = cwd.clone(); let program = program.clone(); let args = args.clone(); let title = title.clone(); let root = root.clone(); let view = view.clone();
            dialog.title(format!("{} {title}?", language.text("Launch", "Starte")))
                .description(format!("{}\n\n{}\n{}\n\n{}", language.text(
                    "This process runs with your user permissions, including file and network access. The working folder is NOT a sandbox. Only run tools and extensions you trust.",
                    "Prosessen kjører med brukerrettighetene dine, inkludert fil- og nettverkstilgang. Arbeidsmappen er IKKE en sandkasse. Kjør bare verktøy og utvidelser du stoler på."),
                    cwd.display(), command,
                    language.text("Opens in a new terminal tab. Changes made by tools appear in the vault automatically.", "Åpnes i en ny terminalfane. Endringer fra verktøy vises automatisk i hvelvet.")))
                .show_cancel(true).ok_text(language.text("Launch tool", "Start verktøy"))
                .on_ok(move |_, window, cx| {
                    // Avoid the animated close restoring focus after the terminal opens.
                    window.close_dialog(cx);
                    view.update(cx, |this, cx| {
                        if this.vault.read(cx).root.as_ref() != Some(&root) { return; }
                        match crate::terminal_session::Session::spawn(&program, &args, &cwd) {
                            Ok(session) => {
                                let terminal = cx.new(|cx| crate::terminal::Terminal::new(session, &this.settings, cx));
                                let focus = terminal.focus_handle(cx);
                                window.on_next_frame(move |window, cx| focus.focus(window, cx));
                                this.next_terminal_id += 1;
                                this.terminals.push(TerminalTab { id: this.next_terminal_id, terminal, name: title.clone(), cwd: cwd.clone() });
                                this.active_terminal = Some(this.terminals.len() - 1);
                                this.terminal_visible = true;
                                this.zen = false;
                                cx.notify();
                            },
                            Err(_) => this.note_status(this.tr("Could not launch tool. Check that it is installed and its executable path is correct.", "Kunne ikke starte verktøyet. Kontroller at det er installert og at programstien er riktig."), cx),
                        }
                    }); false
                })
        });
    }

    pub(super) fn render_terminal_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(active) = self.active_terminal.and_then(|ix| self.terminals.get(ix)) else {
            return div().into_any_element();
        };
        let terminal = active.terminal.clone();
        let tabs = self.terminals.iter().map(|tab| {
            let id = tab.id;
            Tab::new()
                .label(format!(
                    "{} {} · {}",
                    tab.name,
                    id,
                    tab.cwd.file_name().unwrap_or_default().to_string_lossy()
                ))
                .prefix(Icon::new(assets::IconName::Terminal).size_3p5())
                .suffix(
                    Button::new(("close-terminal", id))
                        .ghost()
                        .xsmall()
                        .icon(assets::IconName::Close)
                        .tooltip(self.tr(
                            "Close terminal (stops process)",
                            "Lukk terminalen (stopper prosessen)",
                        ))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_terminal(id, window, cx);
                        })),
                )
        });
        v_flex()
            .size_full()
            .bg(cx.theme().group_box)
            .rounded(cx.theme().radius_lg)
            .overflow_hidden()
            .child(
                h_flex()
                    .h_9()
                    .px_3()
                    .gap_2()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            TabBar::new("terminal-tabs")
                                .segmented()
                                .small()
                                .bg(cx.theme().transparent)
                                .selected_index(self.active_terminal.unwrap_or(0))
                                .children(tabs)
                                .on_click(cx.listener(|this, &ix, window, cx| {
                                    if let Some(tab) = this.terminals.get(ix) {
                                        this.active_terminal = Some(ix);
                                        tab.terminal.focus_handle(cx).focus(window, cx);
                                        cx.notify();
                                    }
                                })),
                        ),
                    )
                    .child(
                        Button::new("terminal-new")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Plus)
                            .tooltip(self.tr("New terminal", "Ny terminal"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_tool(this.shell_launcher(), window, cx);
                            })),
                    )
                    .child(
                        Button::new("terminal-tools")
                            .ghost()
                            .xsmall()
                            .label(self.tr("Tools…", "Verktøy…"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_open_tools(&OpenTools, window, cx)
                            })),
                    )
                    .child(
                        Button::new("terminal-copy")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Copy)
                            .tooltip(self.tr("Copy visible output", "Kopier synlig utdata"))
                            .on_click({
                                let terminal = terminal.clone();
                                move |_, _, cx| terminal.update(cx, |t, cx| t.copy_output(cx))
                            }),
                    )
                    .child(
                        Button::new("terminal-interrupt")
                            .ghost()
                            .xsmall()
                            .label("Ctrl+C")
                            .on_click({
                                let terminal = terminal.clone();
                                move |_, _, cx| terminal.update(cx, |t, cx| t.interrupt(cx))
                            }),
                    )
                    .child(
                        Button::new("terminal-stop")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Square)
                            .tooltip(self.tr("Stop process", "Stopp prosessen"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(tab) =
                                    this.active_terminal.and_then(|ix| this.terminals.get(ix))
                                {
                                    this.close_terminal(tab.id, window, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("terminal-hide")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::ChevronDown)
                            .tooltip(self.tr(
                                "Hide terminal (keeps running)",
                                "Skjul terminalen (fortsetter å kjøre)",
                            ))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_toggle_terminal(&ToggleTerminal, window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .px_3()
                    .text_xs()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(active.cwd.display().to_string()),
            )
            .child(div().flex_1().min_h_0().px_3().pb_2().child(terminal))
            .into_any_element()
    }

    fn close_terminal(&mut self, id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.terminals.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.terminals.remove(ix);
        self.active_terminal =
            active_after_close(self.active_terminal.unwrap_or(0), ix, self.terminals.len());
        if let Some(tab) = self.active_terminal.and_then(|ix| self.terminals.get(ix)) {
            tab.terminal.focus_handle(cx).focus(window, cx);
        } else {
            self.terminal_visible = false;
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }
}

fn active_after_close(active: usize, removed: usize, remaining: usize) -> Option<usize> {
    if remaining == 0 {
        None
    } else {
        Some(if removed < active { active - 1 } else { active }.min(remaining - 1))
    }
}

fn expand_tool_args(
    args: &[String],
    vault: &Path,
    folder: &Path,
    file: Option<&Path>,
) -> Option<Vec<String>> {
    args.iter()
        .map(|arg| {
            if arg.contains("{{file}}") && file.is_none() {
                return None;
            }
            Some(
                arg.replace("{{vault}}", &vault.to_string_lossy())
                    .replace("{{folder}}", &folder.to_string_lossy())
                    .replace(
                        "{{file}}",
                        &file.map(|p| p.to_string_lossy()).unwrap_or_default(),
                    ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::expand_tool_args;
    use std::path::Path;
    #[test]
    fn closing_tabs_preserves_selection_or_selects_a_neighbor() {
        assert_eq!(super::active_after_close(2, 0, 2), Some(1));
        assert_eq!(super::active_after_close(0, 2, 2), Some(0));
        assert_eq!(super::active_after_close(1, 1, 2), Some(1));
        assert_eq!(super::active_after_close(2, 2, 2), Some(1));
        assert_eq!(super::active_after_close(0, 0, 0), None);
    }
    #[test]
    fn context_remains_literal_arguments_and_requires_active_file() {
        let args = vec!["{{file}}".into(), "{{vault}}".into()];
        assert!(expand_tool_args(&args, Path::new("/v"), Path::new("/v"), None).is_none());
        assert_eq!(
            expand_tool_args(
                &args,
                Path::new("/my vault"),
                Path::new("/my vault"),
                Some(Path::new("/my vault/$(touch pwned).md"))
            )
            .unwrap(),
            ["/my vault/$(touch pwned).md", "/my vault"]
        );
    }
}
