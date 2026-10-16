use crate::model::Node;
use gpui::{Bounds, Path, PathBuilder, Pixels, Point, Window, point, px, rgb};
use std::f32::consts::TAU;
use std::sync::Arc;

pub struct Segment {
    path: Vec<usize>,
    color: u32,
    shape: Shape,
}

enum Shape {
    Ring {
        center: Point<Pixels>,
        inner: f32,
        outer: f32,
        start: f32,
        end: f32,
    },
    Rect(Bounds<Pixels>),
}

/// Drawing state for the current view only. New scans always provide a new tree.
#[derive(Default)]
pub struct Drawing {
    node: Option<Node>,
    bounds: Bounds<Pixels>,
    treemap: bool,
    segments: Vec<Segment>,
    paths: Vec<(Path<Pixels>, u32)>,
}

impl Drawing {
    pub fn update(&mut self, node: &Node, bounds: Bounds<Pixels>, treemap: bool) -> bool {
        if self.node.as_ref().is_some_and(|previous| {
            Arc::ptr_eq(&previous.children, &node.children) && previous.bytes == node.bytes
        }) && self.bounds == bounds
            && self.treemap == treemap
        {
            return false;
        }
        self.segments = layout(node, bounds, treemap);
        self.paths = self
            .segments
            .iter()
            .filter_map(|segment| segment.build_path().map(|path| (path, segment.color)))
            .collect();
        self.node = Some(node.clone());
        self.bounds = bounds;
        self.treemap = treemap;
        true
    }

    pub fn paint(&self, window: &mut Window) {
        for (path, color) in &self.paths {
            window.paint_path(path.clone(), rgb(*color));
        }
    }

    pub fn hit(&self, position: Point<Pixels>) -> Option<&[usize]> {
        self.segments
            .iter()
            .rev()
            .find(|s| s.contains(position))
            .map(|s| s.path.as_slice())
    }
}

impl Segment {
    fn build_path(&self) -> Option<Path<Pixels>> {
        let mut builder = PathBuilder::fill();
        match self.shape {
            Shape::Ring {
                center,
                inner,
                outer,
                start,
                end,
            } => {
                let steps = ((end - start) * outer / 3.).ceil().max(2.) as usize;
                for i in 0..=steps {
                    let angle = start + (end - start) * i as f32 / steps as f32;
                    let p = center + point(px(angle.cos() * outer), px(angle.sin() * outer));
                    if i == 0 {
                        builder.move_to(p);
                    } else {
                        builder.line_to(p);
                    }
                }
                for i in (0..=steps).rev() {
                    let angle = start + (end - start) * i as f32 / steps as f32;
                    builder
                        .line_to(center + point(px(angle.cos() * inner), px(angle.sin() * inner)));
                }
            }
            Shape::Rect(b) => {
                builder.move_to(b.origin);
                builder.line_to(b.top_right());
                builder.line_to(b.bottom_right());
                builder.line_to(b.bottom_left());
            }
        }
        builder.close();
        builder.build().ok()
    }

    fn contains(&self, p: Point<Pixels>) -> bool {
        match self.shape {
            Shape::Rect(b) => b.contains(&p),
            Shape::Ring {
                center,
                inner,
                outer,
                start,
                end,
            } => {
                let x = f32::from(p.x - center.x);
                let y = f32::from(p.y - center.y);
                let r = x.hypot(y);
                let angle = y.atan2(x).rem_euclid(TAU);
                r >= inner && r <= outer && angle >= start && angle <= end
            }
        }
    }
}

pub fn layout(node: &Node, bounds: Bounds<Pixels>, treemap: bool) -> Vec<Segment> {
    let mut segments = vec![];
    if treemap {
        rectangles(node, bounds, vec![], 0, 0, &mut segments);
    } else {
        let center = bounds.center();
        let radius = f32::from(bounds.size.width.min(bounds.size.height)) / 2. - 5.;
        rings(node, center, radius, 0., TAU, 0, vec![], 0, &mut segments);
    }
    segments
}

#[cfg(test)]
pub fn hit(
    node: &Node,
    bounds: Bounds<Pixels>,
    treemap: bool,
    p: Point<Pixels>,
) -> Option<Vec<usize>> {
    layout(node, bounds, treemap)
        .into_iter()
        .rev()
        .find(|s| s.contains(p))
        .map(|s| s.path)
}

// Saturated branch colors, with darker descendants rather than pale tints.
const COLORS: [u32; 8] = [
    0xe01b24, 0xe09508, 0x59a846, 0x249b83, 0x2875b5, 0x6651aa, 0xa33291, 0xd45a17,
];

fn splits_colors(node: &Node, depth: usize) -> bool {
    depth == 0
        || (depth <= 2
            && node
                .children
                .iter()
                .filter(|child| child.bytes > node.bytes / 100)
                .take(2)
                .count()
                == 2)
}

fn branch_color(parent: u32, index: usize, split: bool) -> u32 {
    if split {
        COLORS[index % COLORS.len()]
    } else {
        parent
    }
}

fn shade(color: u32, level: usize) -> u32 {
    let brightness = 1. - level as f32 * 0.05;
    let c = |shift: u32| (((color >> shift) & 255u32) as f32 * brightness) as u32;
    c(16) << 16 | c(8) << 8 | c(0)
}

