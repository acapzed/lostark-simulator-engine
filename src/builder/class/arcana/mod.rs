pub mod ark_grid;
pub mod ark_passive;
pub mod card;

use crate::builder::dto::{ArcanaCardInput, ArkGridClassCoreInput, ArkPassiveClassNodeInput, BuildRequest};
use crate::builder::modifier::ModifierManager;
use crate::builder::skill::SkillPipelineMap;
use crate::builder::stat::StatBuilder;
use crate::profile::{actor::ArcanaCardPoolEntry, GlobalTrigger, SkillCategory, SkillProfile, SkillSlot, TriggerEffect};

pub(super) const ARCANA_RUIN_STACK_BUFF_ID: &str = "arcana_ruin_stack";

/// User-adopted reference totals at specialization 1800, without gauge bonuses.
/// Other tripods retain their raw hit routing and proportional per-hit distribution.
pub fn register_identity_reference(pipelines: &mut SkillPipelineMap, req: &BuildRequest) {
    if req.character.class_name != "아르카나" { return; }
    let spec_factor = 1.0 + 1800.0 / 699.0 * 0.25;
    for data in &req.skill_data {
        // Hand-authored requests with independent gauge units keep their explicit values.
        if !data.hits.iter().any(|hit| hit.ultimate_point_gain > 0.0) { continue; }
        let (percent, original) = match data.game_skill_id {
            19350 => (44.12,6396.0), 19140 => (17.20,1092.0),
            19160 => (26.85,1716.0), 19310 => (12.81,884.0),
            19320 => (5.97,676.0), 19150 => (13.20,936.0),
            19180 => (6.55,416.0), 19190 => (15.56,936.0),
            19240 => (0.0,1.0), _ => continue,
        };
        let seren = data.game_skill_id == 19240;
        let absolute_hits: Vec<bool> = data.hits.iter().map(|h| h.identity_gain.as_ref().is_some_and(|g| g.is_absolute)).collect();
        let Some(pipeline) = pipelines.get_mut(&data.name) else { continue; };
        pipeline.add_trigger(move |profile| {
            for (index, hit) in profile.hits.iter_mut().enumerate() {
                let multiplier = if seren {
                    if hit.phase == 0 { 5.54 * 100.0 / spec_factor / 1456.0 }
                    else { 4.15 * 100.0 / spec_factor / 1144.0 }
                } else { percent * 100.0 / spec_factor / original };
                for trigger in &mut hit.triggers {
                    match &mut trigger.effect {
                        TriggerEffect::GainResource { resource, amount } if resource == "CardGauge" && !absolute_hits.get(index).copied().unwrap_or(false) => *amount *= multiplier,
                        TriggerEffect::ConsumeTargetBuff { preserve_resource_gain: Some((resource,amount)), .. }
                            if seren && resource == "CardGauge" => *amount *= multiplier,
                        TriggerEffect::DealDamageByTargetBuffStacks { card_gauge_by_stack, .. } if seren => {
                            // Only four-stack gain is constrained by the supplied observations.
                            *card_gauge_by_stack = vec![0.0,0.0,0.0,19.70 * 100.0 / spec_factor];
                        }
                        _ => {}
                    }
                }
            }
            Ok(())
        });
    }
}

#[cfg(test)]
mod absolute_identity_tests {
    use super::*;

    #[test]
    fn absolute_hit_gain_bypasses_reference_totals_and_keeps_bonus_pipeline() {
        let mut json: serde_json::Value = serde_json::from_str(include_str!("../../../../fixtures/arcana-representative-request.json")).unwrap();
        let name = json["skillData"].as_array().unwrap().iter().find(|s| s["gameSkillId"] == 19150).unwrap()["name"].as_str().unwrap().to_owned();
        json["character"]["skills"].as_array_mut().unwrap().retain(|s| s["name"] == name);
        json["skillData"].as_array_mut().unwrap().retain(|s| s["name"] == name);
        for value in [0.0, 123.5, 10000.0] {
            let mut input = json.clone();
            for hit in input["skillData"][0]["hits"].as_array_mut().unwrap() {
                hit["identityGain"] = serde_json::json!({"name":"CardGauge","value":value,"isAbsolute":true});
            }
            let req: BuildRequest = serde_json::from_value(input).unwrap();
            let mut pipelines = SkillPipelineMap::new([name.clone()]);
            register_identity_reference(&mut pipelines, &req);
            apply_card_gauge_multiplier(&mut pipelines, &req, 20.0);
            let mut profiles = pipelines.build_all(&req.skill_data, &req.character.skills).unwrap();
            apply_specialization(&mut profiles, 699.0);
            for hit in &profiles[0].hits {
                let amounts: Vec<f64> = hit.triggers.iter().filter_map(|t| match &t.effect {
                    TriggerEffect::GainResource {resource,amount} if resource == "CardGauge" => Some(*amount),
                    _ => None,
                }).collect();
                if value == 0.0 { assert!(amounts.is_empty()); }
                else { assert_eq!(amounts.len(), 1); assert!((amounts[0] - value * 1.2 * 1.25).abs() < 1e-8); }
            }
        }
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
                    if let TriggerEffect::DealDamageByTargetBuffStacks { card_gauge_by_stack, .. } = &mut trigger.effect {
                        for amount in card_gauge_by_stack { *amount *= multiplier; }
                    }
                }
            }
            Ok(())
        });
    }
}

