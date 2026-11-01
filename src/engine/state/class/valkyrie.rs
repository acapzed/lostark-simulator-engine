use super::ClassState;
use crate::profile::valkyrie::ValkyrieIdentity;

pub const KNIGHT: u32 = 48060;
pub const FINALE: u32 = 48070;
pub const MOBILITY: u32 = 48020;
pub const SWORD_BUFF: &str = "valkyrie:sword";
pub const LAST_LIGHT_BUFF: &str = "valkyrie:last_light";
pub const LOST_BUFF: &str = "valkyrie:lost_sword";
pub const WEAKNESS_BUFF: &str = "valkyrie:weakness";
pub const CORE_FINALE_BUFF: &str = "valkyrie:core:finale";
pub const CORE_ORIGIN_BUFF: &str = "valkyrie:core:origin";
pub const CORE_WHISPER_BUFF: &str = "valkyrie:core:whisper";

#[derive(Debug, Clone)]
pub struct ValkyrieState {
    mp: f64,
    max_mp: f64,
    ultimate_point: f64,
    pub light: u32,
    pub swords: u32,
    pub lost_sword: bool,
    pub last_light: bool,
    action_end: u64,
    link_windows: Vec<(u64, u64, Vec<u32>)>,
    reference: ValkyrieIdentity,
    pub core_runtime: crate::profile::valkyrie_cores::CoreRuntime,
    pub destiny_count: u32,
    pub core_finale: bool,
    pub core_origin: u32,
    pub core_whisper: bool,
}

