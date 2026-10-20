//! Best-effort independent accessory sequencing for supervised GRBL jobs.
//! This is NOT a safety controller. Never attach fire suppression or interlocks.
use crate::accessories::{Accessory, AccessoryConfig, AccessoryTransport, send_independent};
use std::sync::mpsc::{self,Receiver};
use std::time::Duration;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Stage { Idle, Starting, Ready, Running, Stopping, Fault }
pub struct Coordinator { pub stage:Stage, pub log:Vec<String>, pub active:Vec<Accessory>, receiver:Option<Receiver<Result<Vec<Accessory>,String>>>, pub pending_gcode:Option<String> }
impl Default for Coordinator { fn default()->Self { Self{stage:Stage::Idle,log:Vec::new(),active:Vec::new(),receiver:None,pending_gcode:None} } }
impl Coordinator {
 pub fn start(&mut self,config:&AccessoryConfig,gcode:String)->Result<(),String>{
  if self.stage!=Stage::Idle {return Err("Accessory coordinator is busy; reset after verifying outputs".into());}
  config.validate()?;
  let devices:Vec<_>=config.outputs.iter().filter(|a|a.enabled&&a.endpoint.transport!=AccessoryTransport::Gcode).cloned().collect();
  self.stage=Stage::Starting;self.pending_gcode=Some(gcode);self.log.push(format!("Starting {} independent accessories",devices.len()));
  let (tx,rx)=mpsc::channel();self.receiver=Some(rx);
  std::thread::spawn(move||{
   let mut started=Vec::new();
   for a in devices {
    if let Err(e)=send_independent(&a,true){
     // Attempt rollback, including the device whose activation failed.
     let _=send_independent(&a,false);
     for d in started.iter().rev(){let _=send_independent(d,false);}
     let _=tx.send(Err(format!("{}: {e}; rollback attempted, physical states unverified",a.name)));
     return;
    }
    std::thread::sleep(Duration::from_millis(a.pre_delay_ms.min(30000) as u64));
    started.push(a);
   }
   let _=tx.send(Ok(started));
  });Ok(())
 }
 pub fn poll(&mut self)->Option<Result<(),String>>{
  let result=self.receiver.as_ref()?.try_recv().ok()?;
  self.receiver=None;
  match result {
   Ok(active) if self.stage==Stage::Starting=>{self.active=active;self.stage=Stage::Ready;self.log.push("Accessories commanded ON; physical state unverified".into());Some(Ok(()))},
   Ok(_)=>{self.stage=Stage::Idle;self.active.clear();self.pending_gcode=None;self.log.push("Accessory OFF sequence completed; physical state unverified".into());Some(Ok(()))},
   Err(e)=>{self.stage=Stage::Fault;self.pending_gcode=None;self.log.push(e.clone());Some(Err(e))}
  }
 }
 pub fn shutdown(&mut self){
  if matches!(self.stage,Stage::Stopping|Stage::Idle){return;}
  self.stage=Stage::Stopping;self.pending_gcode=None;
  let devices=std::mem::take(&mut self.active);
  let (tx,rx)=mpsc::channel();self.receiver=Some(rx);
  std::thread::spawn(move||{
   let mut errors=Vec::new();
   for a in devices.into_iter().rev(){
    std::thread::sleep(Duration::from_millis(a.post_delay_ms.min(30000) as u64));
    if let Err(e)=send_independent(&a,false){errors.push(format!("{}: {e}",a.name));}
   }
   let _=tx.send(if errors.is_empty(){Ok(Vec::new())}else{Err(format!("OFF failures: {}",errors.join("; ")))});
  });
 }
 pub fn mark_running(&mut self){if self.stage==Stage::Ready{self.stage=Stage::Running;}}
 pub fn reset_fault(&mut self){if self.stage==Stage::Fault {self.stage=Stage::Idle;self.log.push("Fault reset by operator; confirm hardware outputs manually".into());}}
}
