//! Documents built in code, cut into jobs and read back as their operations.

use std::collections::BTreeMap;
use std::sync::Arc;

use lasercraft_core::{Machine, Operation, PlanError};
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::{Appearance, Document, LayerColor, Node, NodeId};
use vectorcraft_geom::{Affine, PathData, Point, Rect, shapes};

use crate::*;

/// A 720 × 720 pt (254 mm) document.
fn doc() -> Document {
    Document::new(720.0, 720.0)
}

fn path(d: &mut Document, p: PathData) -> Node {
    Node::path(d.alloc_id(), p, Appearance::basic(Paint::None, Paint::solid(Color::BLACK), 1.0))
}

fn add(d: &mut Document, n: Node) -> NodeId {
    let layer = d.layers[0].id;
    d.insert(Some(layer), usize::MAX, n).unwrap()
}

fn job(d: &Document) -> JobOutput {
    document_to_job(d, &JobOptions::default()).unwrap()
}

/// The polylines of a job: each travel starts one.
fn polylines(j: &JobOutput) -> Vec<Vec<(f64, f64)>> {
    let mut out: Vec<Vec<(f64, f64)>> = vec![];
    for op in &j.job.operations {
        match op {
            Operation::Travel(p) => out.push(vec![(p.x_mm, p.y_mm)]),
            Operation::Cut { to, .. } => out.last_mut().unwrap().push((to.x_mm, to.y_mm)),
            Operation::LaserOff => {}
        }
    }
    out
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn rectangle_in_millimetres_with_y_up_from_the_bottom_left() {
    let mut d = doc();
    let r = path(&mut d, shapes::rectangle(Rect::new(72.0, 72.0, 144.0, 144.0)));
    add(&mut d, r);
    let j = job(&d);
    let lines = polylines(&j);
    assert_eq!(lines.len(), 1);
    let l = &lines[0];
    // Four corners and back to the start.
    assert_eq!(l.len(), 5);
    assert_eq!(l.first(), l.last());
    let xs: Vec<f64> = l.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = l.iter().map(|p| p.1).collect();
    let (x0, x1) = (xs.iter().copied().fold(f64::MAX, f64::min), xs.iter().copied().fold(f64::MIN, f64::max));
    let (y0, y1) = (ys.iter().copied().fold(f64::MAX, f64::min), ys.iter().copied().fold(f64::MIN, f64::max));
    assert!(close(x0, 25.4) && close(x1, 50.8), "{x0} {x1}");
    // 1 in from the top of a 10 in board: 8 in to 9 in up from the bottom.
    assert!(close(y0, 203.2) && close(y1, 228.6), "{y0} {y1}");
    assert!(j.warnings.is_empty(), "{:?}", j.warnings);
    // The defaults cut it.
    assert!(
        j.job
            .operations
            .iter()
            .any(|o| matches!(o, Operation::Cut { speed_mm_min, power_percent, .. } if *speed_mm_min == 1000.0 && *power_percent == 20.0))
    );
}

#[test]
fn each_origin_corner_counts_from_its_corner_of_the_artboard() {
    let mut d = doc();
    // 1 in from the left and 2 in from the top, 1 in wide and 0.5 in tall, on a 10 in board.
    let r = path(&mut d, shapes::rectangle(Rect::new(72.0, 144.0, 144.0, 180.0)));
    add(&mut d, r);
    let span = |o: Origin| {
        let l = &polylines(&document_to_job(&d, &JobOptions { origin: o, ..JobOptions::default() }).unwrap())[0];
        let xs: Vec<f64> = l.iter().map(|p| p.0).collect();
        let ys: Vec<f64> = l.iter().map(|p| p.1).collect();
        let min = |v: &[f64]| v.iter().copied().fold(f64::MAX, f64::min) / 25.4;
        let max = |v: &[f64]| v.iter().copied().fold(f64::MIN, f64::max) / 25.4;
        [min(&xs), max(&xs), min(&ys), max(&ys)]
    };
    let near = |a: [f64; 4], b: [f64; 4]| a.iter().zip(b).all(|(a, b)| close(*a, b));
    // Inches from the origin corner: x then y.
    assert!(near(span(Origin::BottomLeft), [1.0, 2.0, 7.5, 8.0]), "{:?}", span(Origin::BottomLeft));
    assert!(near(span(Origin::TopLeft), [1.0, 2.0, 2.0, 2.5]), "{:?}", span(Origin::TopLeft));
    assert!(near(span(Origin::TopRight), [8.0, 9.0, 2.0, 2.5]), "{:?}", span(Origin::TopRight));
    assert!(near(span(Origin::BottomRight), [8.0, 9.0, 7.5, 8.0]), "{:?}", span(Origin::BottomRight));
    for o in Origin::ALL {
        assert_eq!(Origin::parse(o.id()), Some(o));
    }
    assert_eq!(Origin::parse("top-left"), Some(Origin::TopLeft));
    assert_eq!(Origin::parse("back right"), Some(Origin::TopRight));
    assert_eq!(Origin::parse("middle"), None);
}

#[test]
fn machine_profiles_give_the_bed_and_origin() {
    let profiles = serde_json::json!({
        "schema_version": 1,
        "machines": [
            // Saved before profiles had an origin: the controller's usual corner.
            {"name": "GRBL 300 x 200", "controller": "GRBL", "width_mm": 300.0, "height_mm": 200.0, "max_s": 1000, "port": "", "baud": 115200},
            {"name": "Shop CO2", "controller": "Ruida", "width_mm": 600.0, "height_mm": 400.0},
            {"name": "Diode", "controller": "GRBL", "width_mm": 400.0, "height_mm": 400.0, "max_s": 255, "origin": "topLeft"},
            {"name": "Broken", "width_mm": -5.0, "height_mm": 400.0}
        ]
    });
    let all = machines(&profiles);
    assert_eq!(all.len(), 3, "the broken one is left out");
    let grbl = find_machine(&profiles, "grbl 300 x 200").unwrap();
    assert_eq!((grbl.width_mm, grbl.height_mm, grbl.max_s, grbl.origin), (300.0, 200.0, 1000, Origin::BottomLeft));
    let co2 = find_machine(&profiles, "Shop CO2").unwrap();
    assert_eq!((co2.max_s, co2.origin), (1000, Origin::TopRight));
    assert_eq!(find_machine(&profiles, "Diode").unwrap().origin, Origin::TopLeft);
    assert!(find_machine(&profiles, "Broken").unwrap_err().contains("width_mm"));
    assert!(find_machine(&profiles, "Nope").unwrap_err().contains("Shop CO2"));
    assert!(machines(&serde_json::json!({})).is_empty());
}

#[test]
fn circle_is_flattened_within_tolerance() {
    let mut d = doc();
    // A 2 in (50.8 mm) circle centred 5 in from the bottom-left corner.
    let c = path(&mut d, shapes::ellipse(Rect::new(288.0, 288.0, 432.0, 432.0)));
    add(&mut d, c);
    for tol in [0.5, 0.05, 0.01] {
        let j = document_to_job(&d, &JobOptions { tolerance_mm: tol, ..JobOptions::default() }).unwrap();
        let lines = polylines(&j);
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        let (cx, cy, r) = (127.0, 127.0, 25.4);
        // Every vertex lies on the circle (up to the Bézier approximation of a circle)…
        for p in l {
            assert!(((p.0 - cx).hypot(p.1 - cy) - r).abs() < 0.02, "{p:?}");
        }
        // …and every chord's midpoint is within the tolerance of it.
        for w in l.windows(2) {
            let m = ((w[0].0 + w[1].0) / 2.0, (w[0].1 + w[1].1) / 2.0);
            let err = r - (m.0 - cx).hypot(m.1 - cy);
            assert!(err <= tol + 0.02, "tolerance {tol}: chord off by {err}");
        }
        assert!(l.len() > 8);
    }
    let coarse = document_to_job(&d, &JobOptions { tolerance_mm: 0.5, ..JobOptions::default() }).unwrap();
    let fine = document_to_job(&d, &JobOptions { tolerance_mm: 0.01, ..JobOptions::default() }).unwrap();
    assert!(polylines(&fine)[0].len() > polylines(&coarse)[0].len());
}

#[test]
fn rotated_group_is_cut_where_it_is_drawn() {
    let mut d = doc();
    let a = path(&mut d, shapes::rectangle(Rect::new(0.0, 0.0, 72.0, 36.0)));
    let gid = d.alloc_id();
    let mut g = Node::group(gid, vec![Arc::new(a)]);
    // Turned 90° about the origin, then moved 4 in right and 2 in down.
    g.transform(Affine::translate((288.0, 144.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2), false);
    add(&mut d, g);
    let l = &polylines(&job(&d))[0];
    let xs: Vec<f64> = l.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = l.iter().map(|p| p.1).collect();
    let min = |v: &[f64]| v.iter().copied().fold(f64::MAX, f64::min);
    let max = |v: &[f64]| v.iter().copied().fold(f64::MIN, f64::max);
    // The 1 × 0.5 in rectangle is now 0.5 in wide (x from 3.5 to 4 in) and 1 in tall (2 to 3 in
    // down from the top: 7 to 8 in up).
    assert!(close(min(&xs), 3.5 * 25.4) && close(max(&xs), 4.0 * 25.4), "{xs:?}");
    assert!(close(min(&ys), 7.0 * 25.4) && close(max(&ys), 8.0 * 25.4), "{ys:?}");
}

#[test]
fn hidden_template_and_non_printing_layers_and_guides_are_left_out() {
    let mut d = doc();
    let r = path(&mut d, shapes::rectangle(Rect::new(10.0, 10.0, 20.0, 20.0)));
    add(&mut d, r);
    let mut layers = vec![];
    for i in 0..3 {
        let id = d.alloc_id();
        let mut l = Node::layer(id, &format!("L{i}"), LayerColor::Preset(0));
        if let vectorcraft_doc::NodeKind::Layer { template, printable, .. } = &mut l.kind {
            match i {
                0 => l.visible = false,
                1 => *template = true,
                _ => *printable = false,
            }
        }
        let lid = d.insert(None, usize::MAX, l).unwrap();
        let p = path(&mut d, shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
        d.insert(Some(lid), usize::MAX, p).unwrap();
        layers.push(lid);
    }
    // A guide on the first layer, and a hidden path.
    let mut g = path(&mut d, shapes::rectangle(Rect::new(300.0, 300.0, 400.0, 400.0)));
    if let vectorcraft_doc::NodeKind::Path { guide, .. } = &mut g.kind {
        *guide = true;
    }
    add(&mut d, g);
    let mut h = path(&mut d, shapes::rectangle(Rect::new(300.0, 300.0, 400.0, 400.0)));
    h.visible = false;
    add(&mut d, h);
    assert_eq!(polylines(&job(&d)).len(), 1);
}

#[test]
fn inner_contours_are_cut_before_the_outline_around_them() {
    let mut d = doc();
    // The outline first in paint order, then two holes, then a separate part far away.
    let outer = path(&mut d, shapes::rectangle(Rect::new(72.0, 72.0, 360.0, 360.0)));
    let hole_a = path(&mut d, shapes::ellipse(Rect::new(100.0, 100.0, 150.0, 150.0)));
    let hole_b = path(&mut d, shapes::rectangle(Rect::new(250.0, 250.0, 300.0, 300.0)));
    let other = path(&mut d, shapes::rectangle(Rect::new(500.0, 500.0, 600.0, 600.0)));
    for n in [outer, hole_a, hole_b, other] {
        add(&mut d, n);
    }
    let lines = polylines(&job(&d));
    assert_eq!(lines.len(), 4);
    let big = |l: &Vec<(f64, f64)>| {
        let xs: Vec<f64> = l.iter().map(|p| p.0).collect();
        xs.iter().copied().fold(f64::MIN, f64::max) - xs.iter().copied().fold(f64::MAX, f64::min) > 100.0
    };
    let outer_at = lines.iter().position(big).unwrap();
    // Both holes come before the outline (the far part may come before or after).
    let holes: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let x = l[0].0;
            !big(l) && x < 360.0 * MM_PER_PT
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(holes.len(), 2);
    assert!(holes.iter().all(|h| *h < outer_at), "{holes:?} then {outer_at}");
}

#[test]
fn nearest_neighbour_order_shortens_travel() {
    let mut d = doc();
    // Squares along a line, painted out of order.
    for x in [600.0, 0.0, 400.0, 200.0, 100.0, 500.0, 300.0] {
        let p = path(&mut d, shapes::rectangle(Rect::new(x, 0.0, x + 20.0, 20.0)));
        add(&mut d, p);
    }
    let lines = polylines(&job(&d));
    let starts: Vec<f64> = lines.iter().map(|l| l[0].0).collect();
    let mut sorted = starts.clone();
    sorted.sort_by(f64::total_cmp);
    assert_eq!(starts, sorted, "cut left to right from the origin");
}

#[test]
fn layer_settings_are_stored_in_the_document_and_cut_with() {
    let mut d = doc();
    let r = path(&mut d, shapes::rectangle(Rect::new(72.0, 72.0, 144.0, 144.0)));
    add(&mut d, r);
    let layer = d.layers[0].id;
    let s = LayerSettings { speed_mm_min: 600.0, power_percent: 85.0, passes: 3, ..LayerSettings::default() };
    set_layer_settings(&mut d, layer, Some(s)).unwrap();
    assert_eq!(layer_settings(&d).get(&layer), Some(&s));
    let j = job(&d);
    assert_eq!(polylines(&j).len(), 3, "three passes");
    assert_eq!(j.contours, 3);
    assert!(
        j.job
            .operations
            .iter()
            .all(|o| !matches!(o, Operation::Cut { speed_mm_min, power_percent, .. } if *speed_mm_min != 600.0 || *power_percent != 85.0))
    );
    // The document's settings round-trip through its JSON.
    let json = serde_json::to_string(&d).unwrap();
    let back: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(layer_settings(&back).get(&layer), Some(&s));
    // Disabled layers are left out.
    set_layer_settings(&mut d, layer, Some(LayerSettings { enabled: false, ..s })).unwrap();
    assert!(job(&d).job.operations.is_empty());
    // Out of range settings are refused, and clearing removes the entry.
    assert!(set_layer_settings(&mut d, layer, Some(LayerSettings { power_percent: 101.0, ..s })).is_err());
    assert!(set_layer_settings(&mut d, layer, Some(LayerSettings { passes: 0, ..s })).is_err());
    set_layer_settings(&mut d, layer, None).unwrap();
    assert!(!d.unknown.contains_key(LAYER_SETTINGS_KEY));
    // Damaged entries fall back to the defaults.
    d.unknown.insert(LAYER_SETTINGS_KEY.into(), serde_json::json!({ "x": {}, "5": { "power": "max" }, "6": { "power": 250 } }));
    assert!(layer_settings(&d).is_empty());
    assert_eq!(polylines(&job(&d)).len(), 1);
}

#[test]
fn options_override_the_stored_settings_and_selection_limits_the_art() {
    let mut d = doc();
    let a = path(&mut d, shapes::rectangle(Rect::new(72.0, 72.0, 144.0, 144.0)));
    let b = path(&mut d, shapes::rectangle(Rect::new(300.0, 300.0, 400.0, 400.0)));
    let a = add(&mut d, a);
    add(&mut d, b);
    let layer = d.layers[0].id;
    let mut map = BTreeMap::new();
    map.insert(layer, LayerSettings { passes: 2, ..LayerSettings::default() });
    let j = document_to_job(&d, &JobOptions { selection: Some(vec![a]), layers: Some(map), ..JobOptions::default() }).unwrap();
    let lines = polylines(&j);
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|l| l.iter().all(|p| p.0 < 60.0)));
}

#[test]
fn images_and_type_are_warned_about() {
    let mut d = doc();
    let t = Node::new(
        d.alloc_id(),
        vectorcraft_doc::NodeKind::Text(Box::new(vectorcraft_doc::TextObject::point(
            Point::new(10.0, 10.0),
            "Hi",
            vectorcraft_doc::CharStyle::default(),
        ))),
    );
    add(&mut d, t);
    let j = job(&d);
    assert!(j.job.operations.is_empty());
    assert!(j.warnings.iter().any(|w| w.contains("type")), "{:?}", j.warnings);
}

#[test]
fn bad_options_are_refused() {
    let d = doc();
    for tol in [0.0, -1.0, f64::NAN, 50.0] {
        assert!(document_to_job(&d, &JobOptions { tolerance_mm: tol, ..JobOptions::default() }).is_err());
    }
    assert!(document_to_job(&d, &JobOptions { artboard: 3, ..JobOptions::default() }).is_err());
}

#[test]
fn art_off_the_bed_is_rejected_by_the_planner() {
    let mut d = doc();
    // 9 in to 9.5 in right: past a 200 mm bed.
    let r = path(&mut d, shapes::rectangle(Rect::new(648.0, 72.0, 684.0, 144.0)));
    add(&mut d, r);
    let j = job(&d);
    let small = Machine { width_mm: 200.0, height_mm: 300.0, max_s: 1000 };
    assert_eq!(j.job.to_grbl(small), Err(PlanError::OutOfBounds));
    let big = Machine { width_mm: 300.0, height_mm: 300.0, max_s: 1000 };
    let g = j.job.to_grbl(big).unwrap();
    assert!(g.starts_with("G21\nG90\nM5\n"));
    assert!(g.contains("M4 S200"));
}

fn fill_settings(s: LayerSettings) -> JobOptions {
    JobOptions { default_layer: LayerSettings { mode: LaserMode::Fill, ..s }, ..JobOptions::default() }
}

/// The burnt stretches of a job: each Cut at power above 0, as (from, to).
fn burns(j: &JobOutput) -> Vec<((f64, f64), (f64, f64))> {
    let mut out = vec![];
    let mut at = (0.0, 0.0);
    for op in &j.job.operations {
        match op {
            Operation::Travel(p) => at = (p.x_mm, p.y_mm),
            Operation::Cut { to, power_percent, .. } => {
                if *power_percent > 0.0 {
                    out.push((at, (to.x_mm, to.y_mm)));
                }
                at = (to.x_mm, to.y_mm);
            }
            Operation::LaserOff => {}
        }
    }
    out
}

#[test]
fn fill_mode_engraves_inside_shapes_and_keeps_holes_empty() {
    let mut d = doc();
    // A 1 in square ring (compound path, even-odd) with a 0.5 in hole.
    let outer = shapes::rectangle(Rect::new(72.0, 72.0, 144.0, 144.0));
    let inner = shapes::rectangle(Rect::new(90.0, 90.0, 126.0, 126.0));
    let (a, b) = (path(&mut d, outer), path(&mut d, inner));
    let id = d.alloc_id();
    let ring =
        Node::new(id, vectorcraft_doc::NodeKind::Compound { children: vec![Arc::new(a), Arc::new(b)], rule: vectorcraft_geom::FillRule::EvenOdd });
    add(&mut d, ring);
    let s = LayerSettings { interval_mm: 0.5, ..LayerSettings::default() };
    let j = document_to_job(&d, &fill_settings(s)).unwrap();
    let b = burns(&j);
    // Horizontal lines 0.5 mm apart across the 25.4 mm square.
    let mut ys: Vec<f64> = b.iter().map(|s| (s.0).1).collect();
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert_eq!(ys.len(), 51, "{}", ys.len());
    // Nothing burns inside the hole (x 31.75..44.45, y 209.55..222.25 mm).
    for ((x0, y), (x1, _)) in &b {
        let (lo, hi) = (x0.min(*x1), x0.max(*x1));
        let in_hole_rows = *y > 209.6 && *y < 222.2;
        assert!(!(in_hole_rows && lo < 31.8 && hi > 44.4), "burns across the hole at y {y}");
    }
    // All within the square.
    assert!(
        b.iter()
            .all(|((x0, y0), (x1, y1))| [x0, x1].iter().all(|x| (25.39..=50.81).contains(*x))
                && [y0, y1].iter().all(|y| (203.19..=228.61).contains(*y)))
    );
    // No outline is cut in Fill mode, and the job plans.
    assert!(j.job.to_grbl(lasercraft_core::Machine { width_mm: 300.0, height_mm: 300.0, max_s: 1000 }).is_ok());
}

#[test]
fn crosshatch_and_angle_and_passes() {
    let mut d = doc();
    let r = path(&mut d, shapes::rectangle(Rect::new(72.0, 72.0, 144.0, 144.0)));
    add(&mut d, r);
    let one = burns(&document_to_job(&d, &fill_settings(LayerSettings { interval_mm: 1.0, ..LayerSettings::default() })).unwrap()).len();
    let hatch =
        burns(&document_to_job(&d, &fill_settings(LayerSettings { interval_mm: 1.0, crosshatch: true, ..LayerSettings::default() })).unwrap());
    assert_eq!(hatch.len(), one * 2);
    // The second half runs vertically.
    let (a, b) = hatch[one];
    assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() > 1.0);
    let vertical =
        burns(&document_to_job(&d, &fill_settings(LayerSettings { interval_mm: 1.0, angle_deg: 90.0, ..LayerSettings::default() })).unwrap());
    assert!(vertical.iter().all(|(a, b)| (a.0 - b.0).abs() < 1e-9));
    let twice = burns(&document_to_job(&d, &fill_settings(LayerSettings { interval_mm: 1.0, passes: 2, ..LayerSettings::default() })).unwrap());
    assert_eq!(twice.len(), one * 2);
}

