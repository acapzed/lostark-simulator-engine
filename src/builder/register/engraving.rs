use crate::builder::dto::BuildRequest;
use crate::builder::modifier::ModifierManager;
use crate::builder::skill::SkillPipelineMap;
use crate::builder::register::sim_effects;
use crate::builder::stat::StatBuilder;
use crate::profile::skill::{
    BuffSpec, GlobalTrigger, StackType, TriggerCondition, TriggerEffect, TriggerFilter,
};
use crate::profile::{SkillTag, StatField};

fn raw_value(values: &[f64], index: usize) -> f64 {
    values.get(index).copied().unwrap_or(0.0) / 10_000.0
}

fn raw_percent(values: &[f64], index: usize) -> f64 {
    values.get(index).copied().unwrap_or(0.0) / 100.0
}

fn raw_grade(grade: &str) -> Option<u32> {
    match grade {
        "희귀" => Some(2),
        "영웅" => Some(3),
        "전설" => Some(4),
        "유물" => Some(5),
        _ => None,
    }
}

fn selected_feature(
    engraving: &crate::builder::dto::EngravingInput,
) -> Option<&crate::builder::dto::EngravingFeatureRowInput> {
    let data = engraving.feature_data.as_ref()?;
    if let Some(grade) = raw_grade(&engraving.grade) {
        return data
            .grades
            .iter()
            .find(|row| row.grade == grade)?
            .stages
            .iter()
            .find(|row| row.stage == engraving.level.saturating_add(engraving.ability_level));
    }
    data.base_levels
        .get(engraving.level.saturating_sub(1) as usize)
}

fn apply_adrenaline(
    global_triggers: &mut Vec<GlobalTrigger>,
    feature: &crate::builder::dto::EngravingFeatureRowInput,
    source: &str,
) -> bool {
    let attack_buff_id = feature.values.get(1).copied().unwrap_or(0.0) as u32;
    let Some(attack) = feature.skill_buffs.iter().find(|buff| {
        buff.id == attack_buff_id
            && buff.passive_option.r#type == 2
            && buff.passive_option.key_stat == 49
            && buff.duration > 0
            && buff.overlap > 0
    }) else {
        return false;
    };
    let Some(crit) = feature.skill_buffs.iter().find(|buff| {
        buff.passive_option.r#type == 2 && buff.passive_option.key_stat == 74
    }) else {
        return false;
    };

    let attack_id = format!("{source}:attack");
    let duration_ms = attack.duration as u32;
    for filter in [TriggerFilter::SkillCast, TriggerFilter::CardUse] {
        global_triggers.push(GlobalTrigger {
            filter: filter.clone(),
            condition: TriggerCondition::Always,
            effect: TriggerEffect::ApplyBuffStacks {
                spec: BuffSpec {
                    id: attack_id.clone(),
                    effects: hashbrown::HashMap::from([(
                        StatField::AttackPowerMul,
                        attack.passive_option.value / 10_000.0,
                    )]),
                    max_stacks: attack.overlap,
                    stack_type: StackType::Refresh,
                    duration_ms,
                },
                stacks: 1,
            },
            cooldown_ms: None,
        });
        global_triggers.push(GlobalTrigger {
            filter,
            condition: TriggerCondition::BuffAtMaxStacks(attack_id.clone()),
            effect: TriggerEffect::ApplyBuff(BuffSpec {
                id: format!("{source}:crit"),
                effects: hashbrown::HashMap::from([(
                    StatField::CritRate,
                    crit.passive_option.value / 100.0,
                )]),
                max_stacks: 1,
                stack_type: StackType::Refresh,
                duration_ms,
            }),
            cooldown_ms: None,
        });
    }
    true
}

