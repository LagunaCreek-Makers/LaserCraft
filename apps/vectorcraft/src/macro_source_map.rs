//! Convert offline interpreter provenance to visual canvas block paths.
//! A repeat iteration is provenance metadata, not a distinct editor block.
//! This module never dispatches commands to hardware.
use crate::macro_routines::Step;

pub fn canvas_path(source: &str) -> Result<Vec<usize>, String> {
    let mut result = Vec::new();
    let parts: Vec<_> = source.split('/').collect();
    if parts.is_empty() || parts.len() % 2 == 0 { return Err("Invalid source path".into()); }
    for (i, token) in parts.iter().enumerate() {
        if i % 2 == 0 {
            result.push(token.parse::<usize>().map_err(|_| format!("Invalid block index: {token}"))?);
        } else {
            let branch = match *token {
                "then" => 0,
                "else" => 1,
                t if t.starts_with("repeat[") && t.ends_with(']') => {
                    let n = &t[7..t.len()-1];
                    n.parse::<usize>().map_err(|_| "Invalid repeat iteration".to_owned())?;
                    0
                },
                _ => return Err(format!("Invalid branch: {token}")),
            };
            result.push(branch);
        }
    }
    Ok(result)
}

pub fn valid_canvas_path(steps: &[Step], path: &[usize]) -> bool {
    if path.is_empty() || path.len() % 2 == 0 { return false; }
    let mut lane = steps;
    for (i, chunk) in path.chunks(2).enumerate() {
        let Some(step) = lane.get(chunk[0]) else { return false; };
        if i == path.len()/2 { return true; }
        lane = match (step, chunk.get(1)) {
            (Step::If {then_steps,..}, Some(0)) => then_steps,
            (Step::If {else_steps,..}, Some(1)) => else_steps,
            (Step::Repeat {steps,..}, Some(0)) => steps,
            _ => return false,
        };
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn nested_then_else_and_repeat() {
        assert_eq!(canvas_path("0/then/2/repeat[3]/1").unwrap(), vec![0,0,2,0,1]);
        assert_eq!(canvas_path("1/else/0").unwrap(), vec![1,1,0]);
        assert!(canvas_path("0/repeat[abc]/0").is_err());
        assert!(canvas_path("0/then").is_err());
    }
    #[test] fn source_must_resolve_to_existing_block() {
        let blocks = vec![Step::If {condition:crate::macro_routines::Expr::True,
            then_steps:vec![Step::Repeat {times:2, steps:vec![Step::SetOn{device:1}]}],else_steps:vec![]}];
        assert!(valid_canvas_path(&blocks, &canvas_path("0/then/0/repeat[1]/0").unwrap()));
        assert!(!valid_canvas_path(&blocks, &[0,1,0]));
    }
}
