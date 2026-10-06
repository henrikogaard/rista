//! Vault link graph — Obsidian's Graph view. Nodes are notes (plus
//! unresolved `[[targets]]` as dimmer ghosts), edges are `[[wiki]]`,
//! `![[embed]]` and `[label](path)` links. Force-directed layout
//! (Fruchterman–Reingold) settles live over the first few seconds;
//! drag nodes, drag background to pan, scroll to zoom, click a node
//! to open its note.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::actions::CloseGraph;
use crate::app::Workspace;

/// One simulated node — `path` is `None` for ghosts (unresolved link
/// targets, which Obsidian shows too).
struct GNode {
    path: Option<PathBuf>,
    label: SharedString,
    pos: Point<f32>,
    vel: Point<f32>,
    degree: usize,
    ghost: bool,
}

enum Drag {
    Pan { last: Point<Pixels>, moved: bool },
    Node { ix: usize, moved: bool },
}

pub struct GraphView {
    focus_handle: FocusHandle,
    workspace: WeakEntity<Workspace>,
    nodes: Vec<GNode>,
    edges: Vec<(usize, usize)>,
    /// Adjacency list for hover highlighting.
    adjacent: Vec<Vec<usize>>,
    hovered: Option<usize>,
    scale: f32,
    /// Pan offset in screen pixels.
    offset: Point<f32>,
    drag: Option<Drag>,
    /// Remaining layout iterations — the canvas animates while > 0.
    steps: u32,
    /// Last painted canvas bounds — hit tests and the label overlay
    /// read it (it is refreshed every prepaint).
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

const REPULSION: f32 = 110.0; // ideal spacing k
const STEPS_INIT: u32 = 120; // silent warmup before first paint
const STEPS_LIVE: u32 = 600; // animated settle

impl GraphView {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        vault: Entity<crate::vault::Vault>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut nodes: Vec<GNode> = Vec::new();
        let mut by_path: HashMap<PathBuf, usize> = HashMap::new();
        let mut ghosts: HashMap<String, usize> = HashMap::new();
        let mut edges: Vec<(usize, usize)> = Vec::new();

        let vault = vault.read(cx);
        for path in &vault.notes {
            let ix = nodes.len();
            by_path.insert(path.clone(), ix);
            nodes.push(GNode {
                path: Some(path.clone()),
                label: path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
                    .into(),
                pos: point(0., 0.),
                vel: point(0., 0.),
                degree: 0,
                ghost: false,
            });
        }
        for path in &vault.notes {
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            let from = by_path[path];
            let from_dir = path.parent().unwrap_or(std::path::Path::new("/"));
            let (resolved, unresolved) = vault.outgoing_from(&text, from_dir);
            for target in resolved {
                let Some(&to) = by_path.get(&target) else {
                    continue;
                };
                if to != from && !edges.contains(&(from, to)) && !edges.contains(&(to, from)) {
                    edges.push((from, to));
                    nodes[from].degree += 1;
                    nodes[to].degree += 1;
                }
            }
            for name in unresolved {
                let to = *ghosts.entry(name.clone()).or_insert_with(|| {
                    let ix = nodes.len();
                    nodes.push(GNode {
                        path: None,
                        label: name.clone().into(),
                        pos: point(0., 0.),
                        vel: point(0., 0.),
                        degree: 0,
                        ghost: true,
                    });
                    ix
                });
                if to != from && !edges.contains(&(from, to)) && !edges.contains(&(to, from)) {
                    edges.push((from, to));
                    nodes[from].degree += 1;
                    nodes[to].degree += 1;
                }
            }
        }

        let mut adjacent = vec![Vec::new(); nodes.len()];
        for (a, b) in &edges {
            adjacent[*a].push(*b);
            adjacent[*b].push(*a);
        }

        // Circle seed — deterministic layout start.
        let n = nodes.len().max(1) as f32;
        let radius = (60. + 30. * n.sqrt()).max(120.);
        for (ix, node) in nodes.iter_mut().enumerate() {
            let t = ix as f32 / n * std::f32::consts::TAU;
            node.pos = point(radius * t.cos(), radius * t.sin());
        }

        let mut view = Self {
            focus_handle: cx.focus_handle(),
            workspace,
            nodes,
            edges,
            adjacent,
            hovered: None,
            scale: 1.0,
            offset: point(0., 0.),
            drag: None,
            steps: STEPS_LIVE,
            bounds: Rc::new(Cell::new(Bounds::default())),
        };
        for _ in 0..STEPS_INIT.min(view.steps) {
            view.step();
        }
        view.kick(window, cx);
        view
    }