fn apply_mana_flow(
    global_triggers: &mut Vec<GlobalTrigger>,
    feature: &crate::builder::dto::EngravingFeatureRowInput,
    source: &str,
) -> bool {
    let stack_buff_id = feature.values.get(1).copied().unwrap_or(0.0) as u32;
    let Some(stack) = feature.skill_buffs.iter().find(|buff| {
        buff.id == stack_buff_id && buff.duration > 0 && buff.overlap > 0
    }) else {
        return false;
    };
    let Some(cooldown) = feature.skill_buffs.iter().find(|buff| {
        buff.passive_option.r#type == 35 && buff.passive_option.key_index == 36
    }) else {
        return false;
    };

    let buff_id = format!("{source}:mana-flow");
    global_triggers.push(GlobalTrigger {
        filter: TriggerFilter::SkillCast,
        condition: TriggerCondition::BuffAtMaxStacks(buff_id.clone()),
        effect: TriggerEffect::ReduceSkillCooldown {
            percent: cooldown.passive_option.value / 10_000.0,
        },
        cooldown_ms: None,
    });
    global_triggers.push(GlobalTrigger {
        filter: TriggerFilter::SkillCast,
        condition: TriggerCondition::Always,
        effect: TriggerEffect::ApplyBuffStacks {
            spec: BuffSpec {
                id: buff_id,
                effects: hashbrown::HashMap::new(),
                max_stacks: stack.overlap,
                stack_type: StackType::Refresh,
                duration_ms: stack.duration as u32,
            },
            stacks: 1,
        },
        cooldown_ms: None,
    });
    true
}

fn apply_lightning_fury(
    global_triggers: &mut Vec<GlobalTrigger>,
    feature: &crate::builder::dto::EngravingFeatureRowInput,
    source: &str,
) -> bool {
    let Some(stack) = feature.skill_buff.as_ref().filter(|buff| {
        buff.id == feature.values.get(1).copied().unwrap_or(0.0) as u32
            && buff.duration > 0
            && buff.overlap > 0
    }) else {
        return false;
    };
    let Some(damage) = feature.skill_effects.iter().find(|effect| effect.key == 2) else {
        return false;
    };

    let buff_id = format!("{source}:lightning-orb");
    global_triggers.push(GlobalTrigger {
        filter: TriggerFilter::AnyHit,
        condition: TriggerCondition::Chance(raw_value(&feature.values, 0)),
        effect: TriggerEffect::ApplyBuffStacks {
            spec: BuffSpec {
                id: buff_id.clone(),
                effects: hashbrown::HashMap::new(),
                max_stacks: stack.overlap,
                stack_type: StackType::Refresh,
                duration_ms: stack.duration as u32,
            },
            stacks: 1,
        },
        cooldown_ms: Some(feature.cooldown),
    });
    global_triggers.push(GlobalTrigger {
        filter: TriggerFilter::AnyHit,
        condition: TriggerCondition::BuffAtMaxStacks(buff_id.clone()),
        effect: TriggerEffect::Sequence(vec![
            TriggerEffect::ScheduleDot {
                dot_id: format!("{source}:lightning-explosion"),
                hit: crate::profile::skill::RuntimeDamageSpec {
                    hit_id: "lightning_fury_explosion".to_string(),
                    name: "번개의 분노 폭발".to_string(),
                    base_damage: (damage.value_a + damage.value_b) / 2.0,
                    skill_modifier: damage.value_f / 10_000.0,
                    damage_increase: 0.0,
                    crit_chance_bonus: 0.0,
                    crit_damage_bonus: 0.0,
                    random_crit_damage_chance: 0.0,
                    random_crit_damage_bonus: 0.0,
                    defense_ignore: 0.0,
                    defense_ignore_chance: 0.0,
                },
                first_tick_ms: 0,
                tick_interval_ms: 0,
                tick_count: 1,
                refresh: false,
            },
            TriggerEffect::ConsumeBuffStacks {
                buff_id,
                stacks: stack.overlap,
            },
        ]),
        cooldown_ms: None,
    });
    true
}

