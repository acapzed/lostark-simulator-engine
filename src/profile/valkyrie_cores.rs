//! Source-compiled class cores. Research selections cannot supply arbitrary effect values.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CoreSelection {
    pub key: String,
    pub grade: u32,
    pub points: u32,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CoreEffect {
    Damage {
        skill_id: u32,
        category: u32,
        value: f64,
        crit_only: bool,
    },
    FateOnCast {
        skill_id: u32,
    },
    FateLight {
        amount: u32,
    },
    FateFinale {
        value: f64,
    },
    FateOrigin {
        value: f64,
        stacks: u32,
    },
    FateMana {
        value: f64,
    },
    FateWhisperCooldown {
        amount_ms: u32,
    },
    FateWhisper {
        value: f64,
    },
    Speed {
        skill_id: u32,
        ratio: f64,
    },
    Cooldown {
        skill_id: u32,
        amount_ms: i32,
    },
    Mana {
        skill_id: u32,
        ratio: f64,
    },
    Category {
        skill_id: u32,
        category: u32,
    },
    Metadata {
        reason: String,
    },
}
#[derive(Deserialize)]
struct Catalog {
    families: Vec<Family>,
}
#[derive(Deserialize)]
struct Family {
    key: String,
    slot: u32,
    status: String,
    grades: Vec<Grade>,
}
#[derive(Deserialize)]
struct Grade {
    grade: u32,
    options: Vec<CoreOption>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoreOption {
    point_requirement: u32,
    effects: Vec<CoreEffect>,
}
fn catalog() -> &'static Catalog {
    static DATA: OnceLock<Catalog> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../skill-data/valkyrie/backend/cores-runtime.json"
        ))
        .expect("invalid source core catalog")
    })
}
#[derive(Debug, Clone, Default)]
pub struct CoreRuntime {
    pub effects: Vec<CoreEffect>,
}
impl CoreRuntime {
    pub fn from_selected(selected: &[CoreSelection]) -> Result<Self, String> {
        let mut slots = std::collections::HashSet::new();
        let mut effects = Vec::new();
        for s in selected {
            let f = catalog()
                .families
                .iter()
                .find(|f| f.key == s.key && f.status == "research_supported")
                .ok_or("미지원 발키리 코어")?;
            if !slots.insert(f.slot) || s.points > 20 {
                return Err("코어 슬롯 중복 또는 포인트 범위 밖".into());
            }
            let g = f
                .grades
                .iter()
                .find(|g| g.grade == s.grade)
                .ok_or("코어 등급 범위 밖")?;
            for option in &g.options {
                if s.points >= option.point_requirement {
                    effects.extend(option.effects.clone());
                }
            }
        }
        Ok(Self { effects })
    }
    pub fn damage_factor(&self, id: u32, category: Option<u32>, critical: bool) -> f64 {
        if critical {
            return 1.
                + self
                    .effects
                    .iter()
                    .filter_map(|e| match e {
                        CoreEffect::Damage {
                            skill_id,
                            category: cat,
                            value,
                            crit_only: true,
                        } if (*skill_id != 0 && *skill_id == id)
                            || (*cat != 0 && Some(*cat) == category) =>
                        {
                            Some(*value)
                        }
                        _ => None,
                    })
                    .sum::<f64>();
        }
        self.effects
            .iter()
            .filter_map(|e| match e {
                CoreEffect::Damage {
                    skill_id,
                    category: cat,
                    value,
                    crit_only,
                } if *crit_only == critical
                    && ((*skill_id != 0 && *skill_id == id)
                        || (*cat != 0 && Some(*cat) == category)) =>
                {
                    Some(1. + value)
                }
                _ => None,
            })
            .product()
    }
    pub fn origin_bonus(&self) -> f64 {
        self.effects
            .iter()
            .filter_map(|e| {
                if let CoreEffect::FateOrigin { value, .. } = e {
                    Some(*value)
                } else {
                    None
                }
            })
            .last()
            .unwrap_or(0.)
    }
    pub fn finale_bonus(&self) -> f64 {
        self.effects
            .iter()
            .filter_map(|e| {
                if let CoreEffect::FateFinale { value } = e {
                    Some(*value)
                } else {
                    None
                }
            })
            .last()
            .unwrap_or(0.)
    }
    pub fn whisper_bonus(&self) -> f64 {
        self.effects
            .iter()
            .filter_map(|e| {
                if let CoreEffect::FateWhisper { value } = e {
                    Some(*value)
                } else {
                    None
                }
            })
            .last()
            .unwrap_or(0.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grade_thresholds_and_tooltip_target_do_not_leak_to_dawn() {
        let build = |grade, points| {
            CoreRuntime::from_selected(&[CoreSelection {
                key: "finalKnight".into(),
                grade,
                points,
            }])
            .unwrap()
        };
        assert_eq!(build(0, 20).damage_factor(48070, None, false), 1.04);
        let p19 = build(3, 19).damage_factor(48070, None, false);
        let p20 = build(3, 20).damage_factor(48070, None, false);
        assert!((p20 / p19 - 1.0025).abs() < 1e-12);
        assert_eq!(build(3, 20).damage_factor(48500, Some(68), false), 1.);
        assert!(CoreRuntime::from_selected(&[CoreSelection {
            key: "finalKnight".into(),
            grade: 4,
            points: 20
        }])
        .is_err());
        let critical = CoreRuntime::from_selected(&[CoreSelection {
            key: "trueFinale".into(),
            grade: 3,
            points: 20,
        }])
        .unwrap();
        assert!((critical.damage_factor(48070, None, true) - 1.0775).abs() < 1e-12);
        let justice = CoreRuntime::from_selected(&[CoreSelection {
            key: "enhancedJustice".into(),
            grade: 1,
            points: 14,
        }])
        .unwrap();
        assert!(justice.effects.iter().any(|e| matches!(
            e,
            CoreEffect::Category {
                skill_id: 48170,
                category: 67
            }
        )));
        assert_eq!(justice.damage_factor(48170, Some(67), false), 2.5);
    }
}
