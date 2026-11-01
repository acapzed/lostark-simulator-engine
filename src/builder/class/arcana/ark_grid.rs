use crate::builder::dto::{ArkGridClassCoreInput, ArkGridClassCoreOption, BuildRequest};
use crate::builder::modifier::{Modifier, ModifierManager, OpType};
use crate::builder::skill::SkillPipelineMap;
use crate::builder::stat::StatBuilder;
use crate::profile::{
    BuffSpec, GlobalTrigger, HitCondition, HitTrigger, HitTriggerOn, SkillTag, StackType,
    StatField, TriggerCondition, TriggerEffect, TriggerFilter,
};
use hashbrown::HashMap;

/// 아르카나 아크 그리드 질서 코어.
/// 코어 이름 + 젬 포인트 기준으로 해금된 옵션만 스킬 파이프라인에 적용한다.
///
pub fn apply(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    class_cores: &[ArkGridClassCoreInput],
    req: &BuildRequest,
) {
    let active_effects: Vec<_> = class_cores
        .iter()
        .flat_map(|core| {
            core.options
                .iter()
                .filter(|option| core.points >= option.point_requirement)
        })
        .flat_map(|option| option.runtime_effects.iter())
        .collect();

    for core in class_cores {
        apply_core(stat, modifier, core);
        for option in core
            .options
            .iter()
            .filter(|option| core.points >= option.point_requirement)
        {
            for effect in &option.runtime_effects {
                apply_skill_timing(skill_pipelines, req, effect);
                apply_conditional_target_damage(skill_pipelines, req, effect);
                apply_card_gauge_gain(skill_pipelines, req, effect);
                apply_charge_and_speed_buff(
                    global_triggers,
                    skill_pipelines,
                    req,
                    option.raw_option_id,
                    effect,
                );
                apply_tripod_crit_damage_stack(skill_pipelines, req, effect);
                apply_tripod_effect_damage(skill_pipelines, req, effect);
                apply_tripod_card_draw(skill_pipelines, req, effect);
            }
        }
    }
    apply_destiny(global_triggers, skill_pipelines, req, &active_effects);
}

fn matches_destiny_group(skill: &crate::builder::dto::SkillDataInput, effect: &crate::builder::dto::ArkGridClassCoreRuntimeEffect) -> bool {
    if effect.skill_ids.is_empty() { skill.properties.skill_group_ids.contains(&effect.skill_group_id) }
    else { effect.skill_ids.contains(&skill.game_skill_id) }
}

