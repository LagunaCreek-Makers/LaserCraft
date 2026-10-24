//! A machine's job hooks: the air-assist and accessory G-code wrapped around every job it runs
//! (`air`, `accessories`, saved in its machine profile). [`job_hooks`] gives them as the lines
//! before and after the job, which `laser.exportGcode` takes as `prologue` and `epilogue`, so the
//! saved file and the job the laser panel runs carry the same ones.

pub mod accessories;
pub mod air;

use accessories::AccessoryConfig;
use air::{AirConfig, AirMode};

/// Stands for the job while the hooks are wrapped around it.
const JOB: &str = "; LASERCRAFT JOB";

/// The lines `air` (in `mode`) and then `accessories` put before and after a job, as the panel
/// applies them to the job it runs: an error when either configuration is refused.
pub fn job_hooks(air: &AirConfig, mode: AirMode, accessories: &AccessoryConfig) -> Result<(Vec<String>, Vec<String>), String> {
    let wrapped = accessories.apply(&air.apply(JOB, mode)?)?;
    let lines: Vec<&str> = wrapped.lines().collect();
    let at = lines.iter().position(|l| *l == JOB).ok_or("the air-assist or accessory hooks dropped the job")?;
    let (before, after) = lines.split_at(at);
    Ok((before.iter().map(|l| l.to_string()).collect(), after.iter().skip(1).map(|l| l.to_string()).collect()))
}

#[cfg(test)]
mod tests {
    use super::accessories::Accessory;
    use super::*;

    #[test]
    fn no_hooks_by_default() {
        assert_eq!(job_hooks(&AirConfig::default(), AirMode::Inherit, &AccessoryConfig::default()).unwrap(), (vec![], vec![]));
    }

    #[test]
    fn hooks_are_what_applying_them_to_a_job_adds() {
        let air = AirConfig { enabled: true, default_on: true, preflow_ms: 500, postflow_ms: 1000, ..AirConfig::default() };
        let acc = AccessoryConfig {
            outputs: vec![Accessory {
                enabled: true,
                name: "Fan".into(),
                on_command: "M106".into(),
                off_command: "M107".into(),
                ..Accessory::default()
            }],
        };
        let (pro, epi) = job_hooks(&air, AirMode::Inherit, &acc).unwrap();
        let job = "G21\nG90\nM5\nG0 X1 Y1\nM4 S100\nG1 X2 Y2 F600.0\nM5\n";
        let applied = acc.apply(&air.apply(job, AirMode::Inherit).unwrap()).unwrap();
        let mut joined = pro.join("\n");
        joined.push('\n');
        joined.push_str(job);
        joined.push_str(&epi.join("\n"));
        joined.push('\n');
        assert_eq!(joined, applied);
        assert!(pro.iter().any(|l| l == "M8") && pro.iter().any(|l| l == "M106"));
        assert!(epi.iter().any(|l| l == "M9") && epi.iter().any(|l| l == "M107"));
        // Air off for the layer: M9 before the job.
        let (pro, _) = job_hooks(&air, AirMode::Off, &acc).unwrap();
        assert!(pro.iter().any(|l| l == "M9") && !pro.iter().any(|l| l == "M8"));
    }

    #[test]
    fn refused_configurations_are_errors() {
        let air = AirConfig { enabled: true, on_command: "M8; G1 X5".into(), ..AirConfig::default() };
        assert!(job_hooks(&air, AirMode::On, &AccessoryConfig::default()).is_err());
    }
}