    /// One Fruchterman–Reingold iteration in place.
    fn step(&mut self) {
        let k = REPULSION;
        let n = self.nodes.len();
        let mut disp: Vec<Point<f32>> = vec![point(0., 0.); n];
        // Repulsion between every pair.
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = self.nodes[i].pos.x - self.nodes[j].pos.x;
                let dy = self.nodes[i].pos.y - self.nodes[j].pos.y;
                let d = (dx * dx + dy * dy).sqrt().max(1.0);
                let f = (k * k / d).min(2000.) / d;
                let fx = dx * f;
                let fy = dy * f;
                disp[i].x += fx;
                disp[i].y += fy;
                disp[j].x -= fx;
                disp[j].y -= fy;
            }
        }
        // Edge springs.
        for (a, b) in &self.edges {
            let dx = self.nodes[*b].pos.x - self.nodes[*a].pos.x;
            let dy = self.nodes[*b].pos.y - self.nodes[*a].pos.y;
            let d = (dx * dx + dy * dy).sqrt().max(1.0);
            let f = (d * d / k).min(4000.) / d * 0.5;
            let fx = dx * f;
            let fy = dy * f;
            disp[*a].x += fx;
            disp[*a].y += fy;
            disp[*b].x -= fx;
            disp[*b].y -= fy;
        }
        // Gentle pull to center keeps stray clusters on screen.
        for (node, dvec) in self.nodes.iter().zip(disp.iter_mut()) {
            dvec.x -= node.pos.x * 0.02;
            dvec.y -= node.pos.y * 0.02;
        }
        // Cool the temperature; cap displacement by it.
        let t = (STEPS_LIVE as f32 * 0.5) * (self.steps as f32 / STEPS_LIVE as f32) + 1.0;
        let mut max_move = 0f32;
        for (node, dvec) in self.nodes.iter_mut().zip(disp.iter()) {
            let d = (dvec.x * dvec.x + dvec.y * dvec.y).sqrt();
            if d < 0.01 {
                continue;
            }
            let capped = d.min(t);
            node.pos.x += dvec.x / d * capped;
            node.pos.y += dvec.y / d * capped;
            max_move = max_move.max(capped);
        }
        if max_move < 0.4 {
            self.steps = 0;
        }
    }

    /// Queue the next sim frame while steps remain.
    fn kick(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.steps == 0 {
            return;
        }
        cx.on_next_frame(window, |view, window, cx| {
            if view.steps == 0 {
                return;
            }
            view.steps -= 1;
            view.step();
            cx.notify();
            view.kick(window, cx);
        });
    }

    /// World → screen: center of the painted bounds, then pan/zoom.
    fn to_screen(&self, pos: Point<f32>, bounds: Bounds<Pixels>) -> Point<Pixels> {
        let cx = bounds.origin.x + bounds.size.width / 2.;
        let cy = bounds.origin.y + bounds.size.height / 2.;
        point(
            cx + px(pos.x * self.scale + self.offset.x),
            cy + px(pos.y * self.scale + self.offset.y),
        )
    }

    fn node_radius(&self, ix: usize) -> f32 {
        let node = &self.nodes[ix];
        (4. + (node.degree as f32).sqrt() * 1.6) * if node.ghost { 0.7 } else { 1. }
    }

    /// Nearest node within its radius (+ slack) of a screen point.
    fn hit(&self, at: Point<Pixels>, bounds: Bounds<Pixels>) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for ix in 0..self.nodes.len() {
            let c = self.to_screen(self.nodes[ix].pos, bounds);
            let dx = f32::from(at.x) - f32::from(c.x);
            let dy = f32::from(at.y) - f32::from(c.y);
            let d = (dx * dx + dy * dy).sqrt();
            let r = self.node_radius(ix) * self.scale + 8.;
            if d < r && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((ix, d));
            }
        }
        best.map(|(ix, _)| ix)
    }

    /// Neighbourhood of `hovered` (or empty) — for edge highlight.
    fn lit(&self) -> Vec<bool> {
        let mut lit = vec![false; self.nodes.len()];
        if let Some(h) = self.hovered {
            lit[h] = true;
            for &n in &self.adjacent[h] {
                lit[n] = true;
            }
        }
        lit
    }

    fn to_world(&self, at: Point<Pixels>, bounds: Bounds<Pixels>) -> Point<f32> {
        let cx = bounds.origin.x + bounds.size.width / 2.;
        let cy = bounds.origin.y + bounds.size.height / 2.;
        point(
            (f32::from(at.x) - f32::from(cx) - self.offset.x) / self.scale,
            (f32::from(at.y) - f32::from(cy) - self.offset.y) / self.scale,
        )
    }
}

impl Focusable for GraphView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

struct Painted {
    nodes: Vec<(Point<Pixels>, Pixels)>, // center, radius
    ghost: Vec<bool>,
    lit: Vec<bool>,
}

