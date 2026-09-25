use crate::builder::dto::{ArkPassiveClassNodeInput, BuildRequest};
use crate::builder::modifier::{Modifier, ModifierManager, OpType};
use crate::builder::skill::SkillPipelineMap;
use crate::builder::stat::StatBuilder;
use crate::profile::skill::{
    GlobalTrigger, HitCondition, HitTrigger, HitTriggerOn, SkillTag, TriggerEffect,
};
use crate::profile::StatField;

use super::ARCANA_RUIN_STACK_BUFF_ID;

/// 아르카나 아크 패시브 클래스 전용 노드.
/// sim_effects로 표현 불가한 복잡한 메커니즘을 처리한다.
/// (카드덱 관련 노드, 운명 트리거, 조건부 버프 등)
///
pub fn apply(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    class_nodes: &[ArkPassiveClassNodeInput],
    req: &BuildRequest,
) {
    for node in class_nodes {
        apply_class_node(stat, modifier, node);
        apply_skill_class_node(skill_pipelines, node, req);
        apply_runtime_class_node(global_triggers, skill_pipelines, node, req);
    }
}

fn apply_class_node(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    node: &ArkPassiveClassNodeInput,
) {
    match node.name.as_str() {
        "황후의 탐욕" => {
            add_damage_modifier(modifier, node, SkillTag::CategoryRuin, value(node, 0))
        }
        "황제의 만찬" => {
            add_damage_modifier(modifier, node, SkillTag::CategoryNormal, value(node, 1))
        }
        "황후의 속삭임" => {
            add_damage_modifier(modifier, node, SkillTag::CategoryRuin, value(node, 0));
            add_cooldown_modifier(modifier, node, SkillTag::CategoryNormal, value(node, 1));
            add_cooldown_modifier(modifier, node, SkillTag::CategoryStacked, value(node, 1));
        }
        "또 다른 황제" => stat.add_stat(
            StatField::TargetDamageIncrease,
            format!("arcana:ark_passive:{}:L{}", node.name, node.level),
            value(node, 0) / 100.0,
        ),
        "황제의 심판" => {
            add_damage_modifier(modifier, node, SkillTag::CategoryRuin, -value(node, 0));
            add_damage_modifier(modifier, node, SkillTag::CategoryNormal, value(node, 1));
            add_damage_modifier(modifier, node, SkillTag::CategoryStacked, value(node, 1));
        }
        "초월적인 힘" => {
            add_damage_modifier(
                modifier,
                node,
                SkillTag::CategorySuperAwakening,
                value(node, 0),
            )
        }
        "풀려난 힘" => {
            add_damage_modifier(modifier, node, SkillTag::CategoryHyper, value(node, 0))
        }
        "숨겨진 패" => {
            add_damage_modifier(modifier, node, SkillTag::SkillId(19340), value(node, 0))
        }
        "폴스 딜" => {
            add_damage_modifier(modifier, node, SkillTag::SkillId(19340), value(node, 0))
        }
        "악마의 눈속임" => {
            add_damage_modifier(modifier, node, SkillTag::SkillId(19350), value(node, 0))
        }
        "쿼즈" => add_damage_modifier(modifier, node, SkillTag::SkillId(19350), value(node, 0)),
        _ => {}
    }
}

fn add_ruin_mana_refund(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    percent: f64,
) {
    for skill in &req.skill_data {
        if skill.identity_category != 30 {
            continue;
        }
        let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
            continue;
        };
        pipeline.add_trigger(move |profile| {
            let mut phases = Vec::new();
            for hit in &mut profile.hits {
                if phases.contains(&hit.phase)
                    || !hit.triggers.iter().any(|trigger| matches!(
                        trigger.effect,
                        TriggerEffect::DealDamageByTargetBuffStacks { .. }
                    ))
                {
                    continue;
                }
                phases.push(hit.phase);
                hit.triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::RefundCastMp { percent },
                    chance: 1.0,
                });
            }
            Ok(())
        });
    }
}