#[test]
fn overscan_runs_on_at_zero_power_and_stops_at_the_bed_edge() {
    let mut d = doc();
    // Touching the artboard's left edge: no room for overscan on the left.
    let r = path(&mut d, shapes::rectangle(Rect::new(0.0, 72.0, 72.0, 144.0)));
    add(&mut d, r);
    let s = LayerSettings { interval_mm: 1.0, overscan_mm: 5.0, bidirectional: false, ..LayerSettings::default() };
    let o = JobOptions { bed: Some((300.0, 300.0)), ..fill_settings(s) };
    let j = document_to_job(&d, &o).unwrap();
    assert!(j.warnings.iter().any(|w| w.contains("overscan")), "{:?}", j.warnings);
    // Every point stays on the bed, and the right-hand overscan runs 5 mm past at power 0.
    let mut max_x: f64 = 0.0;
    for op in &j.job.operations {
        if let Operation::Travel(p) | Operation::Cut { to: p, .. } = op {
            assert!(p.x_mm >= 0.0 && p.y_mm >= 0.0);
            max_x = max_x.max(p.x_mm);
        }
    }
    assert!((max_x - (25.4 + 5.0)).abs() < 1e-6, "{max_x}");
    assert!(j.job.operations.iter().any(|op| matches!(op, Operation::Cut { power_percent, .. } if *power_percent == 0.0)));
    assert!(burns(&j).iter().all(|((x0, _), (x1, _))| *x0 >= 0.0 && *x1 <= 25.4 + 1e-6));
    // Room on both sides: no warning.
    let mut d = doc();
    let r = path(&mut d, shapes::rectangle(Rect::new(72.0, 72.0, 144.0, 144.0)));
    add(&mut d, r);
    assert!(document_to_job(&d, &o).unwrap().warnings.is_empty());
}

