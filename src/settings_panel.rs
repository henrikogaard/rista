//! Settings sheet — segmented controls and switches, applied live.

use crate::app::Workspace;
use crate::settings::{Appearance, Settings, ViewMode, EDITOR_FONTS};
use gpui_kit::base::StyledExt;
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, IndexPath};
use gpui_kit::*;

pub struct SettingsView {
    workspace: WeakEntity<Workspace>,
    font_select: Entity<SelectState<SearchableVec<String>>>,
    font_size_select: Entity<SelectState<SearchableVec<String>>>,
    ui_size_select: Entity<SelectState<SearchableVec<String>>>,
    tab_size_select: Entity<SelectState<SearchableVec<String>>>,
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

        let mut subs = Vec::new();
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

        Self {
            workspace,
            font_select,
            font_size_select,
            ui_size_select,
            tab_size_select,
            _subscriptions: subs,
        }
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
            .size_full()
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
                        "Theme",
                        TabBar::new("appearance")
                            .segmented()
                            .selected_index(appearance_ix)
                            .children([
                                Tab::new().label("Dark"),
                                Tab::new().label("Light"),
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
                    .child(Self::row(
                        cx,
                        "Editor font",
                        Select::new(&self.font_select).w(px(180.)),
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
