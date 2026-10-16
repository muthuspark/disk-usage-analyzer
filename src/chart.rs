use crate::model::Node;
use gpui::{Bounds, PathBuilder, Pixels, Point, Window, point, px, rgb};
use std::f32::consts::TAU;

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

impl Segment {
    pub fn paint(self, window: &mut Window) {
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
        if let Ok(path) = builder.build() {
            window.paint_path(path, rgb(self.color));
        }
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
        rectangles(node, bounds, vec![], 0, &mut segments);
    } else {
        let center = bounds.center();
        let radius = f32::from(bounds.size.width.min(bounds.size.height)) / 2. - 5.;
        rings(node, center, radius, 0., TAU, 0, vec![], 0, &mut segments);
    }
    segments
}

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

const COLORS: [u32; 8] = [
    0xe01b24, 0x4676d5, 0x9141ac, 0x26a269, 0x2b98af, 0xc53b76, 0x6860bb, 0x3584b5,
];

fn shade(color: u32, level: usize) -> u32 {
    let factor = 1. - level as f32 * 0.065;
    let c = |shift: u32| (((color >> shift) & 255u32) as f32 * factor) as u32;
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
    for (i, child) in node.children.iter().enumerate() {
        let sweep = (end - start) * child.bytes as f32 / node.bytes as f32;
        if sweep < 0.001 {
            continue;
        }
        let mut child_path = path.clone();
        child_path.push(i);
        let gap = 0.0015_f32.min(sweep * 0.15);
        let base = if depth == 0 {
            COLORS[i % COLORS.len()]
        } else if path.first() == Some(&0) && depth == 1 {
            [0xe01b24, 0x27b46e, 0x2b98af, 0x7653b5][i % 4]
        } else if path.first() == Some(&1) && depth == 1 {
            [0x4676d5, 0x7653b5, 0x9141ac, 0xc53b76][i % 4]
        } else {
            color
        };
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
    segments: &mut Vec<Segment>,
) {
    if node.bytes == 0 || depth > 3 {
        return;
    }
    let horizontal = bounds.size.width > bounds.size.height;
    let mut offset = 0.;
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
        segments.push(Segment {
            color: shade(COLORS[child_path[0] % COLORS.len()], depth),
            path: child_path.clone(),
            shape: Shape::Rect(b),
        });
        if b.size.width > px(16.) && b.size.height > px(16.) {
            rectangles(child, b, child_path, depth + 1, segments);
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
}
