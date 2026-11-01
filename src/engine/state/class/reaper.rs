use super::ClassState;
use crate::profile::reaper::ReaperScenario;

pub const PERSONA: u32 = 26040;
pub const SHADOW_DOT: u32 = 26070;
pub const DISTORTION: u32 = 26510;
pub const RAGE_SPEAR: u32 = 26810;
pub const SILENT_SMASHER: u32 = 26820;
pub const FATAL_STEP: u32 = 26980;
pub const NIGHTMARE: u32 = 26090;
pub const NIGHTMARE_RETURN: u32 = 26100;

pub fn is_rush(game_id: u32) -> bool { matches!(game_id, 26800 | RAGE_SPEAR | SILENT_SMASHER | 26830 | 26970) }

#[derive(Debug, Clone, Copy)]
pub struct ReaperBoost {
    pub damage: f64,
    pub attack_power: f64,
    pub crit_rate: f64,
    pub attack_speed: f64,
    pub crit_damage: f64,
}

impl Default for ReaperBoost {
    fn default() -> Self {
        Self { damage: 1.0, attack_power: 1.0, crit_rate: 0.0, attack_speed: 0.0, crit_damage: 0.0 }
    }
}

#[derive(Debug, Clone)]
pub struct ReaperState {
    pub scenario: ReaperScenario,
    mp: f64,
    max_mp: f64,
    pub dark: f64,
    pub heat: f64,
    pub persona: bool,
    pub chaos_until: u64,
    pub fatal_until: u64,
    pub weakness: u32,
    pub blade: bool,
    pub now: u64,
    shadow_dot_gain_multiplier: f64,
    ultimate_point: f64,
    pub nightmare_until: u64,
    selected_passives: Option<serde_json::Value>,
    handwork_stacks: u32,
    handwork_until: u64,
    identity_model: Option<crate::profile::reaper::ReaperIdentityModel>,
    chaos_tripod_damage: std::collections::HashMap<u32, f64>,
    identity_tripod_gain: std::collections::HashMap<u32, f64>,
    identity_grid_multiplier: f64,
}

impl ReaperState {
    pub fn refresh_chaos_after_dagger_cast(&mut self) {
        if self.chaos_until > self.now { self.chaos_until = self.now + 15_000; }
    }
    pub fn from_reference(max_mp: f64, reference: &crate::profile::reaper::ReaperReference) -> Self {
        let mut state = Self::new(max_mp, reference.scenario);
        if let Some(test) = &reference.gauge_test { state.dark = test.initial_dark; }
        state.shadow_dot_gain_multiplier = reference.metadata["shadow_dot_gain_multiplier"].as_f64().unwrap_or(1.0);
        if reference.metadata["input_mode"] == "configured_build" { state.ultimate_point = 10000.0; }
        state.selected_passives = reference.metadata.get("selected_passives").cloned();
        state.chaos_tripod_damage = reference.metadata.get("chaos_tripod_damage")
            .map(|values| serde_json::from_value(values.clone()).unwrap()).unwrap_or_default();
        state.identity_tripod_gain = reference.metadata.get("identity_tripod_gain")
            .map(|values| serde_json::from_value(values.clone()).unwrap()).unwrap_or_default();
        state.identity_grid_multiplier = 1.0 + reference.metadata["identity_grid_gain"].as_f64().unwrap_or(0.0);
        state.identity_model = reference.metadata.get("identity_model").map(|model| serde_json::from_value(model.clone()).unwrap());
        if let Some(model) = &state.identity_model {
            state.dark = model.initial_dark;
            state.heat = model.initial_heat;
            state.chaos_until = (model.initial_chaos_seconds * 1000.0) as u64;
            if state.chaos_until > 0 { state.dark = 1.0; state.heat = 1.0; }
        }
        state
    }
    pub fn new(max_mp: f64, scenario: ReaperScenario) -> Self {
        let thirst = scenario == ReaperScenario::ThirstChaosSustain;
        Self {
            scenario, mp: max_mp, max_mp,
            dark: if thirst { 1.0 } else { 0.0 },
            heat: if thirst { 1.0 } else { 0.0 },
            persona: false, chaos_until: if thirst { 15_000 } else { 0 },
            fatal_until: 0, weakness: 0, blade: false, now: 0,
            shadow_dot_gain_multiplier: 1.0,
            ultimate_point: 0.0,
            nightmare_until: 0,
            selected_passives: None, handwork_stacks: 0, handwork_until: 0,
            identity_model: None,
            chaos_tripod_damage: Default::default(),
            identity_tripod_gain: Default::default(),
            identity_grid_multiplier: 1.0,
        }
    }

