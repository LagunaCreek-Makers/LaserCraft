//! LaserCraft CAM: a VectorCraft [`Document`] as a controller-independent [`lasercraft_core::Job`].
//!
//! Live effects are baked first ([`vectorcraft_effects::bake_document`]); then each visible,
//! printable layer's paths are flattened to polylines within a tolerance, converted from points to
//! millimetres and placed on the bed from the chosen origin (y up from the artboard's bottom-left
//! corner, or down from its top-left). Each layer is cut with its [`LayerSettings`], layers in
//! paint order (bottom first), inner contours before the outlines around them and otherwise
//! nearest first. Nothing here talks to a machine.

mod order;
pub mod settings;
mod walk;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use lasercraft_core::{Job, Operation, Point as MmPoint};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_geom::{Affine, Point, Rect};

pub use settings::{LAYER_SETTINGS_KEY, LaserMode, LayerSettings, layer_settings, set_layer_settings};

/// Millimetres per point.
pub const MM_PER_PT: f64 = 25.4 / 72.0;
/// The finest and coarsest flattening tolerance (mm).
pub const TOLERANCE_RANGE: std::ops::RangeInclusive<f64> = 0.001..=5.0;

/// Where the bed's origin sits on the artboard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Origin {
    /// x right and y up from the artboard's bottom-left corner (GRBL's usual front-left home).
    #[default]
    BottomLeft,
    /// x right and y down from the artboard's top-left corner (machines homing at the back-left
    /// whose y grows towards the front).
    TopLeft,
}

impl Origin {
    pub fn id(self) -> &'static str {
        match self {
            Origin::BottomLeft => "bottomLeft",
            Origin::TopLeft => "topLeft",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
            "bottomleft" | "bl" => Some(Origin::BottomLeft),
            "topleft" | "tl" => Some(Origin::TopLeft),
            _ => None,
        }
    }
}

/// How a document becomes a job.
#[derive(Clone, Debug)]
pub struct JobOptions {
    /// Curves are flattened to within this distance (mm).
    pub tolerance_mm: f64,
    pub origin: Origin,
    /// The artboard whose corner is the bed origin (the art's bounds when the document has none).
    pub artboard: usize,
    /// Only these objects (and what they hold); `None`: everything.
    pub selection: Option<Vec<NodeId>>,
    /// Settings by layer id; `None`: the settings the document holds ([`layer_settings`]).
    pub layers: Option<BTreeMap<NodeId, LayerSettings>>,
    /// The settings of layers that have none.
    pub default_layer: LayerSettings,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self { tolerance_mm: 0.05, origin: Origin::BottomLeft, artboard: 0, selection: None, layers: None, default_layer: LayerSettings::default() }
    }
}

/// A job and what was left out of it.
#[derive(Clone, Debug)]
pub struct JobOutput {
    pub job: Job,
    /// One line for each kind of art left out or changed (each said once).
    pub warnings: Vec<String>,
    /// How many polylines are cut (each pass counted).
    pub contours: usize,
}

/// The transform from document space (points, y down) to bed millimetres for `origin` on `frame`.
pub fn bed_transform(frame: Rect, origin: Origin) -> Affine {
    let k = MM_PER_PT;
    match origin {
        Origin::BottomLeft => Affine::new([k, 0.0, 0.0, -k, -frame.x0 * k, frame.y1 * k]),
        Origin::TopLeft => Affine::new([k, 0.0, 0.0, k, -frame.x0 * k, -frame.y0 * k]),
    }
}

/// `doc` as a laser job with `o`.
pub fn document_to_job(doc: &Document, o: &JobOptions) -> Result<JobOutput, String> {
    if !(o.tolerance_mm.is_finite() && TOLERANCE_RANGE.contains(&o.tolerance_mm)) {
        return Err(format!("tolerance must be from {} to {} mm, not {}", TOLERANCE_RANGE.start(), TOLERANCE_RANGE.end(), o.tolerance_mm));
    }
    o.default_layer.validate()?;
    let frame = match doc.artboards.get(o.artboard) {
        Some(a) => a.rect,
        None if doc.artboards.is_empty() => doc.art_bounds().unwrap_or(Rect::ZERO),
        None => return Err(format!("no artboard {} (the document has {})", o.artboard.saturating_add(1), doc.artboards.len())),
    };
    let baked = vectorcraft_effects::bake_document(doc);
    let doc = baked.as_ref().unwrap_or(doc);
    let stored;
    let settings = match &o.layers {
        Some(l) => l,
        None => {
            stored = layer_settings(doc);
            &stored
        }
    };
    for (id, s) in settings {
        s.validate().map_err(|e| format!("layer {id}: {e}"))?;
    }
    let walked = walk::Walker::new(doc, bed_transform(frame, o.origin), o.tolerance_mm, settings, o.default_layer, o.selection.as_deref()).run();
    let mut ops = vec![];
    let mut head = Point::ZERO;
    let mut contours = 0;
    for (_, s, list) in walked.layers {
        let (list, end) = order::order(list, head);
        head = end;
        for _ in 0..s.passes {
            for c in &list {
                emit(c, &s, &mut ops);
                contours += 1;
            }
        }
    }
    Ok(JobOutput { job: Job { operations: ops }, warnings: walked.warnings, contours })
}

fn emit(c: &walk::Contour, s: &LayerSettings, ops: &mut Vec<Operation>) {
    let mm = |p: &Point| MmPoint { x_mm: p.x, y_mm: p.y };
    let Some((first, rest)) = c.points.split_first() else { return };
    let cut = |p: &Point| Operation::Cut { to: mm(p), speed_mm_min: s.speed_mm_min, power_percent: s.power_percent };
    ops.push(Operation::Travel(mm(first)));
    ops.extend(rest.iter().map(cut));
    if c.closed {
        ops.push(cut(first));
    }
    ops.push(Operation::LaserOff);
}
