use crate::builder::dto::BuildRequest;
use crate::builder::register;
use crate::profile::{
    BuffSpec, GlobalTrigger, HitCondition, HitTrigger, HitTriggerOn, RuntimeDamageSpec,
    SkillCategory, SkillSlot, SkillTag, StackType, StatField, TriggerCondition, TriggerEffect,
    TriggerFilter,
};
use hashbrown::HashMap;

pub fn build_effects(
    req: &BuildRequest,
    skills: &mut [crate::profile::SkillProfile],
    global_triggers: &mut Vec<GlobalTrigger>,
) -> Result<(HashMap<u32, TriggerEffect>, HashMap<u32, String>), String> {
    let mut arcana_card_effects = HashMap::new();
    let mut arcana_card_blocking_buffs = HashMap::new();
    for card in &req.card_data {
        let mut effects = Vec::new();
        for effect in &card.runtime_effects {
            match effect.kind.as_str() {
                "self_move_speed_buff" => effects.push(TriggerEffect::ApplyBuff(BuffSpec {
                    id: effect.buff_id.to_string(),
                    effects: HashMap::from([(
                        StatField::MovementSpeed,
                        effect.move_speed_percent / 100.0,
                    )]),
                    max_stacks: 1,
                    stack_type: StackType::Refresh,
                    duration_ms: effect.duration_ms,
                })),
                "self_stat_buff" => effects.push(TriggerEffect::ApplyBuff(BuffSpec {
                            id: effect.buff_id.to_string(),
                            effects: HashMap::from([
                                (StatField::CritRate, effect.crit_rate_percent),
                                (StatField::CritDmg, effect.crit_damage_percent),
                            ]),
                            max_stacks: 1,
                            stack_type: StackType::Refresh,
                            duration_ms: effect.duration_ms,
                        })),
                "mana_cost_freeze_buff" => effects.push(TriggerEffect::ApplyBuff(BuffSpec {
                    id: effect.buff_id.to_string(),
                    effects: HashMap::new(),
                    max_stacks: 1,
                    stack_type: StackType::Refresh,
                    duration_ms: effect.duration_ms,
                })),
                "restore_max_mana_percent" => effects.push(TriggerEffect::Schedule {
                    delay_ms: effect.delay_ms,
                    effect: Box::new(TriggerEffect::GainResourcePercent {
                        resource: "Mp".to_string(),
                        percent: effect.value_percent / 100.0,
                    }),
                }),
                "self_cooldown_reduction_buff" => {
                    let buff_id = effect.buff_id.to_string();
                    effects.push(TriggerEffect::ApplyBuff(BuffSpec {
                        id: buff_id.clone(),
                        effects: HashMap::new(),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: effect.duration_ms,
                    }));
                    global_triggers.push(GlobalTrigger {
                        filter: TriggerFilter::SkillCast,
                        condition: TriggerCondition::BuffActive(buff_id),
                        effect: TriggerEffect::ReduceSkillCooldown {
                            percent: effect.cooldown_reduction_percent / 100.0,
                        },
                        cooldown_ms: None,
                    });
                }
                "self_on_hit_target_damage_buff" => {
                    let buff_id = effect.buff_id.to_string();
                    effects.push(TriggerEffect::ApplyBuff(BuffSpec {
                        id: buff_id.clone(),
                        effects: HashMap::new(),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: effect.duration_ms,
                    }));
                    let target_buff = TriggerEffect::ApplyTargetBuff(BuffSpec {
                        id: effect.target_buff_id.to_string(),
                        effects: HashMap::from([(
                            StatField::TargetDamageIncrease,
                            effect.value_percent / 100.0,
                        )]),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: effect.target_duration_ms,
                    });
                    global_triggers.push(GlobalTrigger {
                        filter: TriggerFilter::AnyHit,
                        condition: TriggerCondition::BuffActive(buff_id),
                        effect: TriggerEffect::Chance {
                            chance: effect.chance_percent / 100.0,
                            effect: Box::new(target_buff),
                        },
                        cooldown_ms: None,
                    });
                }
                "self_on_skill_cast_stack_buff" => {
                    let buff_id = effect.buff_id.to_string();
                    effects.push(TriggerEffect::ApplyBuff(BuffSpec {
                        id: buff_id.clone(),
                        effects: HashMap::new(),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: effect.duration_ms,
                    }));
                    global_triggers.push(GlobalTrigger {
                        filter: TriggerFilter::SkillCast,
                        condition: TriggerCondition::BuffActive(buff_id),
                        effect: TriggerEffect::ApplyBuff(BuffSpec {
                            id: effect.stack_buff_id.to_string(),
                            effects: HashMap::from([(
                                StatField::AttackSpeed,
                                effect.attack_speed_percent / 100.0,
                            )]),
                            max_stacks: effect.max_stacks,
                            stack_type: StackType::Refresh,
                            duration_ms: effect.stack_duration_ms,
                        }),
                        cooldown_ms: None,
                    });
                }
                "random_self_damage_buff" => {
                    let choices = effect
                        .result_buff_ids
                        .iter()
                        .zip(&effect.damage_percents)
                        .map(|(_, value)| TriggerEffect::ApplyBuff(BuffSpec {
                            id: effect.unique_group_id.to_string(),
                            effects: HashMap::from([(
                                StatField::DamageIncrease,
                                value / 100.0,
                            )]),
                            max_stacks: 1,
                            stack_type: StackType::Refresh,
                            duration_ms: effect.duration_ms,
                        }))
                        .collect::<Vec<_>>();
                    if !choices.is_empty() {
                        effects.push(TriggerEffect::RandomChoice(choices));
                    }
                }
                "random_reduce_cooldowns" => {
                    let skill_ids = skills
                        .iter()
                        .filter(|skill| skill.slot == SkillSlot::Normal)
                        .map(|skill| skill.skill_id)
                        .collect::<Vec<_>>();
                    let choices = effect
                        .effect_ids
                        .iter()
                        .zip(&effect.cooldown_reduction_percents)
                        .map(|(_, percent)| TriggerEffect::ReduceCooldowns {
                            skill_ids: skill_ids.clone(),
                            percent: percent / 100.0,
                        })
                        .collect::<Vec<_>>();
                    if !choices.is_empty() {
                        effects.push(TriggerEffect::RandomChoice(choices));
                    }
                }
                "fill_card_slots" => effects.extend(
                    (0..effect.max_card_slots).map(|_| TriggerEffect::DrawCard),
                ),
                "redraw_last_used_card" => effects.push(TriggerEffect::DrawLastUsedCard),
                "stacked_skill_extra_target_stack_buff" => {
                    let buff_id = effect.buff_id.to_string();
                    effects.push(TriggerEffect::ApplyBuff(BuffSpec {
                        id: buff_id.clone(),
                        effects: HashMap::new(),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: effect.duration_ms,
                    }));
                    for skill in skills
                        .iter_mut()
                        .filter(|skill| skill.category == SkillCategory::Stacked)
                    {
                        for hit in &mut skill.hits {
                            let spec = hit.triggers.iter().find_map(|trigger| match &trigger.effect {
                                TriggerEffect::ApplyTargetBuff(spec)
                                | TriggerEffect::ApplyTargetBuffStacks { spec, .. }
                                    if spec.id == effect.target_buff_key => Some(spec.clone()),
                                _ => None,
                            });
                            if let Some(spec) = spec {
                                hit.triggers.push(HitTrigger {
                                    on: HitTriggerOn::OnHitIf(HitCondition::BuffActive(
                                        buff_id.clone(),
                                    )),
                                    effect: TriggerEffect::ApplyTargetBuffStacks {
                                        spec,
                                        stacks: effect.extra_stacks,
                                    },
                                    chance: 1.0,
                                });
                            }
                        }
                    }
                }
                "force_ruin_stack_damage_buff" => {
                    let buff_id = effect.buff_id.to_string();
                    effects.push(TriggerEffect::ApplyBuff(BuffSpec {
                        id: buff_id.clone(),
                        effects: HashMap::new(),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: effect.duration_ms,
                    }));
                    for skill in skills
                        .iter_mut()
                        .filter(|skill| skill.category == SkillCategory::Ruin)
                    {
                        for hit in &mut skill.hits {
                            for trigger in &mut hit.triggers {
                                if let TriggerEffect::DealDamageByTargetBuffStacks {
                                    buff_id: target_buff_id,
                                    forced_stacks_while_buff,
                                    ..
                                } = &mut trigger.effect
                                {
                                    if *target_buff_id == effect.target_buff_key {
                                        *forced_stacks_while_buff =
                                            Some((buff_id.clone(), effect.max_stacks));
                                    }
                                }
                            }
                        }
                    }
                }
                "normal_skill_damage_buff" => {
                    let buff_id = effect.buff_id.to_string();
                    effects.push(TriggerEffect::ApplyBuff(BuffSpec {
                        id: buff_id.clone(),
                        effects: HashMap::new(),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: effect.duration_ms,
                    }));
                    for skill in skills
                        .iter_mut()
                        .filter(|skill| skill.category == SkillCategory::Normal)
                    {
                        skill
                            .cast_buff_damage_bonuses
                            .push((buff_id.clone(), effect.value_percent / 100.0));
                    }
                }
                "normal_skill_crit_rate_buff" => {
                    let buff_id = effect.buff_id.to_string();
                    effects.push(TriggerEffect::ApplyBuff(BuffSpec {
                        id: buff_id.clone(),
                        effects: HashMap::new(),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: effect.duration_ms,
                    }));
                    for skill in skills
                        .iter_mut()
                        .filter(|skill| skill.category == SkillCategory::Normal)
                    {
                        skill
                            .cast_buff_crit_rate_bonuses
                            .push((buff_id.clone(), effect.crit_rate_percent / 100.0));
                    }
                }
                "next_skill_cooldown_reset_buff" => {
                    let buff_id = effect.buff_id.to_string();
                    effects.push(TriggerEffect::ApplyBuff(BuffSpec {
                        id: buff_id.clone(),
                        effects: HashMap::new(),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: u32::MAX,
                    }));
                    arcana_card_blocking_buffs.insert(card.skill_id, buff_id.clone());
                    global_triggers.extend(skills.iter().map(|skill| GlobalTrigger {
                        filter: TriggerFilter::SkillCastId(skill.skill_id),
                        condition: TriggerCondition::BuffActive(buff_id.clone()),
                        effect: TriggerEffect::ResetConsumedSkillCooldown {
                            buff_id: buff_id.clone(),
                            max_charges: skill.max_stacks,
                        },
                        cooldown_ms: None,
                    }));
                }
                "direct_damage" => {
                    effects.push(TriggerEffect::DealRuntimeDamage(RuntimeDamageSpec {
                        hit_id: effect.hit_id.clone(),
                        name: effect.name.clone(),
                        base_damage: effect.fixed_damage,
                        skill_modifier: effect.damage_ratio,
                        damage_increase: effect.value_percent / 100.0,
                        crit_chance_bonus: 0.0,
                        crit_damage_bonus: 0.0,
                        random_crit_damage_chance: 0.0,
                        random_crit_damage_bonus: 0.0,
                        defense_ignore: 0.0,
                        defense_ignore_chance: 0.0,
                    }));
                    effects.extend((0..effect.draw_card_count).map(|_| TriggerEffect::DrawCard));
                }
                "basic_attack_card_buff" => effects.push(TriggerEffect::ApplyBuff(BuffSpec {
                    id: effect.buff_id.to_string(),
                    effects: HashMap::new(),
                    max_stacks: 1,
                    stack_type: StackType::Refresh,
                    duration_ms: effect.duration_ms,
                })),
                other => return Err(format!("카드 {}: 알 수 없는 실행 효과: {other}", card.name)),
            }
        }
        if !effects.is_empty() {
            arcana_card_effects.insert(card.skill_id, TriggerEffect::Sequence(effects));
        }
    }
    register::gem::register_arcana_card_damage(&mut arcana_card_effects, req);
    apply_arcana_card_ark_passives(req, &mut arcana_card_effects);
    apply_arcana_card_ark_grid(
        req,
        skills,
        global_triggers,
        &mut arcana_card_effects,
    );
    Ok((arcana_card_effects, arcana_card_blocking_buffs))
}

