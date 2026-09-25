use crate::builder::dto::BuildRequest;
use crate::builder::skill::SkillPipelineMap;
use crate::builder::stat::StatBuilder;

/// EFTable_ItemGradeOptionRandom의 Type 5(피해), Value / 10000.
const LEGACY_DAMAGE_VALUES: [f64; 10] =
    [0.03, 0.06, 0.09, 0.12, 0.15, 0.18, 0.21, 0.24, 0.30, 0.40];
const T4_DAMAGE_VALUES: [f64; 10] = [0.08, 0.12, 0.16, 0.20, 0.24, 0.28, 0.32, 0.36, 0.40, 0.44];

/// EFTable_ItemGradeOptionRandom의 Type 27(재사용 대기시간), Value / 10000.
const LEGACY_COOLDOWN_VALUES: [f64; 10] =
    [0.02, 0.04, 0.06, 0.08, 0.10, 0.12, 0.14, 0.16, 0.18, 0.20];
const T4_COOLDOWN_VALUES: [f64; 10] = [0.06, 0.08, 0.10, 0.12, 0.14, 0.16, 0.18, 0.20, 0.22, 0.24];

/// T4 겁화/작열 StaticOption의 AddonType 2, Stat 150, Value / 10000.
const T4_BASE_AP_VALUES: [f64; 10] = [
    0.0, 0.0005, 0.001, 0.002, 0.003, 0.0045, 0.006, 0.008, 0.01, 0.012,
];

fn level_value(values: &[f64; 10], level: u32) -> f64 {
    values[(level.saturating_sub(1) as usize).min(values.len() - 1)]
}

fn damage_gem_value(gem_type: &str, level: u32) -> f64 {
    level_value(
        if matches!(gem_type, "겁화" | "광휘") {
            &T4_DAMAGE_VALUES
        } else {
            &LEGACY_DAMAGE_VALUES
        },
        level,
    )
}

fn cooldown_gem_value(gem_type: &str, level: u32) -> f64 {
    level_value(
        if matches!(gem_type, "작열" | "광휘") {
            &T4_COOLDOWN_VALUES
        } else {
            &LEGACY_COOLDOWN_VALUES
        },
        level,
    )
}

fn base_ap_gem_value(gem_type: &str, level: u32) -> f64 {
    if matches!(gem_type, "겁화" | "작열" | "광휘") {
        level_value(&T4_BASE_AP_VALUES, level)
    } else {
        0.0
    }
}

/// 보석 효과를 StatBuilder 및 SkillPipeline에 등록한다.
///
/// - 겁화/멸화 → 해당 스킬의 모든 히트 damage_increase 누적
/// - 작열/홍염 → 해당 스킬 cooldown_ms 비율 감소
/// - 레벨별 기본 공격력 증가율 → stat.base_ap_mul 누적 (보석 1개당 독립 합산)
pub fn register(
    stat: &mut StatBuilder,
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
) {
    for gem in &req.character.gems {
        if gem.level == 0 {
            continue;
        }

        // 기본 공격력 증가 (D 항목)
        let base_ap_bonus = base_ap_gem_value(&gem.gem_type, gem.level);
        if base_ap_bonus > 0.0 {
            let src = format!("gem:base_ap:{}", gem.skill_name);
            stat.add_additive(src, move |s| s.base_ap_mul += base_ap_bonus);
        }

        // 스킬별 효과
        let Some(pipeline) = skill_pipelines.get_mut(&gem.skill_name) else {
            continue;
        };

        let known_type = matches!(gem.gem_type.as_str(), "겁화" | "멸화" | "작열" | "홍염");
        let is_damage = matches!(gem.gem_type.as_str(), "겁화" | "멸화")
            || (!known_type && gem.skill_effect.contains("피해"));
        let is_cooldown = matches!(gem.gem_type.as_str(), "작열" | "홍염")
            || (!known_type
                && (gem.skill_effect.contains("쿨타임")
                    || gem.skill_effect.contains("재사용 대기시간")));
        if is_damage {
            let bonus = damage_gem_value(&gem.gem_type, gem.level);
            pipeline.add_stats(move |skill| {
                for hit in &mut skill.hits {
                    hit.damage_increase += bonus;
                }
                Ok(())
            });
        } else if is_cooldown {
            let reduction = cooldown_gem_value(&gem.gem_type, gem.level);
            pipeline.add_stats(move |skill| {
                let reduce_ms = (skill.cooldown_ms as f64 * reduction) as u32;
                skill.cooldown_ms = skill.cooldown_ms.saturating_sub(reduce_ms);
                Ok(())
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_raw_legacy_and_t4_gem_values() {
        assert_eq!(damage_gem_value("멸화", 8), 0.24);
        assert_eq!(damage_gem_value("겁화", 10), 0.44);
        assert_eq!(cooldown_gem_value("홍염", 10), 0.20);
        assert_eq!(cooldown_gem_value("작열", 10), 0.24);
        assert_eq!(base_ap_gem_value("겁화", 6), 0.0045);
        assert_eq!(base_ap_gem_value("작열", 9), 0.01);
        assert_eq!(base_ap_gem_value("광휘", 10), 0.012);
        assert_eq!(base_ap_gem_value("멸화", 10), 0.0);
    }
}
