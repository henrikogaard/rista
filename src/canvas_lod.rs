//! Level of detail for zoomed-out Canvas boards: a cached plan of flat
//! rectangles and straight lines, so panning a large board stays cheap.

use crate::canvas::{CanvasDocument, Geometry};
use gpui_kit::{point, Hsla, Point};
use std::rc::Rc;

/// Below this zoom, boards paint as flat shapes instead of card elements.
pub(crate) const LOD_ZOOM: f64 = 0.35;

/// The theme colours a plan bakes in; a change rebuilds the plan.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct LodColors {
    pub(crate) palette: [Hsla; 6],
    pub(crate) border: Hsla,
    pub(crate) group_box: Hsla,
    pub(crate) edge_default: Hsla,
}

pub(crate) struct LodCard {
    pub(crate) id: String,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) fill: Hsla,
    pub(crate) border: Hsla,
    pub(crate) group: bool,
}

pub(crate) struct LodEdge {
    pub(crate) id: String,
    pub(crate) from: Point<f64>,
    pub(crate) to: Point<f64>,
    pub(crate) color: Hsla,
}

/// One primitive per card and per edge, in paint order (groups first).
pub(crate) struct LodPlan {
    pub(crate) cards: Vec<LodCard>,
    pub(crate) edges: Vec<LodEdge>,
}

#[derive(Default)]
pub(crate) struct LodCache {
    plan: Option<(LodColors, Rc<LodPlan>)>,
    builds: usize,
}

impl LodCache {
    /// The cached plan, rebuilt only after `invalidate` or a colour change.
    pub(crate) fn plan(&mut self, model: &CanvasDocument, colors: LodColors) -> Rc<LodPlan> {
        if let Some((cached, plan)) = &self.plan {
            if *cached == colors {
                return plan.clone();
            }
        }
        self.builds += 1;
        let plan = Rc::new(build_plan(model, &colors, |_, geometry| geometry));
        self.plan = Some((colors, plan.clone()));
        plan
    }

    pub(crate) fn invalidate(&mut self) {
        self.plan = None;
    }
}

/// Builds a plan; `stage` lets a drag in progress move cards without
/// touching the cache.
pub(crate) fn build_plan(
    model: &CanvasDocument,
    colors: &LodColors,
    stage: impl Fn(&str, Geometry) -> Geometry,
) -> LodPlan {
    let cards = model
        .nodes_in_paint_order()
        .filter_map(|node| {
            let id = node["id"].as_str()?;
            let geometry = stage(id, model.node_geometry(node)?);
            let card_color = node["color"]
                .as_str()
                .and_then(|value| parse_color(value, &colors.palette));
            Some(LodCard {
                id: id.to_string(),
                x: geometry.x as f64,
                y: geometry.y as f64,
                width: geometry.width as f64,
                height: geometry.height as f64,
                fill: card_color.map_or(colors.group_box, |color| {
                    colors.group_box.blend(color.opacity(0.12))
                }),
                border: card_color.unwrap_or(colors.border),
                group: node["type"].as_str() == Some("group"),
            })
        })
        .collect();
    let edges = model
        .edges()
        .iter()
        .filter_map(|edge| {
            let from_node = model.node(edge["fromNode"].as_str()?)?;
            let to_node = model.node(edge["toNode"].as_str()?)?;
            let from_geometry = stage(from_node["id"].as_str()?, model.node_geometry(from_node)?);
            let to_geometry = stage(to_node["id"].as_str()?, model.node_geometry(to_node)?);
            Some(LodEdge {
                id: edge["id"].as_str()?.to_string(),
                from: edge_anchor(from_geometry, edge["fromSide"].as_str(), to_geometry),
                to: edge_anchor(to_geometry, edge["toSide"].as_str(), from_geometry),
                color: edge["color"]
                    .as_str()
                    .and_then(|value| parse_color(value, &colors.palette))
                    .unwrap_or(colors.edge_default),
            })
        })
        .collect();
    LodPlan { cards, edges }
}