pub fn apply_arcana_card_ark_passives(
    req: &BuildRequest,
    effects_by_card: &mut HashMap<u32, TriggerEffect>,
) {
    let Some(passive) = &req.character.ark_passive else { return };
    let value = |name: &str, index: usize| {
        passive.class_nodes.iter()
            .find(|node| node.name == name)
            .and_then(|node| node.values.get(index))
            .copied()
            .unwrap_or(0.0)
    };

    scale_card_damage(
        effects_by_card.get_mut(&19282),
        1.0 + (value("황제의 칙령", 0) + value("또 다른 황제", 2)) / 100.0,
    );
    scale_card_damage(
        effects_by_card.get_mut(&19288),
        1.0 + value("황후의 기사", 0) / 100.0,
    );
    extend_card_buff(effects_by_card.get_mut(&19281), value("황후의 계략", 0));
    extend_card_buff(effects_by_card.get_mut(&19098), value("황후의 계략", 1));

    if value("또 다른 황제", 1) > 0.0 {
        let Some(emperor) = effects_by_card.get(&19282).cloned() else { return };
        let chance = value("또 다른 황제", 1) / 100.0;
        for card in &req.card_data {
            if card.skill_id == 19282 { continue; }
            let proc = TriggerEffect::Chance { chance, effect: Box::new(emperor.clone()) };
            match effects_by_card.entry(card.skill_id).or_insert_with(|| TriggerEffect::Sequence(Vec::new())) {
                TriggerEffect::Sequence(effects) => effects.push(proc),
                effect => *effect = TriggerEffect::Sequence(vec![effect.clone(), proc]),
            }
        }
    }
}