fn apply_skill_class_node(
    skill_pipelines: &mut SkillPipelineMap,
    node: &ArkPassiveClassNodeInput,
    req: &BuildRequest,
) {
    match node.name.as_str() {
        "각성 증폭기" => add_awakening_uses(skill_pipelines, req, first_value(node) as u32),
        "폴스 딜" => add_skill_hit_stats(skill_pipelines, req, 19340, 0.0, value(node, 1)),
        "악마의 눈속임" => {
            add_skill_hit_stats(skill_pipelines, req, 19350, value(node, 1), 0.0);
            set_ruin_stack_preserve_chance(skill_pipelines, req, 19350, value(node, 2) / 100.0);
        }
        _ => {}
    }
}

fn add_awakening_uses(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    additional_uses: u32,
) {
    for skill in &req.skill_data {
        if skill.skill_slot != "Awakening" {
            continue;
        }
        if let Some(pipeline) = skill_pipelines.get_mut(&skill.name) {
            pipeline.add_stats(move |profile| {
                profile.max_uses += additional_uses;
                Ok(())
            });
        }
    }
}

fn apply_runtime_class_node(
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    node: &ArkPassiveClassNodeInput,
    req: &BuildRequest,
) {
    match node.name.as_str() {
        "충전된 분노" => add_charged_fury(global_triggers, req, first_value(node) / 100.0),
        "황후의 은총" => add_ruin_mana_refund(skill_pipelines, req, value(node, 0) / 100.0),
        "황후의 탐욕" => {
            crate::builder::register::sim_effects::apply_card_gauge_multiplier_for_skill_group(
                skill_pipelines,
                req,
                2190402,
                value(node, 1),
            );
            crate::builder::register::sim_effects::apply_card_gauge_multiplier_for_skill_group(
                skill_pipelines,
                req,
                2190401,
                -value(node, 2),
            );
        }
        "황제의 만찬" => {
            add_normal_skill_mana_restore(global_triggers, req, value(node, 0) / 100.0)
        }
        "황제의 자비" => global_triggers.push(GlobalTrigger {
            filter: crate::profile::skill::TriggerFilter::CardUse,
            condition: crate::profile::skill::TriggerCondition::Always,
            effect: TriggerEffect::ApplyBuff(crate::profile::skill::BuffSpec {
                id: format!("arcana_ark_passive_2190700_L{}", node.level),
                effects: hashbrown::HashMap::from([(
                    StatField::DamageIncrease,
                    value(node, 1) / 100.0,
                )]),
                max_stacks: 1,
                stack_type: crate::profile::skill::StackType::Refresh,
                duration_ms: (value(node, 0) * 1000.0) as u32,
            }),
            cooldown_ms: None,
        }),
        "황후의 속삭임" => {
            add_four_stack_ruin_speed_buff(skill_pipelines, req, value(node, 2), value(node, 3))
        }
        "황후의 연회" => {
            add_four_stack_ruin_damage_trigger(skill_pipelines, req, first_value(node) / 100.0)
        }
        "잠재력 해방" => {
            add_skill_cooldown(skill_pipelines, req, 19340, value(node, 0));
            add_skill_cooldown(skill_pipelines, req, 19350, value(node, 0));
        }
        "즉각적인 주문" => {
            add_skill_cast_speed_and_mana(
                skill_pipelines,
                req,
                19340,
                value(node, 0),
                value(node, 1),
            );
            add_skill_cast_speed_and_mana(
                skill_pipelines,
                req,
                19350,
                value(node, 0),
                value(node, 1),
            );
        }
        "숨겨진 패" => {
            add_card_draw_on_effect(skill_pipelines, req, 19340, 193402, value(node, 1) / 100.0)
        }
        "쿼즈" => {
            add_four_stack_skill_damage_trigger(skill_pipelines, req, 19350, value(node, 1) / 100.0)
        }
        _ => {}
    }
}

