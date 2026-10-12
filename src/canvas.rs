use serde_json::{Map, Number, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const MAX_SAFE_WORLD: i64 = 1_000_000_000;
const MIN_NODE_SIZE: i64 = 40;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Geometry {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

impl Geometry {
    pub(crate) fn right(self) -> Option<i64> {
        self.x.checked_add(self.width)
    }

    pub(crate) fn bottom(self) -> Option<i64> {
        self.y.checked_add(self.height)
    }

    fn is_safe(self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= MAX_SAFE_WORLD
            && self.height <= MAX_SAFE_WORLD
            && self.x.unsigned_abs() <= MAX_SAFE_WORLD as u64
            && self.y.unsigned_abs() <= MAX_SAFE_WORLD as u64
            && self
                .right()
                .is_some_and(|right| right.unsigned_abs() <= (MAX_SAFE_WORLD * 2) as u64)
            && self
                .bottom()
                .is_some_and(|bottom| bottom.unsigned_abs() <= (MAX_SAFE_WORLD * 2) as u64)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CanvasFragment {
    pub nodes: Vec<Value>,
    pub edges: Vec<Value>,
}

#[derive(Clone)]
pub(crate) struct CanvasDocument {
    original: Arc<str>,
    value: Value,
    changed: bool,
}

impl CanvasDocument {
    pub(crate) fn parse(raw: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(raw).map_err(|error| error.to_string())?;
        let object = value
            .as_object()
            .ok_or_else(|| "Canvas root must be an object".to_string())?;
        for field in ["nodes", "edges"] {
            if object.get(field).is_some_and(|value| !value.is_array()) {
                return Err(format!("Canvas {field} must be an array"));
            }
        }
        let nodes = object
            .get("nodes")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let edges = object
            .get("edges")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut ids = HashSet::new();
        for node in nodes {
            let object = node
                .as_object()
                .ok_or_else(|| "Canvas node must be an object".to_string())?;
            let id = required_string(object, "id", "node")?;
            if !ids.insert(id.to_string()) {
                return Err(format!("Duplicate Canvas id: {id}"));
            }
            required_string(object, "type", "node")?;
            for name in ["x", "y", "width", "height"] {
                let number = object
                    .get(name)
                    .and_then(Value::as_number)
                    .ok_or_else(|| format!("Canvas node {id} is missing numeric {name}"))?;
                if !is_integer_number(number) {
                    return Err(format!("Canvas node {id} has non-integer {name}"));
                }
            }
            if object["width"].as_i64().is_some_and(|width| width <= 0)
                || object["height"].as_i64().is_some_and(|height| height <= 0)
            {
                return Err(format!("Canvas node {id} has non-positive size"));
            }
        }
        for edge in edges {
            let object = edge
                .as_object()
                .ok_or_else(|| "Canvas edge must be an object".to_string())?;
            let id = required_string(object, "id", "edge")?;
            if !ids.insert(id.to_string()) {
                return Err(format!("Duplicate Canvas id: {id}"));
            }
            let from = required_string(object, "fromNode", "edge")?;
            let to = required_string(object, "toNode", "edge")?;
            if !nodes.iter().any(|node| node["id"] == from)
                || !nodes.iter().any(|node| node["id"] == to)
            {
                return Err(format!("Canvas edge {id} refers to a missing node"));
            }
        }
        Ok(Self {
            original: Arc::from(raw),
            value,
            changed: false,
        })
    }

    pub(crate) fn serialize(&self) -> Result<String, String> {
        if !self.changed {
            return Ok(self.original.to_string());
        }
        serde_json::to_string(&self.value).map_err(|error| error.to_string())
    }

    pub(crate) fn nodes(&self) -> &[Value] {
        self.value
            .get("nodes")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(crate) fn edges(&self) -> &[Value] {
        self.value
            .get("edges")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(crate) fn node(&self, id: &str) -> Option<&Value> {
        self.nodes()
            .iter()
            .find(|node| node["id"].as_str() == Some(id))
    }

    pub(crate) fn edge(&self, id: &str) -> Option<&Value> {
        self.edges()
            .iter()
            .find(|edge| edge["id"].as_str() == Some(id))
    }

    pub(crate) fn node_geometry(&self, node: &Value) -> Option<Geometry> {
        let object = node.as_object()?;
        let geometry = Geometry {
            x: object.get("x")?.as_i64()?,
            y: object.get("y")?.as_i64()?,
            width: object.get("width")?.as_i64()?,
            height: object.get("height")?.as_i64()?,
        };
        geometry.is_safe().then_some(geometry)
    }

    pub(crate) fn geometry_is_unsupported(&self, node: &Value) -> bool {
        node.as_object()
            .and_then(|object| {
                Some((
                    object.get("x")?.as_i64()?,
                    object.get("y")?.as_i64()?,
                    object.get("width")?.as_i64()?,
                    object.get("height")?.as_i64()?,
                ))
            })
            .is_none_or(|(x, y, width, height)| {
                !Geometry {
                    x,
                    y,
                    width,
                    height,
                }
                .is_safe()
            })
    }

    pub(crate) fn set_node_field(
        &mut self,
        id: &str,
        field: &str,
        value: Value,
    ) -> Result<bool, String> {
        if field == "color" {
            if let Some(color) = value.as_str() {
                if !color.is_empty()
                    && !matches!(color, "1" | "2" | "3" | "4" | "5" | "6")
                    && !valid_custom_color(color)
                {
                    return Err("Canvas color must be a palette value or hexadecimal color".into());
                }
            }
        }
        let node = self
            .node_mut(id)
            .ok_or_else(|| "Canvas node no longer exists".to_string())?;
        let object = node
            .as_object_mut()
            .ok_or_else(|| "Canvas node is malformed".to_string())?;
        if object.get(field) == Some(&value) {
            return Ok(false);
        }
        object.insert(field.to_string(), value);
        self.changed = true;
        Ok(true)
    }

    pub(crate) fn set_node_geometry(
        &mut self,
        id: &str,
        geometry: Geometry,
    ) -> Result<bool, String> {
        if !geometry.is_safe() {
            return Err("Canvas geometry is outside the supported range".into());
        }
        let node = self
            .node_mut(id)
            .ok_or_else(|| "Canvas node no longer exists".to_string())?;
        let object = node
            .as_object_mut()
            .ok_or_else(|| "Canvas node is malformed".to_string())?;
        let next = [
            ("x", geometry.x),
            ("y", geometry.y),
            ("width", geometry.width),
            ("height", geometry.height),
        ];
        let changed = next
            .iter()
            .any(|(name, value)| object.get(*name).and_then(Value::as_i64) != Some(*value));
        if changed {
            for (name, value) in next {
                object.insert(name.to_string(), Value::Number(Number::from(value)));
            }
            self.changed = true;
        }
        Ok(changed)
    }

    pub(crate) fn add_node(&mut self, mut node: Value) -> Result<String, String> {
        let object = node
            .as_object_mut()
            .ok_or_else(|| "Canvas node must be an object".to_string())?;
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.unique_id());
        if self.has_id(&id) {
            return Err("Canvas node id already exists".into());
        }
        object.insert("id".into(), Value::String(id.clone()));
        validate_new_node(&node)?;
        self.array_mut("nodes").push(node);
        self.changed = true;
        Ok(id)
    }

    pub(crate) fn add_edge(&mut self, mut edge: Value) -> Result<String, String> {
        let object = edge
            .as_object_mut()
            .ok_or_else(|| "Canvas edge must be an object".to_string())?;
        let from = object
            .get("fromNode")
            .and_then(Value::as_str)
            .ok_or_else(|| "Canvas edge is missing fromNode".to_string())?;
        let to = object
            .get("toNode")
            .and_then(Value::as_str)
            .ok_or_else(|| "Canvas edge is missing toNode".to_string())?;
        if self.node(from).is_none() || self.node(to).is_none() {
            return Err("Canvas edges must connect existing nodes".into());
        }
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| self.unique_id());
        if self.has_id(&id) {
            return Err("Canvas edge id already exists".into());
        }
        object.insert("id".into(), Value::String(id.clone()));
        self.array_mut("edges").push(edge);
        self.changed = true;
        Ok(id)
    }

    pub(crate) fn add_text(&mut self, x: i64, y: i64, text: &str) -> Result<String, String> {
        self.add_node(node_value(
            "text",
            Geometry {
                x,
                y,
                width: 280,
                height: 180,
            },
            [("text", Value::String(text.into()))],
        ))
    }

    pub(crate) fn add_file(&mut self, x: i64, y: i64, path: &str) -> Result<String, String> {
        self.add_node(node_value(
            "file",
            Geometry {
                x,
                y,
                width: 280,
                height: 180,
            },
            [("file", Value::String(path.into()))],
        ))
    }

    pub(crate) fn add_link(&mut self, x: i64, y: i64, url: &str) -> Result<String, String> {
        if !valid_external_url(url) {
            return Err("Canvas links must use an http or https URL".into());
        }
        self.add_node(node_value(
            "link",
            Geometry {
                x,
                y,
                width: 280,
                height: 180,
            },
            [("url", Value::String(url.into()))],
        ))
    }

    pub(crate) fn add_group(
        &mut self,
        selected: &[String],
        label: &str,
        margin: i64,
    ) -> Result<String, String> {
        let margin = margin.max(0);
        let members = self.included_node_ids(selected);
        let bounds = members
            .iter()
            .filter_map(|id| self.node(id).and_then(|node| self.node_geometry(node)))
            .fold(None, |bounds: Option<(i64, i64, i64, i64)>, rect| {
                let right = rect.right()?;
                let bottom = rect.bottom()?;
                Some(match bounds {
                    Some((left, top, right0, bottom0)) => (
                        left.min(rect.x),
                        top.min(rect.y),
                        right0.max(right),
                        bottom0.max(bottom),
                    ),
                    None => (rect.x, rect.y, right, bottom),
                })
            })
            .ok_or_else(|| "Select at least one supported Canvas card".to_string())?;
        let (x, y, right, bottom) = bounds;
        let x = x
            .checked_sub(margin)
            .ok_or_else(|| "Canvas group is out of range".to_string())?;
        let y = y
            .checked_sub(margin)
            .ok_or_else(|| "Canvas group is out of range".to_string())?;
        let width = right
            .checked_sub(x)
            .and_then(|value| value.checked_add(margin))
            .ok_or_else(|| "Canvas group is out of range".to_string())?;
        let height = bottom
            .checked_sub(y)
            .and_then(|value| value.checked_add(margin))
            .ok_or_else(|| "Canvas group is out of range".to_string())?;
        self.add_node(node_value(
            "group",
            Geometry {
                x,
                y,
                width,
                height,
            },
            [("label", Value::String(label.into()))],
        ))
    }

    pub(crate) fn included_node_ids(&self, selected: &[String]) -> Vec<String> {
        let mut included = selected.iter().cloned().collect::<HashSet<_>>();
        loop {
            let groups = self
                .nodes()
                .iter()
                .filter(|node| {
                    node["type"].as_str() == Some("group")
                        && node["id"].as_str().is_some_and(|id| included.contains(id))
                })
                .filter_map(|node| {
                    Some((node["id"].as_str()?.to_string(), self.node_geometry(node)?))
                })
                .collect::<Vec<_>>();
            let before = included.len();
            for (group_id, group) in groups {
                for node in self.nodes() {
                    let Some(id) = node["id"].as_str() else {
                        continue;
                    };
                    if id == group_id {
                        continue;
                    }
                    if self
                        .node_geometry(node)
                        .is_some_and(|geometry| fully_inside(geometry, group))
                    {
                        included.insert(id.to_string());
                    }
                }
            }
            if before == included.len() {
                break;
            }
        }
        self.nodes()
            .iter()
            .filter_map(|node| {
                let id = node["id"].as_str()?;
                included.contains(id).then(|| id.to_string())
            })
            .collect()
    }

    pub(crate) fn move_selection(
        &mut self,
        selected: &[String],
        dx: i64,
        dy: i64,
    ) -> Result<bool, String> {
        let ids = self.included_node_ids(selected);
        let mut updates = Vec::new();
        for id in ids {
            let node = self
                .node(&id)
                .ok_or_else(|| "Canvas node no longer exists".to_string())?;
            let geometry = self
                .node_geometry(node)
                .ok_or_else(|| "This Canvas card has unsupported geometry".to_string())?;
            let geometry = Geometry {
                x: geometry
                    .x
                    .checked_add(dx)
                    .ok_or_else(|| "Canvas move is outside the supported range".to_string())?,
                y: geometry
                    .y
                    .checked_add(dy)
                    .ok_or_else(|| "Canvas move is outside the supported range".to_string())?,
                ..geometry
            };
            if !geometry.is_safe() {
                return Err("Canvas move is outside the supported range".into());
            }
            updates.push((id, geometry));
        }
        let mut changed = false;
        for (id, geometry) in updates {
            changed |= self.set_node_geometry(&id, geometry)?;
        }
        Ok(changed)
    }

    pub(crate) fn resize_node(
        &mut self,
        id: &str,
        width: i64,
        height: i64,
    ) -> Result<bool, String> {
        let mut geometry = self
            .node(id)
            .and_then(|node| self.node_geometry(node))
            .ok_or_else(|| "This Canvas card has unsupported geometry".to_string())?;
        geometry.width = width.max(MIN_NODE_SIZE);
        geometry.height = height.max(MIN_NODE_SIZE);
        self.set_node_geometry(id, geometry)
    }

    pub(crate) fn remove_selection(&mut self, selected: &[String]) -> bool {
        let selected = selected.iter().cloned().collect::<HashSet<_>>();
        let old_nodes = self.nodes().len();
        let removed = self
            .nodes()
            .iter()
            .filter_map(|node| {
                let id = node["id"].as_str()?;
                selected.contains(id).then(|| id.to_string())
            })
            .collect::<HashSet<_>>();
        let edges = self
            .edges()
            .iter()
            .filter(|edge| {
                let id = edge["id"].as_str().unwrap_or_default();
                let from = edge["fromNode"].as_str().unwrap_or_default();
                let to = edge["toNode"].as_str().unwrap_or_default();
                !selected.contains(id) && !removed.contains(from) && !removed.contains(to)
            })
            .cloned()
            .collect::<Vec<_>>();
        self.array_mut("nodes")
            .retain(|node| node["id"].as_str().is_some_and(|id| !removed.contains(id)));
        let old_edges = self.edges().len();
        if self.edges() != edges {
            *self.array_mut("edges") = edges;
        }
        let changed = self.nodes().len() != old_nodes || self.edges().len() != old_edges;
        self.changed |= changed;
        changed
    }

    pub(crate) fn copy_selection(&self, selected: &[String]) -> CanvasFragment {
        let included = self
            .included_node_ids(selected)
            .into_iter()
            .collect::<HashSet<_>>();
        let nodes = self
            .nodes()
            .iter()
            .filter(|node| node["id"].as_str().is_some_and(|id| included.contains(id)))
            .cloned()
            .collect::<Vec<_>>();
        let edges = self
            .edges()
            .iter()
            .filter(|edge| {
                let from = edge["fromNode"].as_str().unwrap_or_default();
                let to = edge["toNode"].as_str().unwrap_or_default();
                included.contains(from) && included.contains(to)
            })
            .cloned()
            .collect();
        CanvasFragment { nodes, edges }
    }

    pub(crate) fn duplicate_selection(
        &mut self,
        selected: &[String],
        offset: i64,
    ) -> Result<Vec<String>, String> {
        let fragment = self.copy_selection(selected);
        self.paste_fragment(fragment, offset, offset)
    }

    pub(crate) fn paste_fragment(
        &mut self,
        fragment: CanvasFragment,
        dx: i64,
        dy: i64,
    ) -> Result<Vec<String>, String> {
        let mut id_map = HashMap::new();
        for node in &fragment.nodes {
            let old = node["id"]
                .as_str()
                .ok_or_else(|| "Clipboard Canvas node has no id".to_string())?;
            id_map.insert(old.to_string(), self.unique_id());
        }
        let mut cloned_nodes = Vec::new();
        for mut node in fragment.nodes {
            let object = node
                .as_object_mut()
                .ok_or_else(|| "Clipboard Canvas node is malformed".to_string())?;
            let old = object["id"].as_str().unwrap_or_default().to_string();
            object.insert("id".into(), Value::String(id_map[&old].clone()));
            if let Some(geometry) = self.node_geometry(&node) {
                let moved =
                    Geometry {
                        x: geometry.x.checked_add(dx).ok_or_else(|| {
                            "Canvas paste is outside the supported range".to_string()
                        })?,
                        y: geometry.y.checked_add(dy).ok_or_else(|| {
                            "Canvas paste is outside the supported range".to_string()
                        })?,
                        ..geometry
                    };
                if !moved.is_safe() {
                    return Err("Canvas paste is outside the supported range".into());
                }
                set_geometry_value(&mut node, moved)?;
            }
            cloned_nodes.push(node);
        }
        let mut cloned_edges = Vec::new();
        for mut edge in fragment.edges {
            let object = edge
                .as_object_mut()
                .ok_or_else(|| "Clipboard Canvas edge is malformed".to_string())?;
            let from = object["fromNode"]
                .as_str()
                .ok_or_else(|| "Clipboard Canvas edge has no source".to_string())?;
            let to = object["toNode"]
                .as_str()
                .ok_or_else(|| "Clipboard Canvas edge has no target".to_string())?;
            let (Some(from), Some(to)) = (id_map.get(from), id_map.get(to)) else {
                continue;
            };
            object.insert("id".into(), Value::String(self.unique_id()));
            object.insert("fromNode".into(), Value::String(from.clone()));
            object.insert("toNode".into(), Value::String(to.clone()));
            cloned_edges.push(edge);
        }
        let ids = cloned_nodes
            .iter()
            .filter_map(|node| node["id"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        self.array_mut("nodes").extend(cloned_nodes);
        self.array_mut("edges").extend(cloned_edges);
        if !ids.is_empty() {
            self.changed = true;
        }
        Ok(ids)
    }

    pub(crate) fn convert_text_to_file(&mut self, id: &str, file_path: &str) -> Result<(), String> {
        let path = Path::new(file_path);
        if path.as_os_str().is_empty()
            || path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
        {
            return Err("Canvas file references must be vault-relative".into());
        }
        let node = self
            .node_mut(id)
            .ok_or_else(|| "Canvas node no longer exists".to_string())?;
        let object = node
            .as_object_mut()
            .ok_or_else(|| "Canvas node is malformed".to_string())?;
        if object.get("type").and_then(Value::as_str) != Some("text") {
            return Err("Only text cards can be converted".into());
        }
        object.insert("type".into(), Value::String("file".into()));
        object.insert("file".into(), Value::String(file_path.into()));
        object.remove("text");
        self.changed = true;
        Ok(())
    }

    pub(crate) fn search_projection(&self) -> String {
        let mut fields = Vec::new();
        for node in self.nodes() {
            let Some(object) = node.as_object() else {
                continue;
            };
            for key in ["text", "label", "url", "file"] {
                if let Some(value) = object.get(key).and_then(Value::as_str) {
                    fields.push(value.to_string());
                }
            }
        }
        fields.join("\n")
    }

    pub(crate) fn outgoing_targets(&self) -> Vec<(String, bool)> {
        let mut targets = Vec::new();
        for node in self.nodes() {
            if let Some(path) = node["file"].as_str() {
                targets.push((path.to_string(), true));
            }
            if let Some(background) = node["background"].as_str() {
                targets.push((background.to_string(), true));
            }
            if node["type"].as_str() == Some("text") {
                if let Some(text) = node["text"].as_str() {
                    targets.extend(crate::vault::local_link_targets(text));
                }
            }
        }
        targets
    }

    pub(crate) fn retarget_paths(&mut self, source: &Path, destination: &Path) -> bool {
        let source = source.to_string_lossy().replace('\\', "/");
        let destination = destination.to_string_lossy().replace('\\', "/");
        let mut did_change = false;
        for node in self.array_mut("nodes") {
            let Some(object) = node.as_object_mut() else {
                continue;
            };
            for field in ["file", "background"] {
                let Some(path) = object.get(field).and_then(Value::as_str) else {
                    continue;
                };
                let Some(next) = retarget_reference(path, &source, &destination) else {
                    continue;
                };
                object.insert(field.into(), Value::String(next));
                did_change = true;
            }
        }
        self.changed |= did_change;
        did_change
    }

    pub(crate) fn has_id(&self, id: &str) -> bool {
        self.nodes()
            .iter()
            .chain(self.edges())
            .any(|value| value["id"].as_str() == Some(id))
    }

    fn unique_id(&self) -> String {
        loop {
            let epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let id = format!("{epoch:032x}{counter:016x}");
            if !self.has_id(&id) {
                return id;
            }
        }
    }

    fn node_mut(&mut self, id: &str) -> Option<&mut Value> {
        self.value
            .get_mut("nodes")?
            .as_array_mut()?
            .iter_mut()
            .find(|node| node["id"].as_str() == Some(id))
    }

    fn array_mut(&mut self, name: &str) -> &mut Vec<Value> {
        let object = self.value.as_object_mut().expect("validated root object");
        object
            .entry(name)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("validated Canvas array")
    }
}

pub(crate) fn is_canvas(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("canvas"))
}

pub(crate) fn valid_external_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|parsed| {
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
    })
}

pub(crate) fn valid_custom_color(color: &str) -> bool {
    let color = color.strip_prefix('#').unwrap_or(color);
    matches!(color.len(), 3 | 6 | 8) && color.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn resolve_vault_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err("Canvas paths must stay inside the vault".into());
    }
    let root = root
        .canonicalize()
        .map_err(|_| "The vault path is unavailable".to_string())?;
    let candidate = root.join(relative);
    let resolved = if candidate.exists() {
        candidate
            .canonicalize()
            .map_err(|_| "The Canvas file path is unavailable".to_string())?
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| "The Canvas file path is unavailable".to_string())?
            .canonicalize()
            .map_err(|_| "The Canvas file path is unavailable".to_string())?;
        parent.join(
            candidate
                .file_name()
                .ok_or_else(|| "The Canvas file path is unavailable".to_string())?,
        )
    };
    if !resolved.starts_with(&root) {
        return Err("Canvas paths must stay inside the vault".into());
    }
    Ok(resolved)
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    kind: &str,
) -> Result<&'a str, String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Canvas {kind} is missing {field}"))
}

