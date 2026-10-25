//! Per-layer laser settings (mode, speed, power, passes, enabled), stored with the document in
//! `Document.unknown["laserLayers"]`: a map from the layer's object id to its settings. Files
//! without it open as before (every layer gets [`LayerSettings::default`]), and VectorCraft keeps
//! the entry when it saves, since `unknown` round-trips.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use crate::raster::Dither;
use serde_json::Value;
use vectorcraft_doc::{Document, NodeId};

/// The `Document.unknown` key holding the per-layer settings.
pub const LAYER_SETTINGS_KEY: &str = "laserLayers";

/// Fastest speed a layer may ask for (mm/min).
pub const MAX_SPEED_MM_MIN: f64 = 100_000.0;
/// Most passes a layer may ask for.
pub const MAX_PASSES: u32 = 100;

/// Finest and coarsest Fill line interval (mm).
pub const INTERVAL_RANGE: std::ops::RangeInclusive<f64> = 0.01..=10.0;
/// Coarsest and finest Image pixel size along the line (pixels per inch: 2540 is 0.01 mm).
pub const DPI_RANGE: std::ops::RangeInclusive<f64> = 10.0..=2540.0;
/// Longest Fill overscan (mm).
pub const MAX_OVERSCAN_MM: f64 = 50.0;

/// How a layer's art is cut.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaserMode {
    /// Cut along the paths.
    #[default]
    Line,
    /// Engrave the inside of closed shapes with parallel scan lines (their fill rule decides
    /// what is inside: holes stay empty).
    Fill,
    /// Engrave placed images line by line, darkness as power (dithered or grey levels).
    Image,
}

impl LaserMode {
    pub const ALL: [LaserMode; 3] = [LaserMode::Line, LaserMode::Fill, LaserMode::Image];
    pub fn id(self) -> &'static str {
        match self {
            LaserMode::Line => "line",
            LaserMode::Fill => "fill",
            LaserMode::Image => "image",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            LaserMode::Line => "Line",
            LaserMode::Fill => "Fill",
            LaserMode::Image => "Image",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id().eq_ignore_ascii_case(s.trim()))
    }
}

/// What a layer's art is cut with.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LayerSettings {
    pub mode: LaserMode,
    /// Feed rate while cutting, mm/min.
    #[serde(rename = "speed")]
    pub speed_mm_min: f64,
    /// Laser power, percent of the controller's maximum.
    #[serde(rename = "power")]
    pub power_percent: f64,
    /// How many times the layer's art is cut (the whole layer, then again).
    pub passes: u32,
    /// Disabled layers are left out of the job.
    pub enabled: bool,
    /// Fill: the distance between scan lines (mm).
    #[serde(rename = "interval")]
    pub interval_mm: f64,
    /// Fill: the scan lines' angle, counter-clockwise degrees from the bed's x axis (0:
    /// horizontal lines, 90: vertical).
    #[serde(rename = "angle")]
    pub angle_deg: f64,
    /// Fill: every other scan line runs back (else all run the same way, with a travel between).
    pub bidirectional: bool,
    /// Fill: the head runs on this far past each end of a scan line with the laser off, so it
    /// is at speed while it burns (mm; shortened at the bed's edges).
    #[serde(rename = "overscan")]
    pub overscan_mm: f64,
    /// Fill: scan again at right angles to the first lines.
    pub crosshatch: bool,
    /// Image: how grey levels become power.
    pub dither: Dither,
    /// Image: the power of the lightest grey that burns (Grayscale; `power` is the darkest's).
    #[serde(rename = "minPower")]
    pub min_power_percent: f64,
    /// Image: burn the light parts instead of the dark ones.
    pub negative: bool,
    /// Image: pixels per inch along each line (the pixel size); `None` (and settings saved
    /// before it existed): one pixel per line interval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dpi: Option<f64>,
}