fn add_charged_fury(global_triggers: &mut Vec<GlobalTrigger>, req: &BuildRequest, percent: f64) {
    let skill_ids = req
        .skill_data
        .iter()
        .enumerate()
        .filter_map(|(index, skill)| (skill.skill_slot == "Awakening").then_some(index as u32))
        .collect::<Vec<_>>();
    if skill_ids.is_empty() {
        return;
    }
    global_triggers.push(GlobalTrigger {
        filter: crate::profile::skill::TriggerFilter::AnyHit,
        condition: crate::profile::skill::TriggerCondition::Always,
        effect: TriggerEffect::ReduceCooldownsOnResourceFull {
            resource: "UltimatePoint".to_string(),
            skill_ids,
            percent,
        },
        cooldown_ms: None,
    });
}

fn add_normal_skill_mana_restore(
    global_triggers: &mut Vec<GlobalTrigger>,
    req: &BuildRequest,
    percent: f64,
) {
    for (skill_id, skill) in req.skill_data.iter().enumerate() {
        if skill.identity_category != 28 {
            continue;
        }
        global_triggers.push(GlobalTrigger {
            filter: crate::profile::skill::TriggerFilter::SkillCastId(skill_id as u32),
            condition: crate::profile::skill::TriggerCondition::Always,
            effect: TriggerEffect::GainResourcePercent {
                resource: "Mp".to_string(),
                percent,
            },
            cooldown_ms: None,
        });
    }
}

fn add_skill_cooldown(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    game_skill_id: u32,
    percent: f64,
) {
    let Some(skill) = req
        .skill_data
        .iter()
        .find(|skill| skill.game_skill_id == game_skill_id)
    else {
        return;
    };
    let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
        return;
    };
    pipeline.add_stats(move |profile| {
        profile.cooldown_ms = (profile.cooldown_ms as f64 * (1.0 - percent / 100.0)) as u32;
        profile.charge_recovery_ms =
            (profile.charge_recovery_ms as f64 * (1.0 - percent / 100.0)) as u32;
        Ok(())
    });
}

fn add_skill_cast_speed_and_mana(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    game_skill_id: u32,
    speed_percent: f64,
    mana_reduction_percent: f64,
) {
    let Some(skill) = req
        .skill_data
        .iter()
        .find(|skill| skill.game_skill_id == game_skill_id)
    else {
        return;
    };
    let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
        return;
    };
    pipeline.add_stats(move |profile| {
        for duration in &mut profile.cast_times_ms {
            *duration = (*duration as f64 / (1.0 + speed_percent / 100.0)) as u32;
        }
        if let Some(cost) = profile.resource_costs.get_mut("Mp") {
            *cost *= 1.0 - mana_reduction_percent / 100.0;
        }
        Ok(())
    });
}

fn add_card_draw_on_effect(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    game_skill_id: u32,
    effect_id: u32,
    chance: f64,
) {
    let Some(skill) = req
        .skill_data
        .iter()
        .find(|skill| skill.game_skill_id == game_skill_id)
    else {
        return;
    };
    let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
        return;
    };
    pipeline.add_trigger(move |profile| {
        if let Some(hit) = profile
            .hits
            .iter_mut()
            .find(|hit| hit.effect_id == effect_id)
        {
            hit.triggers.push(HitTrigger {
                on: HitTriggerOn::OnHit,
                effect: TriggerEffect::DrawCard,
                chance,
            });
        }
        Ok(())
    });
}

fn add_four_stack_ruin_speed_buff(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    duration_seconds: f64,
    speed_percent: f64,
) {
    for skill in &req.skill_data {
        if skill.identity_category != 30 {
            continue;
        }
        let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
            continue;
        };
        let buff = crate::profile::skill::BuffSpec {
            id: "arcana_ark_passive_2190900_speed".to_string(),
            effects: hashbrown::HashMap::from([(StatField::AttackSpeed, speed_percent / 100.0)]),
            max_stacks: 1,
            stack_type: crate::profile::skill::StackType::Refresh,
            duration_ms: (duration_seconds * 1000.0) as u32,
        };
        pipeline.add_trigger(move |profile| {
            for hit in &mut profile.hits {
                if let Some(index) = hit.triggers.iter().position(|trigger| matches!(
                    &trigger.effect,
                    TriggerEffect::ConsumeTargetBuff { buff_id, .. } if buff_id == ARCANA_RUIN_STACK_BUFF_ID
                )) {
                    hit.triggers.insert(index, HitTrigger {
                        on: HitTriggerOn::OnHitIf(HitCondition::TargetBuffAtMaxStacks(ARCANA_RUIN_STACK_BUFF_ID.to_string())),
                        effect: TriggerEffect::ApplyBuff(buff.clone()),
                        chance: 1.0,
                    });
                }
            }
            Ok(())
        });
    }
}