pub fn apply_arcana_card_ark_grid(
    req: &BuildRequest,
    skills: &mut [crate::profile::SkillProfile],
    global_triggers: &mut Vec<GlobalTrigger>,
    effects_by_card: &mut HashMap<u32, TriggerEffect>,
) {
    let effects = req
        .character
        .ark_grids
        .as_ref()
        .into_iter()
        .flat_map(|grid| &grid.class_cores)
        .flat_map(|core| {
            core.options
                .iter()
                .filter(|option| core.points >= option.point_requirement)
        })
        .flat_map(|option| &option.runtime_effects);

    for effect in effects {
        match effect.kind.as_str() {
            "on_card_use_skill_group_buff" => {
                let buff_id = effect.buff_id.to_string();
                append_card_effect(
                    effects_by_card,
                    effect.card_id,
                    TriggerEffect::ApplyBuff(BuffSpec {
                        id: buff_id.clone(),
                        effects: HashMap::new(),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: effect.duration_ms,
                    }),
                );
                for skill in skills.iter_mut().filter(|skill| {
                    skill.tags.contains(&SkillTag::SkillGroup(effect.skill_group_id))
                }) {
                    skill.cast_buff_damage_bonuses
                        .push((buff_id.clone(), effect.value_percent / 100.0));
                }
            }
            "on_card_use_delayed_skill_group_cooldown_reduction" => {
                let skill_ids = skills.iter()
                    .filter(|skill| skill.tags.contains(&SkillTag::SkillGroup(effect.skill_group_id)))
                    .map(|skill| skill.skill_id)
                    .collect();
                append_card_effect(
                    effects_by_card,
                    effect.card_id,
                    TriggerEffect::Schedule {
                        delay_ms: effect.delay_ms,
                        effect: Box::new(TriggerEffect::ReduceCooldowns {
                            skill_ids,
                            percent: effect.cooldown_reduction_percent / 100.0,
                        }),
                    },
                );
            }
            "on_skill_cast_skill_group_buff" => {
                let Some(source) = skills.iter().find(|skill| {
                    skill.tags.contains(&SkillTag::SkillId(effect.skill_id))
                }) else { continue };
                let buff_id = effect.buff_id.to_string();
                global_triggers.push(GlobalTrigger {
                    filter: TriggerFilter::SkillCastId(source.skill_id),
                    condition: TriggerCondition::Always,
                    effect: TriggerEffect::ApplyBuff(BuffSpec {
                        id: buff_id.clone(),
                        effects: HashMap::new(),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: effect.duration_ms,
                    }),
                    cooldown_ms: None,
                });
                for skill in skills.iter_mut().filter(|skill| {
                    skill.tags.contains(&SkillTag::SkillGroup(effect.skill_group_id))
                }) {
                    skill.cast_buff_damage_bonuses
                        .push((buff_id.clone(), effect.value_percent / 100.0));
                }
            }
            "card_damage_multiplier" => scale_card_damage(
                effects_by_card.get_mut(&effect.card_id),
                1.0 + effect.value_percent / 100.0,
            ),
            "twisted_fate_override" => {
                effects_by_card.insert(effect.card_id, TriggerEffect::WeightedRandomChoice(vec![
                    (1.02, card_damage_buff(190900, effect.duration_ms, 0.0)),
                    (32.98, card_damage_buff(190900, effect.duration_ms, 10.0)),
                    (33.0, card_damage_buff(190900, effect.duration_ms, 20.0)),
                    (33.0, card_damage_buff(190900, effect.duration_ms, 40.0)),
                ]));
            }
            "card_crit_damage_bonus" => adjust_card_buff_stat(
                effects_by_card.get_mut(&effect.card_id),
                StatField::CritDmg,
                effect.value_percent,
            ),
            "card_target_damage_override" => {
                for trigger in global_triggers.iter_mut() {
                    adjust_target_buff_stat(
                        &mut trigger.effect,
                        effect.target_buff_id.as_str(),
                        effect.value_percent / 100.0,
                    );
                }
            }
            _ => {}
        }
    }
}

