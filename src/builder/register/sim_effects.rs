use hashbrown::HashMap;

use crate::builder::dto::{BuildRequest, SimEffect};
use crate::builder::modifier::{Modifier, ModifierManager, OpType};
use crate::builder::skill::SkillPipelineMap;
use crate::builder::stat::StatBuilder;
use crate::profile::skill::{
    BuffSpec, GlobalTrigger, SkillSlot, SkillTag, StackType,
    TriggerCondition, TriggerEffect, TriggerFilter,
};
use crate::profile::StatField;

/// SimEffect 목록을 순서대로 평가하여 StatBuilder / ModifierManager / GlobalTrigger에 등록한다.
///
/// ## 분기 순서
/// 1. `kind == "stack_trigger"` → GlobalTrigger 등록
/// 2. `tag` 있음 + `field == "cooldown_reduction"` → ModifierManager (CooldownReduction)
/// 3. `tag` 있음 (나머지) → ModifierManager (DamageMultiplier)
/// 4. `condition` 있음 → BuildRequest 플래그 확인 후 stat bake
/// 5. `kind == "flat"` → 절대값 스탯 직접 += (치명/특화 등 전투 특성)
/// 6. 기본(`kind == "stat"`) → value/100 후 StatField HashMap 삽입
///
/// ## 값 단위
/// - `kind == "stat"` / tag modifier: 퍼센트 단위 (15.0 = 15%), register에서 /100 처리
/// - `kind == "flat"`: 절대값 그대로 (치명 500 → s.crit += 500.0)
pub fn apply_effect_list(
    stat: &mut StatBuilder,
    modifier_mgr: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    req: &BuildRequest,
    effects: &[SimEffect],
    source: &str,
) {
    for effect in effects {
        if effect.kind == "stack_trigger" {
            build_stack_triggers(global_triggers, effect, source);
        } else if effect.kind == "on_hit_dot" {
            build_on_hit_dot(global_triggers, effect);
        } else if effect.kind == "on_hit_target_debuff" {
            build_on_hit_target_debuff(global_triggers, effect, source);
        } else if let Some(tag_str) = &effect.tag {
            if let Some(tag) = str_to_skill_tag(tag_str) {
                let (stat_name, value) = if effect.field == "cooldown_reduction" {
                    ("CooldownReduction".to_string(), -(effect.value / 100.0))
                } else {
                    ("DamageMultiplier".to_string(), effect.value / 100.0)
                };
                modifier_mgr.add_modifier(Modifier {
                    target_tag: tag,
                    stat_name,
                    value,
                    op_type: OpType::Multiply,
                    source: source.to_string(),
                });
            }
        } else if let Some(condition) = &effect.condition {
            let active = match condition.as_str() {
                "hp_below_50" => req.assumed_low_hp,
                "hp_above_65" => req.assumed_high_hp,
                _ => false,
            };
            if active {
                apply_stat(stat, effect, source);
            }
        } else if effect.kind == "flat" {
            apply_flat_stat(stat, effect, source);
        } else {
            apply_stat(stat, effect, source);
        }
    }
}

// ---------------------------------------------------------------------------
// 내부 헬퍼
// ---------------------------------------------------------------------------

fn apply_stat(stat: &mut StatBuilder, effect: &SimEffect, source: &str) {
    if let Some(field) = str_to_stat_field(&effect.field) {
        stat.add_stat(field, source, effect_delta(field, effect.value));
    }
}

fn effect_delta(field: StatField, value: f64) -> f64 {
    if matches!(field, StatField::CritRate | StatField::CritDmg) {
        value
    } else {
        value / 100.0
    }
}

/// kind == "flat" — 절대값을 스탯에 직접 += 한다.
fn apply_flat_stat(stat: &mut StatBuilder, effect: &SimEffect, source: &str) {
    let v = effect.value;
    let src = source.to_string();
    match effect.field.as_str() {
        "crit"               => stat.add_flat(src, move |s| s.crit           += v),
        "specialization"     => stat.add_flat(src, move |s| s.specialization  += v),
        "swiftness"          => stat.add_flat(src, move |s| s.swiftness        += v),
        "domination"         => stat.add_flat(src, move |s| s.domination       += v),
        "endurance"          => stat.add_flat(src, move |s| s.endurance        += v),
        "expertise"          => stat.add_flat(src, move |s| s.expertise        += v),
        "base_main_stat"     => stat.add_flat(src, move |s| s.base_main_stat   += v),
        "base_vitality"      => stat.add_flat(src, move |s| s.base_vitality    += v),
        "base_attack_power"  => stat.add_flat(src, move |s| s.base_attack_power += v),
        "max_mana"           => stat.add_flat(src, move |s| s.max_mana += v),
        "on_crit_damage_bonus" => stat.add_flat(src, move |s| s.on_crit_damage_bonus += v),
        "weapon_ap_mul"      => stat.add_flat(src, move |s| s.weapon_ap_mul += v),
        // 무기 공격력 고정값 — WeaponAttackPower 버킷에 raw 삽입 (/100 없음)
        "weapon_attack_power" => stat.add_stat(StatField::WeaponAttackPower, src, v),
        _ => {}
    }
}