pub(crate) fn apply_destiny(
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    effects: &[&crate::builder::dto::ArkGridClassCoreRuntimeEffect],
) {
    let mut destiny_effects = Vec::new();
    for kind in ["on_destiny_ruin_consumable_damage", "on_destiny_skill_consumable_crit_back"] {
        let Some(edge) = effects.iter().copied().filter(|effect| effect.kind == kind)
            .max_by(|a, b| a.value_percent.total_cmp(&b.value_percent)) else { continue; };
        let buff_id = edge.buff_id.to_string();
        for skill in req.skill_data.iter().filter(|skill| {
            matches_destiny_group(skill, edge)
        }) {
            let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
                continue;
            };
            let cast_buff_id = buff_id.clone();
            let damage_bonus = edge.value_percent / 100.0;
            pipeline.add_stats(move |profile| {
                if kind == "on_destiny_skill_consumable_crit_back" {
                    profile.cast_buff_crit_rate_bonuses.push((cast_buff_id.clone(), damage_bonus));
                    profile.cast_buff_back_attack_id = Some(cast_buff_id.clone());
                } else {
                    profile.cast_buff_damage_bonuses.push((cast_buff_id.clone(), damage_bonus));
                }
                Ok(())
            });
            if let Some(skill_id) = req
                .character
                .skills
                .iter()
                .position(|selected| selected.name == skill.name)
            {
                global_triggers.push(GlobalTrigger {
                    filter: TriggerFilter::SkillCastId(skill_id as u32),
                    condition: TriggerCondition::BuffActive(buff_id.clone()),
                    effect: TriggerEffect::ConsumeBuffStacks {
                        buff_id: buff_id.clone(),
                        stacks: 1,
                    },
                    cooldown_ms: None,
                });
            }
        }
        destiny_effects.push(TriggerEffect::ApplyBuffStacks {
            spec: BuffSpec {
                id: buff_id,
                effects: HashMap::new(),
                max_stacks: edge.max_stacks,
                stack_type: StackType::Refresh,
                duration_ms: u32::MAX,
            },
            stacks: edge.stacks,
        });
    }

    let mut group_buffs = HashMap::new();
    for effect in effects
        .iter()
        .copied()
        .filter(|effect| effect.kind == "on_destiny_skill_group_buff")
    {
        let current: &mut &crate::builder::dto::ArkGridClassCoreRuntimeEffect =
            group_buffs.entry(effect.skill_group_id).or_insert(effect);
        if effect.value_percent > current.value_percent {
            *current = effect;
        }
    }
    for effect in group_buffs.into_values() {
        let buff_id = effect.buff_id.to_string();
        for skill in req.skill_data.iter().filter(|skill| {
            matches_destiny_group(skill, effect)
        }) {
            let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
                continue;
            };
            if effect.value_percent > 0.0 {
                let id = buff_id.clone();
                let bonus = effect.value_percent / 100.0;
                pipeline.add_trigger(move |profile| {
                    for hit in &mut profile.hits {
                        hit.triggers.push(HitTrigger {
                            on: HitTriggerOn::OnHitIf(HitCondition::BuffActive(id.clone())),
                            effect: TriggerEffect::DamageBonus(bonus),
                            chance: 1.0,
                        });
                    }
                    Ok(())
                });
            }
            if effect.cooldown_reduction_percent > 0.0 {
                if let Some(skill_id) = req
                    .character
                    .skills
                    .iter()
                    .position(|selected| selected.name == skill.name)
                {
                    global_triggers.push(GlobalTrigger {
                        filter: TriggerFilter::SkillCastId(skill_id as u32),
                        condition: TriggerCondition::BuffActive(buff_id.clone()),
                        effect: TriggerEffect::ReduceSkillCooldown {
                            percent: effect.cooldown_reduction_percent / 100.0,
                        },
                        cooldown_ms: None,
                    });
                }
            }
        }
        destiny_effects.push(TriggerEffect::ApplyBuff(BuffSpec {
            id: buff_id,
            effects: HashMap::new(),
            max_stacks: 1,
            stack_type: StackType::Refresh,
            duration_ms: effect.duration_ms,
        }));
    }

    if let Some(effect) = effects
        .iter()
        .copied()
        .filter(|effect| effect.kind == "on_destiny_identity_category_damage_buff")
        .max_by(|a, b| a.value_percent.total_cmp(&b.value_percent))
    {
        let buff_id = effect.buff_id.to_string();
        let bonus = effect.value_percent / 100.0;
        for skill in req
            .skill_data
            .iter()
            .filter(|skill| skill.identity_category == effect.identity_category)
        {
            if let Some(pipeline) = skill_pipelines.get_mut(&skill.name) {
                let id = buff_id.clone();
                pipeline.add_trigger(move |profile| {
                    for hit in &mut profile.hits {
                        hit.triggers.push(HitTrigger {
                            on: HitTriggerOn::OnHitIf(HitCondition::BuffActive(id.clone())),
                            effect: TriggerEffect::DamageBonus(bonus),
                            chance: 1.0,
                        });
                    }
                    Ok(())
                });
            }
        }
        destiny_effects.push(TriggerEffect::ApplyBuff(BuffSpec {
            id: buff_id,
            effects: HashMap::new(),
            max_stacks: 1,
            stack_type: StackType::Refresh,
            duration_ms: effect.duration_ms,
        }));
    }

    if let Some(effect) = effects
        .iter()
        .copied()
        .filter(|effect| effect.kind == "on_destiny_damage_buff")
        .max_by(|a, b| a.value_percent.total_cmp(&b.value_percent))
    {
        destiny_effects.push(TriggerEffect::ApplyBuff(BuffSpec {
            id: effect.buff_id.to_string(),
            effects: HashMap::from([(StatField::DamageIncrease, effect.value_percent / 100.0)]),
            max_stacks: 1,
            stack_type: StackType::Refresh,
            duration_ms: effect.duration_ms,
        }));
    }
    let mut stackable_buff_ids = Vec::new();
    for effect in effects.iter().copied().filter(|effect| effect.kind == "on_destiny_damage_buff_stackable") {
        if stackable_buff_ids.contains(&effect.buff_id) { continue; }
        stackable_buff_ids.push(effect.buff_id);
        destiny_effects.push(TriggerEffect::ApplyBuff(BuffSpec {
            id: effect.buff_id.to_string(),
            effects: HashMap::from([(StatField::DamageIncrease, effect.value_percent / 100.0)]),
            max_stacks: 1,
            stack_type: StackType::Refresh,
            duration_ms: effect.duration_ms,
        }));
    }

    if let Some(effect) = effects
        .iter()
        .copied()
        .find(|effect| effect.kind == "on_destiny_skill_group_cooldown_reduction")
    {
        let skill_ids = req
            .skill_data
            .iter()
            .filter(|skill| {
                matches_destiny_group(skill, effect)
            })
            .filter_map(|skill| {
                req.character
                    .skills
                    .iter()
                    .position(|selected| selected.name == skill.name)
                    .map(|id| id as u32)
            })
            .collect();
        destiny_effects.push(TriggerEffect::ReduceCooldowns {
            skill_ids,
            percent: effect.cooldown_reduction_percent / 100.0,
        });
    }

    for effect in effects.iter().copied().filter(|effect| effect.kind == "on_destiny_skill_group_cooldown_reduction_flat") {
        let skill_ids = req.skill_data.iter()
            .filter(|skill| matches_destiny_group(skill, effect))
            .filter_map(|skill| req.character.skills.iter().position(|selected| selected.name == skill.name).map(|id| id as u32))
            .collect();
        destiny_effects.push(TriggerEffect::ReduceCooldownsFlat {
            skill_ids,
            amount_ms: effect.cooldown_reduction_ms as u64,
        });
    }
    for effect in effects.iter().copied().filter(|effect| effect.kind == "on_destiny_blood_explosion") {
        destiny_effects.push(TriggerEffect::ScheduleDot {
            dot_id: "reaper:destiny:blood_explosion".into(),
            hit: crate::profile::RuntimeDamageSpec {
                destruction: 0,
                hit_id: "reaper:destiny:blood_explosion".into(),
                name: "혈액폭발".into(),
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
            first_tick_ms: 0,
            tick_interval_ms: 1000,
            tick_count: 1,
            refresh: true,
            independent: true,
        });
    }

    let cards = effects
        .iter()
        .copied()
        .filter(|effect| effect.kind == "on_destiny_draw_cards")
        .map(|effect| effect.cards)
        .max()
        .unwrap_or(0);
    destiny_effects.extend((0..cards).map(|_| TriggerEffect::DrawCard));

    if destiny_effects.is_empty() && !effects.iter().any(|effect| effect.kind == "destiny_after_skill_group_casts") {
        return;
    }
    let destiny_effect = TriggerEffect::Sequence(destiny_effects);
    for source in effects.iter().copied().filter(|effect| effect.kind == "destiny_after_skill_group_casts" && effect.hit_count > 0) {
        let counter_id=source.buff_id.to_string();
        for skill in req.skill_data.iter().filter(|skill| matches_destiny_group(skill, source)) {
            let Some(skill_id)=req.character.skills.iter().position(|selected| selected.name==skill.name) else {continue;};
            let filter=TriggerFilter::SkillCastId(skill_id as u32);
            global_triggers.push(GlobalTrigger {filter:filter.clone(),condition:TriggerCondition::Always,
                effect:TriggerEffect::ApplyBuff(BuffSpec {id:counter_id.clone(),effects:HashMap::new(),
                    max_stacks:source.max_stacks,stack_type:StackType::Refresh,duration_ms:u32::MAX}),cooldown_ms:None});
            global_triggers.push(GlobalTrigger {filter,
                condition:TriggerCondition::BuffStacksAtLeast {buff_id:counter_id.clone(),stacks:source.hit_count},
                // shortcut: Consume five after cast; server order and overflow policy still need game observation.
                effect:TriggerEffect::Sequence(vec![TriggerEffect::ConsumeBuffStacks {buff_id:counter_id.clone(),stacks:source.hit_count},destiny_effect.clone()]),cooldown_ms:None});
        }
    }
    if let Some(source) = effects
        .iter()
        .copied()
        .find(|effect| effect.kind == "destiny_on_card_use")
    {
        global_triggers.push(GlobalTrigger {
            filter: TriggerFilter::CardUse,
            condition: TriggerCondition::Always,
            effect: TriggerEffect::Chance {
                chance: source.chance_percent / 100.0,
                effect: Box::new(destiny_effect.clone()),
            },
            cooldown_ms: None,
        });
    }
    if let Some(source) = effects
        .iter()
        .copied()
        .find(|effect| effect.kind == "destiny_after_card_uses" && effect.hit_count > 0)
    {
        let counter_id = "arcana_destiny_card_use_counter".to_string();
        global_triggers.push(GlobalTrigger {
            filter: TriggerFilter::CardUse,
            condition: TriggerCondition::Always,
            effect: TriggerEffect::ApplyBuff(BuffSpec {
                id: counter_id.clone(),
                effects: HashMap::new(),
                max_stacks: source.hit_count,
                stack_type: StackType::Refresh,
                duration_ms: u32::MAX,
            }),
            cooldown_ms: None,
        });
        global_triggers.push(GlobalTrigger {
            filter: TriggerFilter::CardUse,
            condition: TriggerCondition::BuffAtMaxStacks(counter_id.clone()),
            effect: TriggerEffect::Sequence(vec![
                TriggerEffect::ConsumeBuffStacks {
                    buff_id: counter_id,
                    stacks: source.hit_count,
                },
                destiny_effect.clone(),
            ]),
            cooldown_ms: None,
        });
    }
    for source in effects
        .iter()
        .copied()
        .filter(|effect| effect.kind == "destiny_on_skill_cast" && effect.chance_percent == 100.0)
    {
        let Some(skill_name) = req
            .skill_data
            .iter()
            .find(|skill| skill.game_skill_id == source.skill_id)
            .map(|skill| skill.name.as_str())
        else {
            continue;
        };
        let Some(skill_id) = req
            .character
            .skills
            .iter()
            .position(|skill| skill.name == skill_name)
        else {
            continue;
        };
        global_triggers.push(GlobalTrigger {
            filter: TriggerFilter::SkillCastId(skill_id as u32),
            condition: TriggerCondition::Always,
            effect: destiny_effect.clone(),
            cooldown_ms: None,
        });
    }

    for source in effects
        .iter()
        .copied()
        .filter(|effect| effect.kind == "destiny_on_skill_group_cast" || effect.kind == "destiny_on_persona_skill_group_cast")
    {
        for skill in req.skill_data.iter().filter(|skill| {
            matches_destiny_group(skill, source)
        }) {
            let Some(skill_id) = req
                .character
                .skills
                .iter()
                .position(|selected| selected.name == skill.name)
            else {
                continue;
            };
            let persona = source.kind == "destiny_on_persona_skill_group_cast";
            let marker = format!("reaper_destiny_persona_cast_{skill_id}");
            if persona {
                // Rush clears Persona during use; qualify before it and grant destiny after it.
                global_triggers.push(GlobalTrigger {
                    filter: TriggerFilter::BeforeSkillCastId(skill_id as u32),
                    condition: TriggerCondition::ResourceAbove { resource: "ReaperPersona".into(), threshold: 1.0 },
                    effect: TriggerEffect::ApplyBuffStacks {
                        spec: BuffSpec { id: marker.clone(), effects: HashMap::new(), max_stacks: 1,
                            stack_type: StackType::Refresh, duration_ms: u32::MAX },
                        stacks: 1,
                    },
                    cooldown_ms: None,
                });
            }
            global_triggers.push(GlobalTrigger {
                filter: TriggerFilter::SkillCastId(skill_id as u32),
                condition: if persona { TriggerCondition::BuffActive(marker.clone()) } else { TriggerCondition::Always },
                effect: if persona {
                    TriggerEffect::Sequence(vec![TriggerEffect::ConsumeBuffStacks { buff_id: marker, stacks: 1 },destiny_effect.clone()])
                } else { TriggerEffect::Chance {
                    chance: source.chance_percent / 100.0,
                    effect: Box::new(destiny_effect.clone()),
                } },
                cooldown_ms: None,
            });
        }
    }

    for source in effects
        .iter()
        .copied()
        .filter(|effect| effect.kind == "destiny_after_skill_group_hits" && effect.hit_count > 0)
    {
        let counter_id = format!("arcana_destiny_counter_{}", source.skill_group_id);
        let filter = TriggerFilter::SkillTag(SkillTag::SkillGroup(source.skill_group_id));
        global_triggers.push(GlobalTrigger {
            filter: filter.clone(),
            condition: TriggerCondition::Always,
            effect: TriggerEffect::ApplyBuff(BuffSpec {
                id: counter_id.clone(),
                effects: HashMap::new(),
                max_stacks: source.hit_count,
                stack_type: StackType::Refresh,
                duration_ms: u32::MAX,
            }),
            cooldown_ms: None,
        });
        global_triggers.push(GlobalTrigger {
            filter,
            condition: TriggerCondition::BuffAtMaxStacks(counter_id.clone()),
            effect: TriggerEffect::Sequence(vec![
                TriggerEffect::ConsumeBuffStacks {
                    buff_id: counter_id,
                    stacks: source.hit_count,
                },
                destiny_effect.clone(),
            ]),
            cooldown_ms: None,
        });
    }
}