fn append_card_effect(
    effects_by_card: &mut HashMap<u32, TriggerEffect>,
    card_id: u32,
    effect: TriggerEffect,
) {
    match effects_by_card.entry(card_id)
        .or_insert_with(|| TriggerEffect::Sequence(Vec::new()))
    {
        TriggerEffect::Sequence(effects) => effects.push(effect),
        current => *current = TriggerEffect::Sequence(vec![current.clone(), effect]),
    }
}

fn card_damage_buff(buff_id: u32, duration_ms: u32, value_percent: f64) -> TriggerEffect {
    TriggerEffect::ApplyBuff(BuffSpec {
        id: buff_id.to_string(),
        effects: HashMap::from([(StatField::DamageIncrease, value_percent / 100.0)]),
        max_stacks: 1,
        stack_type: StackType::Refresh,
        duration_ms,
    })
}

fn adjust_card_buff_stat(effect: Option<&mut TriggerEffect>, field: StatField, value: f64) {
    let Some(effect) = effect else { return };
    match effect {
        TriggerEffect::Sequence(effects) | TriggerEffect::RandomChoice(effects) => {
            for effect in effects { adjust_card_buff_stat(Some(effect), field, value); }
        }
        TriggerEffect::WeightedRandomChoice(effects) => {
            for (_, effect) in effects { adjust_card_buff_stat(Some(effect), field, value); }
        }
        TriggerEffect::Chance { effect, .. } | TriggerEffect::Schedule { effect, .. } => {
            adjust_card_buff_stat(Some(effect), field, value);
        }
        TriggerEffect::ApplyBuff(buff) => *buff.effects.entry(field).or_default() += value,
        _ => {}
    }
}

