//! Bounded, offline routine interpreter. Preview only: no hardware dispatch.
//! Never use for safety interlocks, extinguishing, or gas-release systems.
use serde::{Serialize,Deserialize};
use std::collections::{BTreeMap,HashSet};
#[derive(Clone,Debug,Serialize,Deserialize)]
pub enum Expr { True, False, InputEquals{key:String,value:i64}, VariableEquals{key:String,value:i64}, And(Vec<Expr>), Or(Vec<Expr>), Not(Box<Expr>) }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub enum Step {
 SetOn{device:usize}, SetOff{device:usize}, SetPwm{device:usize,percent:u8},
 WaitMs{ms:u32}, SetVariable{name:String,value:i64},
 If{condition:Expr,then_steps:Vec<Step>,else_steps:Vec<Step>},
 Repeat{times:u16,steps:Vec<Step>}, Call{routine:String},
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Routine {pub name:String,pub steps:Vec<Step>}
#[derive(Clone,Debug,Default,Serialize,Deserialize)]
#[serde(default)]
pub struct RoutineDatabase {pub routines:Vec<Routine>, pub bindings:Vec<EventBinding>}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct EventBinding {pub event:String,pub routine:String,pub enabled:bool}
#[derive(Clone,Debug,Default)]
pub struct Context {pub inputs:BTreeMap<String,i64>,pub variables:BTreeMap<String,i64>}
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum PlannedStep {On(usize),Off(usize),Pwm(usize,u8),Wait(u32)}
fn eval(expr:&Expr,ctx:&Context)->bool {match expr {
 Expr::True=>true,Expr::False=>false,
 Expr::InputEquals{key,value}=>ctx.inputs.get(key)==Some(value),
 Expr::VariableEquals{key,value}=>ctx.variables.get(key)==Some(value),
 Expr::And(xs)=>xs.iter().all(|x|eval(x,ctx)),Expr::Or(xs)=>xs.iter().any(|x|eval(x,ctx)),Expr::Not(x)=>!eval(x,ctx)
}}
impl RoutineDatabase {
 pub fn validate(&self)->Result<(),String>{
  if self.routines.len()>256{return Err("Maximum 256 routines".into())}
  let mut names=HashSet::new();
  for r in &self.routines {if r.name.trim().is_empty()||!names.insert(&r.name){return Err("Routine names must be unique and nonempty".into())} self.validate_steps(&r.steps,0)?;}
  for r in &self.routines {self.validate_calls(&r.steps)?;}
  if self.bindings.len()>256{return Err("Maximum 256 event bindings".into())}
  for binding in &self.bindings {
   if !matches!(binding.event.as_str(),"JobStarted"|"JobCompleted"|"JobCancelled"|"JobFault"|"LayerStarted"|"InputHigh"|"InputLow"){return Err(format!("Unsupported event: {}",binding.event))}
   if !self.routines.iter().any(|r|r.name==binding.routine){return Err(format!("Binding references missing routine: {}",binding.routine))}
  }
  Ok(())
 }
 fn validate_calls(&self,steps:&[Step])->Result<(),String>{for s in steps {match s {
  Step::Call{routine}=>{if !self.routines.iter().any(|r|&r.name==routine){return Err(format!("Unknown routine: {routine}"))}},
  Step::If{then_steps,else_steps,..}=>{self.validate_calls(then_steps)?;self.validate_calls(else_steps)?},
  Step::Repeat{steps,..}=>self.validate_calls(steps)?,_=>{}
 }}Ok(())}
 fn validate_steps(&self,steps:&[Step],depth:usize)->Result<(),String>{
  if depth>12||steps.len()>256{return Err("Routine nesting or block count exceeds limit".into())}
  for s in steps {match s {
   Step::SetOn{device}|Step::SetOff{device}|Step::SetPwm{device,..} if *device>=256=>return Err("Device index exceeds 255".into()),
   Step::SetPwm{percent,..} if *percent>100=>return Err("PWM exceeds 100%".into()),
   Step::WaitMs{ms} if *ms>60000=>return Err("Wait exceeds 60 seconds".into()),
   Step::Repeat{times,steps}=>{if *times>100{return Err("Repeat exceeds 100".into())}self.validate_steps(steps,depth+1)?},
   Step::If{then_steps,else_steps,..}=>{self.validate_steps(then_steps,depth+1)?;self.validate_steps(else_steps,depth+1)?},_=>{}
  }}Ok(())
 }
 pub fn preview_event(&self,event:&str,ctx:Context)->Result<Vec<(String,Vec<PlannedStep>)>,String>{
  self.validate()?;let mut out=Vec::new();
  for b in &self.bindings {if b.enabled && b.event==event {out.push((b.routine.clone(),self.preview(&b.routine,ctx.clone())?));}}
  Ok(out)
 }
 pub fn preview(&self,name:&str,mut ctx:Context)->Result<Vec<PlannedStep>,String>{
  self.validate()?;let mut out=Vec::new();let mut stack=Vec::new();
  self.expand_routine(name,&mut ctx,&mut out,&mut stack)?;Ok(out)
 }
 fn expand_routine(&self,name:&str,ctx:&mut Context,out:&mut Vec<PlannedStep>,stack:&mut Vec<String>)->Result<(),String>{
  if stack.len()>=12||stack.iter().any(|n|n==name){return Err("Recursive routine call or call depth exceeded".into())}
  let r=self.routines.iter().find(|r|r.name==name).ok_or("Routine not found")?;
  stack.push(name.to_owned());let result=self.expand(&r.steps,ctx,out,stack);stack.pop();result
 }
 fn expand(&self,steps:&[Step],ctx:&mut Context,out:&mut Vec<PlannedStep>,stack:&mut Vec<String>)->Result<(),String>{
  for step in steps {if out.len()>=10000{return Err("Preview exceeds 10000 operations".into())}
   match step {
    Step::SetOn{device}=>out.push(PlannedStep::On(*device)),Step::SetOff{device}=>out.push(PlannedStep::Off(*device)),
    Step::SetPwm{device,percent}=>out.push(PlannedStep::Pwm(*device,*percent)),Step::WaitMs{ms}=>out.push(PlannedStep::Wait(*ms)),
    Step::SetVariable{name,value}=>{ctx.variables.insert(name.clone(),*value);},
    Step::If{condition,then_steps,else_steps}=>self.expand(if eval(condition,ctx){then_steps}else{else_steps},ctx,out,stack)?,
    Step::Repeat{times,steps}=>for _ in 0..*times{self.expand(steps,ctx,out,stack)?},
    Step::Call{routine}=>self.expand_routine(routine,ctx,out,stack)?,
   }
  }Ok(())
 }
}

/// Offline provenance for each expanded device/wait operation. Paths use
/// zero-based indices into routine steps, with branch and repeat labels.
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct TracedStep {
 pub action: PlannedStep,
 pub routine: String,
 pub source_path: String,
 pub call_stack: Vec<String>,
 pub variables: BTreeMap<String,i64>,
}
impl RoutineDatabase {
 pub fn traced_preview(&self,name:&str,mut ctx:Context)->Result<Vec<TracedStep>,String>{
  self.validate()?;
  let mut result=Vec::new(); let mut stack=Vec::new();
  self.trace_routine(name,&mut ctx,&mut result,&mut stack)?;
  Ok(result)
 }
 fn trace_routine(&self,name:&str,ctx:&mut Context,out:&mut Vec<TracedStep>,stack:&mut Vec<String>)->Result<(),String>{
  if stack.len()>=12 || stack.iter().any(|s|s==name){return Err("Recursive routine call or call depth exceeded".into());}
  let routine=self.routines.iter().find(|r|r.name==name).ok_or("Routine not found")?;
  stack.push(name.into());
  let result=self.trace_steps(&routine.steps,"",ctx,out,stack);
  stack.pop(); result
 }
 fn trace_steps(&self,steps:&[Step],prefix:&str,ctx:&mut Context,out:&mut Vec<TracedStep>,stack:&mut Vec<String>)->Result<(),String>{
  for (i,step) in steps.iter().enumerate(){
   if out.len()>=10000{return Err("Preview exceeds 10000 operations".into());}
   let path=if prefix.is_empty(){i.to_string()}else{format!("{prefix}/{i}")};
   let action=match step {
    Step::SetOn{device}=>Some(PlannedStep::On(*device)),
    Step::SetOff{device}=>Some(PlannedStep::Off(*device)),
    Step::SetPwm{device,percent}=>Some(PlannedStep::Pwm(*device,*percent)),
    Step::WaitMs{ms}=>Some(PlannedStep::Wait(*ms)),
    Step::SetVariable{name,value}=>{ctx.variables.insert(name.clone(),*value);None},
    Step::If{condition,then_steps,else_steps}=>{
     let branch=eval(condition,ctx);
     self.trace_steps(if branch{then_steps}else{else_steps},&format!("{path}/{}",if branch{"then"}else{"else"}),ctx,out,stack)?;None
    },
    Step::Repeat{times,steps}=>{for repeat in 0..*times{
     self.trace_steps(steps,&format!("{path}/repeat[{repeat}]"),ctx,out,stack)?;
    }None},
    Step::Call{routine}=>{self.trace_routine(routine,ctx,out,stack)?;None},
   };
   if let Some(action)=action {out.push(TracedStep{action,routine:stack.last().cloned().unwrap_or_default(),source_path:path,call_stack:stack.clone(),variables:ctx.variables.clone()});}
  }
  Ok(())
 }
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn branch_and_repeat(){let db=RoutineDatabase{bindings:vec![],routines:vec![Routine{name:"test".into(),steps:vec![Step::If{condition:Expr::True,then_steps:vec![Step::Repeat{times:3,steps:vec![Step::SetOn{device:2}]}],else_steps:vec![]}]}]};assert_eq!(db.preview("test",Context::default()).unwrap(),vec![PlannedStep::On(2);3]);}
 #[test]fn reject_recursion(){let db=RoutineDatabase{bindings:vec![],routines:vec![Routine{name:"x".into(),steps:vec![Step::Call{routine:"x".into()}]}]};assert!(db.preview("x",Context::default()).is_err());}
 #[test]fn reject_unbounded(){let db=RoutineDatabase{bindings:vec![],routines:vec![Routine{name:"x".into(),steps:vec![Step::Repeat{times:101,steps:vec![]}]}]};assert!(db.validate().is_err());}
}
#[cfg(test)] mod phase37_tests {
 use super::*;
 #[test] fn nested_conditions_and_waits() {
  let db=RoutineDatabase{bindings:vec![],routines:vec![Routine{name:"start".into(),steps:vec![
   Step::SetVariable{name:"ready".into(),value:1},
   Step::If{condition:Expr::And(vec![Expr::VariableEquals{key:"ready".into(),value:1},Expr::InputEquals{key:"door_closed".into(),value:1}]),then_steps:vec![Step::Repeat{times:2,steps:vec![Step::SetPwm{device:4,percent:40},Step::WaitMs{ms:250}]}],else_steps:vec![Step::SetOff{device:4}]}
  ]}]};
  let mut ctx=Context::default();ctx.inputs.insert("door_closed".into(),1);
  assert_eq!(db.preview("start",ctx).unwrap(),vec![PlannedStep::Pwm(4,40),PlannedStep::Wait(250),PlannedStep::Pwm(4,40),PlannedStep::Wait(250)]);
 }
 #[test] fn routine_json_round_trip() {
  let db=RoutineDatabase{bindings:vec![],routines:vec![Routine{name:"delay".into(),steps:vec![Step::WaitMs{ms:500}]}]};
  let json=serde_json::to_string_pretty(&db).unwrap();
  let restored:RoutineDatabase=serde_json::from_str(&json).unwrap();
  assert_eq!(restored.preview("delay",Context::default()).unwrap(),vec![PlannedStep::Wait(500)]);
 }
}

