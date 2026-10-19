//! Vault link graph — the Graph view. Nodes are notes (plus
//! unresolved `[[targets]]` as dimmer ghosts), edges are `[[wiki]]`,
//! `![[embed]]` and `[label](path)` links. Force-directed layout
//! (Fruchterman–Reingold) settles live over the first few seconds;
//! drag nodes, drag background to pan, scroll to zoom, click a node
//! to open its note.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::actions::{CloseGraph, ToggleLocalGraphPanel};
use crate::app::Workspace;
use crate::settings::{GraphColors, Language};

/// One simulated node — `path` is `None` for ghosts (unresolved link
/// targets, which the reference editor shows too).
struct GNode {
    path: Option<PathBuf>,
    label: SharedString,
    /// The note's first tag — the "tag" color group.
    tag: Option<String>,
    pos: Point<f32>,
    vel: Point<f32>,
    degree: usize,
    ghost: bool,
    /// Attachment file (image) — the "Attachments" display
    /// option: smaller, info-tinted, still a real node you can open.
    attachment: bool,
    /// Pinned nodes (the local-graph center) don't move under layout.
    pinned: bool,
}

enum Drag {
    Pan { last: Point<Pixels>, moved: bool },
    Node { ix: usize, moved: bool },
}

pub struct GraphView {
    focus_handle: FocusHandle,
    workspace: WeakEntity<Workspace>,
    vault: Entity<crate::vault::Vault>,
    nodes: Vec<GNode>,
    edges: Vec<(usize, usize)>,
    /// Adjacency list for hover highlighting.
    adjacent: Vec<Vec<usize>>,
    /// Edges whose endpoints link both ways — arrowheads at both ends.
    mutual: HashSet<usize>,
    /// Local-graph center node (pinned at the origin, emphasised).
    local: Option<usize>,
    /// BFS hop distance from the local center — ring 1 bright,
    /// ring 2 mid, the rest far (the local-graph depth).
    dist: Option<Vec<usize>>,
    /// Local graphs show only nodes within this many hops of the center.
    depth: usize,
    /// Restart the animated layout on the next render (local center moved).
    relayout: bool,
    color_mode: GraphColors,
    /// Colored groups, most common first — index = palette slot.
    groups: Vec<String>,
    /// Each node's palette slot, if its group is colored.
    group_of: Vec<Option<usize>>,
    /// The workspace's active document — painted with a halo ring.
    pub(crate) active: Option<PathBuf>,
    docked: bool,
    filter_visible: bool,
    /// Graph search — non-matching nodes fade out.
    filter: String,
    filter_input: Entity<InputState>,
    _filter_sub: gpui::Subscription,
    _settings_sub: Option<gpui::Subscription>,
    hovered: Option<usize>,
    scale: f32,
    auto_fit: bool,
    /// Pan offset in screen pixels.
    offset: Point<f32>,
    drag: Option<Drag>,
    /// Remaining layout iterations — the canvas animates while > 0.
    steps: u32,
    /// Last painted canvas bounds — hit tests and the label overlay
    /// read it (it is refreshed every prepaint).
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// Parsed link targets per note, keyed by mtime and size — a rebuild only
    /// re-reads and re-parses notes that changed.
    link_cache: LinkCache,
}

const REPULSION: f32 = 110.0; // ideal spacing k
const STEPS_INIT: u32 = 120; // silent warmup before first paint
const STEPS_LIVE: u32 = 600; // animated settle
const LABEL_SIZE: [f32; 2] = [120., 20.];
/// `highlight_palette` length — groups past it keep the default fill.
const GROUP_COLORS: usize = 6;
/// Rebuild warm-up budget in node pairs × steps — a full
/// `STEPS_INIT` up to ~300 nodes, fewer above so big vaults don't
/// stall the UI thread on every save.
const WARM_PAIRS: usize = 5_400_000;

fn warmup_steps(nodes: usize) -> u32 {
    let pairs = (nodes * nodes.saturating_sub(1) / 2).max(1);
    STEPS_INIT.min((WARM_PAIRS / pairs) as u32).max(1)
}

#[derive(Default)]
struct CachedLinks {
    root_files: Vec<String>,
    text_targets: Vec<(String, bool)>,
    first_tag: Option<String>,
}

type LinkCache = HashMap<PathBuf, (Option<(std::time::SystemTime, u64)>, CachedLinks)>;

/// `build()`'s product — nodes, the path → index map, edges, and the
/// adjacency lists hover highlighting walks.
type BuiltGraph = (
    Vec<GNode>,
    HashMap<PathBuf, usize>,
    Vec<(usize, usize)>,
    Vec<Vec<usize>>,
    // Edge indices whose endpoints link both ways.
    HashSet<usize>,
);

/// Hop distance from `center` over `adjacent`; `usize::MAX` for
/// unreachable nodes (disconnected components — the reference editor dims them).
fn bfs_dist(center: usize, adjacent: &[Vec<usize>]) -> Vec<usize> {
    let mut dist = vec![usize::MAX; adjacent.len()];
    let mut queue = std::collections::VecDeque::from([center]);
    dist[center] = 0;
    while let Some(ix) = queue.pop_front() {
        for &nb in &adjacent[ix] {
            if dist[nb] == usize::MAX {
                dist[nb] = dist[ix] + 1;
                queue.push_back(nb);
            }
        }
    }
    dist
}

/// Nodes within `depth` hops of the local center.
fn within_depth(dist: &[usize], depth: usize) -> Vec<bool> {
    dist.iter().map(|d| *d <= depth).collect()
}

/// A note's folder group — its first path component under `root`,
/// or "/" for notes at the root.
fn folder_group(path: &Path, root: &Path) -> Option<String> {
    let mut parts = path.strip_prefix(root).ok()?.components();
    let first = parts.next()?;
    Some(if parts.next().is_some() {
        first.as_os_str().to_string_lossy().into_owned()
    } else {
        "/".into()
    })
}

