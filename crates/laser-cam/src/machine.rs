//! Machine profiles: the bed a job must fit, the S value of full power and the corner the machine
//! counts from. They are the `machines` of LaserCraft's `profiles.json` (written by the desktop
//! app's laser panel), read here so that the app and the command line pick the same machine.
//! Profiles saved before `origin` existed get their controller's usual corner.

use std::path::PathBuf;

use serde_json::Value;

/// The corner of the bed the machine counts from. Each axis runs from that corner into the bed:
/// from a top corner y grows towards the front, from a right corner x grows to the left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Origin {
    /// Front-left (GRBL's usual home): x right, y up.
    #[default]
    BottomLeft,
    /// Back-left: x right, y down.
    TopLeft,
    /// Back-right (common on Ruida CO2 machines): x left, y down.
    TopRight,
    /// Front-right: x left, y up.
    BottomRight,
}

impl Origin {
    pub const ALL: [Origin; 4] = [Origin::BottomLeft, Origin::TopLeft, Origin::TopRight, Origin::BottomRight];

    pub fn id(self) -> &'static str {
        match self {
            Origin::BottomLeft => "bottomLeft",
            Origin::TopLeft => "topLeft",
            Origin::TopRight => "topRight",
            Origin::BottomRight => "bottomRight",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Origin::BottomLeft => "Bottom left (front left)",
            Origin::TopLeft => "Top left (back left)",
            Origin::TopRight => "Top right (back right)",
            Origin::BottomRight => "Bottom right (front right)",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
            "bottomleft" | "bl" | "frontleft" => Some(Origin::BottomLeft),
            "topleft" | "tl" | "backleft" => Some(Origin::TopLeft),
            "topright" | "tr" | "backright" => Some(Origin::TopRight),
            "bottomright" | "br" | "frontright" => Some(Origin::BottomRight),
            _ => None,
        }
    }
    /// The corner a controller family usually homes to: top right for Ruida, bottom left for GRBL
    /// and anything else. Machines differ: the profile's own `origin` wins.
    pub fn default_for(controller: &str) -> Self {
        if controller.trim().eq_ignore_ascii_case("ruida") { Origin::TopRight } else { Origin::BottomLeft }
    }
    pub(crate) fn top(self) -> bool {
        matches!(self, Origin::TopLeft | Origin::TopRight)
    }
    pub(crate) fn right(self) -> bool {
        matches!(self, Origin::TopRight | Origin::BottomRight)
    }
}

/// The largest bed side (mm).
pub const MAX_BED_MM: f64 = 10_000.0;
/// The largest S value of full power.
pub const MAX_S: u32 = 1_000_000;

/// One machine of `profiles.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct MachineProfile {
    pub name: String,
    pub controller: String,
    pub width_mm: f64,
    pub height_mm: f64,
    /// The S value of 100 % power (GRBL's `$30`).
    pub max_s: u32,
    pub origin: Origin,
}

impl MachineProfile {
    /// The profile `v` (a `machines` entry): an error when its bed size or S range is missing or
    /// out of range. Without `origin` (or with one not known), the controller's usual corner.
    pub fn from_json(v: &Value) -> Result<Self, String> {
        let name = v["name"].as_str().unwrap_or("Unnamed").to_string();
        let controller = v["controller"].as_str().unwrap_or("GRBL").to_string();
        let side = |key: &str| {
            v[key]
                .as_f64()
                .filter(|s| s.is_finite() && *s > 0.0 && *s <= MAX_BED_MM)
                .ok_or_else(|| format!("machine `{name}`: {key} must be a bed size of more than 0 and at most {MAX_BED_MM} mm"))
        };
        let max_s = match v.get("max_s") {
            None | Some(Value::Null) => 1000,
            Some(s) => s
                .as_u64()
                .and_then(|s| u32::try_from(s).ok())
                .filter(|s| (1..=MAX_S).contains(s))
                .ok_or_else(|| format!("machine `{name}`: max_s must be a whole number from 1 to {MAX_S}"))?,
        };
        let origin = v["origin"].as_str().and_then(Origin::parse).unwrap_or_else(|| Origin::default_for(&controller));
        Ok(Self { width_mm: side("width_mm")?, height_mm: side("height_mm")?, max_s, origin, name, controller })
    }
}

/// The machines of a `profiles.json` document (`{"machines": [...]}`), leaving out the ones that
/// don't read.
pub fn machines(profiles: &Value) -> Vec<MachineProfile> {
    profiles["machines"].as_array().map(|a| a.iter().filter_map(|m| MachineProfile::from_json(m).ok()).collect()).unwrap_or_default()
}

/// The machine called `name` (any case) in a `profiles.json` document.
pub fn find_machine(profiles: &Value, name: &str) -> Result<MachineProfile, String> {
    let all = profiles["machines"].as_array().map(Vec::as_slice).unwrap_or_default();
    let entry = all.iter().find(|m| m["name"].as_str().is_some_and(|n| n.trim().eq_ignore_ascii_case(name.trim()))).ok_or_else(|| {
        let names: Vec<&str> = all.iter().filter_map(|m| m["name"].as_str()).collect();
        if names.is_empty() {
            format!("no machine profile `{name}`: none are saved")
        } else {
            format!("no machine profile `{name}` (saved: {})", names.join(", "))
        }
    })?;
    MachineProfile::from_json(entry)
}

/// Where LaserCraft keeps `profiles.json`: `lasercraft/profiles.json` in the user's configuration
/// folder (`%APPDATA%`, `~/Library/Application Support`, `$XDG_CONFIG_HOME` or `~/.config`);
/// `None` when there is none (the web).
pub fn default_profiles_path() -> Option<PathBuf> {
    let root = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else if cfg!(target_arch = "wasm32") {
        None
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(root.join("lasercraft").join("profiles.json"))
}