fn apply_tripod_card_draw(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    effect: &crate::builder::dto::ArkGridClassCoreRuntimeEffect,
) {
    if effect.kind != "tripod_stack_and_card_draw" || effect.cards == 0 {
        return;
    }
    let Some(skill_name) = req
        .skill_data
        .iter()
        .find(|skill| skill.game_skill_id == effect.skill_id)
        .map(|skill| skill.name.as_str())
    else {
        return;
    };
    let Some(pipeline) = skill_pipelines.get_mut(skill_name) else {
        return;
    };
    let target_effect_id = effect.target_effect_id;
    let cards = effect.cards;
    let chance = effect.chance_percent / 100.0;
    pipeline.add_trigger(move |profile| {
        for hit in &mut profile.hits {
            if hit.effect_id == target_effect_id {
                for _ in 0..cards {
                    hit.triggers.push(HitTrigger {
                        on: HitTriggerOn::OnHit,
                        effect: TriggerEffect::DrawCard,
                        chance,
                    });
                }
            }
        }
        Ok(())
    });
}

fn apply_tripod_effect_damage(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    effect: &crate::builder::dto::ArkGridClassCoreRuntimeEffect,
) {
    let change_cooldown = match effect.kind.as_str() {
        "tripod_effect_damage" => false,
        "tripod_cooldown_and_effect_damage" => true,
        _ => return,
    };
    let Some(skill_name) = req
        .skill_data
        .iter()
        .find(|skill| skill.game_skill_id == effect.skill_id)
        .map(|skill| skill.name.as_str())
    else {
        return;
    };
    let Some(pipeline) = skill_pipelines.get_mut(skill_name) else {
        return;
    };
    let target_effect_id = effect.target_effect_id;
    let target_dot_id = target_effect_id.to_string();
    let multiplier = 1.0 + effect.value_percent / 100.0;
    let cooldown_delta = effect.value_ms;
    pipeline.add_stats(move |profile| {
        let mut matched = false;
        for hit in &mut profile.hits {
            if hit.effect_id == target_effect_id {
                hit.base_damage *= multiplier;
                hit.skill_modifier *= multiplier;
                matched = true;
            }
            for trigger in &mut hit.triggers {
                if let TriggerEffect::ScheduleDot { dot_id, hit, .. } = &mut trigger.effect {
                    if dot_id == &target_dot_id {
                        hit.base_damage *= multiplier;
                        hit.skill_modifier *= multiplier;
                        matched = true;
                    }
                }
            }
        }
        if matched && change_cooldown {
            profile.cooldown_ms = if cooldown_delta < 0 {
                profile.cooldown_ms.saturating_sub(cooldown_delta.unsigned_abs())
            } else {
                profile.cooldown_ms.saturating_add(cooldown_delta as u32)
            };
        }
        Ok(())
    });
}