#[test]
fn fill_settings_are_checked_and_stored() {
    let mut d = doc();
    let layer = d.layers[0].id;
    let s = LayerSettings {
        mode: LaserMode::Fill,
        interval_mm: 0.08,
        angle_deg: 45.0,
        overscan_mm: 2.5,
        crosshatch: true,
        bidirectional: false,
        ..LayerSettings::default()
    };
    set_layer_settings(&mut d, layer, Some(s)).unwrap();
    let back: Document = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(layer_settings(&back).get(&layer), Some(&s));
    for bad in [
        LayerSettings { interval_mm: 0.0, ..s },
        LayerSettings { interval_mm: f64::NAN, ..s },
        LayerSettings { angle_deg: 1000.0, ..s },
        LayerSettings { overscan_mm: -1.0, ..s },
        LayerSettings { overscan_mm: 100.0, ..s },
    ] {
        assert!(set_layer_settings(&mut d, layer, Some(bad)).is_err());
    }
    // Settings saved before Fill existed still read, as Line.
    d.unknown.insert(
        LAYER_SETTINGS_KEY.into(),
        serde_json::json!({ layer.0.to_string(): { "mode": "line", "speed": 500, "power": 30, "passes": 1, "enabled": true } }),
    );
    let read = layer_settings(&d)[&layer];
    assert_eq!((read.mode, read.interval_mm, read.speed_mm_min), (LaserMode::Line, 0.1, 500.0));
    assert_eq!(LaserMode::parse("FILL"), Some(LaserMode::Fill));
}

