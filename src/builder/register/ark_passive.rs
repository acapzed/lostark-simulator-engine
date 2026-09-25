use crate::builder::register::sim_effects;
use crate::builder::{
    class, dto::BuildRequest, modifier::ModifierManager, skill::SkillPipelineMap, stat::StatBuilder,
};
use crate::profile::skill::GlobalTrigger;
use crate::profile::{
    BuffSpec, SkillSlot, SkillTag, StackType, StatField, TriggerCondition, TriggerEffect,
    TriggerFilter,
};
use hashbrown::HashMap;

pub fn register(
    stat: &mut StatBuilder,
    modifier_manager: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
) {
    let Some(input) = &req.character.ark_passive else {
        return;
    };

    // ── 공통 노드 (진화 + 도약 공통) — 데이터 드리븐, 레벨 기반 선택 ─────
    for node in &input.common_nodes {
        let level_idx = node.level.saturating_sub(1) as usize;
        let Some(effects) = node.effects_by_level.get(level_idx) else {
            continue;
        };
        if effects.is_empty() {
            continue;
        }
        let source = format!("ark_passive:{}:L{}", node.name, node.level);
        let mut mana_furnace = None;
        let mut mana_furnace_cap = 0.0;
        for effect in effects {
            let value = effect.value;
            match effect.kind.as_str() {
                "mana_cost_reduction" => for_each_skill(skill_pipelines, req, move |skill| {
                    if let Some(cost) = skill.resource_costs.get_mut("Mp") {
                        *cost *= 1.0 - value / 100.0;
                    }
                }),
                "mana_skill_cooldown_reduction" => {
                    for_each_skill(skill_pipelines, req, move |skill| {
                        if skill.tags.contains(&SkillTag::UsesMana) {
                            reduce_cooldown(skill, value);
                        }
                    })
                }
                "normal_skill_cooldown_reduction" => {
                    for_each_skill(skill_pipelines, req, move |skill| {
                        if skill.slot == SkillSlot::Normal {
                            reduce_cooldown(skill, value);
                        }
                    })
                }
                "mana_skill_evolution_damage" => {
                    let source = source.clone();
                    for_each_skill(skill_pipelines, req, move |skill| {
                        if skill.tags.contains(&SkillTag::UsesMana) {
                            skill
                                .evolution_damage_bonuses
                                .push((source.clone(), value / 100.0));
                        }
                    });
                }
                "directional_crit_damage" => for_each_skill(skill_pipelines, req, move |skill| {
                    if skill
                        .tags
                        .iter()
                        .any(|tag| matches!(tag, SkillTag::AttackBack | SkillTag::AttackHead))
                    {
                        for hit in &mut skill.hits {
                            hit.crit_damage_bonus += value / 100.0;
                        }
                    }
                }),
                "identity_gain_multiplier" => {
                    sim_effects::apply_card_gauge_multiplier(skill_pipelines, req, value)
                }
                "on_crit_damage_bonus" => {
                    let delta = value / 100.0;
                    stat.add_flat(source.clone(), move |s| s.on_crit_damage_bonus += delta);
                }
                "crit_rate_cap" => {
                    let delta = value / 100.0;
                    stat.add_flat(source.clone(), move |s| s.crit_rate_cap = delta);
                }
                "excess_crit_evolution" => {
                    let delta = value / 100.0;
                    stat.add_flat(source.clone(), move |s| {
                        s.excess_crit_evolution_coefficient = delta
                    });
                }
                "excess_crit_evolution_cap" => {
                    let delta = value / 100.0;
                    stat.add_flat(source.clone(), move |s| s.excess_crit_evolution_cap = delta);
                }
                "speed_evolution" => {
                    let delta = value / 100.0;
                    stat.add_flat(source.clone(), move |s| {
                        s.speed_evolution_coefficient = delta
                    });
                }
                "speed_overcap_evolution" => {
                    let delta = value / 100.0;
                    stat.add_flat(source.clone(), move |s| s.speed_overcap_evolution = delta);
                }
                "speed_overcap_coefficient" => {
                    let delta = value / 100.0;
                    stat.add_flat(source.clone(), move |s| s.speed_overcap_coefficient = delta);
                }
                "speed_evolution_cap" => {
                    let delta = value / 100.0;
                    stat.add_flat(source.clone(), move |s| s.speed_evolution_cap = delta);
                }
                "on_hit_evolution_buff" => global_triggers.push(GlobalTrigger {
                    filter: TriggerFilter::AnyHit,
                    condition: TriggerCondition::Always,
                    effect: TriggerEffect::ApplyBuff(BuffSpec {
                        id: format!("{source}:hit_buff"),
                        effects: HashMap::from([(StatField::EvolutionDamage, value / 100.0)]),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: effect.duration_ms.unwrap_or(0),
                    }),
                    cooldown_ms: effect.cooldown_ms,
                }),
                "extra_max_mana_cost" => for_each_skill(skill_pipelines, req, move |skill| {
                    if skill.tags.contains(&SkillTag::UsesMana) {
                        skill.extra_mp_cost_ratio += value / 100.0;
                    }
                }),
                "mana_furnace" => mana_furnace = Some(value),
                "mana_furnace_cap" => mana_furnace_cap = value,
                _ => {}
            }
        }
        if let Some(per_ten) = mana_furnace {
            let source = source.clone();
            for selected in &req.character.skills {
                let Some(data) = req
                    .skill_data
                    .iter()
                    .find(|data| data.name == selected.name)
                else {
                    continue;
                };
                let Some(pipeline) = skill_pipelines.get_mut(&selected.name) else {
                    continue;
                };
                let base_mana_cost = data.base_mana_cost;
                let bonus = (base_mana_cost / 10.0 * per_ten).min(mana_furnace_cap) / 100.0;
                let source = source.clone();
                pipeline.add_stats(move |skill| {
                    if skill.tags.contains(&SkillTag::UsesMana) && bonus > 0.0 {
                        skill.evolution_damage_bonuses.push((source.clone(), bonus));
                    }
                    Ok(())
                });
            }
        }
        sim_effects::apply_effect_list(
            stat,
            modifier_manager,
            global_triggers,
            req,
            effects,
            &source,
        );
    }

    // ── 클래스 노드 (깨달음 + 도약 클래스) — 클래스 코드 위임 ─────────────
    class::register_ark_passive(
        stat,
        modifier_manager,
        global_triggers,
        skill_pipelines,
        &input.class_nodes,
        req,
    );
}

fn for_each_skill(
    pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    apply: impl Fn(&mut crate::profile::SkillProfile) + Clone + 'static,
) {
    for skill in &req.character.skills {
        let Some(pipeline) = pipelines.get_mut(&skill.name) else {
            continue;
        };
        let apply = apply.clone();
        pipeline.add_stats(move |profile| {
            apply(profile);
            Ok(())
        });
    }
}

fn reduce_cooldown(skill: &mut crate::profile::SkillProfile, percent: f64) {
    let multiplier = 1.0 - percent / 100.0;
    skill.cooldown_ms = (skill.cooldown_ms as f64 * multiplier) as u32;
    skill.charge_recovery_ms = (skill.charge_recovery_ms as f64 * multiplier) as u32;
}
