//! Force approximation, viewport fitting and label spacing, independent of rendering.

use std::collections::HashSet;

pub struct LabelGrid {
    size: [f32; 2],
    occupied: HashSet<(i32, i32)>,
}

impl LabelGrid {
    pub fn new(size: [f32; 2]) -> Self {
        Self {
            size,
            occupied: HashSet::new(),
        }
    }

    /// Reserve the cells touched by a label, keeping dense graphs readable.
    pub fn insert(&mut self, origin: [f32; 2]) -> bool {
        let x = (origin[0] / self.size[0]).floor() as i32;
        let y = (origin[1] / self.size[1]).floor() as i32;
        let cells = [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)];
        if cells.iter().any(|cell| self.occupied.contains(cell)) {
            return false;
        }
        self.occupied.extend(cells);
        true
    }
}

const EXACT_LIMIT: usize = 512;
const THETA_SQUARED: f32 = 0.36;

pub fn repulsion(positions: &[[f32; 2]], strength: f32) -> Vec<[f32; 2]> {
    if positions.len() <= EXACT_LIMIT {
        return exact_repulsion(positions, strength);
    }
    let tree = Cell::build(positions, (0..positions.len()).collect(), 0);
    positions
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let mut force = [0., 0.];
            tree.accumulate(i, p, positions, strength, &mut force);
            force
        })
        .collect()
}

fn force(from: [f32; 2], to: [f32; 2], strength: f32, mass: usize) -> [f32; 2] {
    let dx = from[0] - to[0];
    let dy = from[1] - to[1];
    let distance = (dx * dx + dy * dy).sqrt().max(1.);
    let factor = (strength / distance).min(2000.) / distance * mass as f32;
    [dx * factor, dy * factor]
}

fn exact_repulsion(positions: &[[f32; 2]], strength: f32) -> Vec<[f32; 2]> {
    let mut forces = vec![[0., 0.]; positions.len()];
    for (i, &p) in positions.iter().enumerate() {
        let (head, tail) = forces.split_at_mut(i + 1);
        let current = &mut head[i];
        for (&other, target) in positions[i + 1..].iter().zip(tail) {
            let f = force(p, other, strength, 1);
            current[0] += f[0];
            current[1] += f[1];
            target[0] -= f[0];
            target[1] -= f[1];
        }
    }
    forces
}

struct Cell {
    min: [f32; 2],
    max: [f32; 2],
    center: [f32; 2],
    mass: usize,
    bodies: Vec<usize>,
    children: Vec<Cell>,
}

impl Cell {
    fn build(positions: &[[f32; 2]], bodies: Vec<usize>, depth: usize) -> Self {
        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        let mut sum = [0.; 2];
        for &i in &bodies {
            for axis in 0..2 {
                min[axis] = min[axis].min(positions[i][axis]);
                max[axis] = max[axis].max(positions[i][axis]);
                sum[axis] += positions[i][axis];
            }
        }
        let mass = bodies.len();
        let center = [sum[0] / mass as f32, sum[1] / mass as f32];
        let mut cell = Self {
            min,
            max,
            center,
            mass,
            bodies,
            children: Vec::new(),
        };
        if mass <= 8 || depth >= 24 || (max[0] - min[0]).max(max[1] - min[1]) < 0.001 {
            return cell;
        }
        let midpoint = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
        let mut quadrants: [Vec<usize>; 4] = Default::default();
        for i in cell.bodies.drain(..) {
            let quadrant = usize::from(positions[i][0] >= midpoint[0])
                + 2 * usize::from(positions[i][1] >= midpoint[1]);
            quadrants[quadrant].push(i);
        }
        cell.children = quadrants
            .into_iter()
            .filter(|q| !q.is_empty())
            .map(|q| Self::build(positions, q, depth + 1))
            .collect();
        cell
    }