fn parse_color(value: &str, palette: &[Hsla; 6]) -> Option<Hsla> {
    value
        .parse::<usize>()
        .ok()
        .filter(|index| (1..=6).contains(index))
        .map(|index| palette[index - 1])
        .or_else(|| gpui_kit::component::theme::try_parse_color(value).ok())
}

pub(crate) fn edge_anchor(geometry: Geometry, side: Option<&str>, toward: Geometry) -> Point<f64> {
    let center = point(
        geometry.x as f64 + geometry.width as f64 * 0.5,
        geometry.y as f64 + geometry.height as f64 * 0.5,
    );
    let target = point(
        toward.x as f64 + toward.width as f64 * 0.5,
        toward.y as f64 + toward.height as f64 * 0.5,
    );
    let side = side.unwrap_or_else(|| {
        if (target.x - center.x).abs() >= (target.y - center.y).abs() {
            if target.x < center.x {
                "left"
            } else {
                "right"
            }
        } else if target.y < center.y {
            "top"
        } else {
            "bottom"
        }
    });
    match side {
        "left" => point(geometry.x as f64, center.y),
        "right" => point(geometry.x as f64 + geometry.width as f64, center.y),
        "top" => point(center.x, geometry.y as f64),
        "bottom" => point(center.x, geometry.y as f64 + geometry.height as f64),
        _ => center,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::hsla;

    fn colors() -> LodColors {
        LodColors {
            palette: [hsla(0., 1., 0.5, 1.); 6],
            border: hsla(0., 0., 0.3, 1.),
            group_box: hsla(0., 0., 0.1, 1.),
            edge_default: hsla(0., 0., 0.4, 1.),
        }
    }

    fn big_board() -> CanvasDocument {
        let nodes = (0..300)
            .map(|i| {
                serde_json::json!({
                    "id": format!("n{i}"), "type": "text", "text": format!("card {i}"),
                    "x": (i % 20) * 300, "y": (i / 20) * 200, "width": 250, "height": 120,
                    "color": if i % 3 == 0 { "2" } else { "" },
                })
            })
            .collect::<Vec<_>>();
        let edges = (0..299)
            .map(|i| {
                serde_json::json!({
                    "id": format!("e{i}"), "fromNode": format!("n{i}"),
                    "toNode": format!("n{}", i + 1),
                })
            })
            .collect::<Vec<_>>();
        CanvasDocument::parse(&serde_json::json!({"nodes": nodes, "edges": edges}).to_string())
            .expect("board parses")
    }

    #[test]
    fn low_zoom_plan_is_cached_with_one_primitive_per_card_and_edge() {
        let model = big_board();
        let mut cache = LodCache::default();
        let first = cache.plan(&model, colors());
        let second = cache.plan(&model, colors());
        assert_eq!(cache.builds, 1, "second call must not rebuild");
        assert!(Rc::ptr_eq(&first, &second));
        assert_eq!(second.cards.len(), 300);
        assert_eq!(second.edges.len(), 299);
    }

    #[test]
    fn plan_rebuilds_after_invalidate_or_colour_change() {
        let model = big_board();
        let mut cache = LodCache::default();
        cache.plan(&model, colors());
        cache.invalidate();
        cache.plan(&model, colors());
        assert_eq!(cache.builds, 2);
        let mut other = colors();
        other.border = hsla(0.5, 0.5, 0.5, 1.);
        cache.plan(&model, other);
        assert_eq!(cache.builds, 3);
    }

    #[test]
    fn staged_geometry_moves_cards_and_edge_ends_without_the_cache() {
        let model = big_board();
        let plan = build_plan(&model, &colors(), |id, mut geometry| {
            if id == "n0" {
                geometry.x += 1000;
            }
            geometry
        });
        assert_eq!(plan.cards[0].x, 1000.);
        assert_eq!(plan.edges[0].from.x, 1000.);
    }
}
