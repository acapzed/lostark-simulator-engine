use super::{dto::{BuildRequest, SimEffect}, modifier::ModifierManager, skill::SkillPipelineMap, stat::StatBuilder};
use crate::profile::{GlobalTrigger, TriggerCondition, TriggerEffect, TriggerFilter};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub fn resolve(req: &BuildRequest) -> Result<(Vec<Value>, Vec<SimEffect>), String> {
    let Some(grid) = &req.character.ark_grids else { return Ok((vec![], vec![])); };
    if !grid.gems.is_empty() || !grid.class_cores.is_empty() || !grid.common_cores.is_empty() || grid.reaper_slots.is_empty() {
        return Err("리퍼 아크그리드: 장착 코어·젬 원본 선택이 필요합니다. 외부 효과 테이블은 허용하지 않습니다".into());
    }
    let cores: Vec<Value> = serde_json::from_str(include_str!("../../skill-data/reaper/backend/ark-grid.json")).unwrap();
    let gems: Vec<Value> = serde_json::from_str(include_str!("../../skill-data/common/backend/ark-grid-gems.json")).unwrap();
    let gem_effects: Value = serde_json::from_str(include_str!("../../skill-data/reaper/backend/ark-grid-gem-effects.json")).unwrap();
    let nodes: Vec<Value> = serde_json::from_str(include_str!("../../skill-data/reaper/backend/passive-catalog.json")).unwrap();
    let tripods: Vec<Value> = serde_json::from_str(include_str!("../../skill-data/reaper/backend/tripod-catalog.json")).unwrap();
    let selected_nodes: BTreeSet<u64> = match req.character.ark_passive.as_ref().filter(|passive| !passive.class_nodes.is_empty()) {
        Some(passive) => passive.class_nodes.iter().filter(|node| node.level > 0)
            .filter_map(|chosen| nodes.iter().find(|node| node["name"] == chosen.name).and_then(|node| node["id"].as_u64())).collect(),
        None => BTreeSet::from([if req.reaper_build == Some(crate::profile::reaper::ReaperScenario::ThirstChaosSustain) { 2261000 } else { 2260500 }]),
    };
    let mut symbols = BTreeSet::new();
    let mut levels: BTreeMap<String, u32> = BTreeMap::new();
    let mut effects = vec![];
    for slot in &grid.reaper_slots {
        let core = cores.iter().find(|core| core["name"] == slot.name && core["grade"] == slot.grade
            && core["coreAlignment"] == slot.core_alignment && core["coreSymbol"] == slot.core_symbol
            && core["simulationSupported"] == true)
            .ok_or_else(|| format!("리퍼 미지원 아크그리드 코어: {} ({})", slot.name, slot.grade))?;
        if !symbols.insert((&slot.core_alignment, &slot.core_symbol)) || slot.gems.len() > 4 {
            return Err("리퍼 아크그리드: 질서/혼돈 문양별 코어 1개·젬 최대4개만 허용합니다".into());
        }
        let mut cost = 0;
        let mut points = 0;
        for gem in &slot.gems {
            let data = gems.iter().find(|data| data["name"] == gem.name && data["gemType"] == slot.core_alignment)
                .ok_or_else(|| format!("리퍼 아크그리드 젬 이름/종류 오류: {}", gem.name))?;
            if gem.gem_point > 5 || gem.willpower_efficiency > 5 || gem.effects.len() > 2 {
                return Err("리퍼 아크그리드: 젬 포인트·의지력 효율은0~5, 효과는최대2개입니다".into());
            }
            cost += (data["willpowerRequirement"].as_u64().unwrap() as u32).saturating_sub(gem.willpower_efficiency);
            points += gem.gem_point;
            let mut names = BTreeSet::new();
            for effect in &gem.effects {
                if effect.level == 0 { continue; }
                if effect.level > 5 || gem_effects.get(&effect.name).is_none()
                    || !data["possibleEffects"].as_array().unwrap().iter().any(|name| name == &effect.name)
                    || !names.insert(&effect.name) {
                    return Err(format!("리퍼 아크그리드 젬 효과/레벨/중복 오류: {}", effect.name));
                }
                *levels.entry(effect.name.clone()).or_default() += effect.level;
            }
        }
        if cost > core["willpower"].as_u64().unwrap() as u32 {
            return Err(format!("리퍼 아크그리드 {}: 의지력 예산 초과", slot.name));
        }
        for option in core["options"].as_array().unwrap().iter().filter(|option| option["pointRequirement"].as_u64().unwrap() <= points as u64) {
            if option["includeNodeId"] != 0 && !selected_nodes.contains(&option["includeNodeId"].as_u64().unwrap()) {
                return Err(format!("리퍼 아크그리드 {}: 요구 깨달음 노드를 선택하세요", slot.name));
            }
            effects.extend(option["runtimeEffects"].as_array().unwrap().iter().filter(|effect| {
                if effect["requiresBossTarget"] == true && req.assumed_target_seed_or_lower { return false; }
                let Some(required) = effect.get("requiresTripodId") else { return true; };
                req.character.skills.iter().any(|skill| skill.tripods.iter().any(|choice|
                    tripods.iter().any(|tripod| tripod["skillName"] == skill.name && tripod["name"] == choice.name
                        && tripod["choiceId"] == *required && effect["skillIds"].as_array().unwrap().contains(&tripod["gameSkillId"]))))
            }).cloned());
        }
    }
    // Damage/rotation requests do not consume Life HP. Preserve slot gems and
    // other combat effects; the explicit baseline remains for internal HP probes.
    if req.reaper_hp_baseline.is_none() {
        effects.retain(|effect| effect["kind"] != "raw_hp_delta");
    }
    let mut stats = vec![];
    for (name, level) in levels {
        let row = gem_effects[&name]["effectsByLevel"].get(level as usize - 1)
            .ok_or_else(|| format!("리퍼 아크그리드 젬 합산 레벨 미지원: {name} {level}"))?;
        stats.extend(serde_json::from_value::<Vec<SimEffect>>(row.clone()).unwrap());
    }
    Ok((effects, stats))
}

