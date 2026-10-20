//! Offline geometry helpers. Coordinates are millimetres; no device I/O.
use crate::{Job,Operation,Point};
#[derive(Clone,Copy,Debug)]
pub struct JobStats { pub cut_mm:f64, pub travel_mm:f64, pub cut_minutes:f64, pub segments:usize }
fn distance(a:Point,b:Point)->f64 {((a.x_mm-b.x_mm).powi(2)+(a.y_mm-b.y_mm).powi(2)).sqrt()}
pub fn statistics(job:&Job)->JobStats {
 let mut s=JobStats{cut_mm:0.0,travel_mm:0.0,cut_minutes:0.0,segments:0};let mut previous=None;
 for op in &job.operations {match op {
  Operation::Travel(p)=>{if let Some(a)=previous{s.travel_mm+=distance(a,*p);}previous=Some(*p);},
  Operation::Cut{to,speed_mm_min,..}=>{if let Some(a)=previous{let d=distance(a,*to);s.cut_mm+=d;if *speed_mm_min>0.0{s.cut_minutes+=d/speed_mm_min;}s.segments+=1;}previous=Some(*to);},
  Operation::LaserOff=>{}
 }}s
}
/// Alternating horizontal scanlines clipped to a rectangular region. Laser stays off on travel.
pub fn fill_rectangle(x:f64,y:f64,w:f64,h:f64,interval:f64,speed:f64,power:f64)->Result<Job,String>{
 if ![x,y,w,h,interval,speed,power].iter().all(|v|v.is_finite())||w<=0.0||h<=0.0||interval<=0.0||speed<=0.0||!(0.0..=100.0).contains(&power){return Err("invalid fill parameters".into());}
 let rows=(h/interval).floor() as usize + 1;if rows>100_000{return Err("fill exceeds 100000 scanlines".into());}
 let mut ops=Vec::with_capacity(rows*3);
 for i in 0..rows {let yy=(y+i as f64*interval).min(y+h);let (start,end)=if i%2==0 {(x,x+w)}else{(x+w,x)};
 ops.push(Operation::Travel(Point{x_mm:start,y_mm:yy}));ops.push(Operation::Cut{to:Point{x_mm:end,y_mm:yy},speed_mm_min:speed,power_percent:power});ops.push(Operation::LaserOff);
 }Ok(Job{operations:ops})
}
#[cfg(test)]mod tests{use super::*;#[test]fn fill(){let j=fill_rectangle(0.0,0.0,10.0,2.0,1.0,1000.0,20.0).unwrap();assert_eq!(statistics(&j).segments,3);}}