fn first_value(node: &ArkPassiveClassNodeInput) -> f64 {
    value(node, 0)
}

fn value(node: &ArkPassiveClassNodeInput, index: usize) -> f64 {
    node.values.get(index).copied().unwrap_or(0.0)
}

fn add_damage_modifier(
    modifier: &mut ModifierManager,
    node: &ArkPassiveClassNodeInput,
    target_tag: SkillTag,
    value_percent: f64,
) {
    if value_percent == 0.0 {
        return;
    }
    modifier.add_modifier(Modifier {
        target_tag,
        stat_name: "DamageMultiplier".to_string(),
        value: value_percent / 100.0,
        op_type: OpType::Multiply,
        source: format!("arcana:ark_passive:{}:L{}", node.name, node.level),
    });
}

fn add_cooldown_modifier(
    modifier: &mut ModifierManager,
    node: &ArkPassiveClassNodeInput,
    target_tag: SkillTag,
    value_percent: f64,
) {
    if value_percent == 0.0 {
        return;
    }
    modifier.add_modifier(Modifier {
        target_tag,
        stat_name: "CooldownReduction".to_string(),
        value: -value_percent / 100.0,
        op_type: OpType::Multiply,
        source: format!("arcana:ark_passive:{}:L{}", node.name, node.level),
    });
}

fn add_skill_hit_stats(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    game_skill_id: u32,
    crit_rate_percent: f64,
    crit_damage_percent: f64,
) {
    let Some(skill) = req
        .skill_data
        .iter()
        .find(|skill| skill.game_skill_id == game_skill_id)
    else {
        return;
    };
    let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
        return;
    };
    pipeline.add_stats(move |profile| {
        for hit in &mut profile.hits {
            hit.crit_chance_bonus += crit_rate_percent / 100.0;
            hit.crit_damage_bonus += crit_damage_percent / 100.0;
        }
        Ok(())
    });
}

fn set_ruin_stack_preserve_chance(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    game_skill_id: u32,
    chance: f64,
) {
    let Some(skill) = req
        .skill_data
        .iter()
        .find(|skill| skill.game_skill_id == game_skill_id)
    else {
        return;
    };
    let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
        return;
    };
    pipeline.add_trigger(move |profile| {
        for hit in &mut profile.hits {
            for trigger in &mut hit.triggers {
                if let TriggerEffect::ConsumeTargetBuff {
                    buff_id,
                    preserve_chance,
                    preserve_required_stacks,
                    ..
                } = &mut trigger.effect
                {
                    if buff_id == ARCANA_RUIN_STACK_BUFF_ID {
                        *preserve_chance = chance;
                        *preserve_required_stacks = 4;
                    }
                }
            }
        }
        Ok(())
    });
}

fn add_four_stack_ruin_damage_trigger(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    damage_bonus: f64,
) {
    if damage_bonus == 0.0 {
        return;
    }

    for skill in &req.character.skills {
        if !req
            .skill_data
            .iter()
            .any(|data| data.name == skill.name && data.identity_category == 30)
        {
            continue;
        }
        let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
            continue;
        };
        pipeline.add_trigger(move |profile| {
            let index = profile.hits.iter().position(|hit| {
                hit.triggers.iter().any(|trigger| matches!(
                    &trigger.effect,
                    TriggerEffect::ConsumeTargetBuff { buff_id, .. } if buff_id == ARCANA_RUIN_STACK_BUFF_ID
                ))
            }).unwrap_or(0);
            if let Some(hit) = profile.hits.get_mut(index) {
                hit.triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHitIf(HitCondition::TargetBuffAtMaxStacks(
                        ARCANA_RUIN_STACK_BUFF_ID.to_string(),
                    )),
                    effect: TriggerEffect::DamageBonus(damage_bonus),
                    chance: 1.0,
                });
            }
            Ok(())
        });
    }
}