pub fn apply_card_gauge_multiplier(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    percent: f64,
) {
    apply_card_gauge_multiplier_for_skill_group(skill_pipelines, req, 0, percent);
}

pub fn apply_card_gauge_multiplier_for_skill_group(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    skill_group_id: u32,
    percent: f64,
) {
    let multiplier = 1.0 + percent / 100.0;
    for skill in &req.skill_data {
        if skill_group_id != 0 && !skill.properties.skill_group_ids.contains(&skill_group_id) {
            continue;
        }
        let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
            continue;
        };
        pipeline.add_trigger(move |profile| {
            if let Some(amount) = profile.resource_gains.get_mut("CardGauge") {
                *amount *= multiplier;
            }
            for hit in &mut profile.hits {
                for trigger in &mut hit.triggers {
                    if let TriggerEffect::GainResource { resource, amount } = &mut trigger.effect {
                        if resource == "CardGauge" {
                            *amount *= multiplier;
                        }
                    }
                }
            }
            Ok(())
        });
    }
}

pub fn apply_normal_skill_cooldown_reduction(
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
    percent: f64,
) {
    let multiplier = 1.0 - percent / 100.0;
    for skill in &req.skill_data {
        let Some(pipeline) = skill_pipelines.get_mut(&skill.name) else {
            continue;
        };
        pipeline.add_stats(move |profile| {
            if profile.slot == SkillSlot::Normal {
                profile.cooldown_ms = (profile.cooldown_ms as f64 * multiplier) as u32;
                profile.charge_recovery_ms =
                    (profile.charge_recovery_ms as f64 * multiplier) as u32;
            }
            Ok(())
        });
    }
}

