use crate::builder::dto::{BuildRequest, GemInput};
use crate::builder::skill::{multiply_hit_damage, multiply_skill_dots};
use crate::builder::skill::SkillPipelineMap;
use crate::builder::stat::StatBuilder;
use crate::profile::{SkillProfile, TriggerEffect};
use hashbrown::HashMap;
use super::gem_target;

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

fn is_damage_gem(gem_type: &str, skill_effect: &str) -> bool {
    matches!(gem_type, "겁화" | "멸화")
        || (!matches!(gem_type, "겁화" | "멸화" | "작열" | "홍염")
            && skill_effect.contains("피해"))
}

fn is_cooldown_gem(gem_type: &str, skill_effect: &str) -> bool {
    matches!(gem_type, "작열" | "홍염")
        || (!matches!(gem_type, "겁화" | "멸화" | "작열" | "홍염")
            && (skill_effect.contains("쿨타임") || skill_effect.contains("재사용 대기시간")))
}

fn is_arcana_card_gem(req: &BuildRequest, gem: &GemInput) -> bool {
    req.character.class_name == "아르카나"
        && (gem.skill_id == Some(19282) || (gem.skill_id.is_none() && gem.skill_name == "황제"))
}

// ID owns the target when supplied. An absent local ID retains the equipped
// gem's base AP, but must not borrow a local skill with the same display name.
fn skill_target_names(req: &BuildRequest, gem: &GemInput) -> Vec<String> {
    if is_arcana_card_gem(req, gem) { return vec![]; }
    let target=gem_target::resolve(req,gem,is_damage_gem(&gem.gem_type,&gem.skill_effect));
    if !target.allowed || target.owner.is_some_and(|owner|Some(owner)!=gem_target::class_id(&req.character.class_name)){return vec![];}
    if let Some(name)=target.legacy_name{return vec![name];}
    let mut names=std::collections::HashSet::new();
    req.skill_data.iter().filter(|skill|target.ids.contains(&skill.game_skill_id)&&names.insert(skill.name.clone())).map(|skill|skill.name.clone()).collect()
}

pub fn registered_target(req:&BuildRequest,gem:&GemInput)->bool {
    let target=gem_target::resolve(req,gem,is_damage_gem(&gem.gem_type,&gem.skill_effect));
    target.known&&target.allowed
}

pub fn validate(req: &BuildRequest) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for gem in &req.character.gems {
        if gem.level == 0 { continue; }
        if gem.level > 10 { return Err("보석: 1~10레벨을 입력해주세요.".into()); }
        let card = is_arcana_card_gem(req, gem);
        if gem.skill_name.trim().is_empty() && gem.skill_id.is_none() { return Err("보석: 스킬 대상을 선택해주세요.".into()); }
        let damage = is_damage_gem(&gem.gem_type, &gem.skill_effect);
        let kind = if damage { "피해" } else if is_cooldown_gem(&gem.gem_type, &gem.skill_effect) { "쿨감" }
            else { if gem.gem_type == "광휘" { return Err("광휘 보석: 피해 또는 쿨감 효과를 선택해주세요.".into()); } continue; };
        if (card || gem.skill_id == Some(19282)) && !damage { return Err("황제 보석은 피해 효과만 지원합니다.".into()); }
        let target=gem_target::resolve(req,gem,damage);
        if target.known&&!target.allowed{return Err(format!("보석 대상 오류: {} {kind} 직접 장착 대상이 아닙니다.",gem.skill_name));}
        let targets = [target.key];
        for target in targets {
            if !seen.insert((target,kind)) { return Err(format!("보석 중복: {} {kind} 효과는 하나만 장착해주세요.",gem.skill_name)); }
        }
    }
    Ok(())
}

/// Class tripods/cores can attach conditional hit bonuses after the gem pipeline.
/// Scale only the additive portion of those same-hit bonuses, once all are attached.
/// Independent triggered damage (poison, runes, etc.) does not inherit a skill gem.
pub fn apply_conditional_hit_damage(skills: &mut [SkillProfile], req: &BuildRequest) {
    for gem in &req.character.gems {
        if gem.level == 0 || !is_damage_gem(&gem.gem_type, &gem.skill_effect) { continue; }
        for name in skill_target_names(req,gem) {
            let Some(skill) = skills.iter_mut().find(|s|s.name == name) else { continue; };
            let multiplier = 1.0 + damage_gem_value(&gem.gem_type, gem.level);
            for hit in &mut skill.hits {
                for trigger in &mut hit.triggers {
                    match &mut trigger.effect {
                        TriggerEffect::DamageBonus(value)
                        | TriggerEffect::DamageBonusPerBuffStack {per_stack: value, ..}
                        | TriggerEffect::DamageBonusPerTargetBuffStack {per_stack: value, ..} => *value *= multiplier,
                        _ => {}
                    }
                }
            }
        }
    }
}