    pub fn advance(&mut self, now: u64) -> bool {
        let previous = (self.dark, self.heat, self.persona);
        if let Some(model) = &self.identity_model {
            let elapsed = now.saturating_sub(self.now) as f64 / 1000.0;
            if self.persona {
                self.dark = normalize(self.dark - elapsed * model.persona_dark_loss_per_second);
                if self.dark == 0.0 { self.persona = false; }
            } else if self.chaos_until == 0 {
                self.dark = normalize(self.dark + elapsed * model.dark_per_second);
                self.heat = normalize(self.heat - elapsed * model.heat_loss_per_second);
            }
        }
        self.now = now;
        let mut changed = previous != (self.dark, self.heat, self.persona);
        if self.handwork_until > 0 && self.handwork_until <= now {
            self.handwork_stacks = 0;
            self.handwork_until = 0;
            changed = true;
        }
        if self.nightmare_until > 0 && self.nightmare_until <= now {
            self.nightmare_until = 0;
            changed = true;
        }
        if self.chaos_until > 0 && self.chaos_until <= now {
            self.chaos_until = 0;
            self.fatal_until = 0;
            self.dark = 1.0;
            self.heat = 0.0;
            changed = true;
        }
        if self.fatal_until > 0 && self.fatal_until <= now {
            self.fatal_until = 0;
            changed = true;
        }
        changed
    }

    pub fn has_identity_model(&self) -> bool { self.identity_model.is_some() }

    pub fn can_cast(&self, game_id: u32) -> bool {
        if game_id == NIGHTMARE_RETURN && self.nightmare_until <= self.now { return false; }
        if self.scenario == ReaperScenario::ThirstChaosSustain {
            return (self.has_identity_model() || self.chaos_until > self.now) && game_id != PERSONA
                && (game_id != FATAL_STEP || (self.chaos_until > self.now && self.parameter("fatal_damage", 0.60) > 0.0));
        }
        match game_id {
            PERSONA => !self.persona && self.dark >= 1.0 && self.chaos_until == 0,
            FATAL_STEP => false,
            _ => true,
        }
    }

    pub fn boost(&self, game_id: u32, back_attack: bool) -> ReaperBoost {
        let mut boost = ReaperBoost::default();
        boost.attack_speed = self.handwork_stacks as f64 * 0.01;
        if self.persona {
            boost.crit_rate = self.parameter("persona_crit", 0.10);
            boost.attack_speed += 0.10;
            if is_rush(game_id) {
                let override_damage = self.parameter("persona_damage_override", 1.69);
                boost.damage = 1.0 + if override_damage > 0.0 { override_damage } else { self.parameter("persona_damage", 0.90) };
                boost.damage *= 1.0 + self.parameter("persona_extra_damage", 0.0);
                boost.crit_damage = self.parameter("persona_crit_damage", 0.0);
                if self.blade && back_attack { boost.damage *= 1.0 + self.parameter("blade_damage", 1.25); }
            }
        } else if is_rush(game_id) && self.parameter("persona_extra_damage", 0.0) > 0.0 {
            boost.damage *= 0.50;
        }
        if self.chaos_until > self.now {
            boost.damage *= 1.0 + self.chaos_tripod_damage.get(&game_id).copied().unwrap_or(0.0);
            boost.attack_power = 1.0 + self.parameter("chaos_attack_power", 0.38);
            boost.crit_rate = if self.selected_passives.is_some() && self.parameter("chaos_crit", 0.23) == 0.0 { 0.15 } else { self.parameter("chaos_crit", 0.23) };
            boost.attack_speed += 0.10;
            if matches!(game_id, 26050 | 26060 | 26070 | 26080 | 26110 | 26120 | 26140 | 26510 | 26520 | 26550 | 26560 | 26570 | 26600 | 26960 | FATAL_STEP)
                || is_rush(game_id) {
                boost.damage *= 1.0 + self.parameter("chaos_damage", 0.06);
                if self.fatal_until > self.now { boost.damage *= 1.0 + self.parameter("fatal_damage", 0.60); }
            }
        }
        if (self.persona || self.chaos_until > self.now) && self.passive_group_contains("handwork_damage_ids", game_id) {
            boost.damage *= 1.0 + self.parameter("handwork_damage", 0.0);
        }
        if game_id == 26970 {
            boost.crit_rate += self.parameter("finish_crit", 0.0);
            if self.persona { boost.crit_damage += self.parameter("finish_persona_crit_damage", 0.0); }
            if self.chaos_until > self.now {
                boost.damage *= 1.0 + self.parameter("finish_chaos_damage", 0.0);
                if back_attack { boost.crit_rate += self.parameter("finish_chaos_crit", 0.0); }
            }
        }
        boost
    }

