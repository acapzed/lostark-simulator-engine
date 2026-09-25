use hashbrown::HashMap;

use crate::builder::dto::BuildRequest;
use crate::builder::skill::SkillPipelineMap;
use crate::profile::skill::{
    BuffSpec, GlobalTrigger, HitCondition, HitTrigger, HitTriggerOn, RuntimeDamageSpec, StackType,
    TriggerCondition, TriggerEffect, TriggerFilter,
};
use crate::profile::StatField;

/// 룬 효과를 해당 스킬의 SkillPipeline / GlobalTrigger에 등록한다.
///
pub fn register(
    skill_pipelines: &mut SkillPipelineMap,
    global_triggers: &mut Vec<GlobalTrigger>,
    req: &BuildRequest,
) {
    let normal_skill_ids = req
        .character
        .skills
        .iter()
        .enumerate()
        .filter(|(_, skill)| {
            req.skill_data
                .iter()
                .find(|data| data.name == skill.name)
                .is_some_and(|data| data.skill_slot == "Normal")
        })
        .map(|(skill_id, _)| skill_id as u32)
        .collect::<Vec<_>>();

    for (skill_id, skill) in req.character.skills.iter().enumerate() {
        let Some(rune) = &skill.rune else { continue };
        let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else { continue };
        for effect in &rune.effects {
            match effect.kind.as_str() {
                "cast_speed" => {
                    let multiplier = (1.0 - effect.value / 100.0).max(0.0);
                    pipeline.add_stats(move |profile| {
                        for duration in &mut profile.cast_times_ms {
                            *duration = (*duration as f64 * multiplier).round() as u32;
                        }
                        Ok(())
                    });
                }
                "mana_cost_reduction" => {
                    let multiplier = (1.0 - effect.value / 100.0).max(0.0);
                    pipeline.add_stats(move |profile| {
                        for cost in profile.resource_costs.values_mut() {
                            *cost *= multiplier;
                        }
                        Ok(())
                    });
                }
                "identity_hit_gain" => {
                    let multiplier = 1.0 + effect.value / 100.0;
                    pipeline.add_trigger(move |profile| {
                        for hit in &mut profile.hits {
                            for trigger in &mut hit.triggers {
                                if let TriggerEffect::GainResource { resource, amount } = &mut trigger.effect {
                                    if resource == "CardGauge" {
                                        *amount *= multiplier;
                                    }
                                } else if let TriggerEffect::ConsumeTargetBuff {
                                    preserve_resource_gain: Some((resource, amount)),
                                    ..
                                } = &mut trigger.effect
                                {
                                    if resource == "CardGauge" {
                                        *amount *= multiplier;
                                    }
                                }
                            }
                        }
                        Ok(())
                    });
                }
                "on_cast_cooldown_reduction" => {
                    global_triggers.push(GlobalTrigger {
                        filter: TriggerFilter::SkillCastId(skill_id as u32),
                        condition: TriggerCondition::Always,
                        effect: TriggerEffect::Chance {
                            chance: effect.chance,
                            effect: Box::new(TriggerEffect::ReduceCooldowns {
                                skill_ids: normal_skill_ids.clone(),
                                percent: effect.value / 100.0,
                            }),
                        },
                        cooldown_ms: None,
                    });
                }
                "on_cast_attack_speed_buff" => {
                    let trigger = GlobalTrigger {
                        filter: TriggerFilter::SkillCastId(skill_id as u32),
                        condition: TriggerCondition::Always,
                        effect: TriggerEffect::Chance {
                            chance: effect.chance,
                            effect: Box::new(TriggerEffect::ApplyBuff(BuffSpec {
                                id: effect.buff_id.clone(),
                                effects: HashMap::from([
                                    (StatField::AttackSpeed, effect.value / 100.0),
                                    (StatField::MovementSpeed, effect.value / 100.0),
                                ]),
                                max_stacks: 1,
                                stack_type: StackType::Refresh,
                                duration_ms: effect.duration_ms.unwrap_or(0),
                            })),
                        },
                        cooldown_ms: None,
                    };
                    global_triggers.push(trigger);
                }
                "on_hit_buff" => {
                    let trigger = HitTrigger {
                        on: HitTriggerOn::OnHit,
                        effect: TriggerEffect::ApplyBuff(BuffSpec {
                            id: effect.buff_id.clone(),
                            effects: HashMap::new(),
                            max_stacks: 1,
                            stack_type: StackType::Refresh,
                            duration_ms: effect.duration_ms.unwrap_or(0),
                        }),
                        chance: effect.chance,
                    };
                    pipeline.add_trigger(move |profile| {
                        for hit in &mut profile.hits {
                            hit.triggers.push(trigger.clone());
                        }
                        Ok(())
                    });
                }
                "on_hit_judgment" => {
                    let buff_id = effect.buff_id.clone();
                    let consumes_buff_id = effect.consumes_buff_id.clone();
                    let cooldown_reduction = effect.value / 100.0;
                    let trigger = HitTrigger {
                        on: HitTriggerOn::OnHitIf(HitCondition::BuffActive(
                            consumes_buff_id.clone(),
                        )),
                        effect: TriggerEffect::Sequence(vec![
                            TriggerEffect::ConsumeBuffStacks {
                                buff_id: consumes_buff_id,
                                stacks: 1,
                            },
                            TriggerEffect::ApplyBuff(BuffSpec {
                                id: buff_id,
                                effects: HashMap::from([(
                                    StatField::CooldownReduction,
                                    cooldown_reduction,
                                )]),
                                max_stacks: 1,
                                stack_type: StackType::Refresh,
                                duration_ms: effect.duration_ms.unwrap_or(0),
                            }),
                        ]),
                        chance: effect.chance,
                    };
                    pipeline.add_trigger(move |profile| {
                        for hit in &mut profile.hits {
                            hit.triggers.push(trigger.clone());
                        }
                        Ok(())
                    });
                }
                "on_hit_dot" => {
                    let trigger = HitTrigger {
                        on: HitTriggerOn::OnHit,
                        effect: TriggerEffect::ScheduleDot {
                            dot_id: effect.dot_id.clone(),
                            hit: RuntimeDamageSpec {
                                hit_id: effect.hit_id.clone(),
                                name: effect.name.clone(),
                                base_damage: effect.fixed_damage,
                                skill_modifier: effect.damage_ratio,
                                damage_increase: 0.0,
                                crit_chance_bonus: 0.0,
                                crit_damage_bonus: 0.0,
                                random_crit_damage_chance: 0.0,
                                random_crit_damage_bonus: 0.0,
                                defense_ignore: 0.0,
                                defense_ignore_chance: 0.0,
                            },
                            first_tick_ms: effect.first_tick_ms,
                            tick_interval_ms: effect.tick_interval_ms,
                            tick_count: effect.tick_count,
                            refresh: true,
                        },
                        chance: 1.0,
                    };
                    pipeline.add_trigger(move |profile| {
                        for hit in &mut profile.hits {
                            hit.triggers.push(trigger.clone());
                        }
                        Ok(())
                    });
                }
                _ => {}
            }
        }
    }
}