/// 확정된 Arcana raw 특화 공식을 카드 게이지와 루인 스택 피해에 적용한다.
pub fn apply_specialization(skills: &mut [SkillProfile], specialization: f64) {
    let card_gauge_multiplier = 1.0 + specialization / 699.0 * 0.25;
    let ruin_damage_multiplier = 1.0 + specialization / 699.0 * 0.35;
    let awakening_damage_multiplier = 1.0 + specialization / 699.0 * 0.1528;
    for skill in skills {
        let is_ruin_skill = skill.category == SkillCategory::Ruin
            && matches!(skill.slot, SkillSlot::Normal | SkillSlot::HyperAwakeningTechniques);
        if skill.slot == SkillSlot::Awakening {
            for hit in &mut skill.hits {
                hit.damage_increase =
                    (1.0 + hit.damage_increase) * awakening_damage_multiplier - 1.0;
            }
        }
        if let Some(amount) = skill.resource_gains.get_mut("CardGauge") {
            *amount *= card_gauge_multiplier;
        }
        for hit in &mut skill.hits {
            if is_ruin_skill {
                hit.damage_increase = (1.0 + hit.damage_increase) * ruin_damage_multiplier - 1.0;
            }
            for trigger in &mut hit.triggers {
                match &mut trigger.effect {
                    TriggerEffect::GainResource { resource, amount }
                        if resource == "CardGauge" =>
                    {
                        *amount *= card_gauge_multiplier;
                    }
                    TriggerEffect::ConsumeTargetBuff {
                        preserve_resource_gain: Some((resource, amount)),
                        ..
                    } if resource == "CardGauge" => {
                        *amount *= card_gauge_multiplier;
                    }
                    TriggerEffect::DealDamageByTargetBuffStacks { hits_by_stack, card_gauge_by_stack, .. }
                        if skill.category == SkillCategory::Ruin =>
                    {
                        for amount in card_gauge_by_stack { *amount *= card_gauge_multiplier; }
                        for runtime_hit in hits_by_stack {
                            runtime_hit.damage_increase =
                                (1.0 + runtime_hit.damage_increase) * ruin_damage_multiplier - 1.0;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

pub fn build_card_pool(req: &BuildRequest) -> Vec<ArcanaCardPoolEntry> {
    let mut pool = req.card_data.iter()
        .filter(|card| card.auto_learn)
        .map(|card| ArcanaCardPoolEntry {
            skill_id: card.skill_id,
            draw_weight: card.draw_weight,
        })
        .collect();
    if req.character.engravings.iter()
        .any(|engraving| engraving.name == "황제의 칙령" && engraving.level > 0)
    {
        add_card(&mut pool, &req.card_data, 19282);
        replace_card(&mut pool, &req.card_data, 19099, 19286);
        replace_card(&mut pool, &req.card_data, 19098, 19287);
    }
    if let Some(ark_passive) = &req.character.ark_passive {
        for node in &ark_passive.class_nodes {
            match (node.name.as_str(), node.level) {
                ("황제의 칙령", 1..) => add_card(&mut pool, &req.card_data, 19282),
                ("황제의 하사품", 1) => replace_card(&mut pool, &req.card_data, 19099, 19286),
                ("황제의 하사품", 2..) => {
                    replace_card(&mut pool, &req.card_data, 19099, 19286);
                    replace_card(&mut pool, &req.card_data, 19098, 19287);
                }
                ("황후의 기사", 1..) => add_card(&mut pool, &req.card_data, 19288),
                _ => {}
            }
        }
    }
    pool
}

fn add_card(pool: &mut Vec<ArcanaCardPoolEntry>, cards: &[ArcanaCardInput], card_id: u32) {
    if !pool.iter().any(|card| card.skill_id == card_id) {
        if let Some(card) = cards.iter().find(|card| card.skill_id == card_id) {
            pool.push(ArcanaCardPoolEntry {
                skill_id: card_id,
                draw_weight: card.draw_weight,
            });
        }
    }
}

fn replace_card(pool: &mut Vec<ArcanaCardPoolEntry>, cards: &[ArcanaCardInput], old_card_id: u32, new_card_id: u32) {
    pool.retain(|card| card.skill_id != old_card_id);
    add_card(pool, cards, new_card_id);
}

/// 아크 패시브 클래스 전용 노드를 적용한다.
pub fn apply_ark_passive(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    class_nodes: &[ArkPassiveClassNodeInput],
    req: &BuildRequest,
) {
    ark_passive::apply(stat, modifier, global_triggers, skill_pipelines, class_nodes, req);
}

/// 아크 그리드 질서 코어를 적용한다.
pub fn apply_ark_grid(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    class_cores: &[ArkGridClassCoreInput],
    req: &BuildRequest,
) {
    ark_grid::apply(
        stat,
        modifier,
        global_triggers,
        skill_pipelines,
        class_cores,
        req,
    );
}
