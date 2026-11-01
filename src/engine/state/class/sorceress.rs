//! Source-confirmed research state. Absolute gain/drain rates remain external.
use super::ClassState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Phase { Normal, Enhanced, Released }

#[derive(Debug, Clone, Serialize)]
pub struct SorceressState {
    pub mp:f64, pub base_max_mp:f64, pub max_mp:f64,
    pub magic:f64, pub ultimate_point:f64, pub phase:Phase, pub shield:bool,
}
impl SorceressState {
    pub fn new(max_mp:f64,mp:f64,magic:f64,ultimate_point:f64)->Result<Self,String>{
        if !max_mp.is_finite()||max_mp<=0.||!mp.is_finite()||mp<0.||mp>max_mp||
            !magic.is_finite()||!(0. ..=10000.).contains(&magic)||
            !ultimate_point.is_finite()||!(0. ..=10000.).contains(&ultimate_point){return Err("invalid explicit resource snapshot".into());}
        Ok(Self{mp,base_max_mp:max_mp,max_mp,magic,ultimate_point,phase:Phase::Normal,shield:false})
    }
    pub fn gain_magic(&mut self,amount:f64)->Result<(),String>{
        if !amount.is_finite()||amount<0.{return Err("invalid explicit magic gain".into());}
        if self.phase==Phase::Normal{self.magic=(self.magic+amount).min(10000.);}
        Ok(())
    }
    /// Returns true only for the full-gauge entry (eligible cooldowns ×0.75).
    pub fn activate(&mut self)->Result<bool,String>{
        if self.phase!=Phase::Normal||self.magic<3000.{return Err("magic entry requires normal phase and >=3000".into());}
        let released=self.magic==10000.;self.phase=if released{Phase::Released}else{Phase::Enhanced};Ok(released)
    }
    pub fn drain_magic(&mut self,amount:f64)->Result<(),String>{
        if self.phase==Phase::Normal||!amount.is_finite()||amount<0.{return Err("explicit drain requires active phase".into());}
        self.magic=(self.magic-amount).max(0.);if self.magic==0.{self.phase=Phase::Normal;}Ok(())
    }
    pub fn manual_end(&mut self)->Result<(),String>{
        if self.phase==Phase::Normal{return Err("magic release is not active".into());}
        self.magic=0.;self.phase=Phase::Normal;Ok(())
    }
    pub fn blink(&mut self)->Result<(),String>{
        if self.magic<3000.{return Err("blink requires >=3000".into());}
        self.magic-=3000.;if self.magic==0.{self.phase=Phase::Normal;}Ok(())
    }
    /// No elapsed-time regeneration. Caller supplies the actual MP event.
    pub fn mp_event(&mut self,delta:f64)->Result<(),String>{
        if !delta.is_finite(){return Err("invalid MP delta".into());}
        self.mp=(self.mp+delta).clamp(0.,self.max_mp);
        if self.shield&&self.mp<=self.max_mp*0.2 {self.shield=false;self.max_mp=self.base_max_mp;self.mp=self.mp.min(self.max_mp);}
        Ok(())
    }
    /// Max-MP modifier is known; the current-MP response must be supplied as a
    /// snapshot instead of guessing whether the game preserves ratio or amount.
    pub fn start_shield(&mut self,max_mp_bonus:bool,mp_snapshot:f64)->Result<(),String>{
        let cap=self.base_max_mp*if max_mp_bonus{1.12}else{1.};
        if self.shield||!mp_snapshot.is_finite()||mp_snapshot<=cap*0.2||mp_snapshot>cap{return Err("invalid shield activation MP snapshot".into());}
        self.shield=true;self.max_mp=cap;self.mp=mp_snapshot;Ok(())
    }
    pub fn end_shield(&mut self,restore:bool)->Result<(),String>{
        if !self.shield{return Err("shield is not active".into());}
        self.shield=false;self.max_mp=self.base_max_mp;self.mp=self.mp.min(self.max_mp);
        if restore{self.mp_event(self.max_mp*0.1)?;}Ok(())
    }
}
impl ClassState for SorceressState {
    fn clone_box(&self)->Box<dyn ClassState>{Box::new(self.clone())}
    fn get_resource(&self,name:&str)->f64{match name{
        "Mp"=>self.mp,"Magic"=>self.magic,"UltimatePoint"=>self.ultimate_point,
        "MagicNormal"=>(self.phase==Phase::Normal) as u8 as f64,
        "MagicEnhanced"=>(self.phase==Phase::Enhanced) as u8 as f64,
        "MagicReleased"=>(self.phase==Phase::Released) as u8 as f64,_=>0.}}
    fn get_resource_max(&self,name:&str)->f64{match name{"Mp"=>self.max_mp,"Magic"|"UltimatePoint"=>10000.,_=>0.}}
    fn set_resource(&mut self,name:&str,value:f64){
        if !value.is_finite(){return;}
        match name{
            "Mp"=>{let _=self.mp_event(value-self.mp);},
            "UltimatePoint"=>self.ultimate_point=value.clamp(0.,10000.),
            // Engine gain callbacks cannot replenish active magic.
            "Magic"=>{if value>self.magic{let _=self.gain_magic(value-self.magic);}else{self.magic=value.clamp(0.,10000.);if self.magic==0.{self.phase=Phase::Normal;}}},
            _=>{}
        }
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn entry_thresholds_gain_block_and_reset(){
        for (g,phase) in [(2999.,None),(3000.,Some(Phase::Enhanced)),(9999.,Some(Phase::Enhanced)),(10000.,Some(Phase::Released))]{
            let mut s=SorceressState::new(100.,100.,g,0.).unwrap();
            assert_eq!(s.activate().is_ok(),phase.is_some());if let Some(p)=phase{assert_eq!(s.phase,p);s.gain_magic(10000.).unwrap();assert_eq!(s.magic,g);s.manual_end().unwrap();assert_eq!(s.magic,0.);}
        }
    }
    #[test] fn blink_cost_and_explicit_drain(){
        let mut s=SorceressState::new(100.,100.,10000.,0.).unwrap();s.activate().unwrap();s.blink().unwrap();assert_eq!(s.magic,7000.);s.drain_magic(6999.).unwrap();assert_eq!(s.phase,Phase::Released);s.drain_magic(1.).unwrap();assert_eq!(s.phase,Phase::Normal);assert!(s.blink().is_err());
    }
    #[test] fn shield_explicit_snapshot_threshold_and_manual_refund(){
        let mut s=SorceressState::new(100.,80.,0.,0.).unwrap();s.start_shield(true,80.).unwrap();assert!((s.max_mp-112.).abs()<1e-12);s.mp_event(-57.6).unwrap();assert!(!s.shield);assert_eq!(s.max_mp,100.);
        s.start_shield(false,80.).unwrap();s.end_shield(true).unwrap();assert_eq!(s.mp,90.);
    }
    #[test] fn invalid_external_values_are_rejected_without_mutation(){
        let mut s=SorceressState::new(100.,50.,5000.,0.).unwrap();assert!(s.gain_magic(f64::NAN).is_err());assert!(s.mp_event(f64::INFINITY).is_err());assert!(s.drain_magic(1.).is_err());assert!(s.start_shield(true,0.).is_err());assert_eq!(s.mp,50.);assert_eq!(s.magic,5000.);
    }
}