fn apply_sight_focus(
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    feature: &crate::builder::dto::EngravingFeatureRowInput,
    req: &BuildRequest,
    source: &str,
) -> bool {
    let skill_name = req.sight_focus_skill_name.trim();
    let Some(skill_id) = req
        .character
        .skills
        .iter()
        .position(|skill| skill.name == skill_name)
        .map(|index| index as u32)
    else {
        return false;
    };
    let Some(skill_data) = req.skill_data.iter().find(|skill| skill.name == skill_name) else {
        return false;
    };
    let Some(buff) = feature.skill_buffs.iter().find(|buff| {
        buff.id == feature.values.first().copied().unwrap_or(0.0) as u32 && buff.duration > 0
    }) else {
        return false;
    };
    let Some(pipeline) = skill_pipelines.get_mut(skill_name) else {
        return false;
    };

    let buff_id = format!("{source}:sight-focus");
    let bonus = raw_value(&feature.values, 2)
        * if skill_data.skill_slot == "Awakening" { 0.5 } else { 1.0 };
    let cast_buff_id = buff_id.clone();
    pipeline.add_stats(move |profile| {
        profile.cast_buff_damage_bonuses.push((cast_buff_id.clone(), bonus));
        Ok(())
    });
    global_triggers.push(GlobalTrigger {
        filter: TriggerFilter::BeforeSkillCastId(skill_id),
        condition: TriggerCondition::Always,
        effect: TriggerEffect::ApplyBuff(BuffSpec {
            id: buff_id.clone(),
            effects: hashbrown::HashMap::new(),
            max_stacks: 1,
            stack_type: StackType::Refresh,
            duration_ms: buff.duration as u32,
        }),
        cooldown_ms: Some(feature.cooldown),
    });
    global_triggers.push(GlobalTrigger {
        filter: TriggerFilter::SkillCastId(skill_id),
        condition: TriggerCondition::BuffActive(buff_id.clone()),
        effect: TriggerEffect::ConsumeBuffStacks { buff_id, stacks: 1 },
        cooldown_ms: None,
    });
    true
}

fn apply_crushing_fist(
    skill_pipelines: &mut SkillPipelineMap,
    feature: &crate::builder::dto::EngravingFeatureRowInput,
    req: &BuildRequest,
    source: &str,
) -> bool {
    if !req.assumed_counter_success {
        return false;
    }
    let Some(attack) = feature.skill_buffs.iter().find(|buff| {
        buff.passive_option.r#type == 2 && buff.passive_option.key_stat == 49 && buff.duration > 0
    }) else {
        return false;
    };
    let Some(target) = feature.skill_buffs.iter().find(|buff| {
        buff.key == 88 && buff.values.get(7).copied().unwrap_or(0.0) > 0.0 && buff.duration > 0
    }) else {
        return false;
    };
    let self_buff = BuffSpec {
        id: format!("{source}:attack"),
        effects: hashbrown::HashMap::from([(
            StatField::AttackPowerMul,
            attack.passive_option.value / 10_000.0,
        )]),
        max_stacks: 1,
        stack_type: StackType::Refresh,
        duration_ms: attack.duration as u32,
    };
    let target_buff = BuffSpec {
        id: format!("{source}:target"),
        effects: hashbrown::HashMap::from([(
            StatField::TargetDamageIncrease,
            target.values[7] / 10_000.0,
        )]),
        max_stacks: 1,
        stack_type: StackType::Refresh,
        duration_ms: target.duration as u32,
    };
    let mut attached = false;
    for skill in &req.character.skills {
        let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
            continue;
        };
        attached = true;
        let self_buff = self_buff.clone();
        let target_buff = target_buff.clone();
        pipeline.add_trigger(move |profile| {
            let mut phases = Vec::new();
            for hit in &mut profile.hits {
                if hit.counter_attack && !phases.contains(&hit.phase) {
                    phases.push(hit.phase);
                    hit.triggers.push(crate::profile::skill::HitTrigger {
                        on: crate::profile::skill::HitTriggerOn::OnHit,
                        effect: TriggerEffect::Sequence(vec![
                            TriggerEffect::ApplyBuff(self_buff.clone()),
                            TriggerEffect::ApplyTargetBuff(target_buff.clone()),
                        ]),
                        chance: 1.0,
                    });
                }
            }
            Ok(())
        });
    }
    attached
}