#[allow(clippy::too_many_arguments)]
fn rings(
    node: &Node,
    center: Point<Pixels>,
    radius: f32,
    start: f32,
    end: f32,
    depth: usize,
    path: Vec<usize>,
    color: u32,
    segments: &mut Vec<Segment>,
) {
    if depth >= 5 || node.bytes == 0 {
        return;
    }
    let hole = radius / 6.;
    let width = (radius - hole) / 5.;
    let mut angle = start;
    let split_colors = splits_colors(node, depth);
    for (i, child) in node.children.iter().enumerate() {
        let sweep = (end - start) * child.bytes as f32 / node.bytes as f32;
        if sweep < 0.001 {
            continue;
        }
        let mut child_path = path.clone();
        child_path.push(i);
        let gap = 0.0015_f32.min(sweep * 0.15);
        let base = branch_color(color, i, split_colors);
        segments.push(Segment {
            path: child_path.clone(),
            color: shade(base, depth),
            shape: Shape::Ring {
                center,
                inner: hole + depth as f32 * width + 0.6,
                outer: hole + (depth + 1) as f32 * width - 0.5,
                start: angle + gap,
                end: angle + sweep - gap,
            },
        });
        rings(
            child,
            center,
            radius,
            angle,
            angle + sweep,
            depth + 1,
            child_path,
            base,
            segments,
        );
        angle += sweep;
    }
}

fn rectangles(
    node: &Node,
    bounds: Bounds<Pixels>,
    path: Vec<usize>,
    depth: usize,
    color: u32,
    segments: &mut Vec<Segment>,
) {
    if node.bytes == 0 || depth > 3 {
        return;
    }
    let horizontal = bounds.size.width > bounds.size.height;
    let mut offset = 0.;
    let split_colors = splits_colors(node, depth);
    for (i, child) in node.children.iter().enumerate() {
        let fraction = child.bytes as f32 / node.bytes as f32;
        if fraction < 0.001 {
            continue;
        }
        let mut b = bounds;
        if horizontal {
            b.origin.x += bounds.size.width * offset;
            b.size.width *= fraction;
        } else {
            b.origin.y += bounds.size.height * offset;
            b.size.height *= fraction;
        }
        offset += fraction;
        let mut child_path = path.clone();
        child_path.push(i);
        b.origin += point(px(1.), px(1.));
        b.size.width -= px(2.);
        b.size.height -= px(2.);
        let base = branch_color(color, i, split_colors);
        segments.push(Segment {
            color: shade(base, depth),
            path: child_path.clone(),
            shape: Shape::Rect(b),
        });
        if b.size.width > px(16.) && b.size.height > px(16.) {
            rectangles(child, b, child_path, depth + 1, base, segments);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model;
    use gpui::size;

    fn bounds() -> Bounds<Pixels> {
        Bounds::new(point(px(450.), px(46.)), size(px(450.), px(600.)))
    }

    #[test]
    fn rings_hit_the_correct_directory_and_leave_the_center_empty() {
        let root = model::demo();
        let bounds = bounds();
        assert_eq!(hit(&root, bounds, false, bounds.center()), None);
        for segment in layout(&root, bounds, false) {
            if let Shape::Ring {
                center,
                inner,
                outer,
                start,
                end,
            } = segment.shape
            {
                let angle = (start + end) / 2.;
                let radius = (inner + outer) / 2.;
                let p = center + point(px(angle.cos() * radius), px(angle.sin() * radius));
                assert_eq!(hit(&root, bounds, false, p), Some(segment.path));
            }
        }
    }

    #[test]
    fn treemap_hits_valid_nodes_after_drilling_down() {
        let root = model::demo();
        let node = root.at(&[0]);
        let segments = layout(node, bounds(), true);
        assert!(!segments.is_empty());
        for segment in &segments {
            if let Shape::Rect(b) = segment.shape {
                let path =
                    hit(node, bounds(), true, b.center()).expect("rectangle must be hittable");
                assert!(node.at(&path).bytes > 0);
            }
        }
        assert_eq!(hit(node, bounds(), true, point(px(0.), px(0.))), None);
    }

    #[test]
    fn empty_directories_produce_no_geometry() {
        let root = model::demo();
        let node = root.at(&[4]);
        assert!(layout(node, bounds(), false).is_empty());
        assert!(layout(node, bounds(), true).is_empty());
    }

    #[test]
    fn drawing_reuses_geometry_until_the_view_or_scan_changes() {
        let root = model::demo();
        let mut drawing = Drawing::default();
        assert!(drawing.update(&root, bounds(), false));
        let paths = drawing.paths.as_ptr();
        for _ in 0..100 {
            assert!(!drawing.update(&root.clone(), bounds(), false));
            assert_eq!(drawing.paths.as_ptr(), paths);
        }
        let mut resized = bounds();
        resized.size.width += px(20.);
        assert!(drawing.update(&root, resized, false));
        assert!(drawing.update(&root, resized, true));
        assert!(drawing.update(root.at(&[0]), resized, true));
        assert!(drawing.update(&root, resized, true));
        let mut fresh_scan = root.clone();
        fresh_scan.children = Arc::new(root.children.as_ref().clone());
        assert!(drawing.update(&fresh_scan, resized, true));
    }

    #[test]
    fn drawing_hit_testing_matches_the_rendered_slices() {
        let root = model::demo();
        let mut drawing = Drawing::default();
        for treemap in [false, true] {
            drawing.update(&root, bounds(), treemap);
            for y in (50..650).step_by(17) {
                for x in (450..900).step_by(17) {
                    let p = point(px(x as f32), px(y as f32));
                    assert_eq!(
                        drawing.hit(p).map(<[usize]>::to_vec),
                        hit(&root, bounds(), treemap, p)
                    );
                }
            }
        }
    }
}
