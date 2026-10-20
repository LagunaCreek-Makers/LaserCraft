//! Event-driven non-safety accessory macros. Never use for gas flooding, fire suppression,
//! emergency stops, interlocks, or other hazardous actions.
use serde::{Deserialize, Serialize};
use crate::accessories::{Accessory, AccessoryConfig, AccessoryMode, AccessoryTransport};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MacroEvent { JobStarted, JobCompleted, JobCancelled, JobFault, LayerStarted, InputHigh, InputLow }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MacroAction { SetOn { device: usize }, SetOff { device: usize }, SetPwm { device: usize, percent: u8 } }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MacroRule { pub name: String, pub enabled: bool, pub event: MacroEvent, pub input_name: String, pub layer_name: String, pub actions: Vec<MacroAction> }
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MacroConfig { pub rules: Vec<MacroRule> }
#[derive(Clone, Debug)]
pub struct PlannedAction { pub device: Accessory, pub activate: bool, pub description: String }
impl MacroConfig {
 pub fn validate(&self, accessories: &AccessoryConfig) -> Result<(),String> {
  if self.rules.len()>256 { return Err("At most 256 macro rules".into()); }
  for rule in &self.rules {
   if rule.name.trim().is_empty() || rule.actions.len()>32 { return Err("Macro name required and max 32 actions".into()); }
   if matches!(rule.event,MacroEvent::InputHigh|MacroEvent::InputLow) && rule.input_name.trim().is_empty() {return Err("Input macro requires a named input".into());}
   if rule.event==MacroEvent::LayerStarted && rule.layer_name.trim().is_empty() {return Err("Layer macro requires a layer name".into());}
   for action in &rule.actions {
    let index=match action { MacroAction::SetOn{device}|MacroAction::SetOff{device}|MacroAction::SetPwm{device,..}=>*device };
    let a=accessories.outputs.get(index).ok_or("Macro references missing device")?;
    if !a.enabled || a.endpoint.transport==AccessoryTransport::Gcode {return Err("Macros require enabled independent accessory transports".into());}
    if matches!(action,MacroAction::SetPwm{..}) && a.mode!=AccessoryMode::Pwm {return Err("PWM action requires a PWM device".into());}
   }
  }
  Ok(())
 }
 pub fn plan(&self, event:MacroEvent, input:&str, layer:&str, accessories:&AccessoryConfig)->Result<Vec<PlannedAction>,String>{
  self.validate(accessories)?;
  let mut result=Vec::new();
  for rule in self.rules.iter().filter(|r|r.enabled && r.event==event) {
   if matches!(event,MacroEvent::InputHigh|MacroEvent::InputLow) && rule.input_name!=input {continue;}
   if event==MacroEvent::LayerStarted && rule.layer_name!=layer {continue;}
   for action in &rule.actions {
    let (index,activate,pwm)=match action {MacroAction::SetOn{device}=>(*device,true,None),MacroAction::SetOff{device}=>(*device,false,None),MacroAction::SetPwm{device,percent}=>(*device,*percent>0,Some(*percent))};
    let mut device=accessories.outputs[index].clone();
    if let Some(percent)=pwm {if percent>100 {return Err("PWM percent exceeds 100".into());} device.pwm_value=((device.pwm_max as u64*percent as u64)/100) as u32;}
    result.push(PlannedAction{description:format!("{}: {}",rule.name,device.name),device,activate});
   }
  }
  Ok(result)
 }
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn rejects_missing_device(){let c=MacroConfig{rules:vec![MacroRule{name:"test".into(),enabled:true,event:MacroEvent::JobStarted,input_name:String::new(),layer_name:String::new(),actions:vec![MacroAction::SetOn{device:0}]}]};assert!(c.validate(&AccessoryConfig::default()).is_err());}
 #[test] fn rejects_gcode_transport(){let mut a=Accessory::default();a.enabled=true;let c=MacroConfig{rules:vec![MacroRule{name:"test".into(),enabled:true,event:MacroEvent::JobStarted,input_name:String::new(),layer_name:String::new(),actions:vec![MacroAction::SetOn{device:0}]}]};assert!(c.validate(&AccessoryConfig{outputs:vec![a]}).is_err());}
 #[test] fn filters_event(){let c=MacroConfig::default();assert!(c.plan(MacroEvent::JobCompleted,"","",&AccessoryConfig::default()).unwrap().is_empty());}
}