fn apply_ether_boy(
    global_triggers: &mut Vec<GlobalTrigger>,
    feature: &crate::builder::dto::EngravingFeatureRowInput,
    req: &BuildRequest,
    source: &str,
) -> bool {
    let Some(pickup_delay_ms) = req.ether_pickup_delay_ms else {
        return false;
    };
    let drops: Vec<_> = feature
        .drop_ethers
        .iter()
        .filter(|drop| drop.audience == "self")
        .collect();
    if drops.len() != 5 || (feature.base_ratio * drops.len() as f64 - 100.0).abs() > 1e-9 {
        return false;
    }

    let choices = drops
        .into_iter()
        .map(|drop| {
            let effect = if let Some(effect) = drop.effect.as_ref().filter(|effect| effect.key == 18) {
                TriggerEffect::GainResourcePercent {
                    resource: "Mp".to_string(),
                    percent: effect.value_b / 10_000.0,
                }
            } else if let Some(buff) = drop.skill_buff.as_ref().filter(|buff| {
                buff.passive_option.r#type == 2
                    && buff.passive_option.key_stat == 80
                    && buff.duration > 0
            }) {
                TriggerEffect::ApplyBuff(BuffSpec {
                    id: format!("{source}:ether:{}", drop.id),
                    effects: hashbrown::HashMap::from([(
                        StatField::MovementSpeed,
                        buff.passive_option.value / 10_000.0,
                    )]),
                    max_stacks: 1,
                    stack_type: StackType::Refresh,
                    duration_ms: buff.duration as u32,
                })
            } else {
                TriggerEffect::Sequence(Vec::new())
            };
            TriggerEffect::Schedule {
                delay_ms: drop.delay_time.saturating_add(pickup_delay_ms),
                effect: Box::new(effect),
            }
        })
        .collect();
    global_triggers.push(GlobalTrigger {
        filter: TriggerFilter::AnyHit,
        condition: TriggerCondition::Always,
        effect: TriggerEffect::RandomChoice(choices),
        cooldown_ms: Some(feature.cooldown),
    });
    true
}

fn apply_ether_predator(
    global_triggers: &mut Vec<GlobalTrigger>,
    feature: &crate::builder::dto::EngravingFeatureRowInput,
    req: &BuildRequest,
    source: &str,
) -> bool {
    let Some(pickup_delay_ms) = req.ether_pickup_delay_ms else {
        return false;
    };
    let Some(three_stack_drop) = feature.drop_ethers.first() else {
        return false;
    };
    let Some(one_stack_drop) = feature.drop_ethers.get(1) else {
        return false;
    };
    let Some(buff) = one_stack_drop.skill_buff.as_ref().filter(|buff| {
        buff.passive_option.r#type == 2
            && buff.passive_option.key_stat == 49
            && buff.duration > 0
            && buff.overlap > 0
    }) else {
        return false;
    };
    if three_stack_drop.skill_buff.as_ref().map(|buff| buff.id) != Some(buff.id)
        || feature.values.get(3).copied() != Some(250.0)
        || !matches!(three_stack_drop.effect.as_ref(), Some(effect) if effect.key == 7 && effect.value_b == 3.0)
        || !matches!(one_stack_drop.effect.as_ref(), Some(effect) if effect.key == 7 && effect.value_b == 0.0)
    {
        return false;
    }

    let spec = BuffSpec {
        id: format!("{source}:ether-predator"),
        effects: hashbrown::HashMap::from([(
            StatField::AttackPowerMul,
            buff.passive_option.value / 10_000.0,
        )]),
        max_stacks: buff.overlap,
        stack_type: StackType::Refresh,
        duration_ms: buff.duration as u32,
    };
    let scheduled = |drop: &crate::builder::dto::EngravingDropEtherInput, stacks| {
        TriggerEffect::Schedule {
            delay_ms: drop.delay_time.saturating_add(pickup_delay_ms),
            effect: Box::new(TriggerEffect::ApplyBuffStacks {
                spec: spec.clone(),
                stacks,
            }),
        }
    };
    let one_stack = scheduled(one_stack_drop, 1);
    global_triggers.push(GlobalTrigger {
        filter: TriggerFilter::AnyHit,
        condition: TriggerCondition::Always,
        effect: TriggerEffect::RandomChoice(vec![
            scheduled(three_stack_drop, 3),
            one_stack.clone(),
            one_stack.clone(),
            one_stack,
        ]),
        cooldown_ms: Some(feature.cooldown),
    });
    true
}

