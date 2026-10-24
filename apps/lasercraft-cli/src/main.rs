use lasercraft_core::{Job, Machine, Operation, Point};
use std::{env, process};

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 8 || args[1] != "rectangle" {
        return Err(
            "Usage: lasercraft-cli rectangle WIDTH_MM HEIGHT_MM SPEED_MM_MIN POWER_PERCENT MACHINE_WIDTH_MM MACHINE_HEIGHT_MM > job.gcode".into()
        );
    }
    let vals: Vec<f64> = args[2..].iter().map(|s| s.parse::<f64>().map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
    let (w, h, speed, power, mw, mh) = (vals[0], vals[1], vals[2], vals[3], vals[4], vals[5]);
    if w <= 0.0 || h <= 0.0 {
        return Err("rectangle dimensions must be positive".into());
    }
    let job = Job {
        operations: vec![
            Operation::Travel(Point { x_mm: 0.0, y_mm: 0.0 }),
            Operation::Cut { to: Point { x_mm: w, y_mm: 0.0 }, speed_mm_min: speed, power_percent: power },
            Operation::Cut { to: Point { x_mm: w, y_mm: h }, speed_mm_min: speed, power_percent: power },
            Operation::Cut { to: Point { x_mm: 0.0, y_mm: h }, speed_mm_min: speed, power_percent: power },
            Operation::Cut { to: Point { x_mm: 0.0, y_mm: 0.0 }, speed_mm_min: speed, power_percent: power },
            Operation::LaserOff,
        ],
    };
    let code = job.to_grbl(Machine { width_mm: mw, height_mm: mh, max_s: 1000 }).map_err(|e| format!("job validation failed: {e:?}"))?;
    print!("{code}");
    Ok(())
}
fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        process::exit(2);
    }
}
