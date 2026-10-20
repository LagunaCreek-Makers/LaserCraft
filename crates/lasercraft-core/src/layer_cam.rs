//! Offline color-to-operation mapping for self-contained SVG geometry.
//! Unsupported geometry and transforms fail closed.
use crate::{Job,Operation};
use crate::svg_cam::{svg_to_job,SvgCamError};
#[derive(Clone,Debug)]
pub struct ColorOperation { pub color:String,pub enabled:bool,pub speed:f64,pub power:f64,pub passes:u32 }
fn normalize_color(s:&str)->String {s.trim().to_ascii_lowercase()}
/// Extract SVG primitives in document order, associating their stroke color with a cut layer.
/// Inline `stroke` and inherited `stroke` attributes are supported; CSS classes and stylesheets
/// are rejected to prevent silent misassignment. `fill` colors are not yet mapped.
pub fn colored_svg_to_job(svg:&str,layers:&[ColorOperation])->Result<Job,SvgCamError>{
 let doc=roxmltree::Document::parse(svg).map_err(|e|SvgCamError::Xml(e.to_string()))?;
 let mut operations=Vec::<Operation>::new();
 for n in doc.descendants().filter(|n|n.is_element()) {
  if !matches!(n.tag_name().name(),"rect"|"line"|"polyline"|"polygon"|"path"|"circle"|"ellipse"){continue;}
  if n.ancestors().any(|a|a.is_element()&&(a.attribute("style").is_some()||a.attribute("class").is_some())) {
   return Err(SvgCamError::Unsupported("CSS styling is not yet supported in color mapping".into()));
  }
  let color=n.ancestors().find_map(|a|a.attribute("stroke")).unwrap_or("none");
  if color=="none" {continue;}
  let Some(layer)=layers.iter().find(|l|l.enabled&&normalize_color(&l.color)==normalize_color(color)) else {continue};
  let element=&svg[n.range()];
  let single=format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{element}</svg>");
  let job=svg_to_job(&single,layer.speed,layer.power,layer.passes)?;
  operations.extend(job.operations);
 }
 if operations.is_empty(){return Err(SvgCamError::Invalid("no matching stroked SVG geometry".into()));}
 Ok(Job{operations})
}
#[cfg(test)] mod tests {use super::*;#[test]fn colors(){let svg=r##"<svg><rect stroke="#ff0000" x="10" y="10" width="2" height="2"/><rect stroke="#0000ff" x="20" y="20" width="2" height="2"/></svg>"##;let j=colored_svg_to_job(svg,&[ColorOperation{color:"#ff0000".into(),enabled:true,speed:1200.,power:40.,passes:1}]).unwrap();assert_eq!(j.operations.len(),6);}}