fn add_four_stack_skill_damage_trigger(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    game_skill_id: u32,
    damage_bonus: f64,
) {
    let Some(skill) = req
        .skill_data
        .iter()
        .find(|skill| skill.game_skill_id == game_skill_id)
    else {
        return;
    };
    let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
        return;
    };
    pipeline.add_trigger(move |profile| {
        let index = profile.hits.iter().position(|hit| {
            hit.triggers.iter().any(|trigger| matches!(
                &trigger.effect,
                TriggerEffect::ConsumeTargetBuff { buff_id, .. } if buff_id == ARCANA_RUIN_STACK_BUFF_ID
            ))
        }).unwrap_or(0);
        if let Some(hit) = profile.hits.get_mut(index) {
            hit.triggers.push(HitTrigger {
                on: HitTriggerOn::OnHitIf(HitCondition::TargetBuffAtMaxStacks(
                    ARCANA_RUIN_STACK_BUFF_ID.to_string(),
                )),
                effect: TriggerEffect::DamageBonus(damage_bonus),
                chance: 1.0,
            });
        }
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, level: u32, values: Vec<f64>) -> ArkPassiveClassNodeInput {
        ArkPassiveClassNodeInput {
            name: name.to_string(),
            level,
            values,
        }
    }

    #[test]
    fn empress_greed_adds_ruin_damage_modifier() {
        let mut stat = StatBuilder::new();
        let mut modifier = ModifierManager::new();
        apply_class_node(&mut stat, &mut modifier, &node("황후의 탐욕", 5, vec![4.5]));

        let compiled = modifier.compile();
        let result = compiled.get_multiplier(&SkillTag::CategoryRuin, "DamageMultiplier");

        assert!(
            (result - 1.045).abs() < 1e-9,
            "expected 1.045, got {result}"
        );
    }

    #[test]
    fn empress_festival_is_runtime_only() {
        let mut stat = StatBuilder::new();
        let mut modifier = ModifierManager::new();
        apply_class_node(
            &mut stat,
            &mut modifier,
            &node("황후의 연회", 3, vec![22.0]),
        );

        let compiled = modifier.compile();
        let result = compiled.get_multiplier(&SkillTag::CategoryRuin, "DamageMultiplier");

        assert_eq!(result, 1.0);
    }

    #[test]
    fn emperor_judgment_splits_damage_by_raw_category() {
        let mut stat = StatBuilder::new();
        let mut modifier = ModifierManager::new();
        apply_class_node(
            &mut stat,
            &mut modifier,
            &node("황제의 심판", 5, vec![10.0, 10.0]),
        );
        let compiled = modifier.compile();
        assert!(
            (compiled.get_multiplier(&SkillTag::CategoryRuin, "DamageMultiplier") - 0.9).abs()
                < 1e-9
        );
        assert!(
            (compiled.get_multiplier(&SkillTag::CategoryNormal, "DamageMultiplier") - 1.1).abs()
                < 1e-9
        );
        assert!(
            (compiled.get_multiplier(&SkillTag::CategoryStacked, "DamageMultiplier") - 1.1).abs()
                < 1e-9
        );
    }

    #[test]
    fn transcendental_power_targets_every_super_awakening() {
        let mut stat = StatBuilder::new();
        let mut modifier = ModifierManager::new();
        apply_class_node(
            &mut stat,
            &mut modifier,
            &node("초월적인 힘", 5, vec![50.0]),
        );

        let compiled = modifier.compile();
        assert!((compiled.get_multiplier(
            &SkillTag::CategorySuperAwakening,
            "DamageMultiplier"
        ) - 1.5)
            .abs()
            < 1e-9);
        assert_eq!(
            compiled.get_multiplier(&SkillTag::SkillId(19360), "DamageMultiplier"),
            1.0
        );
    }
}