    fn parameter(&self, name: &str, legacy: f64) -> f64 {
        self.selected_passives.as_ref().map_or(legacy, |parameters| parameters[name].as_f64().unwrap_or(0.0) / 100.0)
    }

    fn passive_group_contains(&self, name: &str, game_id: u32) -> bool {
        self.selected_passives.as_ref().and_then(|parameters| parameters[name].as_array())
            .is_some_and(|ids| ids.iter().any(|id| id.as_u64() == Some(game_id as u64)))
    }

    /// RA07/08/09: capture the cast's class boost before calling this method.
    pub fn cast(&mut self, game_id: u32) {
        match game_id {
            NIGHTMARE_RETURN => self.nightmare_until = 0,
            PERSONA => { self.persona = true; if self.has_identity_model() { self.heat = 0.0; } },
            id if is_rush(id) => {
                if self.blade {
                    self.blade = false;
                } else if self.persona && self.parameter("blade_damage", 1.25) > 0.0 {
                    self.weakness += 1;
                    if self.weakness == 3 {
                        self.weakness = 0;
                        self.blade = true;
                    }
                }
                if self.persona { self.persona = false; self.dark = 0.0; }
                if self.chaos_until > self.now && self.parameter("chaos_damage", 0.06) > 0.0 {
                    self.chaos_until = self.chaos_until.saturating_sub(2_000);
                    self.advance(self.now);
                }
            }
            DISTORTION | 26520 | 26550 | 26560 | 26570 | 26600 | 26960 if self.chaos_until > self.now => self.chaos_until = self.now + 15_000,
            FATAL_STEP if self.chaos_until > self.now => self.fatal_until = self.now + 10_000,
            _ => {}
        }
    }

    pub fn hit(&mut self, game_id: u32, landed: bool) {
        self.hit_with_gain(game_id, landed, None);
    }

    pub fn hit_with_gain(&mut self, game_id: u32, landed: bool, gain: Option<crate::profile::reaper::ReaperGaugeGain>) {
        if !landed { return; }
        if self.passive_group_contains("handwork_hit_ids", game_id) {
            self.handwork_stacks = (self.handwork_stacks + 1).min(5);
            self.handwork_until = self.now + 6000;
        }
        if game_id == NIGHTMARE { self.nightmare_until = self.now + 7000; }
        // RC03: dagger skills refresh chaos on a landed hit, once per hit (no gain inference).
        if matches!(game_id, 26050 | 26060 | SHADOW_DOT | 26080 | 26110 | 26120 | 26140 | NIGHTMARE | NIGHTMARE_RETURN)
            && self.chaos_until > self.now { self.chaos_until = self.now + 15_000; }
        if self.has_identity_model() {
            if !self.persona && self.chaos_until == 0 {
                if let Some(gain) = self.identity_model.as_ref().unwrap().skill_gains.get(&game_id).copied() {
                    let tripod_multiplier = (1.0 + self.identity_tripod_gain.get(&game_id).copied().unwrap_or(0.0)) * self.identity_grid_multiplier;
                    if self.dark < 1.0 {
                        let multiplier = if game_id == 26960 && self.parameter("knife_cooldown", 0.0) > 0.0 { 0.10 }
                            else if game_id == 26960 && self.parameter("knife_damage", 0.0) > 0.0 { 2.0 } else { 1.0 };
                        self.dark = normalize(self.dark + gain.dark * multiplier * tripod_multiplier);
                    }
                    else {
                        self.heat = normalize(self.heat + gain.heat * tripod_multiplier * (1.0 + self.parameter("heat_gain", 0.50)));
                        if self.heat == 1.0 { self.chaos_until = self.now + 15000; }
                    }
                }
            }
        } else if game_id == SHADOW_DOT {
            // ponytail: RA03/04 reference calibration; replace with measured per-hit gains.
            if !self.persona && self.chaos_until == 0 {
                let gain = gain.unwrap_or(crate::profile::reaper::ReaperGaugeGain { dark: self.shadow_dot_gain_multiplier / 18.0, heat: 0.0 });
                if self.dark < 1.0 { self.dark = normalize(self.dark + gain.dark * self.identity_grid_multiplier); }
                else { self.heat = normalize(self.heat + gain.heat * self.identity_grid_multiplier); }
            }
        }
    }