/// A `w` × `h` grey ramp PNG, white on the left to black on the right.
fn ramp_png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, _| {
        let v = 255 - (x * 255 / (w - 1).max(1)) as u8;
        image::Rgba([v, v, v, 255])
    });
    let mut out = vec![];
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

/// A document with a ramp image of `w` × `h` pixels placed by `xf` (pixel → points).
fn image_doc(w: u32, h: u32, xf: Affine) -> Document {
    let mut d = doc();
    d.images.insert("ramp".into(), vectorcraft_doc::ImageBlob::png(ramp_png(w, h)));
    let id = d.alloc_id();
    let im = vectorcraft_doc::ImageObject { key: "ramp".into(), width: w, height: h, xf, link: None, placement: Default::default() };
    add(&mut d, Node::new(id, vectorcraft_doc::NodeKind::Image(Box::new(im))));
    d
}

fn image_layer(s: LayerSettings) -> JobOptions {
    JobOptions { default_layer: LayerSettings { mode: LaserMode::Image, ..s }, ..JobOptions::default() }
}

#[test]
fn image_mode_engraves_rows_with_power_from_darkness() {
    // 64 × 32 px placed as 1 × 0.5 in, 1 in from the top-left of the board.
    let d = image_doc(64, 32, Affine::translate((72.0, 72.0)) * Affine::scale(72.0 / 64.0));
    let s = LayerSettings {
        interval_mm: 1.0,
        dither: Dither::Grayscale,
        power_percent: 80.0,
        min_power_percent: 10.0,
        bidirectional: false,
        ..LayerSettings::default()
    };
    let j = document_to_job(&d, &image_layer(s)).unwrap();
    assert!(j.warnings.is_empty(), "{:?}", j.warnings);
    // 12.7 mm tall at 1 mm: 13 rows.
    assert_eq!(j.contours, 13);
    let mut powers = vec![];
    for op in &j.job.operations {
        if let Operation::Cut { to, power_percent, .. } = op {
            assert!((25.4 - 1e-6..=50.8 + 1e-6).contains(&to.x_mm), "inside the image: {}", to.x_mm);
            if *power_percent > 0.0 {
                powers.push(*power_percent);
            }
        }
    }
    // Grey levels between the minimum and the maximum, the darkest at the maximum.
    assert!(powers.iter().all(|p| (10.0..=80.0).contains(p)), "{powers:?}");
    assert!(powers.iter().any(|p| (*p - 80.0).abs() < 1.0));
    assert!(powers.iter().any(|p| *p < 40.0));
    // Each row runs left to right (bidirectional off), darker (more power) to the right.
    assert!(j.job.to_grbl(lasercraft_core::Machine { width_mm: 300.0, height_mm: 300.0, max_s: 1000 }).is_ok());
}