pub fn register(stat: &mut StatBuilder, modifier: &mut ModifierManager, triggers: &mut Vec<GlobalTrigger>, pipelines: &mut SkillPipelineMap, req: &BuildRequest) -> Result<Vec<Value>, String> {
    let (effects, stats) = resolve(req)?;
    super::register::sim_effects::apply_effect_list(stat, modifier, triggers, req, &stats, "reaper:ark_grid:gem")?;
    let critical_trace = effects.iter().filter(|effect| matches!(effect["rawBuffId"].as_u64(),Some(608261000 | 608263000 | 608264000)))
        .max_by_key(|effect| effect["rawBuffId"].as_u64().unwrap());
    let steel_trace = effects.iter().filter(|effect| matches!(effect["rawBuffId"].as_u64(),Some(608251000 | 608253000 | 608254000)))
        .max_by_key(|effect| effect["rawBuffId"].as_u64().unwrap());
    for effect in effects.iter().filter(|effect| super::register::sim_effects::is_generic_kind(effect["kind"].as_str().unwrap_or(""))) {
        if matches!(effect["rawBuffId"].as_u64(),Some(608261000 | 608263000 | 608264000)) && Some(effect) != critical_trace {continue;}
        if matches!(effect["rawBuffId"].as_u64(),Some(608251000 | 608253000 | 608254000)) && Some(effect) != steel_trace {continue;}
        let source = format!("reaper:ark_grid:core:{}", effect["sourceRow"]);
        let value: SimEffect = serde_json::from_value(effect.clone()).unwrap();
        // Separate option sources prevent repeated stat buckets overwriting earlier thresholds.
        super::register::sim_effects::apply_effect_list(stat, modifier, triggers, req, std::iter::once(&value), &source)?;
    }
    if let Some(heal) = effects.iter().filter(|effect| effect["kind"] == "on_hit_heal" && effect["buffId"] == "608221000")
        .max_by_key(|effect| effect["value"].as_u64().unwrap())
    {
        triggers.push(GlobalTrigger {
            filter: TriggerFilter::AnyHit,
            condition: TriggerCondition::Always,
            effect: TriggerEffect::GainResource { resource: "Hp".into(), amount: heal["value"].as_f64().unwrap() },
            cooldown_ms: Some(heal["cooldownMs"].as_u64().unwrap() as u32),
        });
    }
    for chosen in &req.character.skills {
        let id = req.skill_data.iter().find(|skill| skill.name == chosen.name).unwrap().game_skill_id;
        let mut selected: Vec<_> = effects.iter().filter(|effect| matches!(effect["kind"].as_str(), Some("damage_increase" | "global_damage_increase" | "back_damage" | "resource_damage_increase" | "resource_crit_rate" | "cooldown_ms" | "cooldown_ratio" | "normal_skill_cooldown_reduction")))
            .filter(|effect| effect["skillIds"].as_array().unwrap().is_empty()
            || effect["skillIds"].as_array().unwrap().iter().any(|value| value == id)).cloned().collect();
        selected.sort_by_key(|effect| effect["sourceRow"].as_u64().unwrap());
        pipelines.get_mut(&chosen.name).unwrap().add_stats(move |skill| {
            let cooldown_percent: f64 = selected.iter().filter(|effect| effect["kind"] == "normal_skill_cooldown_reduction")
                .map(|effect| effect["value"].as_f64().unwrap()).sum();
            for effect in selected.iter().filter(|effect| effect["kind"] != "normal_skill_cooldown_reduction") {
                let value = effect["value"].as_f64().unwrap();
                if effect["kind"] == "resource_damage_increase" {
                    skill.cast_resource_damage_bonuses.push((effect["resource"].as_str().unwrap().into(), value));
                } else if effect["kind"] == "resource_crit_rate" {
                    skill.cast_resource_crit_rate_bonuses.push((effect["resource"].as_str().unwrap().into(),value));
                } else if effect["kind"] == "cooldown_ms" {
                    skill.cooldown_ms = (skill.cooldown_ms as f64 + value).max(0.0) as u32;
                } else if effect["kind"] == "cooldown_ratio" {
                    skill.cooldown_ms = (skill.cooldown_ms as f64 * (1.0 + value)) as u32;
                } else if effect["kind"] != "back_damage" || skill.tags.contains(&crate::profile::SkillTag::AttackBack) {
                    for hit in &mut skill.hits { hit.damage_increase = (1.0 + hit.damage_increase) * (1.0 + value) - 1.0; }
                }
            }
            if cooldown_percent > 0.0 {
                let multiplier = 1.0 - cooldown_percent / 100.0;
                skill.cooldown_ms = (skill.cooldown_ms as f64 * multiplier) as u32;
                skill.charge_recovery_ms = (skill.charge_recovery_ms as f64 * multiplier) as u32;
            }
            Ok(())
        });
    }
    for effect in effects.iter().filter(|effect| effect["kind"]=="cast_protection_state") {
        for chosen in req.character.skills.iter().filter(|chosen| req.skill_data.iter().any(|skill|
            skill.name==chosen.name&&effect["skillIds"].as_array().unwrap().iter().any(|id|id==skill.game_skill_id))) {
            let id=effect["buffId"].as_u64().unwrap().to_string();
            pipelines.get_mut(&chosen.name).unwrap().add_stats(move |skill| {
                // shortcut: Record cast protection; real incoming attacks/interruption need a separate input model.
                skill.timed_effects.push(crate::profile::skill::TimedSkillEffect {phase:0,action_delay_ms:0,until_cast_end:true,action_end_ms:None,
                    effect:crate::profile::TriggerEffect::ApplyBuff(crate::profile::BuffSpec {
                        id:id.clone(),effects:Default::default(),max_stacks:1,
                        stack_type:crate::profile::StackType::Refresh,duration_ms:0 })});
                Ok(())
            });
        }
    }
    for effect in effects.iter().filter(|effect| effect["kind"] == "chaos_refresh_after_dagger_cast") {
        let ids = effect["skillIds"].as_array().unwrap();
        for chosen in req.character.skills.iter().filter(|chosen| req.skill_data.iter().any(|skill|
            skill.name == chosen.name && ids.iter().any(|id| id.as_u64() == Some(skill.game_skill_id as u64)))) {
            pipelines.get_mut(&chosen.name).unwrap().add_stats(|skill| {
                for (phase, &duration) in skill.cast_times_ms.iter().enumerate() {
                    skill.timed_effects.push(crate::profile::skill::TimedSkillEffect {
                        phase: phase as u32,
                        action_delay_ms: duration,
                        until_cast_end: false,
                        action_end_ms: None,
                        effect: crate::profile::TriggerEffect::RefreshReaperChaos,
                    });
                }
                Ok(())
            });
        }
    }
    let destiny: Vec<super::dto::ArkGridClassCoreRuntimeEffect> = effects.iter()
        .filter(|effect| effect["kind"].as_str().is_some_and(|kind| kind.starts_with("destiny_") || kind.starts_with("on_destiny_")))
        .map(|effect| serde_json::from_value(effect.clone()).unwrap()).collect();
    super::class::arcana::ark_grid::apply_destiny(triggers, pipelines, req, &destiny.iter().collect::<Vec<_>>());
    Ok(effects)
}