fn apply_tripod_crit_damage_stack(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    effect: &crate::builder::dto::ArkGridClassCoreRuntimeEffect,
) {
    if effect.kind != "tripod_buff_crit_damage_per_stack" {
        return;
    }
    let Some(skill_name) = req
        .skill_data
        .iter()
        .find(|skill| skill.game_skill_id == effect.skill_id)
        .map(|skill| skill.name.as_str())
    else {
        return;
    };
    if !req.character.skills.iter().any(|skill| {
        skill.name == skill_name
            && skill
                .tripods
                .iter()
                .any(|tripod| tripod.name == effect.required_tripod_name)
    }) {
        return;
    }
    let Some(pipeline) = skill_pipelines.get_mut(skill_name) else {
        return;
    };
    let source_buff_id = effect.source_buff_id.to_string();
    let buff = BuffSpec {
        id: effect.buff_id.to_string(),
        effects: HashMap::from([(StatField::CritDmg, effect.value_percent)]),
        max_stacks: effect.max_stacks,
        stack_type: StackType::Refresh,
        duration_ms: effect.duration_ms,
    };
    pipeline.add_trigger(move |profile| {
        for hit in &mut profile.hits {
            if hit.triggers.iter().any(|trigger| matches!(
                &trigger.effect,
                TriggerEffect::ApplyBuff(spec) if spec.id == source_buff_id
            )) {
                hit.triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::ApplyBuff(buff.clone()),
                    chance: 1.0,
                });
            }
        }
        Ok(())
    });
}