fn apply_necromancy(
    global_triggers: &mut Vec<GlobalTrigger>,
    feature: &crate::builder::dto::EngravingFeatureRowInput,
    req: &BuildRequest,
    source: &str,
) -> bool {
    let Some(travel_delay_ms) = req.necromancy_travel_delay_ms else {
        return false;
    };
    let Some(npc) = feature.summon_npc.as_ref() else {
        return false;
    };
    let Some(damage) = npc
        .skills
        .iter()
        .filter(|skill| skill.probability > 0.0)
        .flat_map(|skill| &skill.action_effects)
        .find(|effect| effect.key == 1)
    else {
        return false;
    };
    let coefficient = npc.coefficient / 100.0;
    global_triggers.push(GlobalTrigger {
        filter: TriggerFilter::AnyHit,
        condition: TriggerCondition::Always,
        effect: TriggerEffect::ScheduleDot {
            dot_id: format!("{source}:necromancy"),
            hit: crate::profile::skill::RuntimeDamageSpec {
                hit_id: "necromancy_explosion".to_string(),
                name: "강령술 자폭병 폭발".to_string(),
                base_damage: (damage.value_a + damage.value_b) / 2.0 * coefficient,
                skill_modifier: damage.value_f / 10_000.0 * coefficient,
                damage_increase: 0.0,
                crit_chance_bonus: 0.0,
                crit_damage_bonus: 0.0,
                random_crit_damage_chance: 0.0,
                random_crit_damage_bonus: 0.0,
                defense_ignore: 0.0,
                defense_ignore_chance: 0.0,
            },
            first_tick_ms: travel_delay_ms.saturating_add(damage.time_ms),
            tick_interval_ms: 0,
            tick_count: 1,
            refresh: false,
        },
        cooldown_ms: Some(feature.cooldown),
    });
    true
}

fn apply_raw_feature(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    feature_type: u32,
    values: &[f64],
    combat_effect: Option<&crate::builder::dto::EngravingCombatEffectInput>,
    skill_buff: Option<&crate::builder::dto::EngravingSkillBuffInput>,
    source: &str,
) -> bool {
    match feature_type {
        2 => modifier.add_modifier(crate::builder::modifier::Modifier {
            target_tag: SkillTag::All,
            stat_name: "DamageMultiplier".to_string(),
            value: raw_value(values, 1),
            op_type: crate::builder::modifier::OpType::Multiply,
            source: source.to_string(),
        }),
        3 => stat.add_stat(StatField::AttackSpeed, source, raw_value(values, 0)),
        5 => {
            let full_health_meets_condition = skill_buff.is_some_and(|buff| {
                buff.visible_filter_type == 1 && buff.visible_filter_value <= 100.0
            });
            if let Some(effect) = combat_effect.filter(|effect| {
                full_health_meets_condition && effect.action_type == 1 && effect.action_actor == 2
            }) {
                modifier.add_modifier(crate::builder::modifier::Modifier {
                    target_tag: SkillTag::All,
                    stat_name: "DamageMultiplier".to_string(),
                    value: raw_value(&effect.args, 0),
                    op_type: crate::builder::modifier::OpType::Multiply,
                    source: source.to_string(),
                });
            }
        }
        10 => stat.add_stat(StatField::TargetDamageIncrease, source, raw_value(values, 0)),
        105 => modifier.add_modifier(crate::builder::modifier::Modifier {
            target_tag: SkillTag::All,
            stat_name: "DamageMultiplier".to_string(),
            value: raw_value(values, 1),
            op_type: crate::builder::modifier::OpType::Multiply,
            source: source.to_string(),
        }),
        112 | 120 => modifier.add_modifier(crate::builder::modifier::Modifier {
            target_tag: SkillTag::All,
            stat_name: "DamageMultiplier".to_string(),
            value: raw_value(values, 0),
            op_type: crate::builder::modifier::OpType::Multiply,
            source: source.to_string(),
        }),
        114 => modifier.add_modifier(crate::builder::modifier::Modifier {
            target_tag: SkillTag::All,
            stat_name: "DamageMultiplier".to_string(),
            value: raw_value(values, 1),
            op_type: crate::builder::modifier::OpType::Multiply,
            source: source.to_string(),
        }),
        333 => modifier.add_modifier(crate::builder::modifier::Modifier {
            target_tag: SkillTag::NonDirectionalNonAwakening,
            stat_name: "DamageMultiplier".to_string(),
            value: raw_value(values, 0),
            op_type: crate::builder::modifier::OpType::Multiply,
            source: source.to_string(),
        }),
        334 => {
            stat.add_stat(StatField::CritRate, source, raw_percent(values, 0));
            stat.add_stat(StatField::CritDmg, source, -raw_percent(values, 1));
        }
        339 => {
            stat.add_stat(StatField::AttackSpeed, source, -raw_value(values, 0));
            modifier.add_modifier(crate::builder::modifier::Modifier {
                target_tag: SkillTag::All,
                stat_name: "DamageMultiplier".to_string(),
                value: raw_value(values, 1),
                op_type: crate::builder::modifier::OpType::Multiply,
                source: source.to_string(),
            });
        }
        12 | 340 => {
            let target_tag = if feature_type == 12 {
                SkillTag::Charging
            } else {
                SkillTag::HoldingCasting
            };
            for (stat_name, value) in [
                ("DamageMultiplier", raw_value(values, 1)),
                ("CastTimeReduction", -raw_value(values, 0)),
            ] {
                modifier.add_modifier(crate::builder::modifier::Modifier {
                    target_tag: target_tag.clone(),
                    stat_name: stat_name.to_string(),
                    value,
                    op_type: crate::builder::modifier::OpType::Multiply,
                    source: source.to_string(),
                });
            }
        }
        27 => {
            stat.add_stat(StatField::CritDmg, source, raw_percent(values, 2));
            let chance = raw_value(values, 0);
            let reduction = raw_value(values, 1);
            stat.add_flat(source, move |profile| {
                profile.random_damage_reduction_chance = chance;
                profile.random_damage_reduction = reduction;
            });
        }
        116 | 325 => {
            if let Some(effect) = combat_effect.filter(|effect| {
                effect.action_type == 1 && effect.action_actor == 2
            }) {
                modifier.add_modifier(crate::builder::modifier::Modifier {
                    target_tag: SkillTag::All,
                    stat_name: "DamageMultiplier".to_string(),
                    value: raw_value(&effect.args, 0),
                    op_type: crate::builder::modifier::OpType::Multiply,
                    source: source.to_string(),
                });
            }
            modifier.add_modifier(crate::builder::modifier::Modifier {
                target_tag: if feature_type == 116 {
                    SkillTag::AttackBack
                } else {
                    SkillTag::AttackHead
                },
                stat_name: "DamageMultiplier".to_string(),
                value: raw_value(values, 0),
                op_type: crate::builder::modifier::OpType::Multiply,
                source: source.to_string(),
            });
        }
        121 => {
            let coefficient = raw_value(values, 0);
            stat.add_flat(source, move |profile| {
                profile.movement_speed_damage_coefficient = coefficient;
            });
        }
        279 => modifier.add_modifier(crate::builder::modifier::Modifier {
            target_tag: SkillTag::UsesMana,
            stat_name: "DamageMultiplier".to_string(),
            value: raw_value(values, 1),
            op_type: crate::builder::modifier::OpType::Multiply,
            source: source.to_string(),
        }),
        _ => return false,
    }
    true
}