    fn accumulate(
        &self,
        i: usize,
        p: [f32; 2],
        positions: &[[f32; 2]],
        strength: f32,
        out: &mut [f32; 2],
    ) {
        if self.children.is_empty() {
            for &other in &self.bodies {
                if other != i {
                    let f = force(p, positions[other], strength, 1);
                    out[0] += f[0];
                    out[1] += f[1];
                }
            }
            return;
        }
        let dx = p[0] - self.center[0];
        let dy = p[1] - self.center[1];
        let width = (self.max[0] - self.min[0]).max(self.max[1] - self.min[1]);
        let contains = p[0] >= self.min[0]
            && p[0] <= self.max[0]
            && p[1] >= self.min[1]
            && p[1] <= self.max[1];
        // Never approximate a cell containing the target: that adds self-force.
        if !contains && width * width < THETA_SQUARED * (dx * dx + dy * dy) {
            let f = force(p, self.center, strength, self.mass);
            out[0] += f[0];
            out[1] += f[1];
        } else {
            for child in &self.children {
                child.accumulate(i, p, positions, strength, out);
            }
        }
    }
}

pub fn fit(
    positions: impl Iterator<Item = [f32; 2]>,
    viewport: [f32; 2],
) -> Option<(f32, [f32; 2])> {
    if viewport[0] <= 0. || viewport[1] <= 0. {
        return None;
    }
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for p in positions {
        for axis in 0..2 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    if !min[0].is_finite() {
        return None;
    }
    let scale = ((viewport[0] - 64.).max(1.) / (max[0] - min[0]).max(1.))
        .min((viewport[1] - 64.).max(1.) / (max[1] - min[1]).max(1.))
        .min(1.);
    Some((
        scale,
        [
            -(min[0] + max[0]) * 0.5 * scale,
            -(min[1] + max[1]) * 0.5 * scale,
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_keep_first_priority_and_cull_overlaps() {
        let mut labels = LabelGrid::new([120., 20.]);
        assert!(labels.insert([10., 10.]));
        assert!(!labels.insert([11., 11.]));
        assert!(!labels.insert([-10., 10.]));
        assert!(!labels.insert([125., 10.]));
        assert!(labels.insert([250., 10.]));
        assert!(labels.insert([10., 60.]));
    }

    #[test]
    fn dense_labels_are_bounded_by_viewport_area() {
        let mut labels = LabelGrid::new([120., 20.]);
        let count = (0..10000)
            .filter(|i| labels.insert([(i % 100) as f32 * 8., (i / 100) as f32 * 6.]))
            .count();
        assert!((2..100).contains(&count));
    }

    #[test]
    fn exact_small_graph_and_coincident_nodes() {
        assert_eq!(repulsion(&[], 12100.), Vec::<[f32; 2]>::new());
        assert_eq!(
            repulsion(&[[0., 0.], [110., 0.]], 12100.),
            [[-110., 0.], [110., 0.]]
        );
        assert!(repulsion(&vec![[1., 1.]; 600], 12100.)
            .iter()
            .all(|f| *f == [0., 0.]));
    }

    #[test]
    fn approximation_tracks_exact_forces() {
        let positions: Vec<_> = (0..1000)
            .map(|i| {
                let angle = i as f32 * 2.4;
                let radius = 30. * (i as f32 + 1.).sqrt();
                [radius * angle.cos(), radius * angle.sin()]
            })
            .collect();
        let exact = exact_repulsion(&positions, 12100.);
        let approx = repulsion(&positions, 12100.);
        let error: f32 = exact
            .iter()
            .zip(&approx)
            .map(|(a, b)| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2))
            .sum();
        let energy: f32 = exact.iter().map(|f| f[0] * f[0] + f[1] * f[1]).sum();
        assert!((error / energy).sqrt() < 0.05);
        assert!(approx.iter().flatten().all(|v| v.is_finite()));
    }

    #[test]
    fn fit_keeps_large_offset_graph_in_view() {
        let positions = [[-24000., -18000.], [48000., 26000.], [15000., 1000.]];
        let (scale, offset) = fit(positions.into_iter(), [800., 600.]).unwrap();
        assert!(scale < 0.25);
        for p in positions {
            assert!((p[0] * scale + offset[0]).abs() <= 368.1);
            assert!((p[1] * scale + offset[1]).abs() <= 268.1);
        }
        assert!(fit([].into_iter(), [800., 600.]).is_none());
        assert!(fit(positions.into_iter(), [0., 0.]).is_none());
    }
}