fn adjust_target_buff_stat(effect: &mut TriggerEffect, buff_id: &str, value: f64) {
    match effect {
        TriggerEffect::Sequence(effects) | TriggerEffect::RandomChoice(effects) => {
            for effect in effects { adjust_target_buff_stat(effect, buff_id, value); }
        }
        TriggerEffect::WeightedRandomChoice(effects) => {
            for (_, effect) in effects { adjust_target_buff_stat(effect, buff_id, value); }
        }
        TriggerEffect::Chance { effect, .. } | TriggerEffect::Schedule { effect, .. } => {
            adjust_target_buff_stat(effect, buff_id, value);
        }
        TriggerEffect::ApplyTargetBuff(buff) if buff.id == buff_id => {
            buff.effects.insert(StatField::TargetDamageIncrease, value);
        }
        _ => {}
    }
}

fn scale_card_damage(effect: Option<&mut TriggerEffect>, multiplier: f64) {
    let Some(effect) = effect else { return };
    match effect {
        TriggerEffect::Sequence(effects) | TriggerEffect::RandomChoice(effects) => {
            for effect in effects { scale_card_damage(Some(effect), multiplier); }
        }
        TriggerEffect::WeightedRandomChoice(effects) => {
            for (_, effect) in effects { scale_card_damage(Some(effect), multiplier); }
        }
        TriggerEffect::Chance { effect, .. } | TriggerEffect::Schedule { effect, .. } => {
            scale_card_damage(Some(effect), multiplier);
        }
        TriggerEffect::DealRuntimeDamage(hit) => {
            hit.base_damage *= multiplier;
            hit.skill_modifier *= multiplier;
        }
        _ => {}
    }
}

fn extend_card_buff(effect: Option<&mut TriggerEffect>, seconds: f64) {
    if seconds <= 0.0 { return; }
    let Some(effect) = effect else { return };
    match effect {
        TriggerEffect::Sequence(effects) | TriggerEffect::RandomChoice(effects) => {
            for effect in effects { extend_card_buff(Some(effect), seconds); }
        }
        TriggerEffect::WeightedRandomChoice(effects) => {
            for (_, effect) in effects { extend_card_buff(Some(effect), seconds); }
        }
        TriggerEffect::Chance { effect, .. } | TriggerEffect::Schedule { effect, .. } => {
            extend_card_buff(Some(effect), seconds);
        }
        TriggerEffect::ApplyBuff(buff) => buff.duration_ms += (seconds * 1000.0) as u32,
        _ => {}
    }
}