fn build_on_hit_dot(global_triggers: &mut Vec<GlobalTrigger>, effect: &SimEffect) {
    if effect.dot_id.is_empty() || effect.tick_count == 0 {
        return;
    }
    let trigger = GlobalTrigger {
        filter: TriggerFilter::AnyHit,
        condition: TriggerCondition::Always,
        effect: TriggerEffect::ScheduleDot {
            dot_id: effect.dot_id.clone(),
            hit: crate::profile::RuntimeDamageSpec {
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
        cooldown_ms: None,
    };
    if let Some(existing) = global_triggers.iter_mut().find(|candidate| matches!(
        &candidate.effect,
        TriggerEffect::ScheduleDot { dot_id, .. } if dot_id == &effect.dot_id
    )) {
        *existing = trigger;
    } else {
        global_triggers.push(trigger);
    }
}

fn build_on_hit_target_debuff(
    global_triggers: &mut Vec<GlobalTrigger>,
    effect: &SimEffect,
    source: &str,
) {
    let (Some(field), Some(duration_ms)) =
        (str_to_stat_field(&effect.field), effect.duration_ms)
    else {
        return;
    };
    global_triggers.push(GlobalTrigger {
        filter: TriggerFilter::AnyHit,
        condition: TriggerCondition::Always,
        effect: TriggerEffect::ApplyTargetBuff(BuffSpec {
            id: format!("{source}:target_debuff"),
            effects: HashMap::from([(field, effect_delta(field, effect.value))]),
            max_stacks: 1,
            stack_type: StackType::Refresh,
            duration_ms,
        }),
        cooldown_ms: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_max_mana_is_added_without_percent_conversion() {
        let mut stat = StatBuilder::new();
        apply_flat_stat(
            &mut stat,
            &SimEffect {
                kind: "flat".into(),
                field: "max_mana".into(),
                value: 240.0,
                ..Default::default()
            },
            "chaos-core",
        );
        assert_eq!(stat.build().unwrap().max_mana, 240.0);
    }

    #[test]
    fn crit_rate_keeps_percentage_points_in_the_stat_bucket() {
        let mut stat = StatBuilder::new();
        apply_stat(
            &mut stat,
            &SimEffect { field: "crit_rate".into(), value: 8.0, ..Default::default() },
            "ark-passive",
        );
        assert_eq!(stat.build().unwrap().sum(StatField::CritRate), 8.0);
    }
}

/// stack_trigger 이펙트로부터 GlobalTrigger를 생성하여 등록한다.
fn build_stack_triggers(
    global_triggers: &mut Vec<GlobalTrigger>,
    effect: &SimEffect,
    source: &str,
) {
    let Some(max_stacks)     = effect.max_stacks else { return };
    let Some(stack_duration) = effect.stack_duration else { return };
    let Some(per_stack)      = &effect.per_stack else { return };

    let filter = str_to_trigger_filter(effect.trigger_event.as_deref());

    let duration_ms = (stack_duration * 1000.0) as u32;
    let buff_id     = format!("{source}:stack_buff");

    let mut buff_effects: HashMap<StatField, f64> = HashMap::new();
    for e in per_stack {
        if let Some(field) = str_to_stat_field(&e.field) {
            *buff_effects.entry(field).or_insert(0.0) += effect_delta(field, e.value);
        }
    }

    global_triggers.push(GlobalTrigger {
        filter:      filter.clone(),
        condition:   TriggerCondition::Always,
        effect:      TriggerEffect::ApplyBuff(BuffSpec {
            id:          buff_id.clone(),
            effects:     buff_effects,
            max_stacks,
            stack_type:  StackType::Refresh,
            duration_ms,
        }),
        cooldown_ms: None,
    });

    if let Some(on_max) = &effect.on_max_stack {
        let max_buff_id = format!("{source}:max_stack_buff");
        let mut max_effects: HashMap<StatField, f64> = HashMap::new();
        for e in on_max {
            if let Some(field) = str_to_stat_field(&e.field) {
                *max_effects.entry(field).or_insert(0.0) += effect_delta(field, e.value);
            }
        }
        global_triggers.push(GlobalTrigger {
            filter,
            condition:   TriggerCondition::BuffAtMaxStacks(buff_id),
            effect:      TriggerEffect::ApplyBuff(BuffSpec {
                id:          max_buff_id,
                effects:     max_effects,
                max_stacks:  1,
                stack_type:  StackType::Refresh,
                duration_ms,
            }),
            cooldown_ms: None,
        });
    }
}

/// trigger_event 문자열 → TriggerFilter 매핑.
/// 미지정(None) 또는 "on_cast" → SkillCast (기본값).
fn str_to_trigger_filter(s: Option<&str>) -> TriggerFilter {
    match s {
        Some("on_hit")  => TriggerFilter::AnyHit,
        Some("on_crit") => TriggerFilter::AnyCrit,
        _               => TriggerFilter::SkillCast,
    }
}

// ---------------------------------------------------------------------------
// 문자열 → 열거형 매핑
// ---------------------------------------------------------------------------

pub fn str_to_stat_field(s: &str) -> Option<StatField> {
    match s {
        "damage_increase"        => Some(StatField::DamageIncrease),
        "target_damage_increase" => Some(StatField::TargetDamageIncrease),
        "crit_rate"              => Some(StatField::CritRate),
        "crit_dmg"               => Some(StatField::CritDmg),
        "attack_power_mul"       => Some(StatField::AttackPowerMul),
        "additional_damage"      => Some(StatField::AdditionalDamage),
        "type_damage"            => Some(StatField::TypeDamage),
        "elemental_damage"       => Some(StatField::ElementalDamage),
        "weapon_attack_power"    => Some(StatField::WeaponAttackPower),
        "evolution_damage"       => Some(StatField::EvolutionDamage),
        "attack_speed"           => Some(StatField::AttackSpeed),
        "movement_speed"         => Some(StatField::MovementSpeed),
        "target_defense_reduction" => Some(StatField::TargetDefenseReduction),
        _                        => None,
    }
}

pub fn str_to_skill_tag(s: &str) -> Option<SkillTag> {
    match s {
        "AttackHead"       => Some(SkillTag::AttackHead),
        "AttackBack"       => Some(SkillTag::AttackBack),
        "NonDirectional"   => Some(SkillTag::NonDirectional),
        "All"              => Some(SkillTag::All),
        "CategoryRuin"     => Some(SkillTag::CategoryRuin),
        "CategoryNormal"   => Some(SkillTag::CategoryNormal),
        "CategoryStacked"  => Some(SkillTag::CategoryStacked),
        "CategoryHyper"    => Some(SkillTag::CategoryHyper),
        // 미구현 태그
        "HoldingCasting" | "Charge" => None,
        _                  => None,
    }
}