/// The `slots` most common groups (ties by name), and each key's slot.
fn rank_groups(keys: &[Option<String>], slots: usize) -> (Vec<String>, Vec<Option<usize>>) {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for key in keys.iter().flatten() {
        *counts.entry(key).or_default() += 1;
    }
    let mut ranked: Vec<(&str, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let groups: Vec<String> = ranked
        .into_iter()
        .take(slots)
        .map(|(key, _)| key.to_string())
        .collect();
    let slot = keys
        .iter()
        .map(|key| {
            key.as_ref()
                .and_then(|k| groups.iter().position(|g| g == k))
        })
        .collect();
    (groups, slot)
}

impl GraphView {
    pub(crate) fn local_center_path(&self) -> Option<PathBuf> {
        self.local
            .and_then(|ix| self.nodes.get(ix))
            .and_then(|node| node.path.clone())
    }

    /// Build the node/edge graph from the vault's link index. Also
    /// returns the path → node-index map for the callers that need it
    /// (local-graph center, rebuild position carry-over).
    fn build(vault: &crate::vault::Vault, cache: &mut LinkCache) -> BuiltGraph {
        let mut nodes: Vec<GNode> = Vec::new();
        let mut by_path: HashMap<PathBuf, usize> = HashMap::new();
        let mut ghosts: HashMap<String, usize> = HashMap::new();
        let mut edges: Vec<(usize, usize)> = Vec::new();
        let mut mutual: HashSet<usize> = HashSet::new();
        let mut edge_ix: HashMap<(usize, usize), usize> = HashMap::new();

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
                tag: None,
                pos: point(0., 0.),
                vel: point(0., 0.),
                degree: 0,
                ghost: false,
                attachment: false,
                pinned: false,
            });
        }
        // Attachments — every image in the vault gets a node (the attachments toggle); notes embed them through the same edges.
        for path in vault.images.borrow().values() {
            let ix = nodes.len();
            by_path.insert(path.clone(), ix);
            nodes.push(GNode {
                path: Some(path.clone()),
                label: path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
                    .into(),
                tag: None,
                pos: point(0., 0.),
                vel: point(0., 0.),
                degree: 0,
                ghost: false,
                attachment: true,
                pinned: false,
            });
        }
        cache.retain(|path, _| by_path.contains_key(path));
        for path in &vault.notes {
            // Size backs up mtime — an mtime-preserving sync or two
            // edits within timestamp granularity still re-parse.
            let stamp = std::fs::metadata(path)
                .and_then(|m| Ok((m.modified()?, m.len())))
                .ok();
            if stamp.is_none() || cache.get(path).map(|(st, _)| *st) != Some(stamp) {
                let Ok(text) = std::fs::read_to_string(path) else {
                    cache.remove(path);
                    continue;
                };
                let targets = if crate::canvas::is_canvas(path) {
                    crate::canvas::CanvasDocument::parse(&text)
                        .map(|canvas| CachedLinks {
                            root_files: canvas.vault_file_references(),
                            text_targets: canvas.text_link_targets(),
                            ..Default::default()
                        })
                        .unwrap_or_default()
                } else {
                    CachedLinks {
                        text_targets: crate::vault::local_link_targets(&text),
                        first_tag: crate::properties::first_tag(&text),
                        ..Default::default()
                    }
                };
                cache.insert(path.clone(), (stamp, targets));
            }
            let from = by_path[path];
            nodes[from].tag = cache[path].1.first_tag.clone();
            let from_dir = path.parent().unwrap_or(std::path::Path::new("/"));
            let (resolved, unresolved) = if crate::canvas::is_canvas(path) {
                vault.canvas_outgoing_targets(
                    &cache[path].1.root_files,
                    &cache[path].1.text_targets,
                    from_dir,
                )
            } else {
                vault.outgoing_targets(&cache[path].1.text_targets, from_dir)
            };
            let link = |from: usize,
                        to: usize,
                        edges: &mut Vec<(usize, usize)>,
                        mutual: &mut HashSet<usize>,
                        edge_ix: &mut HashMap<(usize, usize), usize>,
                        nodes: &mut Vec<GNode>| {
                if to == from || edge_ix.contains_key(&(from, to)) {
                    return;
                }
                if let Some(&ix) = edge_ix.get(&(to, from)) {
                    // Both directions link — arrowheads on both rims.
                    mutual.insert(ix);
                    return;
                }
                edge_ix.insert((from, to), edges.len());
                edges.push((from, to));
                nodes[from].degree += 1;
                nodes[to].degree += 1;
            };
            for target in resolved {
                let Some(&to) = by_path.get(&target) else {
                    continue;
                };
                link(from, to, &mut edges, &mut mutual, &mut edge_ix, &mut nodes);
            }
            for name in unresolved {
                let to = *ghosts.entry(name.clone()).or_insert_with(|| {
                    let ix = nodes.len();
                    nodes.push(GNode {
                        path: None,
                        label: name.clone().into(),
                        tag: None,
                        pos: point(0., 0.),
                        vel: point(0., 0.),
                        degree: 0,
                        ghost: true,
                        attachment: false,
                        pinned: false,
                    });
                    ix
                });
                link(from, to, &mut edges, &mut mutual, &mut edge_ix, &mut nodes);
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
        (nodes, by_path, edges, adjacent, mutual)
    }

    /// Base node opacity — filter (non-matches dim) or local depth
    /// (ring 1 bright / 2 mid / rest far). Hover dims further on top.
    fn base_fade(&self, ix: usize) -> f32 {
        if !self.filter.is_empty() {
            return if self.nodes[ix]
                .label
                .to_lowercase()
                .contains(self.filter.as_str())
            {
                1.0
            } else {
                0.12
            };
        }
        match self.dist.as_ref().map(|d| d[ix]) {
            Some(0) | Some(1) => 1.0,
            Some(2) => 0.7,
            Some(_) => 0.5,
            None => 1.0,
        }
    }

    /// Which nodes are on the map — local graphs hide everything past
    /// `depth` hops; the full graph shows all.
    fn shown(&self) -> Vec<bool> {
        match &self.dist {
            Some(dist) => within_depth(dist, self.depth),
            None => vec![true; self.nodes.len()],
        }
    }

    /// Recompute color groups for the current mode, ranked over the vault.
    fn regroup(&mut self, root: Option<&Path>) {
        let keys: Vec<Option<String>> = self
            .nodes
            .iter()
            .map(|n| {
                if n.ghost || n.attachment {
                    return None;
                }
                match self.color_mode {
                    GraphColors::Off => None,
                    GraphColors::Folder => root
                        .zip(n.path.as_deref())
                        .and_then(|(root, path)| folder_group(path, root)),
                    GraphColors::Tag => n.tag.clone(),
                }
            })
            .collect();
        (self.groups, self.group_of) = rank_groups(&keys, GROUP_COLORS);
    }

    /// Pick up graph settings changes — colors regroup, a new depth
    /// re-settles the local layout.
    fn sync_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((mode, depth)) = self.workspace.upgrade().map(|ws| {
            let settings = ws.read(cx).settings();
            (settings.graph_colors, settings.local_graph_depth)
        }) else {
            return;
        };
        if mode != self.color_mode {
            self.color_mode = mode;
            let root = self.vault.read(cx).root.clone();
            self.regroup(root.as_deref());
        }
        if depth != self.depth {
            self.depth = depth;
            self.relayout = self.local.is_some();
        }
        if std::mem::take(&mut self.relayout) {
            let idle = self.steps == 0;
            self.steps = STEPS_LIVE;
            self.auto_fit = true;
            // A running sim chain picks up the new budget by itself.
            if idle {
                self.kick(window, cx);
            }
        }
    }

    fn localized(&self, cx: &App, en: &'static str, nb: &'static str) -> String {
        self.workspace
            .upgrade()
            .map(|workspace| workspace.read(cx).tr(en, nb).to_string())
            .unwrap_or_else(|| en.to_string())
    }

    /// The filter input + its change subscription — every graph owns
    /// one; non-matching nodes fade out like the graph search.
    fn make_filter(
        language: Language,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<InputState>, gpui::Subscription) {
        let placeholder = language.text("Filter graph…", "Filtrer grafen…");
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let sub = cx.subscribe(&input, |this, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.filter = input.read(cx).value().to_lowercase();
                cx.notify();
            }
        });
        (input, sub)
    }

    /// The graph is a cached view and settings live on the workspace —
    /// re-render when the graph settings change.
    fn observe_settings(
        workspace: &WeakEntity<Workspace>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Subscription> {
        let workspace = workspace.upgrade()?;
        Some(cx.observe(&workspace, |this, workspace, cx| {
            let settings = workspace.read(cx).settings();
            if (settings.graph_colors, settings.local_graph_depth) != (this.color_mode, this.depth)
            {
                cx.notify();
            }
        }))
    }

    /// ⌘F while the graph is open — focus the filter field.
    pub(crate) fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.filter_visible {
            self.filter_visible = true;
            cx.notify();
            let filter_input = self.filter_input.clone();
            cx.on_next_frame(window, move |_, window, cx| {
                filter_input.update(cx, |input, cx| input.focus(window, cx));
            });
        } else {
            self.filter_input
                .update(cx, |input, cx| input.focus(window, cx));
        }
    }

    pub(crate) fn toggle_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.filter_visible {
            self.hide_filter(window, cx);
        } else {
            self.focus_filter(window, cx);
        }
    }

    fn hide_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_visible = false;
        self.filter.clear();
        let filter_input = self.filter_input.clone();
        cx.on_next_frame(window, move |_, window, cx| {
            filter_input.update(cx, |input, cx| input.set_value("", window, cx));
        });
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let title = if self.docked || self.local.is_some() {
            self.localized(cx, "Local graph", "Lokal graf")
        } else {
            self.localized(cx, "Graph", "Graf")
        };
        let search_label = self.localized(cx, "Search graph", "Søk i grafen");
        let fit_label = self.localized(cx, "Fit graph to view", "Tilpass grafen til visningen");
        let hide_label = self.localized(cx, "Hide local graph", "Skjul lokal graf");
        let close_label = self.localized(cx, "Close graph (Esc)", "Lukk graf (Esc)");
        h_flex()
            .h_10()
            .flex_shrink_0()
            .px_3()
            .gap_2()
            .items_center()
            .child(
                Icon::new(assets::IconName::Waypoints)
                    .size_3p5()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .font_medium()
                    .child(title),
            )
            .child(
                Button::new("graph-fit")
                    .ghost()
                    .xsmall()
                    .icon(assets::IconName::Maximize)
                    .tooltip(fit_label)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.auto_fit = true;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("graph-filter")
                    .ghost()
                    .xsmall()
                    .icon(assets::IconName::Search)
                    .tooltip(search_label)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_filter(window, cx);
                    })),
            )
            .when(self.docked, |this| {
                this.child(
                    Button::new("close-local-graph-panel")
                        .ghost()
                        .xsmall()
                        .icon(assets::IconName::Close)
                        .tooltip(hide_label)
                        .on_click(cx.listener(|_, _, window, cx| {
                            window.dispatch_action(ToggleLocalGraphPanel.boxed_clone(), cx);
                        })),
                )
            })
            .when(!self.docked, |this| {
                this.child(
                    Button::new("graph-close")
                        .ghost()
                        .xsmall()
                        .icon(assets::IconName::Close)
                        .tooltip(close_label)
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(ws) = this.workspace.upgrade() {
                                ws.update(cx, |ws, cx| ws.close_graph(window, cx));
                            }
                        })),
                )
            })
    }

    fn render_filter_row(&self, cx: &mut Context<Self>) -> Div {
        let input_label = self.localized(cx, "Filter graph", "Filtrer grafen");
        let close_label = self.localized(cx, "Close graph search", "Lukk grafsøk");
        h_flex()
            .h_8()
            .flex_shrink_0()
            .mx_3()
            .mb_2()
            .px_2()
            .gap_2()
            .items_center()
            .bg(cx.theme().muted)
            .rounded(cx.theme().radius)
            .child(
                Icon::new(assets::IconName::Search)
                    .size_3p5()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                Input::new(&self.filter_input)
                    .small()
                    .appearance(false)
                    .bordered(false)
                    .focus_bordered(false)
                    .aria_label(input_label)
                    .flex_1()
                    .min_w_0(),
            )
            .child(
                Button::new("close-graph-filter")
                    .ghost()
                    .xsmall()
                    .icon(assets::IconName::Close)
                    .tooltip(close_label)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.hide_filter(window, cx);
                    })),
            )
    }

    pub fn new(
        workspace: WeakEntity<Workspace>,
        vault: Entity<crate::vault::Vault>,
        language: Language,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut link_cache = LinkCache::new();
        let (nodes, _by_path, edges, adjacent, mutual) =
            Self::build(vault.read(cx), &mut link_cache);
        let (filter_input, _filter_sub) = Self::make_filter(language, window, cx);
        let _settings_sub = Self::observe_settings(&workspace, cx);
        let mut view = Self {
            focus_handle: cx.focus_handle(),
            workspace,
            vault,
            nodes,
            edges,
            adjacent,
            mutual,
            local: None,
            dist: None,
            depth: 1,
            relayout: false,
            color_mode: GraphColors::Off,
            groups: Vec::new(),
            group_of: Vec::new(),
            active: None,
            docked: false,
            filter_visible: true,
            filter: String::new(),
            filter_input,
            _filter_sub,
            _settings_sub,
            hovered: None,
            scale: 1.0,
            auto_fit: true,
            offset: point(0., 0.),
            drag: None,
            steps: STEPS_LIVE,
            bounds: Rc::new(Cell::new(Bounds::default())),
            link_cache,
        };
        for _ in 0..warmup_steps(view.nodes.len()) {
            view.step();
        }
        view.recenter();
        view.kick(window, cx);
        view
    }

    /// Local graph — `center`'s node pinned at the origin; only its
    /// neighbourhood within the depth setting is shown.
    pub fn new_local(
        workspace: WeakEntity<Workspace>,
        vault: Entity<crate::vault::Vault>,
        center: &std::path::Path,
        language: Language,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut link_cache = LinkCache::new();
        let (mut nodes, by_path, edges, adjacent, mutual) =
            Self::build(vault.read(cx), &mut link_cache);
        let (filter_input, _filter_sub) = Self::make_filter(language, window, cx);
        let _settings_sub = Self::observe_settings(&workspace, cx);
        let local = by_path.get(center).copied();
        if let Some(ix) = local {
            nodes[ix].pos = point(0., 0.);
            nodes[ix].pinned = true;
        }
        let dist = local.map(|c| bfs_dist(c, &adjacent));
        let mut view = Self {
            focus_handle: cx.focus_handle(),
            workspace,
            vault,
            nodes,
            edges,
            adjacent,
            mutual,
            local,
            dist,
            depth: 1,
            relayout: false,
            color_mode: GraphColors::Off,
            groups: Vec::new(),
            group_of: Vec::new(),
            active: Some(center.to_path_buf()),
            docked: false,
            filter_visible: true,
            filter: String::new(),
            filter_input,
            _filter_sub,
            _settings_sub,
            hovered: None,
            // Fit the whole neighbourhood until the user pans or zooms.
            scale: 1.0,
            auto_fit: true,
            offset: point(0., 0.),
            drag: None,
            steps: STEPS_LIVE,
            bounds: Rc::new(Cell::new(Bounds::default())),
            link_cache,
        };
        for _ in 0..warmup_steps(view.nodes.len()) {
            view.step();
        }
        view.kick(window, cx);
        view
    }

    pub fn new_docked(
        workspace: WeakEntity<Workspace>,
        vault: Entity<crate::vault::Vault>,
        center: Option<&std::path::Path>,
        language: Language,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = if let Some(center) = center {
            Self::new_local(workspace, vault, center, language, window, cx)
        } else {
            Self::new(workspace, vault, language, window, cx)
        };
        view.docked = true;
        view.filter_visible = false;
        view
    }

    pub(crate) fn set_local_center(
        &mut self,
        path: Option<&std::path::Path>,
        cx: &mut Context<Self>,
    ) {
        let next = path.and_then(|path| {
            self.nodes
                .iter()
                .position(|node| node.path.as_deref() == Some(path))
        });
        if self.local == next && self.active.as_deref() == path {
            return;
        }
        for node in &mut self.nodes {
            node.pinned = false;
        }
        if let Some(ix) = next {
            let center = self.nodes[ix].pos;
            for node in &mut self.nodes {
                node.pos.x -= center.x;
                node.pos.y -= center.y;
            }
            self.nodes[ix].pos = point(0., 0.);
            self.nodes[ix].pinned = true;
        }
        self.local = next;
        self.active = path.map(Path::to_path_buf);
        self.dist = self.local.map(|center| bfs_dist(center, &self.adjacent));
        // The neighbourhood changed — let it settle (render has the window).
        self.relayout = self.local.is_some();
        if self.docked {
            self.offset = point(0., 0.);
            self.auto_fit = true;
        }
        cx.notify();
    }

    /// Rebuild the node set after vault changes — positions carry over
    /// by path (and ghosts by label) so the map doesn't jump.
    pub(crate) fn rebuild(&mut self, cx: &mut Context<Self>) {
        let (mut nodes, _by_path, edges, adjacent, mutual) =
            Self::build(self.vault.read(cx), &mut self.link_cache);
        let mut old_pos: HashMap<String, Point<f32>> = HashMap::new();
        for n in &self.nodes {
            let key = n
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("ghost:{}", n.label));
            old_pos.insert(key, n.pos);
        }
        let mut placed = vec![false; nodes.len()];
        for (n, placed) in nodes.iter_mut().zip(&mut placed) {
            let key = n
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("ghost:{}", n.label));
            if let Some(pos) = old_pos.get(&key) {
                n.pos = *pos;
                *placed = true;
            }
        }
        let carried = placed.iter().filter(|p| **p).count();
        // New nodes start beside their already-placed neighbours, not
        // out on the seed circle — the short warm-up below can't
        // haul them in on a big graph.
        if carried > 0 {
            for ix in 0..nodes.len() {
                if placed[ix] {
                    continue;
                }
                let near: Vec<Point<f32>> = adjacent[ix]
                    .iter()
                    .filter(|nb| placed[**nb])
                    .map(|nb| nodes[*nb].pos)
                    .collect();
                if near.is_empty() {
                    continue;
                }
                let len = near.len() as f32;
                let mx = near.iter().map(|p| p.x).sum::<f32>() / len;
                let my = near.iter().map(|p| p.y).sum::<f32>() / len;
                // Golden-angle offset keeps siblings from stacking.
                let a = ix as f32 * 2.4;
                nodes[ix].pos = point(
                    mx + REPULSION * 0.5 * a.cos(),
                    my + REPULSION * 0.5 * a.sin(),
                );
            }
        }
        // Local center may sit at a new index — re-find it by path.
        if let Some(c) = self.local {
            if let Some(old) = self.nodes.get(c).and_then(|n| n.path.clone()) {
                self.local = nodes
                    .iter()
                    .position(|n| n.path.as_deref() == Some(old.as_path()));
                if let Some(ix) = self.local {
                    nodes[ix].pos = point(0., 0.);
                    nodes[ix].pinned = true;
                }
            } else {
                self.local = None;
            }
        }
        // A vault refresh can remove or reorder indexed nodes.
        self.hovered = None;
        self.drag = None;
        self.nodes = nodes;
        self.edges = edges;
        self.adjacent = adjacent;
        self.mutual = mutual;
        let root = self.vault.read(cx).root.clone();
        self.regroup(root.as_deref());
        self.dist = self.local.map(|c| bfs_dist(c, &self.adjacent));
        if self.docked {
            let active = self.active.clone();
            self.set_local_center(active.as_deref(), cx);
        }
        // Bound synchronous warm-up on both new and carried-over layouts.
        // Cooling keeps an already settled map from jumping.
        let warm = warmup_steps(self.nodes.len());
        let live = self.steps;
        self.steps = warm;
        while self.steps > 0 {
            self.step();
            self.steps = self.steps.saturating_sub(1);
        }
        self.steps = live;
        self.recenter();
        cx.notify();
    }

    /// One Fruchterman–Reingold iteration in place.
    fn step(&mut self) {
        let k = REPULSION;
        // Hidden (out-of-depth) nodes neither push, pull nor move.
        let shown = self.shown();
        let ids: Vec<usize> = (0..self.nodes.len()).filter(|ix| shown[*ix]).collect();
        let pos: Vec<[f32; 2]> = ids
            .iter()
            .map(|ix| [self.nodes[*ix].pos.x, self.nodes[*ix].pos.y])
            .collect();
        let mut disp: Vec<Point<f32>> = vec![point(0., 0.); self.nodes.len()];
        for (ix, [x, y]) in ids.iter().zip(crate::graph_layout::repulsion(&pos, k * k)) {
            disp[*ix] = point(x, y);
        }
        // Edge springs.
        for (a, b) in &self.edges {
            if !(shown[*a] && shown[*b]) {
                continue;
            }
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
            dvec.x -= node.pos.x * 0.04;
            dvec.y -= node.pos.y * 0.04;
        }
        // Cool the temperature; cap displacement by it.
        let t = (STEPS_LIVE as f32 * 0.5) * (self.steps as f32 / STEPS_LIVE as f32) + 1.0;
        let mut max_move = 0f32;
        for ((node, dvec), shown) in self.nodes.iter_mut().zip(disp.iter()).zip(shown) {
            if node.pinned || !shown {
                continue;
            }
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

    /// Shift the whole map so its centroid sits at the origin —
    /// keeps the settled layout centred in the pane. Local graphs
    /// keep their pinned centre at the origin instead.
    fn recenter(&mut self) {
        if self.nodes.is_empty() || self.local.is_some() {
            return;
        }
        let mut sx = 0f32;
        let mut sy = 0f32;
        for n in &self.nodes {
            sx += n.pos.x;
            sy += n.pos.y;
        }
        let n = self.nodes.len() as f32;
        let mean = point(sx / n, sy / n);
        for node in &mut self.nodes {
            node.pos.x -= mean.x;
            node.pos.y -= mean.y;
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
        let shown = self.shown();
        for ix in (0..self.nodes.len()).filter(|ix| shown[*ix]) {
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
    /// Image files — the attachments display option.
    attachment: Vec<bool>,
    lit: Vec<bool>,
    /// Per-node opacity — local mode fades the wider map, hover dims
    /// non-neighbours further.
    fade: Vec<f32>,
    /// Nodes that get a halo ring (local center / active doc).
    ring: Vec<bool>,
    /// Nodes on the map — local graphs hide the far vault.
    shown: Vec<bool>,
    /// Group color, when graph colors are on and the group is ranked.
    group_fill: Vec<Option<Hsla>>,
}

impl Render for GraphView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_settings(window, cx);
        let shown = self.shown();
        if self.auto_fit {
            let bounds = self.bounds.get();
            if let Some((scale, [x, y])) = crate::graph_layout::fit(
                self.nodes
                    .iter()
                    .zip(&shown)
                    .filter(|(_, shown)| **shown)
                    .map(|(n, _)| [n.pos.x, n.pos.y]),
                [f32::from(bounds.size.width), f32::from(bounds.size.height)],
            ) {
                self.scale = scale;
                self.offset = point(x, y);
            }
        }
        let this = cx.entity();
        let filter_visible = self.filter_visible;
        let header = self.render_header(cx);
        let filter_row = self.render_filter_row(cx);
        let mut root = v_flex()
            .id("graph-view")
            .size_full()
            .track_focus(&self.focus_handle)
            .key_context("RistaGraph")
            .on_action(cx.listener(|this, _: &CloseGraph, window, cx| {
                if this.filter_visible {
                    this.hide_filter(window, cx);
                } else if !this.docked {
                    if let Some(ws) = this.workspace.upgrade() {
                        ws.update(cx, |ws, cx| ws.close_graph(window, cx));
                    }
                }
            }))
            .child(header);
        if filter_visible {
            root = root.child(filter_row);
        }
        if self.docked && self.local.is_none() {
            let message = self.localized(
                cx,
                "Open a note in this vault to see its local graph",
                "Åpne et notat i dette hvelvet for å se den lokale grafen",
            );
            return root
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_4()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(message),
                )
                .into_any_element();
        }
        let theme = cx.theme();
        let bounds_slot = self.bounds.clone();
        let scale = self.scale;
        let offset = self.offset;
        let edges = self.edges.clone();
        let mutual = self.mutual.clone();
        let positions: Vec<Point<f32>> = self.nodes.iter().map(|n| n.pos).collect();
        let degrees: Vec<usize> = self.nodes.iter().map(|n| n.degree).collect();
        let ghosts: Vec<bool> = self.nodes.iter().map(|n| n.ghost).collect();
        let attachments: Vec<bool> = self.nodes.iter().map(|n| n.attachment).collect();
        let lit = self.lit();
        let hovered = self.hovered;
        // Base fade — filter matches bright, or local depth rings
        // (hop 1 bright / hop 2 mid / the rest far back).
        let fade: Vec<f32> = (0..self.nodes.len())
            .map(|ix| {
                let base = self.base_fade(ix);
                if hovered.is_some() && !lit[ix] {
                    base * 0.35
                } else {
                    base
                }
            })
            .collect();
        let ring: Vec<bool> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(ix, n)| {
                self.local == Some(ix)
                    || (self.active.is_some() && n.path.as_ref() == self.active.as_ref())
            })
            .collect();
        let edge_color = theme.border;
        let edge_lit = theme.muted_foreground;
        let node_fill = theme.primary;
        let node_hover = theme.primary_hover;
        let ghost_fill = theme.muted_foreground.opacity(0.45);
        // Attachments read as files, not notes — info tint, a bit dimmer.
        let attach_fill = theme.info.opacity(0.75);
        let label_color = theme.muted_foreground;
        let palette = crate::preview::highlight_palette(theme);
        let group_fill: Vec<Option<Hsla>> = (0..self.nodes.len())
            .map(|ix| {
                self.group_of
                    .get(ix)
                    .copied()
                    .flatten()
                    .map(|slot| palette[slot])
            })
            .collect();
        let painted_shown = shown.clone();

        // Screen geometry for hit tests/labels needs the painted bounds —
        // read the slot written by last frame's prepaint.
        let last_bounds = bounds_slot.get();

        let graph = cx.entity().downgrade();
        let canvas = gpui::canvas(
            move |bounds, window, cx| {
                if bounds_slot.replace(bounds) != bounds {
                    // Labels were placed with stale bounds (first paint,
                    // pane resize) — render once more with these.
                    let graph = graph.clone();
                    window.defer(cx, move |_, cx| {
                        let _ = graph.update(cx, |_, cx| cx.notify());
                    });
                }
                let mut nodes_px = Vec::with_capacity(positions.len());
                for (ix, pos) in positions.iter().enumerate() {
                    let cxp = bounds.origin.x + bounds.size.width / 2.;
                    let cyp = bounds.origin.y + bounds.size.height / 2.;
                    let c = point(
                        cxp + px(pos.x * scale + offset.x),
                        cyp + px(pos.y * scale + offset.y),
                    );
                    let r = (4. + (degrees[ix] as f32).sqrt() * 1.6)
                        * if ghosts[ix] {
                            0.7
                        } else if attachments[ix] {
                            0.8
                        } else {
                            1.
                        }
                        * scale;
                    nodes_px.push((c, px(r.max(2.5))));
                }
                Painted {
                    nodes: nodes_px,
                    ghost: ghosts,
                    attachment: attachments,
                    lit,
                    fade,
                    ring,
                    shown: painted_shown,
                    group_fill,
                }
            },
            move |bounds, painted, window, _cx| {
                let painted: Painted = painted;
                // Edges — dim layer first, lit neighbours on top. In
                // local mode edges off the center's neighbourhood fade.
                let mut dim = gpui::PathBuilder::stroke(px(1.));
                let mut dim_far = gpui::PathBuilder::stroke(px(1.));
                let mut any_far = false;
                let mut hot = gpui::PathBuilder::stroke(px(1.5));
                let mut any_hot = false;
                for (a, b) in &edges {
                    if !(painted.shown[*a] && painted.shown[*b]) {
                        continue;
                    }
                    let (ca, ra) = painted.nodes[*a];
                    let (cb, rb) = painted.nodes[*b];
                    // Both ends past the same pane edge — never visible.
                    if (ca.x < bounds.left() && cb.x < bounds.left())
                        || (ca.x > bounds.right() && cb.x > bounds.right())
                        || (ca.y < bounds.top() && cb.y < bounds.top())
                        || (ca.y > bounds.bottom() && cb.y > bounds.bottom())
                    {
                        continue;
                    }
                    let lit_edge = painted.lit[*a] || painted.lit[*b];
                    if lit_edge {
                        any_hot = true;
                        hot.move_to(ca);
                        hot.line_to(cb);
                    } else if painted.fade[*a].min(painted.fade[*b]) < 0.5 {
                        any_far = true;
                        dim_far.move_to(ca);
                        dim_far.line_to(cb);
                    } else {
                        dim.move_to(ca);
                        dim.line_to(cb);
                    }
                    let _ = (ra, rb);
                }
                if let Ok(path) = dim.build() {
                    window.paint_path(path, edge_color);
                }
                if any_far {
                    if let Ok(path) = dim_far.build() {
                        window.paint_path(path, edge_color.opacity(0.3));
                    }
                }
                if any_hot {
                    if let Ok(path) = hot.build() {
                        window.paint_path(path, edge_lit);
                    }
                    // Arrowheads on lit edges only — direction reads
                    // while inspecting, zero clutter at rest.
                    for (eix, (a, b)) in edges.iter().enumerate() {
                        if !(painted.lit[*a] || painted.lit[*b])
                            || !(painted.shown[*a] && painted.shown[*b])
                        {
                            continue;
                        }
                        let (ca, ra) = painted.nodes[*a];
                        let (cb, rb) = painted.nodes[*b];
                        let dx = f32::from(cb.x - ca.x);
                        let dy = f32::from(cb.y - ca.y);
                        let d = (dx * dx + dy * dy).sqrt().max(1.);
                        let (ux, uy) = (dx / d, dy / d);
                        // Tip on the target's rim, wings behind it.
                        for (tip_at, rim) in [(cb, rb)]
                            .into_iter()
                            .chain(mutual.contains(&eix).then_some((ca, ra)))
                        {
                            let r = f32::from(rim);
                            let (ux, uy) = if tip_at == cb { (ux, uy) } else { (-ux, -uy) };
                            let tip = point(tip_at.x - px(ux * r), tip_at.y - px(uy * r));
                            let (wx, wy) = (-uy, ux);
                            let back = point(tip.x - px(ux * 7.), tip.y - px(uy * 7.));
                            let mut arrow = gpui::PathBuilder::fill();
                            arrow.move_to(tip);
                            arrow.line_to(point(back.x + px(wx * 2.6), back.y + px(wy * 2.6)));
                            arrow.line_to(point(back.x - px(wx * 2.6), back.y - px(wy * 2.6)));
                            arrow.close();
                            if let Ok(path) = arrow.build() {
                                window.paint_path(path, edge_lit);
                            }
                        }
                    }
                }
                // Nodes — ghosts first, then normal, hovered last.
                for (ix, (c, r)) in painted.nodes.iter().enumerate() {
                    // Off-pane dots (halo included) — skip the quads.
                    let reach = *r + px(3.);
                    if !painted.shown[ix]
                        || c.x + reach < bounds.left()
                        || c.x - reach > bounds.right()
                        || c.y + reach < bounds.top()
                        || c.y - reach > bounds.bottom()
                    {
                        continue;
                    }
                    let is_hover = hovered == Some(ix);
                    let color = if painted.ghost[ix] {
                        ghost_fill
                    } else if painted.attachment[ix] {
                        attach_fill
                    } else if let Some(fill) = painted.group_fill[ix] {
                        fill
                    } else if is_hover {
                        node_hover
                    } else {
                        node_fill
                    };
                    let color = color.opacity(painted.fade[ix]);
                    if painted.ring[ix] {
                        // Halo ring — the local center / active note;
                        // it fades with its node under the filter.
                        window.paint_quad(gpui::PaintQuad {
                            bounds: Bounds {
                                origin: point(c.x - *r - px(3.), c.y - *r - px(3.)),
                                size: size(*r * 2. + px(6.), *r * 2. + px(6.)),
                            },
                            corner_radii: Corners::all(*r + px(3.)),
                            background: gpui::transparent_black().into(),
                            border_widths: Edges::all(px(1.5)),
                            border_color: node_hover.opacity(painted.fade[ix]),
                            border_style: BorderStyle::default(),
                        });
                    }
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

        // Prioritize the hovered, then the center label; cull overlapping
        // titles before layout. Local graphs label every visible note.
        let mut labels = crate::graph_layout::LabelGrid::new(LABEL_SIZE);
        let first = self
            .hovered
            .into_iter()
            .chain(self.local.filter(|c| Some(*c) != self.hovered));
        let label_layer = first
            .chain(
                (0..self.nodes.len())
                    .filter(|ix| Some(*ix) != self.hovered && Some(*ix) != self.local),
            )
            .filter(|ix| shown[*ix])
            .map(|ix| (ix, &self.nodes[ix]))
            .filter(|(ix, n)| {
                self.hovered == Some(*ix)
                    || if !self.filter.is_empty() {
                        self.base_fade(*ix) >= 1.0
                    } else if self.local.is_some() {
                        !n.ghost
                    } else {
                        n.degree >= 3 && !n.ghost
                    }
            })
            // Off-pane labels still cost layout — big vaults have thousands.
            .filter(|(ix, n)| {
                let c = self.to_screen(n.pos, last_bounds) - last_bounds.origin;
                let top = c.y + px(self.node_radius(*ix) * self.scale + 2.);
                c.x > px(-60.)
                    && c.x < last_bounds.size.width + px(60.)
                    && top > px(-20.)
                    && top < last_bounds.size.height
            })
            .filter(|(ix, n)| {
                let c = self.to_screen(n.pos, last_bounds) - last_bounds.origin;
                labels.insert([
                    f32::from(c.x) - LABEL_SIZE[0] / 2.,
                    f32::from(c.y) + self.node_radius(*ix) * self.scale + 2.,
                ])
            })
            .map(|(ix, n)| {
                // to_screen gives window-absolute points; absolute
                // positioning here is relative to the pane's origin.
                let c = self.to_screen(n.pos, last_bounds) - last_bounds.origin;
                let r = px(self.node_radius(ix) * self.scale);
                let lift = self.hovered == Some(ix);
                let fade = self.base_fade(ix);
                div()
                    .absolute()
                    .left(c.x - px(LABEL_SIZE[0] / 2.))
                    .top(c.y + r + px(2.))
                    .w(px(LABEL_SIZE[0]))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(if lift {
                                theme.foreground
                            } else {
                                label_color.opacity(fade)
                            })
                            .max_w_full()
                            .truncate()
                            .child(n.label.clone()),
                    )
            })
            .collect::<Vec<_>>();

        let canvas_view = div()
            .id("graph-canvas")
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .on_mouse_down(gpui::MouseButton::Left, {
                let this = this.clone();
                move |ev: &gpui::MouseDownEvent, _window, cx| {
                    this.update(cx, |view, _cx| {
                        view.auto_fit = false;
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
                    // Resolve the clicked node's path inside the
                    // update, then open+close AFTER it returns —
                    // open_document runs persist_tabs →
                    // sync_graph_active, which would re-enter this
                    // same entity update and panic.
                    let open = this.update(cx, |view, cx| {
                        let drag = view.drag.take();
                        let path = match drag {
                            Some(Drag::Node { ix, moved: false }) => {
                                view.nodes.get(ix).and_then(|n| {
                                    n.path.clone().or_else(|| {
                                        // Ghost click → create the
                                        // missing note.
                                        let label = n.label.to_string();
                                        if label.is_empty()
                                            || label.contains('/')
                                            || label.contains('\\')
                                        {
                                            return None;
                                        }
                                        Some(
                                            view.vault
                                                .read(cx)
                                                .root
                                                .clone()?
                                                .join(format!("{label}.md")),
                                        )
                                    })
                                })
                            }
                            _ => None,
                        };
                        path.map(|path| (path, view.docked))
                    });
                    if let Some((path, docked)) = open {
                        let _ = std::fs::File::create_new(&path);
                        let ws = this.read(cx).workspace.upgrade();
                        if let Some(ws) = ws {
                            ws.update(cx, |ws, cx| {
                                ws.open_document_pub(path, window, cx);
                                if !docked {
                                    ws.close_graph(window, cx);
                                }
                            });
                        }
                    }
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
                        view.auto_fit = false;
                        view.scale = (view.scale * (1. + d * 0.0025)).clamp(0.001, 4.0);
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
            });
        // Caption counts only what the map shows.
        let notes = self
            .nodes
            .iter()
            .zip(&shown)
            .filter(|(n, shown)| **shown && !n.ghost && !n.attachment)
            .count();
        let links = self
            .edges
            .iter()
            .filter(|(a, b)| shown[*a] && shown[*b])
            .count();
        let canvas_view = canvas_view
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
                    .child(if !self.filter.is_empty() {
                        let matches = self
                            .nodes
                            .iter()
                            .zip(&shown)
                            .filter(|(n, shown)| {
                                **shown && n.label.to_lowercase().contains(self.filter.as_str())
                            })
                            .count();
                        format!("{matches} matching · {notes} notes · {links} links")
                    } else {
                        match self.local {
                            Some(_c) if self.docked => format!("{notes} notes · {links} links"),
                            Some(c) => format!(
                                "local graph · {} · {notes} notes · {links} links",
                                self.nodes[c].label,
                            ),
                            None => format!(
                                "{notes} notes · {links} links · scroll to zoom, drag to pan"
                            ),
                        }
                    }),
            );
        // Legend — the colored groups on the map, palette order.
        let mut on_map = [false; GROUP_COLORS];
        for slot in self
            .group_of
            .iter()
            .zip(&shown)
            .filter_map(|(slot, shown)| slot.filter(|_| *shown))
        {
            on_map[slot] = true;
        }
        let legend = (self.color_mode != GraphColors::Off && on_map.contains(&true)).then(|| {
            h_flex()
                .flex_shrink_0()
                .flex_wrap()
                .px_4()
                .pb_2()
                .gap_x_3()
                .gap_y_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .children(
                    self.groups
                        .iter()
                        .enumerate()
                        .filter(|(slot, _)| on_map[*slot])
                        .map(|(slot, name)| {
                            h_flex()
                                .gap_1()
                                .items_center()
                                .child(div().size_2().rounded_full().bg(palette[slot]))
                                .child(name.clone())
                        }),
                )
        });
        root.child(canvas_view).children(legend).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{bfs_dist, folder_group, rank_groups, within_depth};
    use std::path::Path;

    #[test]
    fn tag_color_uses_the_first_declared_tag() {
        use crate::properties::first_tag;
        let frontmatter = "---\ntags: [zebra, alpha]\n---\nBody #inline\n";
        assert_eq!(first_tag(frontmatter).as_deref(), Some("zebra"));
        assert_eq!(
            first_tag("Body #zebra and #alpha\n").as_deref(),
            Some("zebra")
        );
        assert_eq!(first_tag("No tags here\n"), None);
    }

    #[test]
    fn local_depth_limits_the_visible_set() {
        // 0 - 1 - 2 - 3 - 4, a branch 1 - 5, and an orphan 6.
        let mut adjacent = vec![Vec::new(); 7];
        for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 4), (1, 5)] {
            adjacent[a].push(b);
            adjacent[b].push(a);
        }
        let dist = bfs_dist(0, &adjacent);
        let visible = |depth| -> Vec<usize> {
            within_depth(&dist, depth)
                .into_iter()
                .enumerate()
                .filter_map(|(ix, shown)| shown.then_some(ix))
                .collect()
        };
        assert_eq!(visible(1), [0, 1]);
        assert_eq!(visible(2), [0, 1, 2, 5]);
        assert_eq!(visible(3), [0, 1, 2, 3, 5]);
    }

    #[test]
    fn folder_groups_rank_by_frequency() {
        let root = Path::new("/vault");
        let keys: Vec<Option<String>> = [
            "/vault/work/a.md",
            "/vault/work/deep/b.md",
            "/vault/work/c.md",
            "/vault/top.md",
            "/vault/home/d.md",
            "/vault/home/e.md",
            "/vault/x.md",
            "/elsewhere/f.md",
        ]
        .into_iter()
        .map(|p| folder_group(Path::new(p), root))
        .collect();
        assert_eq!(keys[3].as_deref(), Some("/"));
        assert_eq!(keys[7], None);
        let (groups, slots) = rank_groups(&keys, 6);
        // "/" and "home" tie at two — ties order by name.
        assert_eq!(groups, ["work", "/", "home"]);
        assert_eq!(
            slots,
            [
                Some(0),
                Some(0),
                Some(0),
                Some(1),
                Some(2),
                Some(2),
                Some(1),
                None
            ]
        );
    }

    #[test]
    fn tag_groups_past_the_palette_stay_uncolored() {
        let keys: Vec<Option<String>> = ["b", "a", "b", "c", "a", "b", "d"]
            .into_iter()
            .map(|t| Some(t.to_string()))
            .chain([None])
            .collect();
        let (groups, slots) = rank_groups(&keys, 2);
        assert_eq!(groups, ["b", "a"]);
        assert_eq!(
            slots,
            [
                Some(0),
                Some(1),
                Some(0),
                None,
                Some(1),
                Some(0),
                None,
                None
            ]
        );
    }
}
