//! Offline SVG-to-laser toolpaths. Supports straight SVG paths and basic primitives.
//! SVG coordinates are interpreted as millimetres; transforms and curves are rejected.
use crate::{Job,Operation,Point};
#[derive(Debug)] pub enum SvgCamError { Xml(String), Unsupported(String), Invalid(String) }
fn number(s:&str)->Result<f64,SvgCamError>{let s=s.trim().strip_suffix("mm").unwrap_or(s.trim());s.parse::<f64>().map_err(|_|SvgCamError::Invalid(format!("invalid dimension: {s}")))}
fn attr(n:roxmltree::Node<'_,'_>,key:&str)->Result<f64,SvgCamError>{number(n.attribute(key).ok_or_else(||SvgCamError::Invalid(format!("missing {key}")))?)}
fn curve_point(a:Point,b:Point,c:Point,d:Point,t:f64)->Point {let u=1.0-t;Point{x_mm:u*u*u*a.x_mm+3.0*u*u*t*b.x_mm+3.0*u*t*t*c.x_mm+t*t*t*d.x_mm,y_mm:u*u*u*a.y_mm+3.0*u*u*t*b.y_mm+3.0*u*t*t*c.y_mm+t*t*t*d.y_mm}}
fn quadratic(a:Point,b:Point,c:Point,t:f64)->Point{let u=1.0-t;Point{x_mm:u*u*a.x_mm+2.0*u*t*b.x_mm+t*t*c.x_mm,y_mm:u*u*a.y_mm+2.0*u*t*b.y_mm+t*t*c.y_mm}}
fn emit(points:&[Point],closed:bool,speed:f64,power:f64,passes:u32,out:&mut Vec<Operation>){if points.len()<2{return;}for _ in 0..passes{out.push(Operation::Travel(points[0]));for p in points.iter().skip(1){out.push(Operation::Cut{to:*p,speed_mm_min:speed,power_percent:power});}if closed{out.push(Operation::Cut{to:points[0],speed_mm_min:speed,power_percent:power});}out.push(Operation::LaserOff);}}
/// Convert supported SVG objects to GRBL-independent operations. Never connects to hardware.
pub fn svg_to_job(svg:&str,speed:f64,power:f64,passes:u32)->Result<Job,SvgCamError>{
 if !speed.is_finite()||speed<=0.0||!power.is_finite()||!(0.0..=100.0).contains(&power)||passes==0{return Err(SvgCamError::Invalid("invalid laser settings".into()));}
 let doc=roxmltree::Document::parse(svg).map_err(|e|SvgCamError::Xml(e.to_string()))?;
 let mut ops=Vec::new();
 for n in doc.descendants().filter(|n|n.is_element()){
  let tag=n.tag_name().name();
  if !matches!(tag,"rect"|"line"|"polyline"|"polygon"|"path"|"circle"|"ellipse"){continue;}
  if n.ancestors().any(|p|p.is_element() && p.attribute("transform").is_some()){return Err(SvgCamError::Unsupported("SVG transforms must be flattened first".into()));}
  let mut pts=Vec::new();let mut closed=false;
  match tag{
   "rect"=>{let x=n.attribute("x").map(number).transpose()?.unwrap_or(0.0);let y=n.attribute("y").map(number).transpose()?.unwrap_or(0.0);let w=attr(n,"width")?;let h=attr(n,"height")?;if w<=0.0||h<=0.0{return Err(SvgCamError::Invalid("invalid rectangle".into()));}pts=vec![Point{x_mm:x,y_mm:y},Point{x_mm:x+w,y_mm:y},Point{x_mm:x+w,y_mm:y+h},Point{x_mm:x,y_mm:y+h}];closed=true;},
   "line"=>{pts=vec![Point{x_mm:attr(n,"x1")?,y_mm:attr(n,"y1")?},Point{x_mm:attr(n,"x2")?,y_mm:attr(n,"y2")?}];},
   "polyline"|"polygon"=>{let raw=n.attribute("points").unwrap_or("").replace(',',' ');let nums=raw.split_whitespace().map(number).collect::<Result<Vec<_>,_>>()?;if nums.len()%2!=0{return Err(SvgCamError::Invalid("odd number of polygon coordinates".into()));}pts=nums.chunks_exact(2).map(|p|Point{x_mm:p[0],y_mm:p[1]}).collect();closed=tag=="polygon";},
   "path"=>{use svgtypes::PathSegment;let mut current=Point{x_mm:0.0,y_mm:0.0};let mut start=current;let mut sub=Vec::new();for seg in svgtypes::PathParser::from(n.attribute("d").unwrap_or("")){let seg=seg.map_err(|e|SvgCamError::Invalid(e.to_string()))?;match seg{
     PathSegment::MoveTo{abs,x,y}=>{if !sub.is_empty(){emit(&sub,false,speed,power,passes,&mut ops);sub.clear();}current=Point{x_mm:if abs{x}else{current.x_mm+x},y_mm:if abs{y}else{current.y_mm+y}};start=current;sub.push(current);},
     PathSegment::LineTo{abs,x,y}=>{current=Point{x_mm:if abs{x}else{current.x_mm+x},y_mm:if abs{y}else{current.y_mm+y}};sub.push(current);},
     PathSegment::HorizontalLineTo{abs,x}=>{current.x_mm=if abs{x}else{current.x_mm+x};sub.push(current);},
     PathSegment::VerticalLineTo{abs,y}=>{current.y_mm=if abs{y}else{current.y_mm+y};sub.push(current);},
     PathSegment::ClosePath{..}=>{emit(&sub,true,speed,power,passes,&mut ops);sub.clear();current=start;},
     PathSegment::CurveTo{abs,x1,y1,x2,y2,x,y}=>{let p1=Point{x_mm:if abs{x1}else{current.x_mm+x1},y_mm:if abs{y1}else{current.y_mm+y1}};let p2=Point{x_mm:if abs{x2}else{current.x_mm+x2},y_mm:if abs{y2}else{current.y_mm+y2}};let end=Point{x_mm:if abs{x}else{current.x_mm+x},y_mm:if abs{y}else{current.y_mm+y}};let steps=((((current.x_mm-p1.x_mm).hypot(current.y_mm-p1.y_mm)+(p1.x_mm-p2.x_mm).hypot(p1.y_mm-p2.y_mm)+(p2.x_mm-end.x_mm).hypot(p2.y_mm-end.y_mm))/0.25).ceil() as usize).clamp(4,4096);for i in 1..=steps{sub.push(curve_point(current,p1,p2,end,i as f64/steps as f64));}current=end;},
     PathSegment::Quadratic{abs,x1,y1,x,y}=>{let p1=Point{x_mm:if abs{x1}else{current.x_mm+x1},y_mm:if abs{y1}else{current.y_mm+y1}};let end=Point{x_mm:if abs{x}else{current.x_mm+x},y_mm:if abs{y}else{current.y_mm+y}};let steps=(((current.x_mm-p1.x_mm).hypot(current.y_mm-p1.y_mm)+(p1.x_mm-end.x_mm).hypot(p1.y_mm-end.y_mm))/0.25).ceil() as usize;for i in 1..=steps.clamp(4,4096){sub.push(quadratic(current,p1,end,i as f64/steps.clamp(4,4096) as f64));}current=end;},
     _=>return Err(SvgCamError::Unsupported("unsupported SVG path segment (arc/smooth curve)".into()))
    }}if !sub.is_empty(){emit(&sub,false,speed,power,passes,&mut ops);}continue;},
   "circle"|"ellipse"=>{let cx=n.attribute("cx").map(number).transpose()?.unwrap_or(0.0);let cy=n.attribute("cy").map(number).transpose()?.unwrap_or(0.0);let (rx,ry)=if tag=="circle"{let r=attr(n,"r")?;(r,r)}else{(attr(n,"rx")?,attr(n,"ry")?)};if rx<=0.0||ry<=0.0{return Err(SvgCamError::Invalid("invalid circle/ellipse".into()));}let count=(((rx.max(ry)*std::f64::consts::TAU)/0.25).ceil() as usize).clamp(16,4096);pts=(0..count).map(|i|{let t=i as f64*std::f64::consts::TAU/count as f64;Point{x_mm:cx+rx*t.cos(),y_mm:cy+ry*t.sin()}}).collect();closed=true;},
   _=>unreachable!()
  }
  emit(&pts,closed,speed,power,passes,&mut ops);
 }
 if ops.is_empty(){return Err(SvgCamError::Invalid("no supported geometry found".into()));}
 Ok(Job{operations:ops})
}
#[cfg(test)]mod tests{use super::*;#[test]fn rectangle(){let j=svg_to_job(r#"<svg xmlns="http://www.w3.org/2000/svg"><rect x="10" y="10" width="20" height="30"/></svg>"#,1000.0,30.0,1).unwrap();assert_eq!(j.operations.len(),6);}#[test]fn flatten_curve(){assert!(svg_to_job(r#"<svg><path d="M 0 0 C 1 2 3 4 5 6"/></svg>"#,1000.0,20.0,1).unwrap().operations.len()>5);}}
