//! Settings sheet — segmented controls and switches, applied live.

use crate::app::Workspace;
use crate::hotkeys::{self, CommandSpec, HotkeyNotice};
use crate::settings::{
    Appearance, Language, Period, PropertiesVisibility, Settings, ViewMode, EDITOR_FONTS,
};
use crate::spellcheck;
use crate::theme;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::theme::ThemeMode;
use gpui_kit::component::Disableable;
use gpui_kit::component::Sizable;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, IndexPath};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

pub struct SettingsView {
    terminal_font_input: Entity<InputState>,
    terminal_size_input: Entity<InputState>,
    terminal_shell_input: Entity<InputState>,
    workspace: WeakEntity<Workspace>,
    hotkey_search: Entity<InputState>,
    spell_language_search: Entity<InputState>,
    installed_spell_languages: Vec<String>,
    capture_focus: FocusHandle,
    recording: Option<String>,
    hotkey_notice: Option<HotkeyNotice>,
    font_select: Entity<SelectState<SearchableVec<String>>>,
    font_size_select: Entity<SelectState<SearchableVec<String>>>,
    ui_size_select: Entity<SelectState<SearchableVec<String>>>,
    tab_size_select: Entity<SelectState<SearchableVec<String>>>,
    dark_theme_select: Entity<SelectState<SearchableVec<String>>>,
    light_theme_select: Entity<SelectState<SearchableVec<String>>>,
    theme_write_failed: bool,
    attachments_input: Entity<InputState>,
    templates_input: Entity<InputState>,
    daily_dir_input: Entity<InputState>,
    daily_format_input: Entity<InputState>,
    unique_dir_input: Entity<InputState>,
    unique_format_input: Entity<InputState>,
    /// Folder and format inputs for the weekly … yearly notes.
    periodic_inputs: Vec<(Period, Entity<InputState>, Entity<InputState>)>,
    _subscriptions: Vec<Subscription>,
}

const FONT_SIZES: &[&str] = &["12", "13", "14", "15", "16", "17", "18", "20", "22"];
const UI_SIZES: &[&str] = &["11", "12", "13", "14"];
const TAB_SIZES: &[&str] = &["2", "4", "8"];

