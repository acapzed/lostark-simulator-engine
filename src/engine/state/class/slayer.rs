//! Confirmed identity transitions only. Missing ordinary gain/drain/durations are
//! deliberately supplied as external research events, never inferred here.
use serde::{Deserialize,Serialize};

#[derive(Debug,Clone,Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Rules {
    pub gauge_max:f64,
    pub attack_speed_bonus:f64,
    pub move_speed_bonus:f64,
    pub crit_rate_bonus:f64,
    pub absorption_cooldown_ms:u64,
    pub fatigue_interval_ms:u64,
    pub fatigue_max:u32,
    pub drain_increase_per_fatigue:f64,
}
#[derive(Debug,Clone,Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Setup {
    pub mode:String,
    pub gauge_restore_fraction:f64,
    pub mana_restore_fraction:f64,
    pub punisher_damage_bonus:f64,
    pub no_exhaustion:bool,
    pub refill_after_exhaustion:bool,
    pub max_mp:f64,
    pub initial_mp:f64,
    #[serde(default)]
    pub enhancement:Option<Enhancement>,
}
#[derive(Debug,Clone,Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Enhancement {
    pub crit_damage_bonus:f64,pub manual_exit_allowed:bool,
    pub fixed_exhaustion_ms:Option<u64>,pub fixed_exhaustion_max_fatigue:Option<u32>,
    pub enhancement_max_stacks:u32,pub enhancement_damage_per_stack:f64,
}
#[derive(Debug,Clone,Copy,PartialEq,Deserialize,Serialize)]
#[serde(rename_all="snake_case")]
pub enum Phase {Normal,Burst,Exhaustion}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Snapshot {
    pub time_ms:u64,pub phase:Phase,pub fury:f64,pub mp:f64,pub fatigue:u32,
    pub bloodlust_available:bool,pub ready_to_activate:bool,
    pub attack_speed_bonus:f64,pub move_speed_bonus:f64,pub crit_rate_bonus:f64,
    pub damage_bonus:f64,pub drain_multiplier:f64,
    pub absolute_gain_bound:bool,pub automatic_drain_bound:bool,pub exhaustion_duration_bound:bool,
    pub crit_damage_bonus:f64,pub enhancement_stacks:u32,pub enhancement_damage_bonus:f64,
    pub manual_exit_available:bool,pub exhaustion_until_ms:Option<u64>,
}
#[derive(Debug,Clone)]
pub struct SlayerState {
    rules:Rules,setup:Setup,now:u64,phase:Phase,fury:f64,mp:f64,fatigue:u32,
    burst_started:u64,last_absorption:Option<u64>,
    enhancement_stacks:u32,exhaustion_until:Option<u64>,
}
impl SlayerState {
    pub fn new(rules:Rules,setup:Setup)->Result<Self,String>{
        if !matches!(setup.mode.as_str(),"baseline"|"predator"|"punisher") ||
            !rules.gauge_max.is_finite() || rules.gauge_max<=0. || rules.fatigue_interval_ms==0 ||
            rules.absorption_cooldown_ms==0 || rules.fatigue_max==0 ||
            !setup.max_mp.is_finite() || setup.max_mp<=0. || !setup.initial_mp.is_finite() ||
            setup.initial_mp<0. || setup.initial_mp>setup.max_mp ||
            [setup.gauge_restore_fraction,setup.mana_restore_fraction].iter().any(|v|!v.is_finite()||!(0.0..=1.0).contains(v)) ||
            [rules.attack_speed_bonus,rules.move_speed_bonus,rules.crit_rate_bonus,rules.drain_increase_per_fatigue,setup.punisher_damage_bonus].iter().any(|v|!v.is_finite()||*v<0.) {
            return Err("invalid identity research setup".into());
        }
        if let Some(e)=&setup.enhancement {
            if setup.mode!="predator" || !e.crit_damage_bonus.is_finite() || e.crit_damage_bonus<0. ||
                !e.enhancement_damage_per_stack.is_finite() || e.enhancement_damage_per_stack<0. ||
                e.fixed_exhaustion_ms.is_some()!=e.fixed_exhaustion_max_fatigue.is_some() || e.fixed_exhaustion_ms==Some(0) {
                return Err("invalid predator enhancement".into());
            }
        }
        Ok(Self{mp:setup.initial_mp,rules,setup,now:0,phase:Phase::Normal,fury:0.,fatigue:0,burst_started:0,last_absorption:None,enhancement_stacks:0,exhaustion_until:None})
    }
    pub fn advance(&mut self,at:u64)->Result<(),String>{
        if at<self.now{return Err("identity time cannot move backwards".into());}
        self.now=at;
        if self.phase==Phase::Exhaustion && self.exhaustion_until.is_some_and(|until|at>=until){
            self.external_exhaustion_complete();
        }
        if self.phase==Phase::Burst && self.setup.mode=="predator" {
            self.fatigue=((at-self.burst_started)/self.rules.fatigue_interval_ms).min(self.rules.fatigue_max as u64) as u32;
        }
        Ok(())
    }
    /// Quantity is an explicitly labelled external probe input, not skill data.
    pub fn external_normal_gain(&mut self,amount:f64)->Result<bool,String>{
        if !amount.is_finite()||amount<0.{return Err("invalid external gauge amount".into());}
        if self.phase!=Phase::Normal{return Ok(false);}
        self.fury=(self.fury+amount).min(self.rules.gauge_max);Ok(true)
    }
    pub fn activate(&mut self)->bool{
        if self.phase!=Phase::Normal||self.fury<self.rules.gauge_max{return false;}
        self.phase=Phase::Burst;self.burst_started=self.now;self.fatigue=0;true
    }
    /// Absorption itself is known. It is independent of ordinary skill gain.
    pub fn landed_hit(&mut self)->bool{
        if self.setup.mode!="predator"||self.phase==Phase::Exhaustion||
            self.last_absorption.is_some_and(|at|self.now-at<self.rules.absorption_cooldown_ms){return false;}
        self.last_absorption=Some(self.now);
        self.fury=(self.fury+self.rules.gauge_max*self.setup.gauge_restore_fraction).min(self.rules.gauge_max);
        self.mp=(self.mp+self.setup.max_mp*self.setup.mana_restore_fraction).min(self.setup.max_mp);
        if self.phase==Phase::Burst {
            if let Some(e)=&self.setup.enhancement{self.enhancement_stacks=(self.enhancement_stacks+1).min(e.enhancement_max_stacks);}
        }
        true
    }
    /// Caller supplies the missing depletion event. No guessed duration or tick.
    pub fn external_depletion(&mut self)->bool{
        if self.phase!=Phase::Burst{return false;}
        self.fury=0.;self.enhancement_stacks=0;
        self.phase=if self.setup.no_exhaustion{Phase::Normal}else{Phase::Exhaustion};
        self.exhaustion_until=if self.phase==Phase::Exhaustion {
            self.setup.enhancement.as_ref().and_then(|e|
                e.fixed_exhaustion_max_fatigue.filter(|cap|self.fatigue<=*cap).and(e.fixed_exhaustion_ms))
                .map(|ms|self.now.saturating_add(ms))
        }else{None};true
    }
    pub fn manual_exit(&mut self)->bool{
        if !self.setup.enhancement.as_ref().is_some_and(|e|e.manual_exit_allowed){return false;}
        self.external_depletion()
    }
    /// Caller supplies the missing exhaustion deadline. A no-exhaustion path
    /// never emits exhaustion-complete and cannot accidentally trigger refill.
    pub fn external_exhaustion_complete(&mut self)->bool{
        if self.phase!=Phase::Exhaustion{return false;}
        if self.exhaustion_until.is_some_and(|until|self.now<until){return false;}
        self.exhaustion_until=None;
        self.phase=Phase::Normal;self.fatigue=0;
        if self.setup.refill_after_exhaustion{self.fury=self.rules.gauge_max;}true
    }
    pub fn snapshot(&self)->Snapshot{
        let burst=self.phase==Phase::Burst;
        Snapshot{time_ms:self.now,phase:self.phase,fury:self.fury,mp:self.mp,fatigue:self.fatigue,
            bloodlust_available:burst&&self.setup.mode!="predator",ready_to_activate:self.phase==Phase::Normal&&self.fury>=self.rules.gauge_max,
            attack_speed_bonus:if burst{self.rules.attack_speed_bonus}else{0.},move_speed_bonus:if burst{self.rules.move_speed_bonus}else{0.},
            crit_rate_bonus:if burst{self.rules.crit_rate_bonus}else{0.},damage_bonus:if burst&&self.setup.mode=="punisher"{self.setup.punisher_damage_bonus}else{0.},
            drain_multiplier:if burst&&self.setup.mode=="predator"{1.+self.fatigue as f64*self.rules.drain_increase_per_fatigue}else{1.},
            absolute_gain_bound:false,automatic_drain_bound:false,exhaustion_duration_bound:self.exhaustion_until.is_some(),
            exhaustion_until_ms:self.exhaustion_until,
            crit_damage_bonus:if burst{self.setup.enhancement.as_ref().map_or(0.,|e|e.crit_damage_bonus)}else{0.},
            enhancement_stacks:self.enhancement_stacks,
            enhancement_damage_bonus:self.setup.enhancement.as_ref().map_or(0.,|e|self.enhancement_stacks as f64*e.enhancement_damage_per_stack),
            manual_exit_available:burst&&self.setup.enhancement.as_ref().is_some_and(|e|e.manual_exit_allowed)}
    }
    pub fn duration_multiplier(&self)->f64{if self.setup.mode=="punisher"{0.5}else{1.}}
}