impl Render for GraphView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let this = cx.entity();
        let bounds_slot = self.bounds.clone();
        let scale = self.scale;
        let offset = self.offset;
        let edges = self.edges.clone();
        let positions: Vec<Point<f32>> = self.nodes.iter().map(|n| n.pos).collect();
        let degrees: Vec<usize> = self.nodes.iter().map(|n| n.degree).collect();
        let ghosts: Vec<bool> = self.nodes.iter().map(|n| n.ghost).collect();
        let lit = self.lit();
        let hovered = self.hovered;
        let edge_color = theme.border;
        let edge_lit = theme.muted_foreground;
        let node_fill = theme.primary;
        let node_hover = theme.primary_hover;
        let ghost_fill = theme.muted_foreground.opacity(0.45);
        let label_color = theme.muted_foreground;

        // Screen geometry for hit tests/labels needs the painted bounds —
        // read the slot written by last frame's prepaint.
        let last_bounds = bounds_slot.get();

        let canvas = gpui::canvas(
            move |bounds, _window, _cx| {
                bounds_slot.set(bounds);
                let mut nodes_px = Vec::with_capacity(positions.len());
                for (ix, pos) in positions.iter().enumerate() {
                    let cxp = bounds.origin.x + bounds.size.width / 2.;
                    let cyp = bounds.origin.y + bounds.size.height / 2.;
                    let c = point(
                        cxp + px(pos.x * scale + offset.x),
                        cyp + px(pos.y * scale + offset.y),
                    );
                    let r = (4. + (degrees[ix] as f32).sqrt() * 1.6)
                        * if ghosts[ix] { 0.7 } else { 1. }
                        * scale;
                    nodes_px.push((c, px(r.max(2.5))));
                }
                Painted {
                    nodes: nodes_px,
                    ghost: ghosts,
                    lit,
                }
            },
            move |bounds, painted, window, _cx| {
                let painted: Painted = painted;
                let _ = bounds;
                // Edges — dim layer first, lit neighbours on top.
                let mut dim = gpui::PathBuilder::stroke(px(1.));
                let mut hot = gpui::PathBuilder::stroke(px(1.5));
                let mut any_hot = false;
                for (a, b) in &edges {
                    let (ca, ra) = painted.nodes[*a];
                    let (cb, rb) = painted.nodes[*b];
                    let lit_edge = painted.lit[*a] || painted.lit[*b];
                    if lit_edge {
                        any_hot = true;
                        hot.move_to(ca);
                        hot.line_to(cb);
                    } else {
                        dim.move_to(ca);
                        dim.line_to(cb);
                    }
                    let _ = (ra, rb);
                }
                if let Ok(path) = dim.build() {
                    window.paint_path(path, edge_color);
                }
                if any_hot {
                    if let Ok(path) = hot.build() {
                        window.paint_path(path, edge_lit);
                    }
                }
                // Nodes — ghosts first, then normal, hovered last.
                for (ix, (c, r)) in painted.nodes.iter().enumerate() {
                    let is_hover = hovered == Some(ix);
                    let is_dimmed = hovered.is_some() && !painted.lit[ix];
                    let color = if painted.ghost[ix] {
                        ghost_fill
                    } else if is_hover {
                        node_hover
                    } else {
                        node_fill
                    };
                    let color = if is_dimmed {
                        color.opacity(0.35)
                    } else {
                        color
                    };
                    window.paint_quad(gpui::PaintQuad {
                        bounds: Bounds {
                            origin: point(c.x - *r, c.y - *r),
                            size: size(*r * 2., *r * 2.),
                        },
                        corner_radii: Corners::all(*r),
                        background: color.into(),
                        border_widths: Edges::default(),
                        border_color: gpui::transparent_black(),
                        border_style: BorderStyle::default(),
                    });
                }
            },
        );

        // Labels — degree ≥ 3 or the hovered node, positioned over the dot.
        let label_layer = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(ix, n)| self.hovered == Some(*ix) || (n.degree >= 3 && !n.ghost))
            .map(|(ix, n)| {
                // to_screen gives window-absolute points; absolute
                // positioning here is relative to the pane's origin.
                let c = self.to_screen(n.pos, last_bounds) - last_bounds.origin;
                let r = px(self.node_radius(ix) * self.scale);
                let lift = self.hovered == Some(ix);
                div()
                    .absolute()
                    .left(c.x - px(60.))
                    .top(c.y + r + px(2.))
                    .w(px(120.))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(if lift { theme.foreground } else { label_color })
                            .whitespace_nowrap()
                            .child(n.label.clone()),
                    )
            })
            .collect::<Vec<_>>();

        div()
            .id("graph-view")
            .size_full()
            .relative()
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .key_context("RistaGraph")
            .on_action(cx.listener(|this, _: &CloseGraph, window, cx| {
                if let Some(ws) = this.workspace.upgrade() {
                    ws.update(cx, |ws, cx| ws.close_graph(window, cx));
                }
            }))
            .on_mouse_down(gpui::MouseButton::Left, {
                let this = this.clone();
                move |ev: &gpui::MouseDownEvent, _window, cx| {
                    this.update(cx, |view, _cx| {
                        let b = view.bounds.get();
                        view.drag = Some(match view.hit(ev.position, b) {
                            Some(ix) => Drag::Node { ix, moved: false },
                            None => Drag::Pan {
                                last: ev.position,
                                moved: false,
                            },
                        });
                    });
                }
            })
            .on_mouse_move({
                let this = this.clone();
                move |ev: &gpui::MouseMoveEvent, _window, cx| {
                    this.update(cx, |view, cx| {
                        let b = view.bounds.get();
                        match view.drag.take() {
                            Some(Drag::Pan { last, moved }) => {
                                let dx = f32::from(ev.position.x) - f32::from(last.x);
                                let dy = f32::from(ev.position.y) - f32::from(last.y);
                                if dx.abs() + dy.abs() > 2. {
                                    view.offset.x += dx;
                                    view.offset.y += dy;
                                    cx.notify();
                                }
                                view.drag = Some(Drag::Pan {
                                    last: ev.position,
                                    moved: moved || dx.abs() + dy.abs() > 2.,
                                });
                            }
                            Some(Drag::Node { ix, .. }) => {
                                let w = view.to_world(ev.position, b);
                                view.nodes[ix].pos = w;
                                view.nodes[ix].vel = point(0., 0.);
                                view.drag = Some(Drag::Node { ix, moved: true });
                                cx.notify();
                            }
                            None => {
                                let hit = view.hit(ev.position, b);
                                if hit != view.hovered {
                                    view.hovered = hit;
                                    cx.notify();
                                }
                            }
                        }
                    });
                }
            })
            .on_mouse_up(gpui::MouseButton::Left, {
                let this = this.clone();
                move |_ev: &gpui::MouseUpEvent, window, cx| {
                    this.update(cx, |view, cx| {
                        let drag = view.drag.take();
                        if let Some(Drag::Node { ix, moved: false }) = drag {
                            if let Some(path) = view.nodes.get(ix).and_then(|n| n.path.clone()) {
                                if let Some(ws) = view.workspace.upgrade() {
                                    ws.update(cx, |ws, cx| {
                                        // Click = jump to the note (Obsidian
                                        // local-graph semantics) — the graph
                                        // hands the editor back afterwards.
                                        ws.open_document_pub(path, window, cx);
                                        ws.close_graph(window, cx);
                                    });
                                }
                            }
                        }
                    });
                }
            })
            .on_scroll_wheel({
                let this = this.clone();
                move |ev: &gpui::ScrollWheelEvent, _window, cx| {
                    this.update(cx, |view, cx| {
                        // Zoom anchored at the cursor.
                        let delta = ev.delta.pixel_delta(px(20.));
                        let d = f32::from(delta.y);
                        if d.abs() < 0.01 {
                            return;
                        }
                        let old = view.scale;
                        view.scale = (view.scale * (1. + d * 0.0025)).clamp(0.25, 4.0);
                        if view.scale == old {
                            return;
                        }
                        let b = view.bounds.get();
                        let cxp = b.origin.x + b.size.width / 2.;
                        let cyp = b.origin.y + b.size.height / 2.;
                        // Keep the cursor-anchored world point stationary.
                        let mx = f32::from(ev.position.x) - f32::from(cxp);
                        let my = f32::from(ev.position.y) - f32::from(cyp);
                        view.offset.x = mx - (mx - view.offset.x) * (view.scale / old);
                        view.offset.y = my - (my - view.offset.y) * (view.scale / old);
                        cx.notify();
                    });
                }
            })
            .child(canvas.size_full())
            .children(label_layer)
            .child(
                // Caption — bottom-left, muted.
                div()
                    .absolute()
                    .bottom_3()
                    .left_4()
                    .text_xs()
                    .text_color(theme.muted_foreground.opacity(0.7))
                    .child(format!(
                        "{} notes · {} links · scroll to zoom, drag to pan",
                        self.nodes.iter().filter(|n| !n.ghost).count(),
                        self.edges.len()
                    )),
            )
            .child(
                div().absolute().top_2().right_3().child(
                    Button::new("graph-close")
                        .ghost()
                        .xsmall()
                        .icon(assets::IconName::Close)
                        .tooltip("Close graph (Esc)")
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(ws) = this.workspace.upgrade() {
                                ws.update(cx, |ws, cx| ws.close_graph(window, cx));
                            }
                        })),
                ),
            )
    }
}
