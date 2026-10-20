//! Independent camera-to-bed validation points. No automatic detection or machine motion.
use crate::camera_calibration::Homography;
use crate::camera_lens::Lens;

#[derive(Clone, Copy, Debug)]
pub struct CheckPoint { pub bed_x_mm:f64, pub bed_y_mm:f64, pub image_u:f64, pub image_v:f64 }
#[derive(Clone, Debug)]
pub struct Quality { pub count:usize, pub rms_px:f64, pub max_px:f64, pub rms_mm:f64, pub max_mm:f64 }

/// CSV header: bed_x_mm,bed_y_mm,image_u,image_v (image coordinates normalized 0..1).
pub fn parse_points(csv:&str)->Result<Vec<CheckPoint>,String> {
 let mut points=Vec::new();
 for (i,line) in csv.lines().enumerate() {
  let line=line.trim();if line.is_empty()||line.starts_with('#')||line.starts_with("bed_x_mm") {continue;}
  let parts:Vec<_>=line.split(',').map(str::trim).collect();
  if parts.len()!=4{return Err(format!("Line {}: expected 4 columns",i+1));}
  let mut values=[0.0;4];
  for (j,part) in parts.iter().enumerate(){values[j]=part.parse::<f64>().map_err(|_|format!("Line {}: invalid number",i+1))?;}
  if values.iter().any(|v|!v.is_finite())||!(0.0..=1.0).contains(&values[2])||!(0.0..=1.0).contains(&values[3]) {return Err(format!("Line {}: invalid coordinates",i+1));}
  points.push(CheckPoint{bed_x_mm:values[0],bed_y_mm:values[1],image_u:values[2],image_v:values[3]});
 }
 if points.len()<3 {return Err("At least three independent validation points required".into());}
 Ok(points)
}
pub fn evaluate(h:&Homography,lens:Option<&Lens>,points:&[CheckPoint],image_width:usize,image_height:usize)->Result<Quality,String>{
 if image_width==0||image_height==0||points.len()<3{return Err("Image dimensions and at least three points required".into());}
 let mut sum_px=0.0;let mut sum_mm=0.0;let mut max_px:f64=0.0;let mut max_mm:f64=0.0;
 for p in points {
  let (mut u,mut v)=h.project(p.bed_x_mm,p.bed_y_mm).ok_or("Invalid forward mapping")?;
  if let Some(l)=lens {(u,v)=l.distort(u,v).ok_or("Invalid lens mapping")?;}
  let px=(((u-p.image_u)*image_width as f64).powi(2)+((v-p.image_v)*image_height as f64).powi(2)).sqrt();
  let (ou,ov)=if let Some(l)=lens {l.undistort(p.image_u,p.image_v).ok_or("Lens inversion failed")?}else{(p.image_u,p.image_v)};
  let (x,y)=h.unproject(ou,ov).ok_or("Inverse mapping failed")?;
  let mm=((x-p.bed_x_mm).powi(2)+(y-p.bed_y_mm).powi(2)).sqrt();
  sum_px+=px*px;sum_mm+=mm*mm;max_px=max_px.max(px);max_mm=max_mm.max(mm);
 }
 let n=points.len() as f64;
 Ok(Quality{count:points.len(),rms_px:(sum_px/n).sqrt(),max_px,rms_mm:(sum_mm/n).sqrt(),max_mm})
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn parses_and_checks(){let p=parse_points("bed_x_mm,bed_y_mm,image_u,image_v\n10,10,0.1,0.1\n20,20,0.2,0.2\n30,30,0.3,0.3").unwrap();assert_eq!(p.len(),3);}
 #[test]fn rejects_bad(){assert!(parse_points("1,2,3").is_err());}
 #[test]fn zero_error(){let h=Homography::from_bed_corners(&[[0.,0.],[1.,0.],[1.,1.],[0.,1.]],100.,100.).unwrap();let p=vec![CheckPoint{bed_x_mm:10.,bed_y_mm:10.,image_u:0.1,image_v:0.1},CheckPoint{bed_x_mm:50.,bed_y_mm:50.,image_u:0.5,image_v:0.5},CheckPoint{bed_x_mm:90.,bed_y_mm:90.,image_u:0.9,image_v:0.9}];let q=evaluate(&h,None,&p,1000,1000).unwrap();assert!(q.rms_mm<1e-6);}
}