impl Default for LayerSettings {
    fn default() -> Self {
        Self {
            mode: LaserMode::Line,
            speed_mm_min: 1000.0,
            power_percent: 20.0,
            passes: 1,
            enabled: true,
            interval_mm: 0.1,
            angle_deg: 0.0,
            bidirectional: true,
            overscan_mm: 0.0,
            crosshatch: false,
            dither: Dither::FloydSteinberg,
            min_power_percent: 0.0,
            negative: false,
            dpi: None,
        }
    }
}

impl LayerSettings {
    /// Image: the pixel size along each line (mm), from `dpi`, else the line interval.
    pub fn pixel_mm(&self) -> f64 {
        self.dpi.map_or(self.interval_mm, |d| 25.4 / d)
    }

    /// An error naming the first value out of range.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.speed_mm_min.is_finite() && self.speed_mm_min > 0.0 && self.speed_mm_min <= MAX_SPEED_MM_MIN) {
            return Err(format!("speed must be more than 0 and at most {MAX_SPEED_MM_MIN} mm/min, not {}", self.speed_mm_min));
        }
        if !(self.power_percent.is_finite() && (0.0..=100.0).contains(&self.power_percent)) {
            return Err(format!("power must be from 0 to 100 %, not {}", self.power_percent));
        }
        if !(1..=MAX_PASSES).contains(&self.passes) {
            return Err(format!("passes must be from 1 to {MAX_PASSES}, not {}", self.passes));
        }
        if !(self.interval_mm.is_finite() && INTERVAL_RANGE.contains(&self.interval_mm)) {
            return Err(format!("interval must be from {} to {} mm, not {}", INTERVAL_RANGE.start(), INTERVAL_RANGE.end(), self.interval_mm));
        }
        if !(self.angle_deg.is_finite() && (-360.0..=360.0).contains(&self.angle_deg)) {
            return Err(format!("angle must be from -360 to 360 degrees, not {}", self.angle_deg));
        }
        if let Some(dpi) = self.dpi
            && !(dpi.is_finite() && DPI_RANGE.contains(&dpi))
        {
            return Err(format!("dpi must be from {} to {}, not {dpi}", DPI_RANGE.start(), DPI_RANGE.end()));
        }
        if !(self.min_power_percent.is_finite() && (0.0..=self.power_percent).contains(&self.min_power_percent)) {
            return Err(format!("minimum power must be from 0 to the power ({} %), not {}", self.power_percent, self.min_power_percent));
        }
        if !(self.overscan_mm.is_finite() && (0.0..=MAX_OVERSCAN_MM).contains(&self.overscan_mm)) {
            return Err(format!("overscan must be from 0 to {MAX_OVERSCAN_MM} mm, not {}", self.overscan_mm));
        }
        Ok(())
    }
}

/// The per-layer settings `doc` holds. Entries that don't parse or are out of range are left out
/// (their layers get the defaults), so a damaged file still opens and cuts.
pub fn layer_settings(doc: &Document) -> BTreeMap<NodeId, LayerSettings> {
    let Some(Value::Object(m)) = doc.unknown.get(LAYER_SETTINGS_KEY) else { return BTreeMap::new() };
    m.iter()
        .filter_map(|(k, v)| {
            let id = NodeId(k.parse().ok()?);
            let s = LayerSettings::deserialize(v).ok()?;
            s.validate().ok()?;
            Some((id, s))
        })
        .collect()
}

/// Set (or with `None` clear) the settings of layer `id` in `doc`. Settings out of range are
/// refused.
pub fn set_layer_settings(doc: &mut Document, id: NodeId, settings: Option<LayerSettings>) -> Result<(), String> {
    let mut all = layer_settings(doc);
    match settings {
        Some(s) => {
            s.validate()?;
            all.insert(id, s);
        }
        None => {
            all.remove(&id);
        }
    }
    if all.is_empty() {
        doc.unknown.remove(LAYER_SETTINGS_KEY);
        return Ok(());
    }
    let mut m = serde_json::Map::new();
    for (id, s) in all {
        m.insert(id.0.to_string(), serde_json::to_value(s).map_err(|e| e.to_string())?);
    }
    doc.unknown.insert(LAYER_SETTINGS_KEY.to_string(), Value::Object(m));
    Ok(())
}