#[cfg(test)] mod phase38_tests {
 use super::*;
 #[test] fn event_binding_preview() {
  let db=RoutineDatabase {routines:vec![Routine{name:"running".into(),steps:vec![Step::SetOn{device:0}]}],bindings:vec![EventBinding{event:"JobStarted".into(),routine:"running".into(),enabled:true}]};
  assert_eq!(db.preview_event("JobStarted",Context::default()).unwrap()[0].1,vec![PlannedStep::On(0)]);
  assert!(db.preview_event("JobCompleted",Context::default()).unwrap().is_empty());
 }
 #[test] fn invalid_binding_is_rejected() {
  let db=RoutineDatabase{routines:vec![],bindings:vec![EventBinding{event:"JobStarted".into(),routine:"missing".into(),enabled:true}]};
  assert!(db.validate().is_err());
 }
}

#[cfg(test)] mod phase46_tests {
 use super::*;
 #[test] fn traces_nested_calls_and_variables() {
  let db=RoutineDatabase{bindings:vec![],routines:vec![
   Routine{name:"main".into(),steps:vec![Step::SetVariable{name:"level".into(),value:3},Step::Call{routine:"child".into()}]},
   Routine{name:"child".into(),steps:vec![Step::If{condition:Expr::VariableEquals{key:"level".into(),value:3},then_steps:vec![Step::SetOn{device:1}],else_steps:vec![Step::SetOff{device:1}]}]}]};
  let t=db.traced_preview("main",Context::default()).unwrap();
  assert_eq!(t.len(),1);assert_eq!(t[0].source_path,"0/then/0");
  assert_eq!(t[0].call_stack,vec!["main","child"]);
  assert_eq!(t[0].variables.get("level"),Some(&3));
  assert_eq!(t[0].action,PlannedStep::On(1));
 }
 #[test] fn trace_matches_expanded_actions() {
  let db=RoutineDatabase{bindings:vec![],routines:vec![Routine{name:"loop".into(),steps:vec![Step::Repeat{times:2,steps:vec![Step::WaitMs{ms:50}]}]}]};
  let trace=db.traced_preview("loop",Context::default()).unwrap();
  assert_eq!(trace.iter().map(|x|x.action.clone()).collect::<Vec<_>>(),db.preview("loop",Context::default()).unwrap());
  assert_ne!(trace[0].source_path,trace[1].source_path);
 }
}