fn is_integer_number(number: &Number) -> bool {
    let text = number.to_string();
    !text.contains('.') && !text.contains('e') && !text.contains('E')
}

fn validate_new_node(node: &Value) -> Result<(), String> {
    let object = node.as_object().ok_or("Canvas node must be an object")?;
    required_string(object, "type", "node")?;
    for field in ["x", "y", "width", "height"] {
        if object.get(field).and_then(Value::as_i64).is_none() {
            return Err(format!("Canvas node is missing integral {field}"));
        }
    }
    if object["width"].as_i64().unwrap_or_default() <= 0
        || object["height"].as_i64().unwrap_or_default() <= 0
    {
        return Err("Canvas node size must be positive".into());
    }
    Ok(())
}

fn node_value<const N: usize>(kind: &str, geometry: Geometry, fields: [(&str, Value); N]) -> Value {
    let mut object = Map::new();
    object.insert("id".into(), Value::String(String::new()));
    object.insert("type".into(), Value::String(kind.into()));
    for (name, value) in [
        ("x", geometry.x),
        ("y", geometry.y),
        ("width", geometry.width),
        ("height", geometry.height),
    ] {
        object.insert(name.into(), Value::Number(Number::from(value)));
    }
    for (name, value) in fields {
        object.insert(name.into(), value);
    }
    Value::Object(object)
}