impl ValkyrieState {
    pub fn new(max_mp: f64, reference: ValkyrieIdentity) -> Self {
        reference
            .validate()
            .expect("invalid Valkyrie source contract");
        let core_runtime =
            crate::profile::valkyrie_cores::CoreRuntime::from_selected(&reference.cores).unwrap();
        Self {
            mp: max_mp,
            max_mp,
            ultimate_point: 0.,
            light: reference.initial_light,
            swords: 0,
            lost_sword: false,
            last_light: false,
            action_end: 0,
            link_windows: Vec::new(),
            reference,
            core_runtime,
            destiny_count: 0,
            core_finale: false,
            core_origin: 0,
            core_whisper: false,
        }
    }
    pub fn can_cast(&self, game_id: u32) -> bool {
        match game_id {
            KNIGHT => !self.lost_sword,
            FINALE => self.light == self.reference.light_max,
            _ => self.reference.skill_categories.get(&game_id) != Some(&69),
        }
    }
    /// Only successful skill uses call this. Contacts and elapsed combat time do not generate light.
    pub fn cast(&mut self, game_id: u32) -> bool {
        if !self.can_cast(game_id) {
            return false;
        }
        match game_id {
            KNIGHT => {
                self.swords = self.reference.light_sword_grant;
                self.lost_sword = true;
                self.last_light = self
                    .reference
                    .passives
                    .as_ref()
                    .is_some_and(|p| p.last_light > 0);
            }
            FINALE => {
                self.light = 0;
                self.lost_sword = false;
                self.last_light = false;
                self.core_finale = false;
                self.core_origin = 0;
            }
            _ => match self.reference.skill_categories.get(&game_id) {
                Some(67) => self.gain_light(),
                Some(68) => {
                    self.swords = self.swords.saturating_sub(1);
                    self.core_origin = self.core_origin.saturating_sub(1);
                    self.gain_light();
                }
                _ => {}
            },
        }
        if game_id == 48150 {
            self.core_whisper = false;
        }
        true
    }
    /// Destiny occurs after the source skill pays resources and registers cooldown.
    pub fn on_core_cast(&mut self, id: u32) -> u32 {
        use crate::profile::valkyrie_cores::CoreEffect;
        if !self
            .core_runtime
            .effects
            .iter()
            .any(|e| matches!(e,CoreEffect::FateOnCast {skill_id}if *skill_id==id))
        {
            return 0;
        }
        self.destiny_count += 1;
        let mut reduction = 0;
        for e in self.core_runtime.effects.clone() {
            match e {
                CoreEffect::FateLight { amount } => {
                    self.light = self
                        .light
                        .saturating_add(amount)
                        .min(self.reference.light_max)
                }
                CoreEffect::FateFinale { .. } => self.core_finale = true,
                CoreEffect::FateOrigin { stacks, .. } => {
                    self.core_origin = if self.lost_sword { stacks } else { 0 }
                }
                CoreEffect::FateWhisper { .. } => self.core_whisper = true,
                CoreEffect::FateMana { value } => {
                    self.mp = (self.mp + self.max_mp * value).min(self.max_mp)
                }
                CoreEffect::FateWhisperCooldown { amount_ms } => reduction = amount_ms,
                _ => {}
            }
        }
        reduction
    }
    pub fn core_damage_factor(&self, id: u32, critical: bool) -> f64 {
        self.core_runtime.damage_factor(
            id,
            self.reference.skill_categories.get(&id).copied(),
            critical,
        )
    }
    pub fn tracks_cores(&self) -> bool {
        !self.reference.cores.is_empty()
    }
    pub fn passive_factors(&self, game_id: u32, all_crit: f64) -> Vec<(&'static str, f64)> {
        self.reference.passives.as_ref().map_or_else(Vec::new, |p| {
            p.damage_factors(
                self.reference.skill_categories.get(&game_id).copied(),
                game_id,
                all_crit,
            )
        })
    }
    pub fn tracks_last_light(&self) -> bool {
        self.reference
            .passives
            .as_ref()
            .is_some_and(|p| p.last_light > 0)
    }
    pub fn begin_action(&mut self, id: u32, time: u64, base: u32, actual: u32) -> u64 {
        self.link_windows.clear();
        self.action_end = time + actual.max(1) as u64;
        if !self
            .reference
            .passives
            .as_ref()
            .is_some_and(|p| p.linked_light > 0)
        {
            self.action_end = 0;
            return time + actual.max(1) as u64;
        }
        let scale = |v: u32| {
            if base == 0 {
                v as u64
            } else {
                v as u64 * actual as u64 / base as u64
            }
        };
        for w in self.reference.linked_windows.get(&id).into_iter().flatten() {
            self.link_windows.push((
                time + scale(w.start_ms),
                time + scale(w.end_ms),
                w.skill_ids.clone(),
            ));
        }
        self.next_window(time).unwrap_or(self.action_end)
    }
    pub fn action_allows(&self, id: u32, time: u64) -> bool {
        time >= self.action_end
            || self
                .link_windows
                .iter()
                .any(|(start, end, ids)| time >= *start && time < *end && ids.contains(&id))
    }
    pub fn next_window(&self, time: u64) -> Option<u64> {
        self.link_windows
            .iter()
            .map(|(start, _, _)| *start)
            .chain([self.action_end])
            .filter(|t| *t > time)
            .min()
    }
    fn gain_light(&mut self) {
        self.light = self
            .light
            .saturating_add(self.reference.light_per_use)
            .min(self.reference.light_max);
    }
}