fn apply_charge_and_speed_buff(
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    raw_option_id: u32,
    effect: &crate::builder::dto::ArkGridClassCoreRuntimeEffect,
) {
    if effect.kind != "skill_charge_and_speed_buff" || effect.max_charges == 0 {
        return;
    }
    let Some(skill_name) = req
        .skill_data
        .iter()
        .find(|skill| skill.game_skill_id == effect.skill_id)
        .map(|skill| skill.name.as_str())
    else {
        return;
    };
    let Some(pipeline) = skill_pipelines.get_mut(skill_name) else {
        return;
    };
    let max_charges = effect.max_charges;
    let recharge_ms = effect.recharge_ms;
    let cooldown_delta_ms = effect.value_ms;
    // Set charge structure before gem/passive stat modifiers scale both timers.
    pipeline.add_structure(move |profile| {
        profile.max_stacks = max_charges;
        profile.charge_recovery_ms = recharge_ms;
        profile.cooldown_ms = profile.cooldown_ms.saturating_add_signed(cooldown_delta_ms);
        Ok(())
    });

    let Some(skill_id) = req
        .character
        .skills
        .iter()
        .position(|skill| skill.name == skill_name)
    else {
        return;
    };
    global_triggers.push(GlobalTrigger {
        filter: TriggerFilter::SkillCastId(skill_id as u32),
        condition: TriggerCondition::Always,
        effect: TriggerEffect::ApplyBuff(BuffSpec {
            id: format!("arcana_ark_grid_{raw_option_id}_speed"),
            effects: HashMap::from([(
                StatField::AttackSpeed,
                effect.value_percent / 100.0,
            )]),
            max_stacks: 1,
            stack_type: StackType::Refresh,
            duration_ms: effect.duration_ms,
        }),
        cooldown_ms: None,
    });
}