pub fn apply_stage_speed(skills: &mut [crate::profile::SkillProfile], effects: &[Value]) {
    for effect in effects.iter().filter(|effect| effect["kind"] == "attack_stage_speed") {
        let value = effect["value"].as_f64().unwrap();
        let ids = effect["skillIds"].as_array().unwrap();
        for skill in skills.iter_mut().filter(|skill| skill.tags.iter().any(|tag|
            matches!(tag, crate::profile::SkillTag::SkillId(id) if ids.iter().any(|value| value.as_u64()==Some(*id as u64))))) {
            // Class configuration appends self Notify events; scale after all of them exist.
            let scale = |duration: u32| (duration as f64 / (1.0 + value)).round() as u32;
            for duration in &mut skill.cast_times_ms { *duration = scale(*duration); }
            for hit in &mut skill.hits { hit.action_delay_ms = scale(hit.action_delay_ms); }
            for timed in &mut skill.timed_effects {
                timed.action_delay_ms = scale(timed.action_delay_ms);
                timed.action_end_ms = timed.action_end_ms.map(scale);
            }
        }
    }
}

pub fn apply_runtime_damage(skills: &mut [crate::profile::SkillProfile], triggers: &mut [GlobalTrigger], effects: &[Value]) {
    use crate::profile::TriggerEffect;
    fn apply(effect: &mut TriggerEffect, global: bool, burst: bool, value: f64) {
        match effect {
            TriggerEffect::Sequence(sequence) => {for effect in sequence {apply(effect,global,burst,value);}},
            TriggerEffect::ScheduleTargetStackDot {dot_id,hit,..} if global || burst && dot_id=="reaper:bleeding:burst" => {
                if global {hit.damage_increase=(1.0+hit.damage_increase)*(1.0+value)-1.0;}
                else {hit.base_damage*=1.0+value;hit.skill_modifier*=1.0+value;}
            },
            TriggerEffect::ScheduleDot {hit,..} | TriggerEffect::DealRuntimeDamage(hit)
            | TriggerEffect::DealDamageAndSetTargetBuffStacks {hit,..} if global => {
                hit.damage_increase=(1.0+hit.damage_increase)*(1.0+value)-1.0;
            },
            TriggerEffect::DealDamageByTargetBuffStacks {hits_by_stack,..} if global => {
                for hit in hits_by_stack {hit.damage_increase=(1.0+hit.damage_increase)*(1.0+value)-1.0;}
            },
            TriggerEffect::DamageBonus(value_delta) if !burst => {*value_delta*=1.0+value;},
            TriggerEffect::DamageBonusPerBuffStack {per_stack,..} if !burst => {*per_stack*=1.0+value;},
            _ => {},
        }
    }
    let mut selected:Vec<_>=effects.iter().filter(|e|matches!(e["kind"].as_str(),Some("global_damage_increase"|"runtime_effect_damage")) || e["scaleConditionalDamage"]==true).collect();
    selected.sort_by_key(|effect|effect["sourceRow"].as_u64().unwrap());
    for effect in selected {
        let global=effect["kind"]=="global_damage_increase";let burst=effect["kind"]=="runtime_effect_damage";let value=effect["value"].as_f64().unwrap();
        for skill in skills.iter_mut().filter(|skill|global || skill.tags.iter().any(|tag|matches!(tag,crate::profile::SkillTag::SkillId(id) if effect["skillIds"].as_array().unwrap().iter().any(|value|value==id)))) {
            for hit in &mut skill.hits {for trigger in &mut hit.triggers {apply(&mut trigger.effect,global,burst,value);}}
        }
        if global {for trigger in triggers.iter_mut() {apply(&mut trigger.effect,true,false,value);}}
    }
}
