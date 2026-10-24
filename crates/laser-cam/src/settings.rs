//! Per-layer laser settings (mode, speed, power, passes, enabled), stored with the document in
//! `Document.unknown["laserLayers"]`: a map from the layer's object id to its settings. Files
//! without it open as before (every layer gets [`LayerSettings::default`]), and VectorCraft keeps
//! the entry when it saves, since `unknown` round-trips.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use vectorcraft_doc::{Document, NodeId};

/// The `Document.unknown` key holding the per-layer settings.
pub const LAYER_SETTINGS_KEY: &str = "laserLayers";

/// Fastest speed a layer may ask for (mm/min).
pub const MAX_SPEED_MM_MIN: f64 = 100_000.0;
/// Most passes a layer may ask for.
pub const MAX_PASSES: u32 = 100;

/// How a layer's art is cut. Only Line (cut along the path) exists for now; Fill and Image come
/// later.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaserMode {
    #[default]
    Line,
}

impl LaserMode {
    pub fn id(self) -> &'static str {
        match self {
            LaserMode::Line => "line",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        s.eq_ignore_ascii_case("line").then_some(LaserMode::Line)
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
}

impl Default for LayerSettings {
    fn default() -> Self {
        Self { mode: LaserMode::Line, speed_mm_min: 1000.0, power_percent: 20.0, passes: 1, enabled: true }
    }
}

impl LayerSettings {
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