fn apply_card_gauge_gain(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    effect: &crate::builder::dto::ArkGridClassCoreRuntimeEffect,
) {
    if effect.kind != "card_gauge_gain_multiplier" {
        return;
    }
    super::apply_card_gauge_multiplier(
        skill_pipelines,
        req,
        effect.value_percent,
    );
}

fn apply_conditional_target_damage(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    effect: &crate::builder::dto::ArkGridClassCoreRuntimeEffect,
) {
    if effect.kind != "conditional_target_stack_damage"
        || effect.minimum_stacks != 1
        || effect.target_buff_id.is_empty()
    {
        return;
    }
    let bonus = effect.value_percent / 100.0;
    for skill in req
        .skill_data
        .iter()
        .filter(|skill| skill.identity_category == effect.identity_category)
    {
        let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
            continue;
        };
        let buff_id = effect.target_buff_id.clone();
        pipeline.add_trigger(move |profile| {
            for hit in &mut profile.hits {
                hit.triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHitIf(HitCondition::TargetBuffActive(buff_id.clone())),
                    effect: TriggerEffect::DamageBonus(bonus),
                    chance: 1.0,
                });
            }
            Ok(())
        });
    }
}

fn apply_skill_timing(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    effect: &crate::builder::dto::ArkGridClassCoreRuntimeEffect,
) {
    let Some(skill_name) = req
        .skill_data
        .iter()
        .find(|skill| skill.game_skill_id == effect.skill_id)
        .map(|skill| skill.name.as_str())
    else {
        return;
    };
    let Some(pipeline) = skill_pipelines.get_mut(skill_name) else {
        return;
    };
    match effect.kind.as_str() {
        "static_skill_cooldown_change" => {
            let delta = effect.value_ms;
            pipeline.add_stats(move |skill| {
                skill.cooldown_ms = if delta < 0 {
                    skill.cooldown_ms.saturating_sub(delta.unsigned_abs())
                } else {
                    skill.cooldown_ms.saturating_add(delta as u32)
                };
                Ok(())
            });
        }
        "static_skill_cast_speed" | "static_skill_holding_speed" => {
            let multiplier = (1.0 - effect.value_percent / 100.0).max(0.0);
            pipeline.add_stats(move |skill| {
                for duration in &mut skill.cast_times_ms {
                    *duration = (*duration as f64 * multiplier).round() as u32;
                }
                Ok(())
            });
        }
        _ => {}
    }
}