/// 각인 효과를 simEffects 데이터 기반으로 StatBuilder / ModifierManager / GlobalTrigger에 등록한다.
///
/// 어빌리티 스톤 활성화 효과는 stone.engravings의 이름으로 매칭하여 적용한다.
pub fn register(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
) {
    for engraving in &req.character.engravings {
        let source = format!("engraving:{}:L{}", engraving.name, engraving.level);
        if let Some(feature) = selected_feature(engraving) {
            if feature.feature_type == 2
                && req.target_hp_percent > raw_percent(&feature.values, 0)
            {
                continue;
            }
            if feature.feature_type == 105 && !req.assumed_low_hp {
                continue;
            }
            if feature.feature_type == 112 && !req.assumed_target_staggered {
                continue;
            }
            if feature.feature_type == 120 && !req.assumed_shield_active {
                continue;
            }
            if feature.feature_type == 118 {
                apply_mana_flow(global_triggers, feature, &source);
                continue;
            }
            if feature.feature_type == 122 {
                let cooldown_multiplier = 1.0 - raw_value(&feature.values, 0);
                let additional_uses = feature.values.get(1).copied().unwrap_or(0.0) as u32;
                for skill in req.skill_data.iter().filter(|skill| skill.skill_slot == "Awakening") {
                    if let Some(pipeline) = skill_pipelines.get_mut(&skill.name) {
                        pipeline.add_stats(move |profile| {
                            profile.cooldown_ms =
                                (profile.cooldown_ms as f64 * cooldown_multiplier).round() as u32;
                            profile.max_uses += additional_uses;
                            Ok(())
                        });
                    }
                }
                continue;
            }
            if feature.feature_type == 113 {
                apply_lightning_fury(global_triggers, feature, &source);
                continue;
            }
            if feature.feature_type == 103 {
                apply_crushing_fist(skill_pipelines, feature, req, &source);
                continue;
            }
            if feature.feature_type == 4 {
                apply_ether_predator(global_triggers, feature, req, &source);
                continue;
            }
            if feature.feature_type == 22 {
                apply_ether_boy(global_triggers, feature, req, &source);
                continue;
            }
            if feature.feature_type == 110 {
                apply_necromancy(global_triggers, feature, req, &source);
                continue;
            }
            if feature.feature_type == 335 {
                apply_sight_focus(global_triggers, skill_pipelines, feature, req, &source);
                continue;
            }
            if feature.feature_type == 336 {
                apply_adrenaline(global_triggers, feature, &source);
                continue;
            }
            apply_raw_feature(
                stat,
                modifier,
                feature.feature_type,
                &feature.values,
                feature.combat_effect.as_ref(),
                feature.skill_buff.as_ref(),
                &source,
            );
            continue;
        }
        let Some(sim) = &engraving.sim_effects else {
            continue;
        };

        // 등급별 효과 조회
        let Some(grade_effects) = sim.grades.get(&engraving.grade) else {
            continue;
        };

        // 각인 레벨 효과 (L1/L2/L3)
        let level_idx = engraving.level.saturating_sub(1) as usize;
        if let Some(effects) = grade_effects.levels.get(level_idx) {
            sim_effects::apply_effect_list(stat, modifier, global_triggers, req, effects, &source);
        }

        // 어빌리티 스톤 활성화 효과 — stone.engravings에서 이름 매칭
        if let Some(stone) = &req.character.stone {
            let ability_level = stone
                .engravings
                .iter()
                .find(|e| !e.is_negative && e.name == engraving.name)
                .map(|e| e.level)
                .unwrap_or(0);

            if ability_level > 0 {
                let stone_idx = ability_level.saturating_sub(1) as usize;
                if let Some(effects) = sim.stone.get(stone_idx) {
                    let source = format!("engraving:{}:stone:{}", engraving.name, ability_level);
                    sim_effects::apply_effect_list(
                        stat,
                        modifier,
                        global_triggers,
                        req,
                        effects,
                        &source,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_explicit_raw_engraving_feature_values() {
        let engraving: crate::builder::dto::EngravingInput = serde_json::from_value(
            serde_json::json!({
                "name": "원한", "grade": "유물", "level": 3,
                "featureData": {"baseLevels": [
                    {"featureType": 10, "values": [400]},
                    {"featureType": 10, "values": [1000]},
                    {"featureType": 10, "values": [2000]}
                ]}
            }),
        ).expect("raw featureData should deserialize");
        let grudge = &engraving.feature_data.as_ref().unwrap().base_levels[2];
        let mut stat = StatBuilder::new();
        let mut modifier = ModifierManager::new();

        assert!(apply_raw_feature(&mut stat, &mut modifier, 3, &[1500.0], None, None, "spirit"));
        assert!(apply_raw_feature(&mut stat, &mut modifier, grudge.feature_type, &grudge.values, None, None, "grudge"));
        assert!(apply_raw_feature(&mut stat, &mut modifier, 114, &[2500.0, 1600.0], None, None, "cursed"));
        assert!(apply_raw_feature(&mut stat, &mut modifier, 333, &[1600.0], None, None, "hit-master"));
        assert!(apply_raw_feature(&mut stat, &mut modifier, 334, &[2325.0, 600.0], None, None, "dagger"));
        assert!(apply_raw_feature(&mut stat, &mut modifier, 339, &[1000.0, 2125.0], None, None, "mass"));

        let stat = stat.build().expect("raw engraving stats should build");
        assert!((stat.sum(StatField::AttackSpeed) - 0.05).abs() < 1e-9);
        assert!((stat.sum(StatField::TargetDamageIncrease) - 0.2).abs() < 1e-9);
        assert!((stat.sum(StatField::CritRate) - 23.25).abs() < 1e-9);
        assert!((stat.sum(StatField::CritDmg) + 6.0).abs() < 1e-9);
        let modifier = modifier.compile();
        assert!((modifier.get_multiplier(
            &SkillTag::NonDirectionalNonAwakening,
            "DamageMultiplier",
        ) - 1.16).abs() < 1e-9);
        assert!((modifier.get_multiplier(&SkillTag::All, "DamageMultiplier")
            - 1.16 * 1.2125).abs() < 1e-9);
    }

    #[test]
    fn selects_the_current_raw_grade_and_stage() {
        let engraving: crate::builder::dto::EngravingInput = serde_json::from_value(
            serde_json::json!({
                "name": "원한", "grade": "유물", "level": 4,
                "featureData": {
                    "baseLevels": [{"featureType": 10, "values": [400]}],
                    "grades": [{"grade": 5, "stages": [
                        {"stage": 4, "featureType": 10, "values": [2325, 2000]}
                    ]}]
                }
            }),
        ).expect("current raw engraving should deserialize");
        let feature = selected_feature(&engraving)
            .expect("relic level 4 should select raw stage 84");

        assert_eq!(feature.feature_type, 10);
        assert_eq!(feature.values, vec![2325.0, 2000.0]);
    }

    #[test]
    fn masters_tenacity_requires_the_low_hp_assumption() {
        fn multiplier(assumed_low_hp: bool) -> f64 {
            let req: BuildRequest = serde_json::from_value(serde_json::json!({
                "character": {
                    "name": "test", "className": "아르카나", "stats": {},
                    "bracelet": null, "stone": null, "skills": [],
                    "engravings": [{
                        "name": "달인의 저력", "grade": "유물", "level": 4,
                        "featureData": {"grades": [{"grade": 5, "stages": [{
                            "stage": 4, "featureType": 105,
                            "values": [5000, 1925, 520988]
                        }]}]}
                    }],
                    "arkPassive": null, "arkGrids": null, "arkPassiveKarma": null
                },
                "assumedLowHp": assumed_low_hp
            })).expect("test request should deserialize");
            let mut stat = StatBuilder::new();
            let mut modifiers = ModifierManager::new();
            register(
                &mut stat,
                &mut modifiers,
                &mut Vec::new(),
                &mut SkillPipelineMap::new(Vec::<String>::new()),
                &req,
            );
            modifiers.compile().get_multiplier(&SkillTag::All, "DamageMultiplier")
        }

        assert!((multiplier(false) - 1.0).abs() < 1e-9);
        assert!((multiplier(true) - 1.1925).abs() < 1e-9);
    }

    #[test]
    fn static_combat_state_engravings_require_explicit_inputs() {
        fn multiplier(
            feature_type: u32,
            values: Vec<u32>,
            target_hp_percent: f64,
            target_staggered: bool,
            shield_active: bool,
        ) -> f64 {
            let req: BuildRequest = serde_json::from_value(serde_json::json!({
                "character": {
                    "name": "test", "className": "아르카나", "stats": {},
                    "bracelet": null, "stone": null, "skills": [],
                    "engravings": [{
                        "name": "condition", "grade": "유물", "level": 4,
                        "featureData": {"grades": [{"grade": 5, "stages": [{
                            "stage": 4, "featureType": feature_type, "values": values
                        }]}]}
                    }],
                    "arkPassive": null, "arkGrids": null, "arkPassiveKarma": null
                },
                "targetHpPercent": target_hp_percent,
                "assumedTargetStaggered": target_staggered,
                "assumedShieldActive": shield_active
            })).expect("test request should deserialize");
            let mut stat = StatBuilder::new();
            let mut modifiers = ModifierManager::new();
            register(
                &mut stat,
                &mut modifiers,
                &mut Vec::new(),
                &mut SkillPipelineMap::new(Vec::<String>::new()),
                &req,
            );
            modifiers.compile().get_multiplier(&SkillTag::All, "DamageMultiplier")
        }

        assert!((multiplier(2, vec![3000, 4350], 31.0, false, false) - 1.0).abs() < 1e-9);
        assert!((multiplier(2, vec![3000, 4350], 30.0, false, false) - 1.435).abs() < 1e-9);
        assert!((multiplier(112, vec![4375], 100.0, false, false) - 1.0).abs() < 1e-9);
        assert!((multiplier(112, vec![4375], 100.0, true, false) - 1.4375).abs() < 1e-9);
        assert!((multiplier(120, vec![1925], 100.0, false, false) - 1.0).abs() < 1e-9);
        assert!((multiplier(120, vec![1925], 100.0, false, true) - 1.1925).abs() < 1e-9);
    }
}