impl SettingsView {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        settings: &Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = settings.clone();
        let installed_spell_languages = spellcheck::available_languages();
        let hotkey_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(
                settings
                    .language
                    .text("Search shortcuts…", "Søk etter snarveier…"),
            )
        });
        let spell_language_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(
                settings
                    .language
                    .text("Search installed languages…", "Søk i installerte språk…"),
            )
        });
        let capture_focus = cx.focus_handle();
        let terminal_font_input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(settings.terminal_font.clone(), window, cx);
            input
        });
        let terminal_size_input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(settings.terminal_font_size.to_string(), window, cx);
            input
        });
        let terminal_shell_input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("$SHELL");
            input.set_value(settings.terminal_shell.clone(), window, cx);
            input
        });

        let dark_theme_select =
            Self::theme_select(ThemeMode::Dark, &settings.dark_theme, window, cx);
        let light_theme_select =
            Self::theme_select(ThemeMode::Light, &settings.light_theme, window, cx);

        let font_ix = EDITOR_FONTS
            .iter()
            .position(|f| *f == settings.editor_font_family)
            .unwrap_or(0);
        let font_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    EDITOR_FONTS
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>(),
                ),
                Some(IndexPath::new(font_ix)),
                window,
                cx,
            )
        });
        let size_ix = FONT_SIZES
            .iter()
            .position(|s| s.parse::<f32>().ok() == Some(settings.editor_font_size))
            .unwrap_or(2);
        let font_size_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(FONT_SIZES.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
                Some(IndexPath::new(size_ix)),
                window,
                cx,
            )
        });
        let ui_ix = UI_SIZES
            .iter()
            .position(|s| s.parse::<f32>().ok() == Some(settings.ui_font_size))
            .unwrap_or(2);
        let ui_size_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(UI_SIZES.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
                Some(IndexPath::new(ui_ix)),
                window,
                cx,
            )
        });
        let tab_ix = TAB_SIZES
            .iter()
            .position(|s| s.parse::<usize>().ok() == Some(settings.tab_size))
            .unwrap_or(1);
        let tab_size_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(TAB_SIZES.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
                Some(IndexPath::new(tab_ix)),
                window,
                cx,
            )
        });
        let attachments_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("attachments");
            state.set_value(settings.attachments_dir.clone(), window, cx);
            state
        });
        let templates_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("templates");
            state.set_value(settings.templates_dir.clone(), window, cx);
            state
        });
        let daily_dir_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("vault root");
            state.set_value(settings.daily_dir.clone(), window, cx);
            state
        });
        let daily_format_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("YYYY-MM-DD");
            state.set_value(settings.daily_format.clone(), window, cx);
            state
        });
        let unique_dir_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("vault root");
            state.set_value(settings.unique_note_dir.clone(), window, cx);
            state
        });
        let unique_format_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("YYYYMMDDHHmm");
            state.set_value(settings.unique_note_format.clone(), window, cx);
            state
        });

        let periodic_inputs: Vec<_> = [Period::Week, Period::Month, Period::Quarter, Period::Year]
            .into_iter()
            .map(|period| {
                let (dir, format) = settings.period(period);
                let (dir, format) = (dir.to_string(), format.to_string());
                let dir_input = cx.new(|cx| {
                    let mut state = InputState::new(window, cx).placeholder("vault root");
                    state.set_value(dir, window, cx);
                    state
                });
                let format_input = cx.new(|cx| {
                    let mut state = InputState::new(window, cx);
                    state.set_value(format, window, cx);
                    state
                });
                (period, dir_input, format_input)
            })
            .collect();

        let mut subs = Vec::new();
        subs.push(cx.subscribe_in(
            &hotkey_search,
            window,
            |_this, _state, _event: &InputEvent, _window, cx| cx.notify(),
        ));
        subs.push(cx.subscribe_in(
            &spell_language_search,
            window,
            |_this, _state, _event: &InputEvent, _window, cx| cx.notify(),
        ));
        for (period, dir_input, format_input) in &periodic_inputs {
            let period = *period;
            subs.push(cx.subscribe_in(
                dir_input,
                window,
                move |this, state, event: &InputEvent, window, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        return;
                    }
                    let dir = state.read(cx).value().trim().to_string();
                    if !is_vault_relative(&dir) {
                        return;
                    }
                    this.update_setting(cx, |s| *s.period_mut(period).0 = dir, window);
                },
            ));
            subs.push(cx.subscribe_in(
                format_input,
                window,
                move |this, state, event: &InputEvent, window, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        return;
                    }
                    let format = state.read(cx).value().trim().to_string();
                    if format.is_empty() {
                        return;
                    }
                    this.update_setting(cx, |s| *s.period_mut(period).1 = format, window);
                },
            ));
        }
        for (input, kind) in [
            (&terminal_font_input, 0),
            (&terminal_size_input, 1),
            (&terminal_shell_input, 2),
        ] {
            subs.push(cx.subscribe_in(
                input,
                window,
                move |this, input, event: &InputEvent, window, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        return;
                    }
                    let value = input.read(cx).value().trim().to_string();
                    this.update_setting(
                        cx,
                        |s| match kind {
                            0 => {
                                s.terminal_font = if value.is_empty() {
                                    Settings::default().terminal_font
                                } else {
                                    value
                                }
                            }
                            1 => {
                                if let Ok(size) = value.parse::<f32>() {
                                    if size.is_finite() {
                                        s.terminal_font_size = size.clamp(10., 24.);
                                    }
                                }
                            }
                            _ => s.terminal_shell = value,
                        },
                        window,
                    );
                },
            ));
        }
        for (select, dark) in [(&dark_theme_select, true), (&light_theme_select, false)] {
            subs.push(cx.subscribe_in(
                select,
                window,
                move |this, _, event: &SelectEvent<SearchableVec<String>>, window, cx| {
                    if let SelectEvent::Confirm(Some(name)) = event {
                        this.update_setting(
                            cx,
                            |s| {
                                if dark {
                                    s.dark_theme = name.clone();
                                } else {
                                    s.light_theme = name.clone();
                                }
                            },
                            window,
                        );
                    }
                },
            ));
        }
        subs.push(cx.subscribe_in(
            &font_select,
            window,
            |this, _e, event: &SelectEvent<SearchableVec<String>>, window, cx| {
                if let SelectEvent::Confirm(Some(font)) = event {
                    this.update_setting(cx, |s| s.editor_font_family = font.to_string(), window);
                }
            },
        ));
        subs.push(cx.subscribe_in(
            &font_size_select,
            window,
            |this, _e, event: &SelectEvent<SearchableVec<String>>, window, cx| {
                if let SelectEvent::Confirm(Some(size)) = event {
                    if let Ok(size) = size.parse::<f32>() {
                        this.update_setting(cx, |s| s.editor_font_size = size, window);
                    }
                }
            },
        ));
        subs.push(cx.subscribe_in(
            &ui_size_select,
            window,
            |this, _e, event: &SelectEvent<SearchableVec<String>>, window, cx| {
                if let SelectEvent::Confirm(Some(size)) = event {
                    if let Ok(size) = size.parse::<f32>() {
                        this.update_setting(cx, |s| s.ui_font_size = size, window);
                    }
                }
            },
        ));
        subs.push(cx.subscribe_in(
            &tab_size_select,
            window,
            |this, _e, event: &SelectEvent<SearchableVec<String>>, window, cx| {
                if let SelectEvent::Confirm(Some(size)) = event {
                    if let Ok(size) = size.parse::<usize>() {
                        this.update_setting(cx, |s| s.tab_size = size, window);
                    }
                }
            },
        ));
        subs.push(cx.subscribe_in(
            &attachments_input,
            window,
            |this, state, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    return;
                }
                let dir = state.read(cx).value().trim().to_string();
                // Vault-relative only: no absolute paths or traversal.
                if dir.is_empty() || dir.starts_with('/') || dir.contains("..") {
                    return;
                }
                this.update_setting(cx, |s| s.attachments_dir = dir, window);
            },
        ));
        subs.push(cx.subscribe_in(
            &templates_input,
            window,
            |this, state, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    return;
                }
                let dir = state.read(cx).value().trim().to_string();
                if dir.is_empty() || dir.starts_with('/') || dir.contains("..") {
                    return;
                }
                this.update_setting(cx, |s| s.templates_dir = dir, window);
            },
        ));
        subs.push(cx.subscribe_in(
            &daily_dir_input,
            window,
            |this, state, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    return;
                }
                let dir = state.read(cx).value().trim().to_string();
                if dir.starts_with('/') || dir.contains("..") {
                    return;
                }
                this.update_setting(cx, |s| s.daily_dir = dir, window);
            },
        ));
        subs.push(cx.subscribe_in(
            &daily_format_input,
            window,
            |this, state, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    return;
                }
                let fmt = state.read(cx).value().trim().to_string();
                if fmt.is_empty() {
                    return;
                }
                this.update_setting(cx, |s| s.daily_format = fmt, window);
            },
        ));
        subs.push(cx.subscribe_in(
            &unique_dir_input,
            window,
            |this, state, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    return;
                }
                let dir = state.read(cx).value().trim().to_string();
                if !is_vault_relative(&dir) {
                    return;
                }
                this.update_setting(cx, |s| s.unique_note_dir = dir, window);
            },
        ));
        subs.push(cx.subscribe_in(
            &unique_format_input,
            window,
            |this, state, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    return;
                }
                let fmt = state.read(cx).value().trim().to_string();
                if fmt.is_empty() {
                    return;
                }
                this.update_setting(cx, |s| s.unique_note_format = fmt, window);
            },
        ));

        Self {
            terminal_font_input,
            terminal_size_input,
            terminal_shell_input,
            workspace,
            hotkey_search,
            spell_language_search,
            installed_spell_languages,
            capture_focus,
            recording: None,
            hotkey_notice: None,
            dark_theme_select,
            light_theme_select,
            theme_write_failed: false,
            font_select,
            font_size_select,
            ui_size_select,
            tab_size_select,
            attachments_input,
            templates_input,
            daily_dir_input,
            daily_format_input,
            unique_dir_input,
            unique_format_input,
            periodic_inputs,
            _subscriptions: subs,
        }
    }

    fn theme_select(
        mode: ThemeMode,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<SearchableVec<String>>> {
        let names = theme::theme_names(mode, cx);
        let index = names.iter().position(|n| n == name).unwrap_or(0);
        cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(names),
                Some(IndexPath::new(index)),
                window,
                cx,
            )
        })
    }

    fn reload_themes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        theme::install_themes(cx);
        let Some(ws) = self.workspace.upgrade() else {
            return;
        };
        let settings = ws.read(cx).settings().clone();
        for (select, mode, name) in [
            (
                &self.dark_theme_select,
                ThemeMode::Dark,
                &settings.dark_theme,
            ),
            (
                &self.light_theme_select,
                ThemeMode::Light,
                &settings.light_theme,
            ),
        ] {
            let names = theme::theme_names(mode, cx);
            let index = names.iter().position(|n| n == name).unwrap_or(0);
            select.update(cx, |select, cx| {
                select.set_items(SearchableVec::new(names), window, cx);
                select.set_selected_index(Some(IndexPath::new(index)), window, cx);
            });
        }
        ws.update(cx, |ws, cx| ws.apply_settings(settings, window, cx));
        cx.notify();
    }

    fn apply_hotkey_result(&mut self, result: Result<(), HotkeyNotice>, cx: &mut Context<Self>) {
        self.hotkey_notice = result.err();
        self.sync_hotkey_warning(cx);
        cx.notify();
    }

    fn sync_hotkey_warning(&self, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| workspace.sync_hotkey_warning(cx));
        }
    }

    fn start_hotkey_recording(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = Some(id);
        self.hotkey_notice = None;
        self.capture_focus.focus(window, cx);
        cx.notify();
    }

    fn capture_hotkey(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.recording.clone() else {
            return;
        };
        window.prevent_default();
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        let key = event.keystroke.key.to_lowercase();
        if matches!(
            key.as_str(),
            "shift"
                | "left_shift"
                | "right_shift"
                | "control"
                | "ctrl"
                | "left_control"
                | "right_control"
                | "left_ctrl"
                | "right_ctrl"
                | "alt"
                | "left_alt"
                | "right_alt"
                | "option"
                | "left_option"
                | "right_option"
                | "cmd"
                | "command"
                | "left_command"
                | "right_command"
                | "super"
                | "platform"
                | "function"
                | "fn"
        ) {
            return;
        }
        if matches!(key.as_str(), "escape" | "esc") {
            self.recording = None;
            self.hotkey_notice = None;
            cx.notify();
            return;
        }
        let shortcut = event.keystroke.unparse();
        let result = hotkeys::set_binding(&mut *cx, &id, Some(shortcut));
        self.recording = None;
        self.apply_hotkey_result(result, cx);
    }

    fn shortcut_label(shortcut: Option<&str>, unbound: &str) -> gpui_kit::Div {
        let Some(shortcut) = shortcut else {
            return h_flex().child(div().text_xs().child(unbound.to_string()));
        };
        let keys = shortcut
            .split_whitespace()
            .filter_map(|stroke| Keystroke::parse(stroke).ok())
            .map(Kbd::new)
            .collect::<Vec<_>>();
        if keys.is_empty() {
            h_flex().child(div().text_xs().child(shortcut.to_string()))
        } else {
            h_flex().items_center().gap_1().children(keys)
        }
    }

    fn hotkeys_section(
        &mut self,
        language: Language,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let query = self.hotkey_search.read(cx).value().to_lowercase();
        let commands = hotkeys::commands()
            .into_iter()
            .filter(|command| {
                query.is_empty()
                    || command.id.contains(&query)
                    || command.english.to_lowercase().contains(&query)
                    || command.norwegian.to_lowercase().contains(&query)
            })
            .collect::<Vec<_>>();
        let has_commands = !commands.is_empty();
        let unbound = language.text("Unbound", "Ikke tilordnet");
        let mut rows = v_flex().max_h(px(320.)).overflow_y_scrollbar().gap_1();
        for spec in commands {
            rows = rows.child(self.hotkey_row(spec, language, unbound, cx));
        }
        if !has_commands {
            rows = rows.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text("No matching shortcuts", "Ingen snarveier samsvarer")),
            );
        }

        let mut section = v_flex()
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child(language.text("HOTKEYS", "HURTIGTASTER")),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_1()
                            .child(
                                Button::new("hotkeys-reset-all")
                                    .label(language.text("Reset all", "Tilbakestill alle"))
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let result = hotkeys::reset_all(&mut *cx);
                                        this.apply_hotkey_result(result, cx);
                                    })),
                            )
                            .child(
                                Button::new("hotkeys-reload")
                                    .label(language.text("Reload", "Last på nytt"))
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        hotkeys::reload(&mut *cx);
                                        this.hotkey_notice = None;
                                        this.sync_hotkey_warning(cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(Input::new(&self.hotkey_search).w_full())
            .child(rows);
        if let Some(notice) = self.hotkey_notice.clone().or_else(|| hotkeys::warning(cx)) {
            section = section.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(notice.message(language)),
            );
        }
        div()
            .id("hotkeys-capture-root")
            .track_focus(&self.capture_focus)
            .capture_key_down(
                cx.listener(|this, event, window, cx| this.capture_hotkey(event, window, cx)),
            )
            .child(section)
    }

    fn hotkey_row(
        &mut self,
        spec: CommandSpec,
        language: Language,
        unbound: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let id = spec.id.to_string();
        let current = hotkeys::shortcut(cx, spec.id);
        let current_label = Self::shortcut_label(current.as_deref(), unbound);
        let default_label = Self::shortcut_label(spec.default, unbound);
        let recording = self.recording.as_deref() == Some(spec.id);
        let record_text = if recording {
            language.text("Press a shortcut…", "Trykk en snarvei…")
        } else {
            language.text("Record", "Spill inn")
        };
        let record_id = id.clone();
        let unbind_id = id.clone();
        let reset_id = id;
        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_sm()
                    .child(spec.label(language)),
            )
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(language.text("Current", "Nåværende")),
                    )
                    .child(current_label)
                    .child(
                        div()
                            .ml_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(language.text("Default", "Standard")),
                    )
                    .child(default_label),
            )
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        Button::new(format!("hotkey-record-{record_id}"))
                            .label(record_text)
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_hotkey_recording(record_id.clone(), window, cx);
                            })),
                    )
                    .child(
                        Button::new(format!("hotkey-unbind-{unbind_id}"))
                            .label(language.text("Unbind", "Fjern"))
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let result = hotkeys::set_binding(&mut *cx, &unbind_id, None);
                                this.apply_hotkey_result(result, cx);
                            })),
                    )
                    .child(
                        Button::new(format!("hotkey-reset-{reset_id}"))
                            .label(language.text("Reset", "Tilbakestill"))
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let result = hotkeys::reset_command(&mut *cx, &reset_id);
                                this.apply_hotkey_result(result, cx);
                            })),
                    ),
            )
    }

    /// Patch the workspace settings and flush to disk + live apply.
    fn update_setting(
        &mut self,
        cx: &mut Context<Self>,
        patch: impl FnOnce(&mut Settings),
        window: &mut Window,
    ) {
        let Some(ws) = self.workspace.upgrade() else {
            return;
        };
        let old_language = ws.read(cx).settings().language;
        let mut settings = ws.read(cx).settings().clone();
        patch(&mut settings);
        let new_language = settings.language;
        ws.update(cx, |ws, cx| ws.apply_settings(settings, window, cx));
        if old_language != new_language {
            self.hotkey_search.update(cx, |input, cx| {
                input.set_placeholder(
                    new_language.text("Search shortcuts…", "Søk etter snarveier…"),
                    window,
                    cx,
                );
            });
            self.spell_language_search.update(cx, |input, cx| {
                input.set_placeholder(
                    new_language.text("Search installed languages…", "Søk i installerte språk…"),
                    window,
                    cx,
                );
            });
        }
    }

    fn row(cx: &App, label: impl Into<SharedString>, control: impl IntoElement) -> gpui_kit::Div {
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(label.into()),
            )
            .child(control)
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self
            .workspace
            .upgrade()
            .map(|ws| ws.read(cx).settings().clone())
            .unwrap_or_default();

        let appearance_ix = match settings.appearance {
            Appearance::Dark => 0,
            Appearance::Light => 1,
            Appearance::System => 2,
        };
        let mode_ix = match settings.view_mode {
            ViewMode::Source => 0,
            ViewMode::Split => 1,
            ViewMode::Preview => 2,
        };
        let language_query = self
            .spell_language_search
            .read(cx)
            .value()
            .to_string()
            .to_lowercase();
        let has_invalid_spell_language = settings
            .spellcheck_languages
            .iter()
            .any(|language| !self.installed_spell_languages.contains(language));
        let hotkeys = self.hotkeys_section(settings.language, window, cx);

        v_flex()
            .id("settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .gap_5()
            .child(hotkeys)
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child("APPEARANCE"),
                    )
                    .child(Self::row(
                        cx,
                        settings.language.text("Appearance", "Utseende"),
                        TabBar::new("appearance")
                            .segmented()
                            .selected_index(appearance_ix)
                            .children([
                                Tab::new().label(settings.language.text("Dark", "Mørk")),
                                Tab::new().label(settings.language.text("Light", "Lys")),
                                Tab::new().label("System"),
                            ])
                            .on_click(cx.listener(|this, &ix, window, cx| {
                                this.update_setting(
                                    cx,
                                    |s| {
                                        s.appearance = match ix {
                                            1 => Appearance::Light,
                                            2 => Appearance::System,
                                            _ => Appearance::Dark,
                                        }
                                    },
                                    window,
                                );
                            })),
                    ))
                    .child(Self::row(cx, settings.language.text("Dark palette", "Mørk palett"),
                        Select::new(&self.dark_theme_select).w(px(180.))))
                    .child(Self::row(cx, settings.language.text("Light palette", "Lys palett"),
                        Select::new(&self.light_theme_select).w(px(180.))))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("create-theme")
                                    .small()
                                    .ghost()
                                    .label(settings.language.text("Create theme…", "Lag tema…"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let Some(ws) = this.workspace.upgrade() else {
                                            return;
                                        };
                                        let settings = ws.read(cx).settings().clone();
                                        match theme::create_custom(&settings, cx) {
                                            Ok(path) => {
                                                this.theme_write_failed = false;
                                                this.reload_themes(window, cx);
                                                cx.reveal_path(&path);
                                            }
                                            Err(_) => {
                                                this.theme_write_failed = true;
                                                cx.notify();
                                            }
                                        }
                                    })),
                            )
                            .child(
                                Button::new("reload-themes")
                                    .small()
                                    .ghost()
                                    .label(settings.language.text(
                                        "Reload themes",
                                        "Last inn temaer på nytt",
                                    ))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.reload_themes(window, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(settings.language.text(
                                "Create a copy of the active palette, edit its JSON colours, then reload and select it above.",
                                "Lag en kopi av den aktive paletten, rediger JSON-fargene, last inn på nytt og velg den over.",
                            )),
                    )
                    .when(self.theme_write_failed, |view| {
                        view.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().danger)
                                .child(settings.language.text(
                                    "Could not create theme file. Check folder permissions.",
                                    "Kunne ikke opprette temafilen. Sjekk mappetillatelser.",
                                )),
                        )
                    })
                    .when(
                        !cx.global::<theme::ThemeCatalog>().errors.is_empty(),
                        |view| {
                            view.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().danger)
                                    .child(format!(
                                        "{}: {}",
                                        settings.language.text(
                                            "Invalid or duplicate theme files",
                                            "Ugyldige eller dupliserte temafiler",
                                        ),
                                        cx.global::<theme::ThemeCatalog>().errors.join(", ")
                                    )),
                            )
                        },
                    )
                    .child(Self::row(
                        cx,
                        "Editor font",
                        Select::new(&self.font_select).w(px(180.)),
                    ))
                    .child(Self::row(
                        cx,
                        settings
                            .language
                            .text("Navigation language", "Navigasjonsspråk"),
                        TabBar::new("navigation-language")
                            .segmented()
                            .selected_index(usize::from(settings.language == Language::Norwegian))
                            .children([Tab::new().label("English"), Tab::new().label("Norsk")])
                            .on_click(cx.listener(|this, &ix, window, cx| {
                                this.update_setting(
                                    cx,
                                    |s| {
                                        s.language = if ix == 1 {
                                            Language::Norwegian
                                        } else {
                                            Language::English
                                        }
                                    },
                                    window,
                                );
                            })),
                    ))
                    .child(Self::row(
                        cx,
                        "Editor size",
                        Select::new(&self.font_size_select).w(px(90.)),
                    ))
                    .child(Self::row(
                        cx,
                        "UI size",
                        Select::new(&self.ui_size_select).w(px(90.)),
                    ))
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child("EDITOR"),
                    )
                    .child(Self::row(
                        cx,
                        "Default view",
                        TabBar::new("view-mode")
                            .segmented()
                            .selected_index(mode_ix)
                            .children([
                                Tab::new().label("Source"),
                                Tab::new().label("Split"),
                                Tab::new().label("Preview"),
                            ])
                            .on_click(cx.listener(|this, &ix, window, cx| {
                                this.update_setting(
                                    cx,
                                    |s| {
                                        s.view_mode = match ix {
                                            1 => ViewMode::Split,
                                            2 => ViewMode::Preview,
                                            _ => ViewMode::Source,
                                        }
                                    },
                                    window,
                                );
                            })),
                    ))
                    .child(Self::row(
                        cx,
                        "Soft wrap",
                        Switch::new("soft-wrap")
                            .checked(settings.soft_wrap)
                            .on_click(cx.listener(|this, checked, window, cx| {
                                let checked = *checked;
                                this.update_setting(cx, |s| s.soft_wrap = checked, window);
                            })),
                    ))
                    .child(Self::row(cx, settings.language.text("Show non-Markdown files", "Vis andre filer enn Markdown"),
                        Switch::new("show-other-files")
                            .checked(settings.show_other_files)
                            .on_click(cx.listener(|this, checked, window, cx| {
                                let checked = *checked;
                                this.update_setting(cx, |s| s.show_other_files = checked, window);
                            }))))
                    .child(Self::row(cx, settings.language.text("Properties", "Egenskaper"),
                        TabBar::new("properties-default").segmented()
                            .selected_index(match settings.properties_visibility {
                                PropertiesVisibility::Expanded => 0,
                                PropertiesVisibility::Collapsed => 1,
                                PropertiesVisibility::Hidden => 2,
                            })
                            .children([
                                Tab::new().label(settings.language.text("Expanded", "Utvidet")),
                                Tab::new().label(settings.language.text("Collapsed", "Sammenfoldet")),
                                Tab::new().label(settings.language.text("Hidden", "Skjult")),
                            ])
                            .on_click(cx.listener(|this, &index, window, cx| this.update_setting(cx, |s| {
                                s.properties_visibility = match index {
                                    1 => PropertiesVisibility::Collapsed,
                                    2 => PropertiesVisibility::Hidden,
                                    _ => PropertiesVisibility::Expanded,
                                };
                            }, window)))))
                    .child(Self::row(
                        cx,
                        "Line numbers",
                        Switch::new("line-numbers")
                            .checked(settings.show_line_numbers)
                            .on_click(cx.listener(|this, checked, window, cx| {
                                let checked = *checked;
                                this.update_setting(cx, |s| s.show_line_numbers = checked, window);
                            })),
                    ))
                    .child(Self::row(
                        cx,
                        "Tab size",
                        Select::new(&self.tab_size_select).w(px(90.)),
                    ))
                    .child(Self::row(
                        cx,
                        settings.language.text("Spell checking", "Stavekontroll"),
                        Switch::new("spellcheck-enabled")
                            .disabled(!spellcheck::is_supported())
                            .checked(settings.spellcheck_enabled)
                            .on_click(cx.listener(|this, checked, window, cx| {
                                let checked = *checked;
                                this.update_setting(cx, |s| s.spellcheck_enabled = checked, window);
                            })),
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if !spellcheck::is_supported() {
                                settings.language.text(
                                    "Spell checking is unavailable on this platform.",
                                    "Stavekontroll er ikke tilgjengelig på denne plattformen.",
                                )
                            } else {
                                settings.language.text(
                                    "Choose installed languages, or leave empty for system languages.",
                                    "Velg installerte språk, eller la listen stå tom for systemspråk.",
                                )
                            }),
                    )
                    .when(spellcheck::is_supported(), |view| {
                        view.child(Input::new(&self.spell_language_search).w_full())
                    })
                    .when(
                        spellcheck::is_supported() && settings.spellcheck_languages.is_empty(),
                        |view| {
                        view.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(settings.language.text(
                                    "System languages",
                                    "Systemspråk",
                                )),
                        )
                    })
                    .when(spellcheck::is_supported(), |view| {
                        view.children(settings.spellcheck_languages.iter().map(|selected_language| {
                            let selected_language = selected_language.clone();
                            let remove_language = selected_language.clone();
                            Self::row(
                                cx,
                                selected_language.clone(),
                                Button::new(format!("spell-remove-{selected_language}"))
                                    .small()
                                    .ghost()
                                    .label(settings.language.text("Remove", "Fjern"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let remove_language = remove_language.clone();
                                        this.update_setting(
                                            cx,
                                            |settings| {
                                                settings
                                                    .spellcheck_languages
                                                    .retain(|item| item != &remove_language);
                                            },
                                            window,
                                        );
                                    })),
                            )
                        }))
                    })
                    .when(
                        spellcheck::is_supported() && !settings.spellcheck_languages.is_empty(),
                        |view| {
                            view.child(
                                Button::new("spellcheck-system-languages")
                                    .small()
                                    .ghost()
                                    .label(settings.language.text(
                                        "Use system languages",
                                        "Bruk systemspråk",
                                    ))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.update_setting(
                                            cx,
                                            |settings| settings.spellcheck_languages.clear(),
                                            window,
                                        );
                                    })),
                            )
                        },
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .when(
                                spellcheck::is_supported()
                                    && has_invalid_spell_language,
                                |view| {
                                    view.child(settings.language.text(
                                        "One or more selected languages are unavailable.",
                                        "Ett eller flere valgte språk er ikke tilgjengelige.",
                                    ))
                                },
                            ),
                    )
                    .when(spellcheck::is_supported(), |view| {
                        view.child(
                            div()
                                .id("spellcheck-language-list")
                                .max_h(px(160.))
                                .overflow_y_scrollbar()
                                .children(
                                    self.installed_spell_languages
                                        .iter()
                                        .filter(|language| {
                                            language.to_lowercase().contains(&language_query)
                                        })
                                        .map(|language| {
                                            let language = language.to_string();
                                            let selected = settings
                                                .spellcheck_languages
                                                .contains(&language);
                                            let language_id = language.clone();
                                            Self::row(
                                                cx,
                                                language,
                                                Switch::new(format!(
                                                    "spell-language-{language_id}"
                                                ))
                                                .checked(selected)
                                                .on_click(cx.listener(
                                                    move |this, checked, window, cx| {
                                                        let checked = *checked;
                                                        let language_id = language_id.clone();
                                                        this.update_setting(
                                                            cx,
                                                            |s| {
                                                                if checked {
                                                                    if !s
                                                                        .spellcheck_languages
                                                                        .contains(&language_id)
                                                                    {
                                                                        s.spellcheck_languages
                                                                            .push(language_id.clone());
                                                                    }
                                                                } else {
                                                                    s.spellcheck_languages.retain(
                                                                        |item| item != &language_id,
                                                                    );
                                                                }
                                                            },
                                                            window,
                                                        );
                                                    },
                                                )),
                                            )
                                        }),
                                ),
                        )
                    })
            )
            .child(v_flex().gap_2()
                .child(div().text_xs().font_semibold().text_color(cx.theme().muted_foreground).child(settings.language.text("TERMINAL", "TERMINAL")))
                .child(Self::row(cx, settings.language.text("Font", "Skrifttype"), Input::new(&self.terminal_font_input).w(px(180.))))
                .child(Self::row(cx, settings.language.text("Font size", "Skriftstørrelse"), Input::new(&self.terminal_size_input).w(px(90.))))
                .child(Self::row(cx, settings.language.text("Shell", "Kommandotolk"), Input::new(&self.terminal_shell_input).w(px(180.))))
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child(settings.language.text(
                    "Use an installed monospace or Nerd Font. Leave shell blank to use $SHELL. Shell changes apply to new tabs; existing zsh/Oh My Zsh configuration is preserved.",
                    "Bruk en installert monospace- eller Nerd Font. La kommandotolk stå tom for å bruke $SHELL. Endringen gjelder nye faner; eksisterende zsh-/Oh My Zsh-oppsett beholdes."))))
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child("VAULT"),
                    )
                    .child(Self::row(
                        cx,
                        "Attachment folder",
                        Input::new(&self.attachments_input).w(px(180.)),
                    ))
                    .child(Self::row(
                        cx,
                        "Template folder",
                        Input::new(&self.templates_input).w(px(180.)),
                    ))
                    .child(Self::row(
                        cx,
                        "Daily note folder",
                        Input::new(&self.daily_dir_input).w(px(180.)),
                    ))
                    .child(Self::row(
                        cx,
                        "Daily note format",
                        Input::new(&self.daily_format_input).w(px(180.)),
                    ))
                    .child(Self::row(
                        cx,
                        "Unique note folder",
                        Input::new(&self.unique_dir_input).w(px(180.)),
                    ))
                    .child(Self::row(
                        cx,
                        "Unique note format",
                        Input::new(&self.unique_format_input).w(px(180.)),
                    ))
                    .children(self.periodic_inputs.iter().map(|(period, dir, format)| {
                        let label = match period {
                            Period::Week => "Weekly note folder · format",
                            Period::Month => "Monthly note folder · format",
                            Period::Quarter => "Quarterly note folder · format",
                            _ => "Yearly note folder · format",
                        };
                        Self::row(
                            cx,
                            label,
                            h_flex()
                                .gap_1()
                                .child(Input::new(dir).w(px(88.)))
                                .child(Input::new(format).w(px(88.))),
                        )
                    })),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child("ABOUT"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                            "Rísta — a local-first Markdown editor. Rust + GPUI. Files stay files.",
                        ),
                    ),
            )
    }
}

/// A folder setting must stay inside the vault — `C:\x`, `\\server`,
/// `/x`, and `..` would let `root.join` escape it.
fn is_vault_relative(dir: &str) -> bool {
    let path = std::path::Path::new(dir);
    !path.has_root()
        && !path.is_absolute()
        && !path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
}