impl ClassState for ValkyrieState {
    fn clone_box(&self) -> Box<dyn ClassState> {
        Box::new(self.clone())
    }
    fn as_valkyrie(&self) -> Option<&Self> {
        Some(self)
    }
    fn as_valkyrie_mut(&mut self) -> Option<&mut Self> {
        Some(self)
    }
    fn get_resource(&self, name: &str) -> f64 {
        match name {
            "Mp" => self.mp,
            "UltimatePoint" => self.ultimate_point,
            "Light" => self.light as f64,
            "LightSword" => self.swords as f64,
            "LostLightSword" => self.lost_sword as u8 as f64,
            _ => 0.0,
        }
    }
    fn set_resource(&mut self, name: &str, value: f64) {
        match name {
            "Mp" => self.mp = value.clamp(0., self.max_mp),
            "UltimatePoint" if value.is_finite() => self.ultimate_point = value.clamp(0., 10000.),
            "Light" if value.is_finite() => {
                self.light = value.clamp(0., self.reference.light_max as f64) as u32
            }
            _ => {}
        }
    }
    fn get_resource_max(&self, name: &str) -> f64 {
        match name {
            "Mp" => self.max_mp,
            "UltimatePoint" => 10000.,
            "Light" => self.reference.light_max as f64,
            "LightSword" => self.reference.light_sword_grant as f64,
            "LostLightSword" => 1.,
            _ => 0.,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destiny_origin_refreshes_three_next_uses_and_clears_at_finale() {
        use crate::profile::{valkyrie::ValkyriePassives, valkyrie_cores::CoreSelection};
        let mut reference = state().reference;
        reference.passives = Some(ValkyriePassives {
            trinity: 3,
            ..Default::default()
        });
        reference.light_sword_grant = 3;
        reference.cores = vec![
            CoreSelection {
                key: "origin".into(),
                grade: 3,
                points: 20,
            },
            CoreSelection {
                key: "finalKnight".into(),
                grade: 1,
                points: 14,
            },
        ];
        let mut s = ValkyrieState::new(2000., reference);
        assert_eq!(s.on_core_cast(48100), 0);
        assert_eq!(s.destiny_count, 0);
        s.cast(KNIGHT);
        s.set_resource("Mp", 100.);
        s.on_core_cast(KNIGHT);
        assert_eq!(s.core_origin, 3);
        assert_eq!(s.get_resource("Mp"), 400.);
        assert_eq!(s.core_runtime.origin_bonus(), 0.225);
        s.cast(48100);
        assert_eq!(s.core_origin, 3);
        s.cast(48160);
        assert_eq!(s.core_origin, 2);
        s.on_core_cast(KNIGHT);
        assert_eq!(s.core_origin, 3);
        s.set_resource("Light", 10000.);
        s.cast(FINALE);
        assert_eq!(s.core_origin, 0);
        assert!(!s.lost_sword);
        s.on_core_cast(KNIGHT);
        assert_eq!(s.core_origin, 0); // no active Knight state
    }

    #[test]
    fn linked_window_requires_the_listed_next_action_and_resets_on_every_use() {
        let mut s = state();
        s.reference.passives = Some(crate::profile::valkyrie::ValkyriePassives {
            linked_light: 5,
            ..Default::default()
        });
        s.reference.linked_windows.insert(
            48160,
            vec![crate::profile::valkyrie::ValkyrieLinkWindow {
                start_ms: 500,
                end_ms: 1800,
                skill_ids: vec![48160, 48170, 48180, 48190, 48200, 48210, 48500],
            }],
        );
        s.reference.validate().unwrap();
        assert_eq!(s.begin_action(48160, 1000, 1800, 1500), 1416);
        assert!(!s.action_allows(48180, 1415));
        assert!(s.action_allows(48180, 1416));
        assert!(!s.action_allows(48100, 1416));
        assert!(!s.action_allows(KNIGHT, 1416));
        assert!(s.action_allows(48100, 2500));
        s.begin_action(48100, 1416, 1000, 1000);
        assert!(!s.action_allows(48180, 1500));
        assert_eq!(s.next_window(1500), Some(2416));
        let new = state();
        assert!(new.action_allows(48100, 0));
    }
    fn state() -> ValkyrieState {
        ValkyrieState::new(
            100.,
            ValkyrieIdentity {
                light_per_use: 667,
                light_max: 10000,
                light_sword_grant: 1,
                light_sword_damage_bonus: 0.35,
                initial_light: 0,
                passives: None,
                linked_windows: hashbrown::HashMap::new(),
                cores: Vec::new(),
                skill_categories: hashbrown::HashMap::from([(48100, 67), (48160, 68), (48220, 69)]),
            },
        )
    }
    #[test]
    fn raw_integer_gain_saturates_on_fifteenth_use_and_never_overflows() {
        let mut s = state();
        for i in 1..=14 {
            assert!(s.cast(48100));
            assert_eq!(s.light, 667 * i);
            assert!(!s.can_cast(FINALE));
        }
        assert!(s.cast(48160));
        assert_eq!(s.light, 10000);
        assert!(s.can_cast(FINALE));
        for _ in 0..1000 {
            s.cast(48100);
        }
        assert_eq!(s.light, 10000);
        assert!(s.cast(FINALE));
        assert_eq!(s.light, 0);
        assert!(!s.cast(FINALE));
    }
    #[test]
    fn justice_preserves_sword_radiance_consumes_once_and_finale_only_unlocks() {
        let mut s = state();
        assert!(s.cast(KNIGHT));
        assert_eq!(s.swords, 1);
        assert!(s.lost_sword);
        assert!(!s.cast(KNIGHT));
        assert!(!s.cast(FINALE));
        assert!(!s.cast(48220));
        assert_eq!(s.light, 0);
        s.cast(48100);
        assert_eq!(s.swords, 1);
        s.cast(48160);
        assert_eq!(s.swords, 0);
        assert!(s.lost_sword);
        s.set_resource("Light", 10000.);
        s.cast(FINALE);
        assert!(!s.lost_sword);
        assert_eq!(s.swords, 0);
        assert!(s.cast(KNIGHT));
        s.set_resource("Light", 10000.);
        s.cast(FINALE);
        assert_eq!(s.swords, 1); // no tooltip instruction removes unused light sword
    }
    #[test]
    fn mobility_and_unknown_resources_do_not_generate_faith_or_light() {
        let mut s = state();
        s.cast(MOBILITY);
        assert_eq!(s.light, 0);
        s.set_resource("Faith", 9999.);
        assert_eq!(s.get_resource("Faith"), 0.);
        s.set_resource("Light", 10005.);
        assert_eq!(s.light, 10000);
        s.set_resource("Light", -1.);
        assert_eq!(s.light, 0);
        let clone = s.clone_box();
        assert_eq!(clone.get_resource("Light"), 0.);
        assert_eq!(clone.get_resource_max("Light"), 10000.);
    }
    #[test]
    fn trinity_and_last_light_consume_independently_and_never_enable_other_nodes() {
        let mut s = state();
        s.reference.passives = Some(crate::profile::valkyrie::ValkyriePassives {
            trinity: 1,
            last_light: 5,
            ..Default::default()
        });
        s.reference.light_sword_grant = 3;
        s.reference.validate().unwrap();
        assert!(s.cast(KNIGHT));
        assert_eq!(s.swords, 3);
        assert!(s.last_light);
        assert!(s.passive_factors(48160, 1.).is_empty()); // row10 inherited damage fields must remain off
        s.cast(48100);
        assert_eq!(s.swords, 3);
        assert!(s.last_light);
        for expected in [2, 1, 0, 0] {
            s.cast(48160);
            assert_eq!(s.swords, expected);
            assert!(s.last_light);
        }
        s.set_resource("Light", 10000.);
        s.cast(FINALE);
        assert!(!s.last_light);
        assert_eq!(s.swords, 0);
        s.set_resource("Light", 10000.);
        s.cast(FINALE);
        assert!(!s.last_light);
    }
    #[test]
    fn release_critical_scaling_is_bounded_and_training_skips_identity() {
        let p = crate::profile::valkyrie::ValkyriePassives {
            training: 3,
            release: 3,
            trinity: 3,
            last_light: 0,
            linked_light: 0,
        };
        let product = |category, id, crit| {
            p.damage_factors(category, id, crit)
                .iter()
                .map(|(_, v)| v)
                .product::<f64>()
        };
        assert!((product(Some(68), 48160, 0.5) - 1.215 * 1.14 * 1.275).abs() < 1e-12);
        assert!((product(None, FINALE, 0.5) - 1.14 * 1.275 * 1.75).abs() < 1e-12);
        assert!((product(None, KNIGHT, -1.) - 1.14).abs() < 1e-12);
        assert_eq!(product(None, FINALE, 2.), product(None, FINALE, 1.));
    }
}