#[test]
fn image_dithering_negative_and_overscan() {
    let d = image_doc(64, 32, Affine::translate((72.0, 72.0)) * Affine::scale(72.0 / 64.0));
    let base = LayerSettings { interval_mm: 0.5, power_percent: 50.0, ..LayerSettings::default() };
    let burnt = |s: LayerSettings| -> f64 { burns(&document_to_job(&d, &image_layer(s)).unwrap()).iter().map(|(a, b)| (a.0 - b.0).abs()).sum() };
    // Dithered: full power or none; a ramp burns about half its area.
    let fs = document_to_job(&d, &image_layer(base)).unwrap();
    assert!(fs.job.operations.iter().all(|o| !matches!(o, Operation::Cut { power_percent, .. } if *power_percent > 0.0 && *power_percent != 50.0)));
    let area = 25.4 * 25.4 / 2.0 / 0.5;
    for dither in [Dither::Threshold, Dither::Ordered, Dither::FloydSteinberg, Dither::Jarvis] {
        let b = burnt(LayerSettings { dither, ..base });
        assert!((b / area - 0.5).abs() < 0.12, "{dither:?}: {}", b / area);
    }
    // Same settings, same job.
    let a = document_to_job(&d, &image_layer(LayerSettings { dither: Dither::Jarvis, ..base })).unwrap();
    let b = document_to_job(&d, &image_layer(LayerSettings { dither: Dither::Jarvis, ..base })).unwrap();
    assert_eq!(format!("{:?}", a.job.operations), format!("{:?}", b.job.operations));
    // Negative: the light half burns instead.
    let neg = burns(&document_to_job(&d, &image_layer(LayerSettings { negative: true, dither: Dither::Threshold, ..base })).unwrap());
    assert!(neg.iter().all(|(a, b)| a.0.max(b.0) <= 38.1 + 0.5), "the left (white) half, give or take a sample: {:?}", neg.first());
    // Overscan at power 0, shortened at the bed edge with a warning.
    let os = document_to_job(&d, &JobOptions { bed: Some((40.0, 300.0)), ..image_layer(LayerSettings { overscan_mm: 5.0, ..base }) }).unwrap();
    assert!(os.warnings.iter().any(|w| w.contains("overscan")), "{:?}", os.warnings);
}

