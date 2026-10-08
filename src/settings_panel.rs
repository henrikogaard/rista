//! Settings sheet — segmented controls and switches, applied live.

use crate::app::Workspace;
use crate::settings::{
    Appearance, Language, PropertiesVisibility, Settings, ViewMode, EDITOR_FONTS,
};
use crate::theme;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::theme::ThemeMode;
use gpui_kit::component::Sizable;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, IndexPath};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

pub struct SettingsView {
    workspace: WeakEntity<Workspace>,
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

        let mut subs = Vec::new();
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

        Self {
            workspace,
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
        ws.update(cx, |ws, cx| {
            let mut settings = ws.settings().clone();
            patch(&mut settings);
            ws.apply_settings(settings, window, cx);
        });
    }

    fn row(cx: &App, label: &'static str, control: impl IntoElement) -> gpui_kit::Div {
        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(control)
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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

        v_flex()
            .id("settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .p_4()
            .gap_5()
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
                    )),
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
                    )),
            )
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
                    )),
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
