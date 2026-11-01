//! Light Knight identity attachment shared by research and validated full builds.
use crate::engine::state::class::valkyrie::{
    FINALE, KNIGHT, LAST_LIGHT_BUFF, MOBILITY, SWORD_BUFF, WEAKNESS_BUFF,
};
use crate::profile::{
    valkyrie::ValkyrieIdentity, BuffSpec, HitTrigger, HitTriggerOn, SkillProfile, SkillTag,
    StackType, StatField, TriggerEffect,
};

pub fn attach(skills: &mut [SkillProfile], reference: &ValkyrieIdentity) -> Result<(), String> {
    reference.validate()?;
    let cores = crate::profile::valkyrie_cores::CoreRuntime::from_selected(&reference.cores)?;
    for skill in skills {
        let game_id = skill
            .tags
            .iter()
            .find_map(|tag| match tag {
                SkillTag::SkillId(id) => Some(*id),
                _ => None,
            })
            .ok_or("발키리 원본 스킬 ID 없음")?;
        let category = reference.skill_categories.get(&game_id).copied();
        if category == Some(69)
            || (category.is_none() && ![KNIGHT, FINALE, MOBILITY, 48520, 48540].contains(&game_id))
        {
            return Err("미지원 발키리 아이덴티티 스킬 범위".into());
        }
        if category == Some(68) {
            skill
                .cast_buff_damage_bonuses
                .push((SWORD_BUFF.into(), reference.light_sword_damage_bonus));
            if cores.origin_bonus() > 0. {
                skill.cast_buff_damage_bonuses.push((
                    crate::engine::state::class::valkyrie::CORE_ORIGIN_BUFF.into(),
                    cores.origin_bonus(),
                ));
            }
        }
        if game_id == FINALE {
            if cores.finale_bonus() > 0. {
                skill.cast_buff_damage_bonuses.push((
                    crate::engine::state::class::valkyrie::CORE_FINALE_BUFF.into(),
                    cores.finale_bonus(),
                ));
            }
            if let Some(passives) = &reference.passives {
                let value = passives.last_light_bonus();
                if value > 0. {
                    skill
                        .cast_buff_damage_bonuses
                        .push((LAST_LIGHT_BUFF.into(), value));
                }
            }
        }
        if game_id == 48150 && cores.whisper_bonus() > 0. {
            skill.cast_buff_damage_bonuses.push((
                crate::engine::state::class::valkyrie::CORE_WHISPER_BUFF.into(),
                cores.whisper_bonus(),
            ));
        }
        if game_id == KNIGHT {
            for hit in &mut skill.hits {
                if hit.effect_id != 480032 {
                    return Err("미검토 빛의 기사 타격".into());
                }
                hit.triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    chance: 1.0,
                    effect: TriggerEffect::ApplyTargetBuff(BuffSpec {
                        id: WEAKNESS_BUFF.into(),
                        effects: hashbrown::HashMap::from([(StatField::OnCritDamageTaken, 0.08)]),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: 10000,
                    }),
                });
            }
        }
    }
    Ok(())
}

/// Class113 specialization changes faith/party buffs, not Light or justice damage.
/// The shared stat110 coefficient applies only to regular awakening damage.
pub fn apply_specialization(
    skills: &mut [SkillProfile],
    specialization: f64,
) -> Result<(), String> {
    if !specialization.is_finite() || specialization < 0. {
        return Err("잘못된 발키리 특화".into());
    }
    let factor = 1. + specialization / 699. * 0.1528;
    for skill in skills
        .iter_mut()
        .filter(|s| s.slot == crate::profile::SkillSlot::Awakening)
    {
        for hit in &mut skill.hits {
            hit.damage_increase = (1. + hit.damage_increase) * factor - 1.;
        }
    }
    Ok(())
}
