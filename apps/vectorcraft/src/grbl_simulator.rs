//! Deterministic, offline GRBL 1.1 protocol simulator for transport testing.
//! Does not drive hardware or attempt to model physical laser safety.
use crate::grbl_stream::Stream;
use std::time::Duration;
#[derive(Debug, Clone)]
pub struct SimulatedGrbl {pub state:String,pub accepted:usize,pub reject_at:Option<usize>,pub silence_at:Option<usize>}
impl Default for SimulatedGrbl {fn default()->Self{Self{state:"Idle".into(),accepted:0,reject_at:None,silence_at:None}}}
impl SimulatedGrbl {
 pub fn receive(&mut self, bytes:&str)->Option<String>{
  if bytes=="?" {return Some(format!("<{}|MPos:0.000,0.000,0.000|FS:0,0>",self.state));}
  if bytes=="!" {self.state="Hold:0".into();return None;}
  if bytes=="~" {self.state="Run".into();return None;}
  if !bytes.ends_with('\n'){return Some("error:1".into());}
  self.accepted+=1;
  if self.silence_at==Some(self.accepted){return None;}
  if self.reject_at==Some(self.accepted){return Some("error:2".into());}
  Some("ok".into())
 }
}
/// Exercise the exact conservative Stream state machine without a serial device.
pub fn run_offline(code:&str,sim:&mut SimulatedGrbl)->Result<usize,String>{
 let mut stream=Stream::new(code)?;
 while let Some(line)=stream.next_line(){
  if let Some(response)=sim.receive(&line){stream.response(&response);}else{
   stream.timed_out(Duration::ZERO);
  }
  if let Some(error)=stream.progress.failed.as_ref(){return Err(error.clone());}
 }
 if !stream.progress.finished{return Err("Simulator did not finish".into());}
 Ok(stream.progress.acknowledged)
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn complete_job(){let mut sim=SimulatedGrbl::default();assert_eq!(run_offline("G21\nG90\nG0 X10",&mut sim).unwrap(),3);}
 #[test]fn controller_error(){let mut sim=SimulatedGrbl{reject_at:Some(2),..Default::default()};assert!(run_offline("G21\nG90",&mut sim).is_err());}
 #[test]fn dropped_ack(){let mut sim=SimulatedGrbl{silence_at:Some(1),..Default::default()};assert!(run_offline("G21",&mut sim).unwrap_err().contains("timeout"));}
 #[test]fn hold_resume(){let mut sim=SimulatedGrbl::default();sim.receive("!");assert_eq!(sim.state,"Hold:0");sim.receive("~");assert_eq!(sim.state,"Run");}
}
