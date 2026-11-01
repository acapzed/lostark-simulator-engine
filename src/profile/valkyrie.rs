use hashbrown::HashMap;
use serde::{Deserialize, Serialize};

/// Source-generated Light Knight base identity contract, not a public build input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValkyrieIdentity {
    pub light_per_use: u32,
    pub light_max: u32,
    pub light_sword_grant: u32,
    pub light_sword_damage_bonus: f64,
    pub skill_categories: HashMap<u32, u32>,
    #[serde(default)]
    pub initial_light: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passives: Option<ValkyriePassives>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub linked_windows: HashMap<u32, Vec<ValkyrieLinkWindow>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cores: Vec<super::valkyrie_cores::CoreSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValkyrieLinkWindow {
    pub start_ms: u32,
    pub end_ms: u32,
    pub skill_ids: Vec<u32>,
}

impl ValkyrieIdentity {
    pub fn validate(&self) -> Result<(), String> {
        if self.light_per_use != 667
            || self.light_max != 10000
            || self.light_sword_grant
                != if self.passives.as_ref().is_some_and(|p| p.trinity > 0) {
                    3
                } else {
                    1
                }
            || ![0.10, 0.225, 0.35].contains(&self.light_sword_damage_bonus)
            || self.initial_light > self.light_max
            || self
                .skill_categories
                .values()
                .any(|category| !matches!(category, 67..=69))
        {
            return Err("발키리 기본 아이덴티티 원본 계약 범위 밖".into());
        }
        if let Some(p) = &self.passives {
            p.validate()?;
            if self.light_sword_damage_bonus != 0.35 {
                return Err("상위 패시브는 빛의 기사3레벨이 필요합니다".into());
            }
        }
        for (&id, windows) in &self.linked_windows {
            if (!windows.is_empty() && self.skill_categories.get(&id) != Some(&68))
                || windows.iter().any(|w| {
                    w.start_ms >= w.end_ms
                        || w.skill_ids != [48160, 48170, 48180, 48190, 48200, 48210, 48500]
                })
            {
                return Err("발키리 성휘 연계창 계약 범위 밖".into());
            }
        }
        if !self.cores.is_empty() {
            if !self.passives.as_ref().is_some_and(|p| p.trinity > 0) {
                return Err("발키리 전용 코어는 삼위일체가 필요합니다".into());
            }
            super::valkyrie_cores::CoreRuntime::from_selected(&self.cores)?;
        }
        Ok(())
    }
}

/// Selected levels only. Inherited fields in AbilityFeature rows never enable another node.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValkyriePassives {
    #[serde(default)]
    pub training: u32,
    #[serde(default)]
    pub release: u32,
    #[serde(default)]
    pub last_light: u32,
    #[serde(default)]
    pub trinity: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub linked_light: u32,
}
fn is_zero(v: &u32) -> bool {
    *v == 0
}
impl ValkyriePassives {
    pub fn validate(&self) -> Result<(), String> {
        if self.training > 3
            || self.release > 3
            || self.last_light > 5
            || self.trinity > 3
            || self.linked_light > 5
        {
            return Err("발키리 패시브 레벨 범위 밖".into());
        }
        Ok(())
    }
    pub fn last_light_bonus(&self) -> f64 {
        [0., 0.03, 0.06, 0.09, 0.12, 0.15][self.last_light as usize]
    }
    pub fn damage_factors(
        &self,
        category: Option<u32>,
        game_id: u32,
        all_crit: f64,
    ) -> Vec<(&'static str, f64)> {
        let mut factors = Vec::new();
        if self.linked_light > 0 {
            factors.push((
                "ark_passive:연결되는 빛",
                1. + self.linked_light as f64 * 0.01,
            ));
        }
        let training = [0., 0.12, 0.165, 0.215][self.training as usize];
        if training != 0. && matches!(category, Some(67 | 68)) {
            factors.push(("ark_passive:검술 훈련", 1. + training));
        }
        let trinity = [0., 0., 0.07, 0.14][self.trinity as usize];
        if trinity != 0. {
            factors.push(("ark_passive:삼위일체", 1. + trinity));
        }
        let crit = all_crit.clamp(0., 1.);
        let general = [0., 0.15, 0.35, 0.55][self.release as usize];
        let finale = [0., 0.5, 1., 1.5][self.release as usize];
        if general != 0. {
            factors.push(("ark_passive:성검 개방:전체", 1. + crit * general));
        }
        if finale != 0. && game_id == 48070 {
            factors.push(("ark_passive:성검 개방:종언", 1. + crit * finale));
        }
        factors
    }
}