/// 황제 보석의 KeyIndex=19282는 카드 스킬 ID이며, 실제 피해는 카드 런타임 효과에서 발생한다.
pub fn register_arcana_card_damage(effects: &mut HashMap<u32, TriggerEffect>, req: &BuildRequest) {
    let Some(effect) = effects.get_mut(&19282) else { return };
    for gem in &req.character.gems {
        if gem.level == 0
            || !is_arcana_card_gem(req,gem)
            || !is_damage_gem(&gem.gem_type, &gem.skill_effect)
        {
            continue;
        }
        multiply_runtime_damage(effect, 1.0 + damage_gem_value(&gem.gem_type, gem.level));
    }
}

fn multiply_runtime_damage(effect: &mut TriggerEffect, multiplier: f64) {
    match effect {
        TriggerEffect::Sequence(effects) | TriggerEffect::RandomChoice(effects) => {
            for effect in effects { multiply_runtime_damage(effect, multiplier); }
        }
        TriggerEffect::WeightedRandomChoice(effects) => {
            for (_, effect) in effects { multiply_runtime_damage(effect, multiplier); }
        }
        TriggerEffect::Chance { effect, .. } | TriggerEffect::Schedule { effect, .. } => {
            multiply_runtime_damage(effect, multiplier);
        }
        TriggerEffect::DealRuntimeDamage(hit) => hit.damage_increase = (1.0 + hit.damage_increase) * multiplier - 1.0,
        _ => {}
    }
}

