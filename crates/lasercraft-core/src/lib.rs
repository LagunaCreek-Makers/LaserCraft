//! LaserCraft initial controller-independent laser job planner.
//! No hardware communication is implemented in this starter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point { pub x_mm: f64, pub y_mm: f64 }
#[derive(Clone, Debug)]
pub enum Operation {
    Travel(Point),
    Cut { to: Point, speed_mm_min: f64, power_percent: f64 },
    LaserOff,
}
#[derive(Clone, Debug)]
pub struct Job { pub operations: Vec<Operation> }
#[derive(Clone, Copy, Debug)]
pub struct Machine { pub width_mm: f64, pub height_mm: f64, pub max_s: u32 }
#[derive(Debug, PartialEq)]
pub enum PlanError { OutOfBounds, InvalidParameter }
impl Job {
    pub fn validate(&self, machine: Machine) -> Result<(), PlanError> {
        if !machine.width_mm.is_finite() || !machine.height_mm.is_finite() || machine.width_mm <= 0.0 || machine.height_mm <= 0.0 || machine.max_s == 0 { return Err(PlanError::InvalidParameter); }
        for op in &self.operations {
            let p = match op { Operation::Travel(p) => p, Operation::Cut {to, speed_mm_min, power_percent} => {
                if !speed_mm_min.is_finite() || *speed_mm_min <= 0.0 || !power_percent.is_finite() || !(0.0..=100.0).contains(power_percent) { return Err(PlanError::InvalidParameter); }
                to
            }, Operation::LaserOff => continue };
            if !p.x_mm.is_finite() || !p.y_mm.is_finite() { return Err(PlanError::InvalidParameter); }
            if p.x_mm < 0.0 || p.y_mm < 0.0 || p.x_mm > machine.width_mm || p.y_mm > machine.height_mm { return Err(PlanError::OutOfBounds); }
        }
        Ok(())
    }
    /// Export GRBL laser-mode G-code; assumes $32=1 and calibrated S range.
    /// Review offline before sending to hardware.
    pub fn to_grbl(&self, machine: Machine) -> Result<String, PlanError> {
        self.validate(machine)?;
        let mut out = String::from("G21\nG90\nM5\n");
        for op in &self.operations {
            match op {
                Operation::Travel(p) => out.push_str(&format!("M5\nG0 X{:.3} Y{:.3}\n", p.x_mm, p.y_mm)),
                Operation::Cut {to, speed_mm_min, power_percent} => {
                    let s = (*power_percent / 100.0 * f64::from(machine.max_s)).round() as u32;
                    out.push_str(&format!("M4 S{}\nG1 X{:.3} Y{:.3} F{:.1}\n", s, to.x_mm, to.y_mm, speed_mm_min));
                },
                Operation::LaserOff => out.push_str("M5\n"),
            }
        }
        out.push_str("M5\n");
        Ok(out)
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn export_job() {
        let m = Machine {width_mm: 300.0, height_mm: 200.0, max_s: 1000};
        let j = Job {operations: vec![Operation::Travel(Point{x_mm: 1.0,y_mm: 2.0}), Operation::Cut{to: Point{x_mm: 10.0,y_mm: 20.0}, speed_mm_min: 1200.0,power_percent: 50.0}]};
        let g = j.to_grbl(m).unwrap(); assert!(g.contains("M4 S500")); assert!(g.ends_with("M5\n"));
    }
    #[test] fn reject_out_of_bounds() {
        let j = Job {operations: vec![Operation::Travel(Point{x_mm: -1.0,y_mm: 0.0})]};
        assert_eq!(j.validate(Machine{width_mm: 100.0,height_mm: 100.0,max_s: 1000}),Err(PlanError::OutOfBounds));
    }
}

pub mod svg_cam;

pub mod advanced_cam;
pub mod layer_cam;
