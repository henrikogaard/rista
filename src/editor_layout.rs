use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub(crate) struct GroupId(u64);

impl GroupId {
    pub(crate) fn value(self) -> u64 {
        self.0
    }
}

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub(crate) struct SplitId(u64);

impl SplitId {
    pub(crate) fn value(self) -> u64 {
        self.0
    }
}

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
pub(crate) struct TabToken(u64);

impl TabToken {
    pub(crate) fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum Axis {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum PaneMode {
    #[default]
    Source,
    Split,
    Preview,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Group {
    pub(crate) id: GroupId,
    #[serde(default)]
    pub(crate) tabs: Vec<TabToken>,
    #[serde(default)]
    pub(crate) selected: Option<TabToken>,
    #[serde(default)]
    pub(crate) mode: PaneMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum Node {
    Group(Group),
    Split {
        id: SplitId,
        axis: Axis,
        #[serde(default)]
        sizes: Option<[f32; 2]>,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    #[serde(default = "default_snapshot_version")]
    pub(crate) version: u8,
    pub(crate) root: Node,
    #[serde(default)]
    pub(crate) paths: BTreeMap<TabToken, PathBuf>,
    #[serde(default)]
    pub(crate) focused: GroupId,
}

fn default_snapshot_version() -> u8 {
    1
}

#[derive(Clone, Debug)]
pub(crate) struct EditorLayout {
    root: Node,
    paths: BTreeMap<TabToken, PathBuf>,
    focused: GroupId,
    next_group: u64,
    next_split: u64,
    next_token: u64,
}

impl EditorLayout {
    pub(crate) fn snapshot_for_startup(
        snapshot: Option<Snapshot>,
        has_explicit_paths: bool,
    ) -> Option<Snapshot> {
        if has_explicit_paths {
            None
        } else {
            snapshot
        }
    }

    pub(crate) fn empty() -> Self {
        let group = GroupId(1);
        Self {
            root: Node::Group(Group {
                id: group,
                tabs: Vec::new(),
                selected: None,
                mode: PaneMode::Source,
            }),
            paths: BTreeMap::new(),
            focused: group,
            next_group: 2,
            next_split: 1,
            next_token: 1,
        }
    }

    pub(crate) fn restore(snapshot: Snapshot, existing: &BTreeSet<PathBuf>) -> Self {
        let mut layout = Self::empty();
        let mut used_groups = BTreeSet::new();
        let mut used_splits = BTreeSet::new();
        let mut used_tokens = BTreeSet::new();
        let root = sanitize_node(
            snapshot.root,
            &mut used_groups,
            &mut used_splits,
            &mut used_tokens,
            &snapshot.paths,
            existing,
        );
        layout.root = root;
        layout.paths = snapshot
            .paths
            .into_iter()
            .filter(|(token, path)| used_tokens.contains(token) && existing.contains(path))
            .collect();
        layout.focused = if layout.find_group(snapshot.focused).is_some() {
            snapshot.focused
        } else {
            layout.first_group().unwrap_or(GroupId(1))
        };
        layout.reseed();
        layout
    }

    pub(crate) fn restore_for_documents(
        snapshot: Snapshot,
        document_paths: &[PathBuf],
    ) -> (Self, Vec<Option<TabToken>>) {
        let existing = document_paths.iter().cloned().collect::<BTreeSet<_>>();
        let mut layout = Self::restore(snapshot, &existing);
        let mut by_path = BTreeMap::<PathBuf, VecDeque<usize>>::new();
        for (index, path) in document_paths.iter().enumerate() {
            by_path.entry(path.clone()).or_default().push_back(index);
        }
        let mut bindings = vec![None; document_paths.len()];
        for token in layout.tokens() {
            let Some(path) = layout.path(token) else {
                continue;
            };
            if let Some(index) = by_path.get_mut(path).and_then(VecDeque::pop_front) {
                bindings[index] = Some(token);
            }
        }
        let bound = bindings.iter().flatten().copied().collect::<BTreeSet<_>>();
        let unbound = layout
            .tokens()
            .filter(|token| !bound.contains(token))
            .collect::<Vec<_>>();
        for token in unbound {
            layout.close(token);
        }
        (layout, bindings)
    }

    pub(crate) fn migrate_legacy(
        paths: &[PathBuf],
        active_path: Option<&Path>,
        mode: PaneMode,
    ) -> (Self, Vec<TabToken>, Option<usize>) {
        let mut layout = Self::empty();
        let group = layout.focused();
        layout.set_mode(group, mode);
        let mut tokens = Vec::with_capacity(paths.len());
        let mut active_index = None;
        for (index, path) in paths.iter().enumerate() {
            let token = layout.new_token(path.clone());
            layout.add_tab(group, token);
            if active_index.is_none() && active_path == Some(path.as_path()) {
                active_index = Some(index);
            }
            tokens.push(token);
        }
        if let Some(token) = active_index.and_then(|index| tokens.get(index)).copied() {
            layout.activate(token);
        }
        (layout, tokens, active_index)
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            version: 1,
            root: self.root.clone(),
            paths: self.paths.clone(),
            focused: self.focused,
        }
    }

    pub(crate) fn root(&self) -> &Node {
        &self.root
    }

    pub(crate) fn focused(&self) -> GroupId {
        self.focused
    }

    pub(crate) fn focus(&mut self, group: GroupId) -> bool {
        if self.find_group(group).is_some() {
            self.focused = group;
            true
        } else {
            false
        }
    }

    pub(crate) fn path(&self, token: TabToken) -> Option<&Path> {
        self.paths.get(&token).map(PathBuf::as_path)
    }

    pub(crate) fn replace_path(&mut self, token: TabToken, path: PathBuf) -> bool {
        let Some(existing) = self.paths.get_mut(&token) else {
            return false;
        };
        *existing = path;
        true
    }

    pub(crate) fn tokens(&self) -> impl Iterator<Item = TabToken> + '_ {
        self.paths.keys().copied()
    }

    pub(crate) fn new_token(&mut self, path: PathBuf) -> TabToken {
        let used = self.paths.keys().map(|token| token.0).collect();
        let token = TabToken(take_id(&mut self.next_token, &used));
        self.paths.insert(token, path);
        token
    }

    pub(crate) fn add_tab(&mut self, group: GroupId, token: TabToken) -> bool {
        let Some(group) = self.find_group_mut(group) else {
            return false;
        };
        if group.tabs.contains(&token) {
            return false;
        }
        group.tabs.push(token);
        group.selected = Some(token);
        true
    }

    pub(crate) fn selected(&self, group: GroupId) -> Option<TabToken> {
        self.find_group(group).and_then(|g| g.selected)
    }

    pub(crate) fn group_tokens(&self, group: GroupId) -> Vec<TabToken> {
        self.find_group(group)
            .map(|g| g.tabs.clone())
            .unwrap_or_default()
    }

    pub(crate) fn tokens_to_right(&self, token: TabToken) -> Vec<TabToken> {
        let Some(group) = self.group_of(token) else {
            return Vec::new();
        };
        let tabs = self.group_tokens(group);
        let Some(index) = tabs.iter().position(|candidate| *candidate == token) else {
            return Vec::new();
        };
        tabs.into_iter().skip(index + 1).collect()
    }

    pub(crate) fn group_for(&self, token: TabToken) -> Option<GroupId> {
        self.group_of(token)
    }

    pub(crate) fn group_specs(&self) -> Vec<(GroupId, Vec<TabToken>, Option<TabToken>, PaneMode)> {
        self.groups_in_order()
            .into_iter()
            .filter_map(|id| {
                self.find_group(id)
                    .map(|group| (id, group.tabs.clone(), group.selected, group.mode))
            })
            .collect()
    }

    pub(crate) fn set_mode(&mut self, group: GroupId, mode: PaneMode) -> bool {
        let Some(group) = self.find_group_mut(group) else {
            return false;
        };
        group.mode = mode;
        true
    }

    pub(crate) fn set_split_sizes(&mut self, split: SplitId, sizes: [f32; 2]) -> bool {
        let sizes = if sizes.iter().all(|size| size.is_finite() && *size > 0.0) {
            Some(sizes)
        } else {
            None
        };
        set_split_sizes(&mut self.root, split, sizes)
    }

    pub(crate) fn groups_in_order(&self) -> Vec<GroupId> {
        let mut result = Vec::new();
        collect_groups(&self.root, &mut result);
        result
    }

    pub(crate) fn split(&mut self, group: GroupId, token: TabToken, axis: Axis) -> Option<GroupId> {
        self.split_at(group, token, axis, false)
    }

    pub(crate) fn split_at(
        &mut self,
        group: GroupId,
        token: TabToken,
        axis: Axis,
        before: bool,
    ) -> Option<GroupId> {
        if !self.remove_from_group(group, token) {
            return None;
        }
        let source_node = self.root_group(group)?;
        let mode = match &source_node {
            Node::Group(source) => source.mode,
            Node::Split { .. } => return None,
        };
        let groups = self.groups_in_order().into_iter().map(|id| id.0).collect();
        let splits = {
            let mut ids = BTreeSet::new();
            collect_split_ids(&self.root, &mut ids);
            ids
        };
        let new_group = GroupId(take_id(&mut self.next_group, &groups));
        let split_id = SplitId(take_id(&mut self.next_split, &splits));
        let new_node = Node::Group(Group {
            id: new_group,
            tabs: vec![token],
            selected: Some(token),
            mode,
        });
        let (first, second) = if before {
            (new_node, source_node)
        } else {
            (source_node, new_node)
        };
        if !replace_group(
            &mut self.root,
            group,
            Node::Split {
                id: split_id,
                axis,
                sizes: None,
                first: Box::new(first),
                second: Box::new(second),
            },
        ) {
            self.add_tab(group, token);
            return None;
        }
        self.focused = new_group;
        Some(new_group)
    }

    pub(crate) fn move_tab(&mut self, token: TabToken, destination: GroupId, index: usize) -> bool {
        let Some(source) = self.group_of(token) else {
            return false;
        };
        if source == destination {
            let Some(group) = self.find_group_mut(source) else {
                return false;
            };
            let Some(from) = group.tabs.iter().position(|item| *item == token) else {
                return false;
            };
            group.tabs.remove(from);
            group.tabs.insert(index.min(group.tabs.len()), token);
            group.selected = Some(token);
            return true;
        }
        if self.find_group(destination).is_none() || !self.remove_from_group(source, token) {
            return false;
        }
        let Some(group) = self.find_group_mut(destination) else {
            return false;
        };
        group.tabs.insert(index.min(group.tabs.len()), token);
        group.selected = Some(token);
        self.collapse_empty(source);
        self.focused = destination;
        true
    }

    pub(crate) fn close(&mut self, token: TabToken) -> bool {
        let Some(group) = self.group_of(token) else {
            return false;
        };
        if !self.remove_from_group(group, token) {
            return false;
        }
        self.paths.remove(&token);
        self.collapse_empty(group);
        if self.find_group(self.focused).is_none() {
            self.focused = self.first_group().unwrap_or(GroupId(1));
        }
        true
    }

    pub(crate) fn select(&mut self, group: GroupId, token: TabToken) -> bool {
        let Some(group_ref) = self.find_group_mut(group) else {
            return false;
        };
        if group_ref.tabs.contains(&token) {
            group_ref.selected = Some(token);
            self.focused = group;
            true
        } else {
            false
        }
    }

    pub(crate) fn activate(&mut self, token: TabToken) -> bool {
        let Some(group) = self.group_of(token) else {
            return false;
        };
        self.select(group, token)
    }

    fn root_group(&self, id: GroupId) -> Option<Node> {
        find_group_node(&self.root, id).cloned()
    }

    fn find_group(&self, id: GroupId) -> Option<&Group> {
        find_group_node(&self.root, id).and_then(|n| match n {
            Node::Group(g) => Some(g),
            Node::Split { .. } => None,
        })
    }

    fn find_group_mut(&mut self, id: GroupId) -> Option<&mut Group> {
        find_group_node_mut(&mut self.root, id).and_then(|n| match n {
            Node::Group(g) => Some(g),
            Node::Split { .. } => None,
        })
    }

    fn group_of(&self, token: TabToken) -> Option<GroupId> {
        self.groups_in_order().into_iter().find(|id| {
            self.find_group(*id)
                .is_some_and(|group| group.tabs.contains(&token))
        })
    }

    fn remove_from_group(&mut self, id: GroupId, token: TabToken) -> bool {
        let Some(group) = self.find_group_mut(id) else {
            return false;
        };
        let Some(index) = group.tabs.iter().position(|item| *item == token) else {
            return false;
        };
        group.tabs.remove(index);
        group.selected = group
            .selected
            .filter(|selected| group.tabs.contains(selected))
            .or_else(|| {
                group
                    .tabs
                    .get(index.min(group.tabs.len().saturating_sub(1)))
                    .copied()
            });
        true
    }

    fn collapse_empty(&mut self, id: GroupId) {
        if self.groups_in_order().len() <= 1
            || self.find_group(id).is_none_or(|g| !g.tabs.is_empty())
        {
            return;
        }
        collapse_group(&mut self.root, id);
    }

    fn first_group(&self) -> Option<GroupId> {
        self.groups_in_order().into_iter().next()
    }

    fn reseed(&mut self) {
        let groups = self
            .groups_in_order()
            .into_iter()
            .map(|id| id.0)
            .collect::<BTreeSet<_>>();
        let mut splits = BTreeSet::new();
        collect_split_ids(&self.root, &mut splits);
        let tokens = self.paths.keys().map(|id| id.0).collect::<BTreeSet<_>>();
        self.next_group = next_id(&groups).max(self.next_group);
        self.next_split = next_id(&splits).max(self.next_split);
        self.next_token = next_id(&tokens).max(self.next_token);
    }
}

fn next_id(used: &BTreeSet<u64>) -> u64 {
    let mut candidate = used
        .last()
        .copied()
        .and_then(|id| id.checked_add(1))
        .unwrap_or(0);
    while used.contains(&candidate) {
        candidate = candidate.wrapping_add(1);
    }
    candidate
}

fn take_id(next: &mut u64, used: &BTreeSet<u64>) -> u64 {
    let mut candidate = *next;
    while used.contains(&candidate) {
        candidate = candidate.wrapping_add(1);
    }
    *next = candidate.wrapping_add(1);
    candidate
}

fn sanitize_node(
    node: Node,
    groups: &mut BTreeSet<u64>,
    splits: &mut BTreeSet<u64>,
    tokens: &mut BTreeSet<TabToken>,
    paths: &BTreeMap<TabToken, PathBuf>,
    existing: &BTreeSet<PathBuf>,
) -> Node {
    match node {
        Node::Group(mut group) => {
            group.id = GroupId(unique_id(group.id.0, groups));
            group.tabs.retain(|token| {
                paths.get(token).is_some_and(|path| existing.contains(path))
                    && tokens.insert(*token)
            });
            group.selected = group.selected.filter(|token| group.tabs.contains(token));
            if group.selected.is_none() {
                group.selected = group.tabs.first().copied();
            }
            Node::Group(group)
        }
        Node::Split {
            id,
            axis,
            sizes,
            first,
            second,
        } => {
            let id = SplitId(unique_id(id.0, splits));
            let sizes =
                sizes.filter(|sizes| sizes.iter().all(|size| size.is_finite() && *size > 0.0));
            Node::Split {
                id,
                axis,
                sizes,
                first: Box::new(sanitize_node(
                    *first, groups, splits, tokens, paths, existing,
                )),
                second: Box::new(sanitize_node(
                    *second, groups, splits, tokens, paths, existing,
                )),
            }
        }
    }
}

fn find_group_node(node: &Node, id: GroupId) -> Option<&Node> {
    match node {
        Node::Group(group) => (group.id == id).then_some(node),
        Node::Split { first, second, .. } => {
            find_group_node(first, id).or_else(|| find_group_node(second, id))
        }
    }
}

fn find_group_node_mut(node: &mut Node, id: GroupId) -> Option<&mut Node> {
    match node {
        Node::Group(group) => (group.id == id).then_some(node),
        Node::Split { first, second, .. } => {
            find_group_node_mut(first, id).or_else(|| find_group_node_mut(second, id))
        }
    }
}

fn collect_groups(node: &Node, out: &mut Vec<GroupId>) {
    match node {
        Node::Group(group) => out.push(group.id),
        Node::Split { first, second, .. } => {
            collect_groups(first, out);
            collect_groups(second, out);
        }
    }
}

fn collect_split_ids(node: &Node, ids: &mut BTreeSet<u64>) {
    if let Node::Split {
        id, first, second, ..
    } = node
    {
        ids.insert(id.0);
        collect_split_ids(first, ids);
        collect_split_ids(second, ids);
    }
}

fn unique_id(id: u64, used: &mut BTreeSet<u64>) -> u64 {
    if used.insert(id) {
        return id;
    }
    let mut candidate = 0;
    while !used.insert(candidate) {
        candidate = candidate.wrapping_add(1);
    }
    candidate
}

fn set_split_sizes(node: &mut Node, target: SplitId, sizes: Option<[f32; 2]>) -> bool {
    match node {
        Node::Group(_) => false,
        Node::Split {
            id,
            sizes: current,
            first,
            second,
            ..
        } => {
            if *id == target {
                if *current == sizes {
                    return false;
                }
                *current = sizes;
                true
            } else {
                set_split_sizes(first, target, sizes) || set_split_sizes(second, target, sizes)
            }
        }
    }
}

fn replace_group(root: &mut Node, id: GroupId, replacement: Node) -> bool {
    match root {
        Node::Group(group) => {
            if group.id == id {
                *root = replacement;
                true
            } else {
                false
            }
        }
        Node::Split { first, second, .. } => {
            replace_group(first, id, replacement.clone()) || replace_group(second, id, replacement)
        }
    }
}

fn collapse_group(node: &mut Node, id: GroupId) -> bool {
    match node {
        Node::Group(_) => false,
        Node::Split { first, second, .. } => {
            if matches!(first.as_ref(), Node::Group(group) if group.id == id) {
                *node = (**second).clone();
                true
            } else if matches!(second.as_ref(), Node::Group(group) if group.id == id) {
                *node = (**first).clone();
                true
            } else {
                collapse_group(first, id) || collapse_group(second, id)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> PathBuf {
        PathBuf::from(format!("/vault/{name}.md"))
    }

    #[test]
    fn split_move_reorder_and_prune_keep_stable_tokens() {
        let mut layout = EditorLayout::empty();
        let root = layout.focused();
        let first = layout.new_token(path("first"));
        let second = layout.new_token(path("second"));
        assert!(layout.add_tab(root, first));
        assert!(layout.add_tab(root, second));
        let right = layout.split(root, second, Axis::Horizontal).unwrap();
        assert_eq!(layout.selected(right), Some(second));
        assert!(layout.move_tab(first, right, 0));
        assert_eq!(layout.group_tokens(right), vec![first, second]);
        assert!(layout.close(first));
        assert_eq!(layout.path(second), Some(path("second").as_path()));
    }

    #[test]
    fn active_group_survives_removal_and_final_group_stays() {
        let mut layout = EditorLayout::empty();
        let root = layout.focused();
        let token = layout.new_token(path("one"));
        layout.add_tab(root, token);
        assert!(layout.close(token));
        assert_eq!(layout.groups_in_order().len(), 1);
        assert!(layout.find_group(layout.focused()).is_some());
    }

    #[test]
    fn removing_last_selected_tab_selects_preceding_tab_and_move_keeps_source_selected() {
        let mut layout = EditorLayout::empty();
        let root = layout.focused();
        let first = layout.new_token(path("first"));
        let second = layout.new_token(path("second"));
        let last = layout.new_token(path("last"));
        layout.add_tab(root, first);
        layout.add_tab(root, second);
        layout.add_tab(root, last);
        assert!(layout.close(last));
        assert_eq!(layout.selected(root), Some(second));

        let mut layout = EditorLayout::empty();
        let root = layout.focused();
        let first = layout.new_token(path("move-first"));
        let selected_last = layout.new_token(path("move-last"));
        layout.add_tab(root, first);
        layout.add_tab(root, selected_last);
        let destination = layout.split(root, selected_last, Axis::Horizontal).unwrap();
        assert_eq!(layout.selected(root), Some(first));
        assert_eq!(layout.selected(destination), Some(selected_last));
    }

    #[test]
    fn nested_snapshot_roundtrips_and_drops_missing_paths() {
        let mut layout = EditorLayout::empty();
        let root = layout.focused();
        let kept = layout.new_token(path("kept"));
        let missing = layout.new_token(path("missing"));
        layout.add_tab(root, kept);
        layout.add_tab(root, missing);
        let right = layout.split(root, missing, Axis::Horizontal).unwrap();
        let missing_nested = layout.new_token(path("missing-nested"));
        layout.add_tab(right, missing_nested);
        let bottom = layout.split(right, missing_nested, Axis::Vertical).unwrap();
        let (outer_split, inner_split) = match layout.root() {
            Node::Split { id, second, .. } => match second.as_ref() {
                Node::Split { id: inner, .. } => (*id, *inner),
                Node::Group(_) => unreachable!(),
            },
            Node::Group(_) => unreachable!(),
        };
        layout.set_split_sizes(outer_split, [640.0, 360.0]);
        layout.set_split_sizes(inner_split, [300.0, 300.0]);
        let encoded = serde_json::to_string(&layout.snapshot()).unwrap();
        let snapshot = serde_json::from_str(&encoded).unwrap();
        let restored = EditorLayout::restore(snapshot, &BTreeSet::from([path("kept")]));
        assert_eq!(restored.path(kept), Some(path("kept").as_path()));
        assert_eq!(restored.path(missing), None);
        assert_eq!(restored.path(missing_nested), None);
        assert_eq!(restored.groups_in_order(), vec![root, right, bottom]);
        assert_eq!(restored.group_tokens(root), vec![kept]);
        assert!(restored.group_tokens(right).is_empty());
        assert!(restored.group_tokens(bottom).is_empty());
        match restored.root() {
            Node::Split {
                sizes: outer,
                second,
                ..
            } => {
                assert_eq!(*outer, Some([640.0, 360.0]));
                match second.as_ref() {
                    Node::Split { sizes: inner, .. } => {
                        assert_eq!(*inner, Some([300.0, 300.0]));
                    }
                    Node::Group(_) => unreachable!(),
                }
            }
            Node::Group(_) => unreachable!(),
        }
    }

    #[test]
    fn snapshot_binds_duplicate_paths_to_distinct_documents() {
        let paths = vec![path("same"), path("same")];
        let mut layout = EditorLayout::empty();
        let root = layout.focused();
        let first = layout.new_token(paths[0].clone());
        let second = layout.new_token(paths[1].clone());
        layout.add_tab(root, first);
        layout.add_tab(root, second);
        let (restored, bindings) = EditorLayout::restore_for_documents(layout.snapshot(), &paths);
        assert_eq!(bindings, vec![Some(first), Some(second)]);
        assert_eq!(restored.selected(root), Some(second));
    }

    #[test]
    fn snapshot_preserves_an_intentionally_empty_focused_group() {
        let token = TabToken(1);
        let root = GroupId(1);
        let empty = GroupId(2);
        let snapshot = Snapshot {
            version: 1,
            root: Node::Split {
                id: SplitId(1),
                axis: Axis::Horizontal,
                sizes: None,
                first: Box::new(Node::Group(Group {
                    id: root,
                    tabs: vec![token],
                    selected: Some(token),
                    mode: PaneMode::Source,
                })),
                second: Box::new(Node::Group(Group {
                    id: empty,
                    tabs: Vec::new(),
                    selected: None,
                    mode: PaneMode::Preview,
                })),
            },
            paths: BTreeMap::from([(token, path("same"))]),
            focused: empty,
        };
        let (restored, bindings) = EditorLayout::restore_for_documents(snapshot, &[path("same")]);
        assert_eq!(bindings, vec![Some(token)]);
        assert_eq!(restored.focused(), empty);
        assert!(restored.group_tokens(empty).is_empty());
    }

    #[test]
    fn failed_duplicate_document_load_does_not_leave_a_ghost_tab() {
        let mut layout = EditorLayout::empty();
        let group = layout.focused();
        let first = layout.new_token(path("same"));
        let second = layout.new_token(path("same"));
        layout.add_tab(group, first);
        layout.add_tab(group, second);
        let (restored, bindings) =
            EditorLayout::restore_for_documents(layout.snapshot(), &[path("same")]);
        assert_eq!(bindings, vec![Some(first)]);
        assert_eq!(restored.group_tokens(group), vec![first]);
        assert_eq!(restored.selected(group), Some(first));
        assert_eq!(restored.path(second), None);
    }

    #[test]
    fn legacy_migration_preserves_active_path_and_mode() {
        let paths = vec![path("one"), path("two")];
        let (layout, tokens, active) =
            EditorLayout::migrate_legacy(&paths, Some(path("two").as_path()), PaneMode::Preview);
        assert_eq!(active, Some(1));
        assert_eq!(layout.selected(layout.focused()), Some(tokens[1]));
        assert_eq!(layout.group_specs()[0].3, PaneMode::Preview);
    }

    #[test]
    fn explicit_startup_paths_do_not_restore_saved_snapshot() {
        let snapshot = EditorLayout::empty().snapshot();
        assert!(EditorLayout::snapshot_for_startup(Some(snapshot.clone()), true).is_none());
        assert!(EditorLayout::snapshot_for_startup(Some(snapshot), false).is_some());
    }

    #[test]
    fn close_right_targets_follow_visual_order_after_reorder() {
        let mut layout = EditorLayout::empty();
        let group = layout.focused();
        let first = layout.new_token(path("first"));
        let second = layout.new_token(path("second"));
        let third = layout.new_token(path("third"));
        layout.add_tab(group, first);
        layout.add_tab(group, second);
        layout.add_tab(group, third);
        assert!(layout.move_tab(first, group, 2));
        assert_eq!(layout.group_tokens(group), vec![second, third, first]);
        assert_eq!(layout.tokens_to_right(second), vec![third, first]);
    }

    #[test]
    fn split_sizes_and_duplicate_tokens_are_normalized() {
        let mut layout = EditorLayout::empty();
        let root = layout.focused();
        let first = layout.new_token(path("first"));
        let second = layout.new_token(path("second"));
        layout.add_tab(root, first);
        layout.add_tab(root, second);
        layout.set_mode(root, PaneMode::Preview);
        let right = layout.split(root, second, Axis::Horizontal).unwrap();
        assert!(layout
            .group_specs()
            .into_iter()
            .find(|(id, _, _, _)| *id == right)
            .is_some_and(|(_, _, _, mode)| mode == PaneMode::Preview));
        let split = match layout.root() {
            Node::Split { id, .. } => *id,
            Node::Group(_) => unreachable!(),
        };
        assert!(layout.set_split_sizes(split, [400.0, 600.0]));
        assert!(!layout.set_split_sizes(split, [400.0, 600.0]));
        let snapshot = layout.snapshot();
        let restored =
            EditorLayout::restore(snapshot, &BTreeSet::from([path("first"), path("second")]));
        let restored_size = match restored.root() {
            Node::Split { sizes, .. } => *sizes,
            Node::Group(_) => unreachable!(),
        };
        assert_eq!(restored_size, Some([400.0, 600.0]));
        assert_eq!(restored.groups_in_order(), vec![root, right]);
        assert_eq!(restored.group_tokens(root), vec![first]);
        assert_eq!(restored.group_tokens(right), vec![second]);

        let token = TabToken(42);
        let snapshot = Snapshot {
            version: 1,
            root: Node::Split {
                id: SplitId(u64::MAX),
                axis: Axis::Vertical,
                sizes: Some([f32::NAN, 400.0]),
                first: Box::new(Node::Group(Group {
                    id: GroupId(u64::MAX),
                    tabs: vec![token],
                    selected: Some(token),
                    mode: PaneMode::Source,
                })),
                second: Box::new(Node::Group(Group {
                    id: GroupId(u64::MAX),
                    tabs: vec![token],
                    selected: Some(token),
                    mode: PaneMode::Preview,
                })),
            },
            paths: BTreeMap::from([(token, path("first"))]),
            focused: GroupId(u64::MAX),
        };
        let mut restored = EditorLayout::restore(snapshot, &BTreeSet::from([path("first")]));
        let groups = restored.groups_in_order();
        assert_eq!(groups, vec![GroupId(u64::MAX), GroupId(0)]);
        assert_eq!(restored.group_tokens(groups[0]), vec![token]);
        assert!(restored.group_tokens(groups[1]).is_empty());
        assert_eq!(restored.selected(groups[1]), None);
        let size = match restored.root() {
            Node::Split { sizes, .. } => *sizes,
            Node::Group(_) => unreachable!(),
        };
        assert_eq!(size, None);
        assert!(restored.split(groups[0], token, Axis::Horizontal).is_some());
    }
}