/// 보석 효과를 StatBuilder 및 SkillPipeline에 등록한다.
///
/// - 겁화/멸화 → 해당 스킬의 본타·별도 루인에 독립 피해 배율 적용
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
        for target_name in skill_target_names(req,gem) {
            let Some(pipeline) = skill_pipelines.get_mut(&target_name) else {
                continue;
            };

            let is_damage = is_damage_gem(&gem.gem_type, &gem.skill_effect);
            let is_cooldown = is_cooldown_gem(&gem.gem_type, &gem.skill_effect);
            if is_damage {
                let bonus = damage_gem_value(&gem.gem_type, gem.level);
                let dot_ids: Vec<String> = if req.character.class_name == "리퍼" { Vec::new() } else {
                    req.skill_data.iter().find(|data|data.name == target_name).into_iter().flat_map(|data|
                        data.hits.iter().filter_map(|hit|hit.dot.as_ref().map(|dot|dot.dot_id.clone()))
                            .chain(data.timed_dots.iter().map(|timed|timed.dot.dot_id.clone()))
                    ).collect()
                };
                pipeline.add_stats(move |skill| {
                    multiply_hit_damage(skill, 1.0 + bonus);
                    multiply_skill_dots(skill, &dot_ids, 1.0 + bonus);
                    Ok(())
                });
            } else if is_cooldown {
                let reduction = cooldown_gem_value(&gem.gem_type, gem.level);
                pipeline.add_stats(move |skill| {
                    let reduce_ms = (skill.cooldown_ms as f64 * reduction) as u32;
                    skill.cooldown_ms = skill.cooldown_ms.saturating_sub(reduce_ms);
                    if skill.max_stacks > 1 {
                        skill.charge_recovery_ms -= (skill.charge_recovery_ms as f64 * reduction) as u32;
                    }
                    Ok(())
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gem_request(gems: serde_json::Value) -> BuildRequest {
        serde_json::from_value(serde_json::json!({"character":{"name":"gem probe","className":"아르카나","stats":{},"gems":gems}})).unwrap()
    }

    #[test]
    fn source_group_members_receive_one_effect_and_numeric_domains_do_not_alias() {
        let mut req=gem_request(serde_json::json!([{"type":"겁화","level":10,"skillId":11010,"skillName":"group","skillEffect":"피해 증가"}]));
        req.character.class_name="버서커".into();
        req.skill_data=serde_json::from_value(serde_json::json!([{"name":"base","gameSkillId":16140,"cooldown":1,"hits":[]},{"name":"tier","gameSkillId":16147,"cooldown":1,"hits":[]},{"name":"other","gameSkillId":16120,"cooldown":1,"hits":[]}])).unwrap();
        assert!(validate(&req).is_ok());
        assert_eq!(skill_target_names(&req,&req.character.gems[0]),vec!["base","tier"]);
        req.character.gems[0].skill_id=Some(16147);
        assert_eq!(skill_target_names(&req,&req.character.gems[0]),vec!["base","tier"]);
        req.character.gems[0].skill_id=Some(16000);
        assert!(skill_target_names(&req,&req.character.gems[0]).is_empty());
        req.character.gems[0].skill_id=Some(37290);
        assert!(validate(&req).unwrap_err().contains("보석 대상"));
        req.character.class_name="발키리".into();
        req.character.gems[0].skill_id=None;
        req.character.gems[0].skill_name="신앙 스킬".into();
        req.character.gems[0].gem_type="작열".into();
        assert!(validate(&req).unwrap_err().contains("보석 대상"));
        req.character.gems[0].gem_type="겁화".into();
        assert!(validate(&req).is_ok());
    }

    #[test]
    fn id_owned_card_has_no_ordinary_target_and_rejects_cooldown_effects() {
        let req=gem_request(serde_json::json!([{"type":"겁화","level":10,"skillId":19282,"skillName":"포 카드","skillEffect":"피해 증가"}]));
        assert!(skill_target_names(&req,&req.character.gems[0]).is_empty());
        assert!(is_arcana_card_gem(&req,&req.character.gems[0]));
        assert!(validate(&req).is_ok());
        let req=gem_request(serde_json::json!([{"type":"작열","level":10,"skillId":19282,"skillName":"황제","skillEffect":"재사용 대기시간 감소"}]));
        assert!(validate(&req).unwrap_err().contains("황제 보석"));
        let req=gem_request(serde_json::json!([{"type":"겁화","level":10,"skillId":26070,"skillName":"포 카드","skillEffect":"피해 증가"}]));
        assert!(skill_target_names(&req,&req.character.gems[0]).is_empty());
        assert!(!is_arcana_card_gem(&req,&req.character.gems[0]));
    }

    #[test]
    fn equip_rules_reject_duplicate_effects_across_types_and_runtime_card_aliases() {
        for (left,right) in [("겁화","겁화"),("멸화","겁화"),("작열","홍염"),("광휘","겁화")] {
            let req=gem_request(serde_json::json!([
                {"type":left,"level":10,"skillName":"포 카드","skillEffect":"피해 증가"},
                {"type":right,"level":8,"skillName":"포 카드","skillEffect":"피해 증가"}]));
            assert!(validate(&req).unwrap_err().contains("보석 중복"));
        }
        let req=gem_request(serde_json::json!([
            {"type":"겁화","level":10,"skillId":19282,"skillName":"ID로 식별한 카드","skillEffect":"피해 증가"},
            {"type":"겁화","level":8,"skillName":"황제","skillEffect":"피해 증가"}]));
        assert!(validate(&req).unwrap_err().contains("보석 중복"));
    }

    #[test]
    fn equip_rules_allow_separate_effects_and_zero_diagnostics_but_reject_unfinished_inputs() {
        let req=gem_request(serde_json::json!([
            {"type":"겁화","level":10,"skillName":"포 카드","skillEffect":"피해 증가"},
            {"type":"작열","level":10,"skillName":"포 카드","skillEffect":"재사용 대기시간 감소"},
            {"type":"겁화","level":0,"skillName":"포 카드","skillEffect":"피해 증가"}]));
        assert!(validate(&req).is_ok());
        for (level,name,kind,effect) in [(11,"포 카드","겁화","피해 증가"),(10,"","겁화","피해 증가"),(10,"포 카드","광휘","")] {
            let req=gem_request(serde_json::json!([{"type":kind,"level":level,"skillName":name,"skillEffect":effect}]));
            assert!(validate(&req).is_err());
        }
    }

    #[test]
    fn card_gem_multiplies_an_existing_runtime_damage_bonus() {
        let mut effect = TriggerEffect::Sequence(vec![TriggerEffect::DealRuntimeDamage(crate::profile::RuntimeDamageSpec {
            destruction: 0, hit_id: "emperor".into(), name: "황제".into(), base_damage: 100.0,
            skill_modifier: 0.0, damage_increase: 1.0, crit_chance_bonus: 0.0,
            crit_damage_bonus: 0.0, random_crit_damage_chance: 0.0, random_crit_damage_bonus: 0.0,
            defense_ignore: 0.0, defense_ignore_chance: 0.0,
        })]);
        multiply_runtime_damage(&mut effect, 1.44);
        let TriggerEffect::Sequence(effects) = effect else { panic!() };
        let TriggerEffect::DealRuntimeDamage(hit) = &effects[0] else { panic!() };
        assert!((1.0 + hit.damage_increase - 2.88).abs() < 1e-12);
    }

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