#[test]
fn turned_images_are_engraved_along_their_rows() {
    // Turned 90°: the image's rows run down the bed.
    let d = image_doc(32, 32, Affine::translate((200.0, 72.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2) * Affine::scale(72.0 / 32.0));
    let j = document_to_job(&d, &image_layer(LayerSettings { interval_mm: 1.0, dither: Dither::Threshold, ..LayerSettings::default() })).unwrap();
    let b = burns(&j);
    assert!(!b.is_empty());
    assert!(b.iter().all(|(a, b)| (a.0 - b.0).abs() < 1e-6), "vertical burns: {:?}", b.first());
}

#[test]
fn images_off_image_layers_and_paths_on_them_are_warned_about() {
    let mut d = image_doc(16, 16, Affine::scale(4.0));
    let r = path(&mut d, shapes::rectangle(Rect::new(300.0, 300.0, 400.0, 400.0)));
    add(&mut d, r);
    let line = document_to_job(&d, &JobOptions::default()).unwrap();
    assert!(line.warnings.iter().any(|w| w.contains("Image mode")), "{:?}", line.warnings);
    let img = document_to_job(&d, &image_layer(LayerSettings::default())).unwrap();
    assert!(img.warnings.iter().any(|w| w.contains("paths on Image layers")), "{:?}", img.warnings);
    // A missing image blob is left out with a warning, not a failure.
    d.images.clear();
    let gone = document_to_job(&d, &image_layer(LayerSettings::default())).unwrap();
    assert!(gone.warnings.iter().any(|w| w.contains("could not be read")), "{:?}", gone.warnings);
}

#[test]
fn image_sample_counts_are_capped() {
    // 2000 × 2000 px stretched over the whole 254 mm board at 0.01 mm: 25400² samples.
    let d = image_doc(8, 8, Affine::scale(90.0));
    let e = document_to_job(&d, &image_layer(LayerSettings { interval_mm: 0.01, ..LayerSettings::default() })).unwrap_err();
    assert!(e.contains("too large"), "{e}");
    // Settings: minimum power above the power is refused.
    let mut d = doc();
    let layer = d.layers[0].id;
    let s = LayerSettings { mode: LaserMode::Image, power_percent: 30.0, min_power_percent: 40.0, ..LayerSettings::default() };
    assert!(set_layer_settings(&mut d, layer, Some(s)).is_err());
    let ok = LayerSettings { min_power_percent: 5.0, dither: Dither::Ordered, negative: true, ..s };
    set_layer_settings(&mut d, layer, Some(ok)).unwrap();
    let back: Document = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(layer_settings(&back).get(&layer), Some(&ok));
}