fn apply_core(stat: &mut StatBuilder, modifier: &mut ModifierManager, core: &ArkGridClassCoreInput) {
    for option in core
        .options
        .iter()
        .filter(|opt| core.points >= opt.point_requirement)
    {
        apply_option(stat, modifier, &core.name, option);
    }
}

fn apply_option(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    core_name: &str,
    option: &ArkGridClassCoreOption,
) {
    for effect in &option.runtime_effects {
        if effect.value_percent == 0.0 {
            continue;
        }
        let source = format!(
            "arcana:ark_grid:{core_name}:option:{}",
            option.raw_option_id
        );
        if effect.kind == "static_on_crit_damage" {
            let value = effect.value_percent / 100.0;
            stat.add_flat(source, move |stats| stats.on_crit_damage_bonus += value);
            continue;
        }
        let target_tag = match effect.kind.as_str() {
            "static_skill_group_damage" if effect.skill_group_id != 0 => {
                Some(SkillTag::SkillGroup(effect.skill_group_id))
            }
            "static_skill_damage" if effect.skill_id != 0 => Some(SkillTag::SkillId(effect.skill_id)),
            "static_identity_category_damage" => match effect.identity_category {
                28 => Some(SkillTag::CategoryNormal),
                29 => Some(SkillTag::CategoryStacked),
                30 => Some(SkillTag::CategoryRuin),
                _ => None,
            },
            _ => None,
        };
        let Some(target_tag) = target_tag else {
            continue;
        };
        modifier.add_modifier(Modifier {
            target_tag,
            stat_name: "DamageMultiplier".to_string(),
            value: effect.value_percent / 100.0,
            op_type: OpType::Multiply,
            source,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(
        point_requirement: u32,
        raw_option_id: u32,
        skill_group_id: u32,
        value_percent: f64,
    ) -> ArkGridClassCoreOption {
        ArkGridClassCoreOption {
            point_requirement,
            raw_option_id,
            runtime_effects: vec![crate::builder::dto::ArkGridClassCoreRuntimeEffect {
                kind: "static_skill_group_damage".to_string(),
                skill_ids: Vec::new(),
                card_id: 0,
                skill_group_id,
                skill_id: 0,
                target_effect_id: 0,
                identity_category: 0,
                target_buff_id: String::new(),
                required_tripod_name: String::new(),
                source_buff_id: 0,
                buff_id: 0,
                stacks: 0,
                hit_count: 0,
                minimum_stacks: 0,
                max_stacks: 0,
                cards: 0,
                chance_percent: 0.0,
                cooldown_reduction_percent: 0.0,
                cooldown_reduction_ms: 0,
                max_charges: 0,
                recharge_ms: 0,
                duration_ms: 0,
                delay_ms: 0,
                value_ms: 0,
                value_percent,
                fixed_damage: 0.0,
                damage_ratio: 0.0,
            }],
        }
    }

    fn core(
        name: &str,
        points: u32,
        options: Vec<ArkGridClassCoreOption>,
    ) -> ArkGridClassCoreInput {
        ArkGridClassCoreInput {
            name: name.to_string(),
            points,
            options,
        }
    }

    #[test]
    fn active_ruin_damage_option_adds_ruin_modifier() {
        let mut stat = StatBuilder::new();
        let mut modifier = ModifierManager::new();
        apply_core(
            &mut stat,
            &mut modifier,
            &core("루인 서브셋", 18, vec![option(10, 3191000, 2190904, 2.8)]),
        );

        let compiled = modifier.compile();
        let result = compiled.get_multiplier(&SkillTag::SkillGroup(2190904), "DamageMultiplier");

        assert!(
            (result - 1.028).abs() < 1e-9,
            "expected 1.028, got {result}"
        );
    }

    #[test]
    fn inactive_option_is_ignored() {
        let mut stat = StatBuilder::new();
        let mut modifier = ModifierManager::new();
        apply_core(
            &mut stat,
            &mut modifier,
            &core("루인 서브셋", 9, vec![option(10, 3191000, 2190904, 2.8)]),
        );

        let compiled = modifier.compile();
        let result = compiled.get_multiplier(&SkillTag::SkillGroup(2190904), "DamageMultiplier");

        assert_eq!(result, 1.0);
    }

    #[test]
    fn unsupported_runtime_effect_is_ignored() {
        let mut stat = StatBuilder::new();
        let mut modifier = ModifierManager::new();
        let mut unsupported = option(10, 3191100, 2190904, 8.0);
        unsupported.runtime_effects[0].kind = "on_destiny_cooldown_reduction".to_string();
        apply_core(
            &mut stat,
            &mut modifier,
            &core("루인 서브셋", 18, vec![unsupported]),
        );

        let compiled = modifier.compile();
        let result = compiled.get_multiplier(&SkillTag::SkillGroup(2190904), "DamageMultiplier");

        assert_eq!(result, 1.0);
    }

    #[test]
    fn critical_damage_option_uses_raw_action_kind() {
        let mut stat = StatBuilder::new();
        let mut modifier = ModifierManager::new();
        let mut critical = option(10, 3190000, 0, 2.0);
        critical.runtime_effects[0].kind = "static_on_crit_damage".to_string();

        apply_core(
            &mut stat,
            &mut modifier,
            &core("엣지 오브 페이트", 10, vec![critical]),
        );

        let stats = stat.build().expect("stat build should succeed");
        assert!((stats.on_crit_damage_bonus - 0.02).abs() < 1e-9);
    }

    #[test]
    fn identity_category_option_targets_normal_skills() {
        let mut stat = StatBuilder::new();
        let mut modifier = ModifierManager::new();
        let mut normal = option(10, 3192000, 0, 1.7);
        normal.runtime_effects[0].kind = "static_identity_category_damage".to_string();
        normal.runtime_effects[0].identity_category = 28;

        apply_core(
            &mut stat,
            &mut modifier,
            &core("노말 인핸스", 10, vec![normal]),
        );

        let compiled = modifier.compile();
        let result = compiled.get_multiplier(&SkillTag::CategoryNormal, "DamageMultiplier");
        assert!((result - 1.017).abs() < 1e-9);
    }

    #[test]
    fn exact_skill_option_targets_raw_skill_id() {
        let mut stat = StatBuilder::new();
        let mut modifier = ModifierManager::new();
        let mut checkmate = option(10, 3192500, 0, 12.0);
        checkmate.runtime_effects[0].kind = "static_skill_damage".to_string();
        checkmate.runtime_effects[0].skill_id = 19040;

        apply_core(
            &mut stat,
            &mut modifier,
            &core("임팩트 메이트", 10, vec![checkmate]),
        );

        let compiled = modifier.compile();
        let result = compiled.get_multiplier(&SkillTag::SkillId(19040), "DamageMultiplier");
        assert!((result - 1.12).abs() < 1e-9);
    }
}