fn set_geometry_value(node: &mut Value, geometry: Geometry) -> Result<(), String> {
    let object = node
        .as_object_mut()
        .ok_or_else(|| "Canvas node is malformed".to_string())?;
    for (name, value) in [
        ("x", geometry.x),
        ("y", geometry.y),
        ("width", geometry.width),
        ("height", geometry.height),
    ] {
        object.insert(name.into(), Value::Number(Number::from(value)));
    }
    Ok(())
}

fn fully_inside(inner: Geometry, outer: Geometry) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner
            .right()
            .zip(outer.right())
            .is_some_and(|(a, b)| a <= b)
        && inner
            .bottom()
            .zip(outer.bottom())
            .is_some_and(|(a, b)| a <= b)
}

fn retarget_reference(path: &str, source: &str, destination: &str) -> Option<String> {
    let split = path.find(['#', '^']).unwrap_or(path.len());
    let (file, suffix) = path.split_at(split);
    let normalized = file.replace('\\', "/");
    let next = if normalized == source {
        destination.to_string()
    } else {
        let source = source.trim_end_matches('/');
        normalized
            .strip_prefix(source)
            .filter(|rest| rest.starts_with('/'))
            .map(|rest| format!("{}{}", destination.trim_end_matches('/'), rest))?
    };
    Some(format!("{next}{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_round_trip_preserves_optional_arrays_unknown_keys_and_large_numbers() {
        let source = r#"{
          "custom":{"futureNumber":123456789012345678901234567890123456789},
          "nodes":[
            {"id":"t","type":"text","x":-4,"y":2,"width":420,"height":250,"text":"**hello**","color":"1","future":{"n":99999999999999999999999999999999999999}},
            {"id":"f","type":"file","x":500,"y":2,"width":300,"height":210,"file":"notes/a.md#Heading","subpath":"^block"},
            {"id":"l","type":"link","x":800,"y":2,"width":300,"height":210,"url":"https://example.com","future":true},
            {"id":"g","type":"group","x":0,"y":300,"width":900,"height":600,"label":"Group","background":"assets/bg.png","backgroundStyle":{"fit":"cover","repeat":false}},
            {"id":"future","type":"future-node","x":1,"y":1,"width":100,"height":100,"extra":7}
          ],
          "edges":[{"id":"e","fromNode":"t","toNode":"f","fromSide":"right","toSide":"left","fromEnd":"arrow","toEnd":"none","label":"ref","color":"2","curve":"straight","future":false}]
        }"#;
        let canvas = CanvasDocument::parse(source).unwrap();
        assert_eq!(canvas.serialize().unwrap(), source);
        assert_eq!(canvas.nodes().len(), 5);
        assert_eq!(canvas.edges().len(), 1);
        let value: Value = serde_json::from_str(&canvas.serialize().unwrap()).unwrap();
        assert_eq!(
            value["custom"]["futureNumber"].to_string(),
            "123456789012345678901234567890123456789"
        );
    }

    #[test]
    fn geometry_operations_keep_unknown_fields_and_reject_extreme_geometry() {
        let mut canvas = CanvasDocument::parse(
            r#"{"nodes":[{"id":"a","type":"text","x":1,"y":2,"width":100,"height":80,"text":"a","custom":true},{"id":"b","type":"text","x":500,"y":2,"width":100,"height":80,"text":"b"},{"id":"g","type":"group","x":0,"y":0,"width":700,"height":200,"label":"g"}],"edges":[]}"#,
        )
        .unwrap();
        let ids = vec!["g".to_string()];
        assert!(canvas.move_selection(&ids, 10, 20).unwrap());
        assert_eq!(canvas.node("a").unwrap()["x"], 11);
        assert_eq!(canvas.node("a").unwrap()["custom"], true);
        canvas.resize_node("a", 1, 20).unwrap();
        assert_eq!(
            canvas
                .node_geometry(canvas.node("a").unwrap())
                .unwrap()
                .width,
            MIN_NODE_SIZE
        );
        let extreme = CanvasDocument::parse(
            r#"{"nodes":[{"id":"x","type":"future","x":999999999999999999999,"y":0,"width":100,"height":100}],"edges":[]}"#,
        )
        .unwrap();
        assert!(extreme.geometry_is_unsupported(extreme.node("x").unwrap()));
        assert!(extreme
            .serialize()
            .unwrap()
            .contains("999999999999999999999"));
        assert!(canvas.move_selection(&ids, MAX_SAFE_WORLD, 0).is_err());
    }

    #[test]
    fn duplicate_and_copy_remap_only_internal_edges_and_preserve_future_data() {
        let mut canvas = CanvasDocument::parse(
            r#"{"nodes":[{"id":"a","type":"text","x":0,"y":0,"width":100,"height":100,"text":"A","future":{"x":9}},{"id":"b","type":"link","x":200,"y":0,"width":100,"height":100,"url":"https://example.com"},{"id":"c","type":"text","x":400,"y":0,"width":100,"height":100,"text":"C"}],"edges":[{"id":"inside","fromNode":"a","toNode":"b","futureEdge":true},{"id":"outside","fromNode":"b","toNode":"c"}]}"#,
        )
        .unwrap();
        let fragment = canvas.copy_selection(&["a".into(), "b".into()]);
        assert_eq!(fragment.nodes.len(), 2);
        assert_eq!(fragment.edges.len(), 1);
        let pasted = canvas.paste_fragment(fragment, 20, 30).unwrap();
        assert_eq!(pasted.len(), 2);
        let edge = canvas.edges().last().unwrap();
        assert_ne!(edge["id"], "inside");
        assert_ne!(edge["fromNode"], "a");
        assert_eq!(edge["futureEdge"], true);
        assert_eq!(canvas.node(&pasted[0]).unwrap()["future"]["x"], 9);
    }

    #[test]
    fn text_to_file_conversion_keeps_unknown_card_data_and_rejects_escaping_paths() {
        let mut canvas = CanvasDocument::parse(
            r##"{"nodes":[{"id":"text","type":"text","x":1,"y":2,"width":300,"height":200,"text":"[linked](other.md)","color":"2","future":{"value":123}}],"edges":[],"custom":true}"##,
        )
        .unwrap();
        assert!(canvas
            .convert_text_to_file("text", "../outside.md")
            .is_err());
        assert_eq!(canvas.node("text").unwrap()["type"].as_str(), Some("text"));
        canvas
            .convert_text_to_file("text", "notes/Untitled.md")
            .unwrap();
        let node = canvas.node("text").unwrap();
        assert_eq!(node["type"], "file");
        assert_eq!(node["file"], "notes/Untitled.md");
        assert_eq!(node["color"], "2");
        assert_eq!(node["future"]["value"], 123);
        let value: Value = serde_json::from_str(&canvas.serialize().unwrap()).unwrap();
        assert_eq!(value["custom"], true);
        assert!(node.get("text").is_none());
    }

    #[test]
    fn malformed_canvas_urls_and_paths_fail_without_replacing_source() {
        let malformed =
            r#"{"nodes":[{"id":"a","type":"text","x":0,"y":0,"width":-1,"height":10}],"edges":[]}"#;
        assert!(CanvasDocument::parse(malformed).is_err());
        assert!(!valid_external_url("javascript:alert(1)"));
        assert!(!valid_external_url("file:///etc/passwd"));
        assert!(valid_external_url("https://example.com"));
        assert!(valid_custom_color("#f0a"));
        assert!(!valid_custom_color("#ggg"));
        let root = std::env::temp_dir().join(format!("rista-canvas-path-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("note.md"), "note").unwrap();
        assert!(resolve_vault_path(&root, "../outside").is_err());
        assert_eq!(
            resolve_vault_path(&root, "note.md").unwrap(),
            root.canonicalize().unwrap().join("note.md")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn structured_retarget_keeps_canvas_subpaths_and_projects_markdown_links() {
        let mut canvas = CanvasDocument::parse(
            r##"{"nodes":[{"id":"f","type":"file","x":0,"y":0,"width":100,"height":100,"file":"folder/note.md#Heading"},{"id":"t","type":"text","x":120,"y":0,"width":100,"height":100,"text":"[Local](folder/note.md)"}],"edges":[]}"##,
        )
        .unwrap();
        assert!(canvas.retarget_paths(Path::new("folder/note.md"), Path::new("new/note.md")));
        assert_eq!(canvas.node("f").unwrap()["file"], "new/note.md#Heading");
        assert!(canvas
            .search_projection()
            .contains("[Local](folder/note.md)"));
        assert_eq!(canvas.outgoing_targets()[0].0, "new/note.md#Heading");
    }
}