    pub fn next_expiration(&self) -> Option<u64> {
        let natural = self.identity_model.as_ref().is_some_and(|model|
            (self.persona && model.persona_dark_loss_per_second > 0.0)
            || (!self.persona && self.chaos_until == 0 && ((self.dark < 1.0 && model.dark_per_second > 0.0) || (self.heat > 0.0 && model.heat_loss_per_second > 0.0))));
        [self.chaos_until, self.fatal_until, self.nightmare_until, self.handwork_until,
            if natural { (self.now / 100 + 1) * 100 } else { 0 }].into_iter().filter(|&t| t > self.now).min()
    }
}

fn normalize(value: f64) -> f64 {
    if value >= 1.0 - 1e-12 { 1.0 } else { value.clamp(0.0, 1.0) }
}

impl ClassState for ReaperState {
    fn clone_box(&self) -> Box<dyn ClassState> { Box::new(self.clone()) }
    fn as_reaper(&self) -> Option<&ReaperState> { Some(self) }
    fn as_reaper_mut(&mut self) -> Option<&mut ReaperState> { Some(self) }
    fn get_resource(&self, name: &str) -> f64 {
        match name {
            "Mp" => self.mp,
            "UltimatePoint" => self.ultimate_point,
            "ReaperDark" => self.dark,
            "ReaperHeat" => self.heat,
            "ReaperPersona" => self.persona as u8 as f64,
            "ReaperChaosRemaining" => self.chaos_until.saturating_sub(self.now) as f64 / 1000.0,
            "ReaperWeakness" => self.weakness as f64,
            "ReaperBlade" => self.blade as u8 as f64,
            _ => 0.0,
        }
    }
    fn set_resource(&mut self, name: &str, value: f64) {
        match name {
            "Mp" => self.mp = value.clamp(0.0, self.max_mp),
            "UltimatePoint" => self.ultimate_point = value.clamp(0.0, 10000.0),
            "ReaperDark" => self.dark = normalize(value),
            "ReaperHeat" => self.heat = normalize(value),
            _ => {}
        }
    }
    fn get_resource_max(&self, name: &str) -> f64 {
        match name {
            "Mp" => self.max_mp,
            "UltimatePoint" => 10000.0,
            "ReaperDark" | "ReaperHeat" | "ReaperPersona" | "ReaperBlade" => 1.0,
            "ReaperChaosRemaining" => 15.0,
            "ReaperWeakness" => 3.0,
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_gain_stacks_with_tripods_but_preserves_natural_and_blocked_paths() {
        let mut state=ReaperState::new(100.0,ReaperScenario::DarkShadowDotLegacy);
        state.identity_grid_multiplier=1.006;
        state.identity_tripod_gain.insert(SHADOW_DOT,0.45);
        state.identity_model=Some(crate::profile::reaper::ReaperIdentityModel {
            initial_dark:0.,initial_heat:0.,initial_chaos_seconds:0.,dark_per_second:0.03,
            persona_dark_loss_per_second:0.04,heat_loss_per_second:0.02,
            skill_gains:std::collections::HashMap::from([(SHADOW_DOT,crate::profile::reaper::ReaperGaugeGain {dark:0.1,heat:0.2})])});
        state.advance(1000);assert!((state.dark-0.03).abs()<1e-12);
        state.hit(SHADOW_DOT,false);assert!((state.dark-0.03).abs()<1e-12);
        state.hit(SHADOW_DOT,true);assert!((state.dark-0.03-0.1*1.45*1.006).abs()<1e-12);
        state.dark=0.99;state.heat=0.;state.hit(SHADOW_DOT,true);assert_eq!((state.dark,state.heat),(1.,0.));
        state.hit(SHADOW_DOT,true);assert!((state.heat-0.2*1.45*1.006*1.5).abs()<1e-12);
        let before=state.heat;state.persona=true;state.hit(SHADOW_DOT,true);assert_eq!(state.heat,before);
        state.persona=false;state.chaos_until=10000;state.hit(SHADOW_DOT,true);assert_eq!(state.heat,before);
        state.chaos_until=0;state.identity_model.as_mut().unwrap().skill_gains.clear();state.hit(SHADOW_DOT,true);assert_eq!(state.heat,before);
    }

    #[test]
    fn grid_gain_loads_from_reference_and_scales_reference_and_explicit_hit_inputs() {
        let reference=crate::profile::reaper::ReaperReference {scenario:ReaperScenario::DarkShadowDotLegacy,
            missed_hit_ids:vec![],gauge_test:None,metadata:serde_json::json!({"identity_grid_gain":0.006})};
        let mut state=ReaperState::from_reference(100.,&reference);
        state.hit(SHADOW_DOT,true);assert!((state.dark-1.006/18.).abs()<1e-12);
        state.dark=0.;state.hit_with_gain(SHADOW_DOT,true,Some(crate::profile::reaper::ReaperGaugeGain {dark:0.1,heat:0.2}));
        assert!((state.dark-0.1006).abs()<1e-12);
        state.dark=1.;state.hit_with_gain(SHADOW_DOT,true,Some(crate::profile::reaper::ReaperGaugeGain {dark:0.1,heat:0.2}));
        assert!((state.heat-0.2012).abs()<1e-12);
        assert_eq!(ReaperState::from_reference(100.,&reference).heat,0.);
    }

    #[test]
    fn reference_gain_miss_fullness_and_overflow() {
        let mut state = ReaperState::new(100.0, ReaperScenario::DarkShadowDotLegacy);
        for _ in 0..16 { state.hit(SHADOW_DOT, true); }
        assert!((state.dark - 8.0 / 9.0).abs() < 1e-12);
        assert!(!state.can_cast(PERSONA));
        state.hit(SHADOW_DOT, false);
        state.hit(SHADOW_DOT, true);
        assert!((state.dark - 17.0 / 18.0).abs() < 1e-12);
        state.hit(SHADOW_DOT, true);
        assert_eq!(state.dark, 1.0);
        assert!(state.can_cast(PERSONA));
        state.dark = 0.97;
        state.hit(SHADOW_DOT, true);
        assert_eq!((state.dark, state.heat), (1.0, 0.0));
        state.hit(SHADOW_DOT, true);
        assert_eq!(state.heat, 0.0); // Unknown heat gain is not copied from dark.
        state.set_resource("ReaperHeat", 0.06); // Assignment only, not a calibrated heat-gain event (RP05).
        assert_eq!(state.heat, 0.06);
        let mut cloned = state.clone_box();
        cloned.set_resource("ReaperDark", 0.0);
        assert_eq!(state.dark, 1.0);
    }

    #[test]
    fn chaos_refresh_cost_self_boost_and_expiry_rp06_07_11_12() {
        let mut state = ReaperState::new(100.0, ReaperScenario::ThirstChaosSustain);
        state.advance(14_000);
        state.hit(SHADOW_DOT, false);
        assert_eq!(state.chaos_until, 15_000);
        let boost = state.boost(RAGE_SPEAR, true);
        state.cast(RAGE_SPEAR);
        assert_eq!((state.chaos_until, state.dark, state.heat), (0, 1.0, 0.0));
        assert_eq!(boost.attack_power, 1.38);
        assert!(!state.can_cast(RAGE_SPEAR));
        let mut state = ReaperState::new(100.0, ReaperScenario::ThirstChaosSustain);
        state.advance(14_000);
        state.cast(DISTORTION);
        assert_eq!(state.chaos_until, 29_000);
        state.cast(FATAL_STEP);
        assert!((state.boost(FATAL_STEP, true).damage - 1.06 * 1.60).abs() < 1e-12);
        state.advance(24_000);
        assert_eq!(state.fatal_until, 0);
        state.advance(29_000);
        assert!(!state.can_cast(FATAL_STEP));
    }

    #[test]
    fn persona_counter_and_one_cast_blade_rp08_to_10() {
        let mut state = ReaperState::new(100.0, ReaperScenario::DarkShadowDotLegacy);
        for _ in 0..3 {
            state.dark = 1.0;
            state.cast(PERSONA);
            assert_eq!(state.boost(RAGE_SPEAR, true).damage, 2.69);
            state.cast(RAGE_SPEAR);
            state.hit(RAGE_SPEAR, false); // Use counts even when all hits miss.
        }
        assert!(state.blade);
        assert_eq!(state.weakness, 0);
        state.dark = 1.0;
        state.cast(PERSONA);
        assert_eq!(state.boost(RAGE_SPEAR, false).damage, 2.69);
        let snapshot = state.boost(RAGE_SPEAR, true);
        state.cast(RAGE_SPEAR);
        assert_eq!(snapshot.damage, 2.69 * 2.25);
        assert!(!state.blade);
        assert_eq!(state.weakness, 0);
        assert_eq!(state.boost(RAGE_SPEAR, true).damage, 1.0);
    }
}
