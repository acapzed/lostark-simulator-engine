use wasm_bindgen::prelude::*;

mod builder;
mod constants;
mod engine;
mod profile;

use profile::CharacterProfiles;

#[wasm_bindgen(start)]
pub fn start() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// CharacterProfiles를 빌드하고 힙에 올린 뒤 raw 포인터를 반환한다.
///
/// SharedArrayBuffer 기반 WASM 메모리를 사용하면 이 포인터를
/// 모든 Worker가 복사 없이 공유할 수 있다.
/// 시뮬레이션 완료 후 반드시 free_profiles()로 해제해야 한다.
#[wasm_bindgen]
pub fn prepare(json: &str) -> Result<u32, JsValue> {
    builder::build(json)
        .map(|profiles| Box::into_raw(Box::new(profiles)) as u32)
        .map_err(|e| JsValue::from_str(&e))
}

/// n번 시뮬레이션을 실행하고 SimResult를 JSON 문자열로 반환한다.
///
/// # Safety
/// ptr은 prepare()가 반환한 유효한 CharacterProfiles 포인터여야 한다.
/// free_profiles() 호출 후에는 사용 불가.
#[wasm_bindgen]
pub fn simulate(
    ptr: u32,
    n: u32,
    duration_ms: u32,
    seed_offset: u32,
    progress_cb: Option<js_sys::Function>,
) -> String {
    let profiles = unsafe { &*(ptr as *const CharacterProfiles) };
    let result = engine::simulate(profiles, n, duration_ms, seed_offset, progress_cb.as_ref());
    serde_json::to_string(&result).unwrap_or_else(|e| format!(r#"{{"error":"{}"}}"#, e))
}

/// 지정한 시드로 단 1회 시뮬레이션을 trace 모드로 실행해
/// 1 iter의 SimResult + cast 이벤트 시퀀스를 JSON으로 반환한다.
/// 형식: `{ "result": SimResult, "castSequence": [CastEvent...] }`
///
/// # Safety
/// ptr은 prepare()가 반환한 유효한 CharacterProfiles 포인터여야 한다.
#[wasm_bindgen]
pub fn trace(ptr: u32, seed: u32, duration_ms: u32) -> String {
    let profiles = unsafe { &*(ptr as *const CharacterProfiles) };
    let (result, trace) = engine::trace_one(profiles, seed as u64, duration_ms);
    let payload = serde_json::json!({
        "result": result,
        "castSequence": trace.casts,
        "cardUses": trace.card_uses,
        "damageEvents": trace.damage_events,
    });
    serde_json::to_string(&payload).unwrap_or_else(|e| format!(r#"{{"error":"{}"}}"#, e))
}

/// prepare()로 할당된 CharacterProfiles를 해제한다.
/// 모든 Worker의 시뮬레이션이 완료된 후 한 번만 호출해야 한다.
///
/// # Safety
/// ptr은 prepare()가 반환한 유효한 포인터여야 하며,
/// 이 함수 호출 후 해당 포인터는 더 이상 사용할 수 없다.
#[wasm_bindgen]
pub fn free_profiles(ptr: u32) {
    unsafe { drop(Box::from_raw(ptr as *mut CharacterProfiles)) };
}

#[cfg(test)]
mod tests {
    use super::{builder, engine};
    use crate::profile::{
        BuffSpec, CharacterProfiles, HitCondition, HitTrigger, HitTriggerOn, RuntimeDamageSpec,
        SkillCategory, SkillHit, SkillProfile, SkillSlot, SkillTag, SkillType, StackType,
        StatField, StatProfile, TriggerEffect, TriggerFilter,
    };
    use hashbrown::HashMap;

    fn base_arcana_request(
        tripods_json: &str,
        ark_passive_json: &str,
        ark_grids_json: &str,
    ) -> String {
        format!(
            r#"{{
                "character": {{
                    "name": "test_arcana",
                    "className": "아르카나",
                    "stats": {{"maxhp": 100000}},
                    "skills": [
                        {{"name": "셀레스티얼 레인", "level": 1, "rune": null, "tripods": {tripods_json}}}
                    ],
                    "equipments": [],
                    "accessories": [],
                    "bracelet": null,
                    "stone": null,
                    "gems": [],
                    "engravings": [],
                    "arkPassive": {ark_passive_json},
                    "arkGrids": {ark_grids_json},
                    "arkPassiveKarma": null
                }},
                "aplConfig": "",
                "skillData": [
                    {{
                        "name": "셀레스티얼 레인",
                        "gameSkillId": 19140,
                        "identityCategory": 30,
                        "skillControlType": "Normal",
                        "skillSlot": "Normal",
                        "cooldown": 1.0,
                        "castTimesMs": [1000],
                        "hits": [
                            {{"damageRatio": [1.0], "fixedDamage": [100.0], "phase": 0, "identityGain": null}}
                        ],
                        "resourceCost": null,
                        "identityGain": null,
                        "properties": {{"attackType": "", "element": "", "skillGroupIds": [2190402, 2190904]}}
                    }}
                ]
            }}"#
        )
    }

    fn arcana_quadra_request() -> String {
        r#"{
            "character": {
                "name": "test_arcana",
                "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [
                    {"name": "쿼드라 엑셀레이트", "level": 1, "rune": null, "tripods": []}
                ],
                "equipments": [],
                "accessories": [],
                "bracelet": null,
                "stone": null,
                "gems": [],
                "engravings": [],
                "arkPassive": null,
                "arkGrids": null,
                "arkPassiveKarma": null
            },
            "aplConfig": "",
            "skillData": [
                {
                    "name": "쿼드라 엑셀레이트",
                    "identityCategory": 29,
                    "skillControlType": "Normal",
                    "skillSlot": "Normal",
                    "cooldown": 1.0,
                    "castTimesMs": [1000],
                    "hits": [
                        {"damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 1, "maxStacks": 4, "durationMs": 10000}, "identityGain": null},
                        {"damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "identityGain": null},
                        {"damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "identityGain": null},
                        {"damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 1, "maxStacks": 4, "durationMs": 10000}, "identityGain": null}
                    ],
                    "resourceCost": null,
                    "identityGain": null,
                    "properties": {"attackType": "", "element": ""}
                }
            ]
        }"#.to_string()
    }

    fn arcana_single_hit_skill_request(skill_name: &str) -> String {
        let identity_category = match skill_name {
            "백 플러쉬" => 29,
            "셀레스티얼 레인" => 30,
            _ => 28,
        };
        let target_buff_stack_gain = if skill_name == "백 플러쉬" {
            r#", "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 2, "maxStacks": 4, "durationMs": 10000}"#
        } else {
            ""
        };
        format!(
            r#"{{
                "character": {{
                    "name": "test_arcana",
                    "className": "아르카나",
                    "stats": {{"maxhp": 100000}},
                    "skills": [
                        {{"name": "{skill_name}", "level": 1, "rune": null, "tripods": []}}
                    ],
                    "equipments": [],
                    "accessories": [],
                    "bracelet": null,
                    "stone": null,
                    "gems": [],
                    "engravings": [],
                    "arkPassive": null,
                    "arkGrids": null,
                    "arkPassiveKarma": null
                }},
                "aplConfig": "",
                "skillData": [
                    {{
                        "name": "{skill_name}",
                        "identityCategory": {identity_category},
                        "skillControlType": "Normal",
                        "skillSlot": "Normal",
                        "cooldown": 1.0,
                        "castTimesMs": [1000],
                        "hits": [
                            {{"damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0{target_buff_stack_gain}, "identityGain": null}}
                        ],
                        "resourceCost": null,
                        "identityGain": null,
                        "properties": {{"attackType": "", "element": ""}}
                    }}
                ]
            }}"#
        )
    }

    fn arcana_spiral_edge_request() -> String {
        r#"{
            "character": {
                "name": "test_arcana",
                "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [
                    {"name": "스파이럴 엣지", "level": 1, "rune": null, "tripods": []}
                ],
                "equipments": [],
                "accessories": [],
                "bracelet": null,
                "stone": null,
                "gems": [],
                "engravings": [],
                "arkPassive": null,
                "arkGrids": null,
                "arkPassiveKarma": null
            },
            "aplConfig": "",
            "skillData": [
                {
                    "name": "스파이럴 엣지",
                    "identityCategory": 29,
                    "skillControlType": "Combo",
                    "skillSlot": "Normal",
                    "maxPhase": 2,
                    "cooldown": 14.0,
                    "castTimesMs": [500, 500],
                    "hits": [
                        {"damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 1, "maxStacks": 4, "durationMs": 10000}, "identityGain": null},
                        {"damageRatio": [1.0], "fixedDamage": [10.0], "phase": 1, "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 1, "maxStacks": 4, "durationMs": 10000}, "identityGain": null}
                    ],
                    "resourceCost": null,
                    "identityGain": null,
                    "properties": {"attackType": "", "element": ""}
                }
            ]
        }"#.to_string()
    }

    #[test]
    fn arcana_build_uses_only_auto_learn_cards() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("셀레스티얼 레인")).unwrap();
        request["cardData"] = serde_json::json!([
            {"skillId": 19097, "name": "삼두사", "autoLearn": true},
            {"skillId": 19094, "name": "별", "autoLearn": true},
            {"skillId": 19282, "name": "황제", "autoLearn": false}
        ]);

        let profiles = builder::build(&request.to_string()).unwrap();

        assert_eq!(
            profiles.arcana_card_pool.iter().map(|card| card.skill_id).collect::<Vec<_>>(),
            vec![19097, 19094]
        );
    }

    #[test]
    fn emperor_engraving_adds_and_replaces_cards() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["engravings"] = serde_json::json!([{
            "name": "황제의 칙령",
            "grade": "유물",
            "level": 3
        }]);
        request["cardData"] = serde_json::json!([
            {"skillId": 19099, "name": "균형", "autoLearn": true},
            {"skillId": 19098, "name": "심판", "autoLearn": true},
            {"skillId": 19282, "name": "황제", "autoLearn": false},
            {"skillId": 19286, "name": "재상", "autoLearn": false},
            {"skillId": 19287, "name": "제후", "autoLearn": false}
        ]);

        let profiles = builder::build(&request.to_string()).unwrap();

        assert_eq!(
            profiles.arcana_card_pool.iter().map(|card| card.skill_id).collect::<Vec<_>>(),
            vec![19282, 19286, 19287]
        );
    }

    #[test]
    fn current_raw_engravings_apply_percent_units_and_independent_damage() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["engravings"] = serde_json::json!([
            {
                "name": "저주받은 인형", "grade": "유물", "level": 4,
                "featureData": {"grades": [{"grade": 5, "stages": [
                    {"stage": 4, "featureType": 114, "values": [2500, 1925]}
                ]}]}
            },
            {
                "name": "질량 증가", "grade": "유물", "level": 4,
                "featureData": {"grades": [{"grade": 5, "stages": [
                    {"stage": 4, "featureType": 339, "values": [1000, 2125]}
                ]}]}
            },
            {
                "name": "정밀 단도", "grade": "유물", "level": 4,
                "featureData": {"grades": [{"grade": 5, "stages": [
                    {"stage": 4, "featureType": 334, "values": [2325, 600]}
                ]}]}
            }
        ]);

        let profiles = builder::build(&request.to_string()).unwrap();

        assert!((profiles.stat.sum(StatField::AttackSpeed) + 0.10).abs() < 1e-9);
        assert!((profiles.stat.sum(StatField::CritRate) - 23.25).abs() < 1e-9);
        assert!((profiles.stat.sum(StatField::CritDmg) + 6.0).abs() < 1e-9);
        assert!((profiles.skills[0].hits[0].damage_increase
            - (1.1925 * 1.2125 - 1.0)).abs() < 1e-9);
    }

    #[test]
    fn ability_stone_level_selects_the_combined_raw_engraving_stage() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["engravings"] = serde_json::json!([{
            "name": "원한", "grade": "유물", "level": 4, "abilityLevel": 3,
            "featureData": {"grades": [{"grade": 5, "stages": [
                {"stage": 4, "featureType": 10, "values": [2325, 2000]},
                {"stage": 7, "featureType": 10, "values": [2550, 2000]}
            ]}]}
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();

        assert!((profiles.stat.sum(StatField::TargetDamageIncrease) - 0.255).abs() < 1e-9);
    }

    #[test]
    fn ability_stone_stage_sum_five_grants_raw_base_attack_power_bonus() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["stone"] = serde_json::json!({
            "baseStats": {"vitality": 23481},
            "bonusStats": {"vitality": 3525},
            "engravings": [
                {"name": "원한", "level": 9},
                {"name": "아드레날린", "level": 7}
            ]
        });
        request["character"]["engravings"] = serde_json::json!([
            {"name": "원한", "grade": "유물", "level": 4, "abilityLevel": 3},
            {"name": "아드레날린", "grade": "유물", "level": 4, "abilityLevel": 2}
        ]);

        let profiles = builder::build(&request.to_string()).unwrap();

        assert!((profiles.stat.base_ap_mul - 1.015).abs() < 1e-9);
        assert_eq!(profiles.stat.base_vitality, 27006.0);
    }

    #[test]
    fn lightning_fury_explodes_after_five_cooldown_spaced_procs() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["skillData"][0]["castTimesMs"] = serde_json::json!([5000]);
        request["skillData"][0]["hits"] = serde_json::json!([
            {"damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "fixedDelayMs": 0},
            {"damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "fixedDelayMs": 1000},
            {"damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "fixedDelayMs": 2000},
            {"damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "fixedDelayMs": 3000},
            {"damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "fixedDelayMs": 4000}
        ]);
        request["character"]["engravings"] = serde_json::json!([{
            "name": "번개의 분노", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "featureType": 113, "values": [10000, 502100], "cooldown": 1000,
                "skillBuff": {"id": 502100, "duration": 40000, "overlap": 5},
                "skillEffects": [
                    {"id": 100500008, "key": 12},
                    {"id": 100500009, "key": 2, "valueA": 1364, "valueB": 1667, "valueF": 252600}
                ]
            }]}]}
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();
        let (_, trace) = engine::trace_one(&profiles, 1, 6000);
        let explosions: Vec<_> = trace
            .damage_events
            .iter()
            .filter(|event| event.hit_id == "lightning_fury_explosion")
            .collect();

        assert_eq!(explosions.len(), 1);
        assert_eq!(explosions[0].time, 4.0);
        assert_eq!(explosions[0].hit_name, "번개의 분노 폭발");
    }

    #[test]
    fn sight_focus_buffs_only_the_configured_skill_once_per_cooldown() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["sightFocusSkillName"] = serde_json::json!("운명의 부름");
        request["character"]["engravings"] = serde_json::json!([{
            "name": "시선 집중", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "featureType": 335,
                "values": [521588, 28, 3175, 30, 521600], "cooldown": 30000,
                "skillBuffs": [
                    {"id": 521588, "duration": 6000},
                    {"id": 521600, "duration": 30000}
                ]
            }]}]}
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();
        let (_, trace) = engine::trace_one(&profiles, 1, 31_000);
        let hits = &trace.damage_events;

        assert!(hits.len() >= 31);
        assert!((hits[0].result_damage / hits[1].result_damage - 1.3175).abs() < 1e-9);
        assert!((hits[2].result_damage / hits[1].result_damage - 1.0).abs() < 1e-9);
        assert!((hits[30].result_damage / hits[1].result_damage - 1.3175).abs() < 1e-9);
    }

    #[test]
    fn crushing_fist_uses_the_raw_counter_hit_and_three_second_buffs() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("리턴")).unwrap();
        request["skillData"][0]["hits"][0]["counterAttack"] = serde_json::json!(true);
        request["character"]["engravings"] = serde_json::json!([{
            "name": "분쇄의 주먹", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "featureType": 103, "values": [521288, 521388],
                "skillBuffs": [
                    {"id": 521288, "duration": 3000,
                     "passiveOption": {"type": 2, "keyStat": 49, "value": 1650}},
                    {"id": 521388, "duration": 3000, "key": 88,
                     "values": [0, 0, 0, 0, 0, 0, 2, 1400, 1400, 0, 0]}
                ]
            }]}]}
        }]);

        let disabled = builder::build(&request.to_string()).unwrap();
        assert!(disabled.skills[0].hits[0].triggers.iter().all(|trigger| {
            !matches!(trigger.effect, TriggerEffect::Sequence(_))
        }));

        request["assumedCounterSuccess"] = serde_json::json!(true);
        let profiles = builder::build(&request.to_string()).unwrap();
        let trigger = profiles.skills[0].hits[0].triggers.iter().find_map(|trigger| {
            let TriggerEffect::Sequence(effects) = &trigger.effect else { return None };
            Some(effects)
        }).expect("counter hit should apply crushing fist buffs");
        let TriggerEffect::ApplyBuff(attack) = &trigger[0] else { panic!("missing attack buff") };
        let TriggerEffect::ApplyTargetBuff(target) = &trigger[1] else { panic!("missing target buff") };
        assert_eq!(attack.duration_ms, 3000);
        assert_eq!(target.duration_ms, 3000);
        assert_eq!(attack.effects[&StatField::AttackPowerMul], 0.165);
        assert_eq!(target.effects[&StatField::TargetDamageIncrease], 0.14);

        let (_, trace) = engine::trace_one(&profiles, 1, 2000);
        assert!((trace.damage_events[1].result_damage
            / trace.damage_events[0].result_damage - 1.14).abs() < 1e-9);
    }

    #[test]
    fn ether_boy_uses_five_equal_raw_candidates_after_pickup_delay() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["engravings"] = serde_json::json!([{
            "name": "구슬동자", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "featureType": 22, "baseRatio": 20, "cooldown": 11500,
                "dropEthers": [
                    {"id": 70188, "audience": "self", "delayTime": 1000,
                     "effect": {"key": 18, "valueB": 1240}},
                    {"id": 70288, "audience": "self", "delayTime": 1000,
                     "skillBuff": {"duration": 10000,
                       "passiveOption": {"type": 2, "keyStat": 80, "value": 1240}}},
                    {"id": 70388, "audience": "self", "delayTime": 1000},
                    {"id": 70488, "audience": "self", "delayTime": 1000},
                    {"id": 70588, "audience": "self", "delayTime": 1000},
                    {"id": 73488, "audience": "party", "delayTime": 1000}
                ]
            }]}]}
        }]);
        assert!(builder::build(&request.to_string()).unwrap().global_triggers.is_empty());

        request["etherPickupDelayMs"] = serde_json::json!(500);
        let profiles = builder::build(&request.to_string()).unwrap();
        let trigger = profiles.global_triggers.iter().find(|trigger| {
            matches!(trigger.filter, TriggerFilter::AnyHit)
                && trigger.cooldown_ms == Some(11_500)
        }).expect("ether boy should trigger on hit");
        let TriggerEffect::RandomChoice(choices) = &trigger.effect else {
            panic!("ether boy should choose one raw candidate")
        };
        assert_eq!(choices.len(), 5);
        assert!(choices.iter().all(|choice| matches!(
            choice,
            TriggerEffect::Schedule { delay_ms: 1500, .. }
        )));
        let TriggerEffect::Schedule { effect: mana, .. } = &choices[0] else { unreachable!() };
        assert!(matches!(mana.as_ref(), TriggerEffect::GainResourcePercent { resource, percent }
            if resource == "Mp" && (*percent - 0.124).abs() < 1e-9));
        let TriggerEffect::Schedule { effect: wind, .. } = &choices[1] else { unreachable!() };
        let TriggerEffect::ApplyBuff(wind) = wind.as_ref() else { unreachable!() };
        assert_eq!(wind.duration_ms, 10_000);
        assert_eq!(wind.effects[&StatField::MovementSpeed], 0.124);
    }

    #[test]
    fn ether_predator_uses_twenty_five_percent_three_stack_after_pickup_delay() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["engravings"] = serde_json::json!([{
            "name": "에테르 포식자", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "featureType": 4, "values": [2000, 72188, 72088, 250],
                "cooldown": 5500,
                "dropEthers": [
                    {"id": 72188, "delayTime": 1000,
                     "effect": {"key": 7, "valueB": 3},
                     "skillBuff": {"id": 522088, "duration": 90000, "overlap": 30,
                       "passiveOption": {"type": 2, "keyStat": 49, "value": 62}}},
                    {"id": 72088, "delayTime": 1000,
                     "effect": {"key": 7, "valueB": 0},
                     "skillBuff": {"id": 522088, "duration": 90000, "overlap": 30,
                       "passiveOption": {"type": 2, "keyStat": 49, "value": 62}}}
                ]
            }]}]}
        }]);
        assert!(builder::build(&request.to_string()).unwrap().global_triggers.is_empty());

        request["etherPickupDelayMs"] = serde_json::json!(500);
        let profiles = builder::build(&request.to_string()).unwrap();
        let trigger = profiles.global_triggers.iter().find(|trigger| {
            matches!(trigger.filter, TriggerFilter::AnyHit)
                && trigger.cooldown_ms == Some(5_500)
        }).expect("ether predator should trigger on hit");
        let TriggerEffect::RandomChoice(choices) = &trigger.effect else {
            panic!("ether predator should use four equal outcomes")
        };
        assert_eq!(choices.len(), 4);
        let stacks: Vec<_> = choices.iter().map(|choice| {
            let TriggerEffect::Schedule { delay_ms, effect } = choice else { unreachable!() };
            assert_eq!(*delay_ms, 1_500);
            let TriggerEffect::ApplyBuffStacks { spec, stacks } = effect.as_ref() else {
                unreachable!()
            };
            assert_eq!(spec.max_stacks, 30);
            assert_eq!(spec.duration_ms, 90_000);
            assert_eq!(spec.effects[&StatField::AttackPowerMul], 0.0062);
            *stacks
        }).collect();
        assert_eq!(stacks, vec![3, 1, 1, 1]);
    }

    #[test]
    fn necromancy_uses_raw_damage_after_the_configured_travel_time() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["engravings"] = serde_json::json!([{
            "name": "강령술", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "featureType": 110, "cooldown": 10000,
                "summonNpc": {"id": 900511, "coefficient": 100, "skills": [{
                    "id": 401701, "probability": 100, "actionEffects": [
                        {"timeMs": 463, "id": 40170100, "key": 1,
                         "valueA": 511, "valueB": 625, "valueF": 94650}
                    ]
                }]}
            }]}]}
        }]);
        assert!(builder::build(&request.to_string()).unwrap().global_triggers.is_empty());

        request["necromancyTravelDelayMs"] = serde_json::json!(700);
        let profiles = builder::build(&request.to_string()).unwrap();
        let (_, trace) = engine::trace_one(&profiles, 1, 12_000);
        let explosions: Vec<_> = trace.damage_events.iter()
            .filter(|event| event.hit_id == "necromancy_explosion")
            .collect();

        assert_eq!(explosions.len(), 2);
        assert_eq!(explosions[0].time, 1.163);
        assert_eq!(explosions[1].time, 11.163);
        assert_eq!(explosions[0].hit_name, "강령술 자폭병 폭발");
    }

    #[test]
    fn all_out_attack_applies_only_to_holding_skills() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("체크메이트")).unwrap();
        request["skillData"][0]["skillControlType"] = serde_json::json!("Holding");
        request["skillData"][0]["castTimesMs"] = serde_json::json!([1000]);
        request["character"]["engravings"] = serde_json::json!([{
            "name": "속전속결", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [
                {"stage": 4, "featureType": 340, "values": [1900, 2350]}
            ]}]}
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();

        assert_eq!(profiles.skills[0].cast_times_ms, vec![810]);
        assert!((profiles.skills[0].hits[0].damage_increase - 0.235).abs() < 1e-9);
    }

    #[test]
    fn super_charge_applies_to_raw_charge_skills() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("차지 테스트")).unwrap();
        request["skillData"][0]["skillControlType"] = serde_json::json!("Charge");
        request["skillData"][0]["castTimesMs"] = serde_json::json!([1000]);
        request["character"]["engravings"] = serde_json::json!([{
            "name": "슈퍼 차지", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [
                {"stage": 4, "featureType": 12, "values": [3800, 2350]}
            ]}]}
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();

        assert_eq!(profiles.skills[0].cast_times_ms, vec![620]);
        assert!((profiles.skills[0].hits[0].damage_increase - 0.235).abs() < 1e-9);
    }

    #[test]
    fn keen_blunt_weapon_keeps_crit_damage_and_hit_penalty() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["engravings"] = serde_json::json!([{
            "name": "예리한 둔기", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [
                {"stage": 4, "featureType": 27, "values": [1000, 2000, 5700]}
            ]}]}
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();

        assert!((profiles.stat.sum(StatField::CritDmg) - 57.0).abs() < 1e-9);
        assert!((profiles.stat.random_damage_reduction_chance - 0.10).abs() < 1e-9);
        assert!((profiles.stat.random_damage_reduction - 0.20).abs() < 1e-9);
    }

    #[test]
    fn hit_master_excludes_awakening_skills() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["engravings"] = serde_json::json!([{
            "name": "타격의 대가", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [
                {"stage": 4, "featureType": 333, "values": [1925]}
            ]}]}
        }]);
        request["character"]["skills"].as_array_mut().unwrap().push(serde_json::json!({
            "name": "각성 테스트", "level": 1, "tripods": []
        }));
        let mut awakening = request["skillData"][0].clone();
        awakening["name"] = serde_json::json!("각성 테스트");
        awakening["skillSlot"] = serde_json::json!("Awakening");
        request["skillData"].as_array_mut().unwrap().push(awakening);

        let profiles = builder::build(&request.to_string()).unwrap();
        let normal = profiles.skills.iter().find(|skill| skill.name == "운명의 부름").unwrap();
        let awakening = profiles.skills.iter().find(|skill| skill.name == "각성 테스트").unwrap();

        assert!((normal.hits[0].damage_increase - 0.1925).abs() < 1e-9);
        assert_eq!(awakening.hits[0].damage_increase, 0.0);
    }

    #[test]
    fn directional_master_uses_shared_and_directional_raw_damage() {
        for (name, feature_type, attack_type, combat_effect_id) in [
            ("기습의 대가", 116, "백 어택", 20522688),
            ("결투의 대가", 325, "헤드 어택", 20522588),
        ] {
            let mut request: serde_json::Value =
                serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
            request["character"]["engravings"] = serde_json::json!([{
                "name": name, "grade": "유물", "level": 4,
                "featureData": {"grades": [{"grade": 5, "stages": [{
                    "stage": 4, "featureType": feature_type, "values": [1425],
                    "combatEffect": {
                        "id": combat_effect_id, "actionType": 1, "actionActor": 2,
                        "args": [1000, 0, 0, 0, 0, 0]
                    }
                }]}]}
            }]);
            request["skillData"][0]["properties"]["attackType"] =
                serde_json::json!(attack_type);

            let profiles = builder::build(&request.to_string()).unwrap();

            assert!((profiles.skills[0].hits[0].damage_increase - 0.3195875).abs() < 1e-9);
        }
    }

    #[test]
    fn stabilized_status_uses_full_health_raw_buff_condition() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["engravings"] = serde_json::json!([{
            "name": "안정된 상태", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "featureType": 5, "values": [521988],
                "combatEffect": {
                    "id": 20521988, "actionType": 1, "actionActor": 2,
                    "args": [1925, 0, 0, 0, 0, 0]
                },
                "skillBuff": {"visibleFilterType": 1, "visibleFilterValue": 65}
            }]}]}
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();

        assert!((profiles.skills[0].hits[0].damage_increase - 0.1925).abs() < 1e-9);
    }

    #[test]
    fn raid_captain_uses_move_speed_at_each_cast() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 운명의 부름\ncard_actions:\n  - card: 유령"
        );
        request["character"]["engravings"] = serde_json::json!([{
            "name": "돌격대장", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [
                {"stage": 4, "featureType": 121, "values": [5300]}
            ]}]}
        }]);
        request["cardData"] = serde_json::json!([{
            "skillId": 19090, "name": "유령", "autoLearn": true,
            "runtimeEffects": [{
                "kind": "self_move_speed_buff", "buffId": 190900,
                "durationMs": 16000, "moveSpeedPercent": 20
            }]
        }]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).unwrap();
        let trace = engine::trace_one(&profiles, 1, 3_000).1;

        assert!(trace.damage_events.len() >= 2);
        assert!((trace.damage_events[1].result_damage
            / trace.damage_events[0].result_damage
            - 1.106)
            .abs()
            < 1e-9);
    }

    #[test]
    fn adrenaline_stacks_on_cast_and_grants_crit_after_six_stacks() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["engravings"] = serde_json::json!([{
            "name": "아드레날린", "grade": "유물", "level": 4,
            "icon": "https://example.com/adrenaline.png",
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "featureType": 336, "values": [27, 521788],
                "skillBuffs": [{
                    "id": 521788, "duration": 6000, "overlap": 6,
                    "passiveOption": {"type": 2, "keyStat": 49, "value": 185}
                }, {
                    "id": 521888, "duration": -1,
                    "passiveOption": {"type": 2, "keyStat": 74, "value": 1250}
                }]
            }]}]}
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();
        let trace = engine::trace_one(&profiles, 1, 7_000).1;
        let seventh = &trace.damage_events[6];
        let attack_power_multiplier = seventh.formula.factors.iter()
            .find(|factor| factor.name == "attackPowerMultiplier")
            .unwrap().value;

        assert!((attack_power_multiplier - 1.111).abs() < 1e-9);
        assert!((trace.damage_events[4].formula.crit_rate - 0.0).abs() < 1e-9);
        assert!((trace.damage_events[5].formula.crit_rate - 0.125).abs() < 1e-9);
        assert!((seventh.formula.crit_rate - 0.125).abs() < 1e-9);
        let crit_factor = seventh.formula.factors.iter()
            .find(|factor| factor.name == "critRate")
            .expect("crit rate factor should be traced");
        assert!(crit_factor.sources.iter().any(|source|
            source.source.ends_with(":crit") && (source.value - 0.125).abs() < 1e-9
        ));
        let crit_buff = trace.casts.iter()
            .flat_map(|cast| &cast.active_buff_details)
            .find(|buff| buff.id.ends_with(":crit"))
            .expect("adrenaline crit buff should be traced");
        assert_eq!(crit_buff.name, "치명타 적중률 증가");
        assert_eq!(crit_buff.icon, "https://example.com/adrenaline.png");
        assert_eq!(crit_buff.source, "각인 · 아드레날린");
    }

    #[test]
    fn arcana_card_use_adds_an_adrenaline_stack() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 운명의 부름\ncard_actions:\n  - card: 심판"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19098, "name": "심판", "autoLearn": true
        }]);
        request["enableAwakeningPotion"] = serde_json::json!(true);
        request["character"]["engravings"] = serde_json::json!([{
            "name": "아드레날린", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "featureType": 336, "values": [27, 521788],
                "skillBuffs": [{
                    "id": 521788, "duration": 6000, "overlap": 6,
                    "passiveOption": {"type": 2, "keyStat": 49, "value": 185}
                }, {
                    "id": 521888, "duration": -1,
                    "passiveOption": {"type": 2, "keyStat": 74, "value": 1250}
                }]
            }]}]}
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();
        let trace = engine::trace_one(&profiles, 1, 1).1;
        let attack_power_multiplier = trace.damage_events[0].formula.factors.iter()
            .find(|factor| factor.name == "attackPowerMultiplier")
            .unwrap().value;

        assert_eq!(trace.card_uses.len(), 1);
        assert!((attack_power_multiplier - 1.037).abs() < 1e-9);
    }

    #[test]
    fn mana_efficiency_increase_applies_only_to_mana_skills() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["engravings"] = serde_json::json!([{
            "name": "마나 효율 증가", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "featureType": 279, "values": [10000002, 1850]
            }]}]}
        }]);
        request["skillData"][0]["resourceCost"] =
            serde_json::json!({"name": "Mp", "cost": [10.0]});
        request["character"]["skills"].as_array_mut().unwrap().push(serde_json::json!({
            "name": "무자원 스킬", "level": 1, "rune": null, "tripods": []
        }));
        let mut no_resource = request["skillData"][0].clone();
        no_resource["name"] = serde_json::json!("무자원 스킬");
        no_resource["resourceCost"] = serde_json::Value::Null;
        request["skillData"].as_array_mut().unwrap().push(no_resource);

        let profiles = builder::build(&request.to_string()).unwrap();
        let mana = profiles.skills.iter().find(|skill| skill.name == "운명의 부름").unwrap();
        let no_resource = profiles.skills.iter().find(|skill| skill.name == "무자원 스킬").unwrap();

        assert!((mana.hits[0].damage_increase - 0.185).abs() < 1e-9);
        assert_eq!(no_resource.hits[0].damage_increase, 0.0);
    }

    #[test]
    fn direct_max_mana_engraving_option_deserializes_without_a_feature_type() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["engravings"] = serde_json::json!([{
            "name": "최대 마나 증가", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "optionType": 2, "optionKeyStat": 32,
                "optionKeyIndex": 0, "optionValue": 3500
            }]}]}
        }]);

        builder::build(&request.to_string()).expect("direct engraving option should deserialize");
    }

    #[test]
    fn awakening_engraving_changes_only_regular_awakening_limits() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("프리즈매틱 미러")).unwrap();
        request["character"]["engravings"] = serde_json::json!([{
            "name": "각성", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "featureType": 122, "values": [5600, 4]
            }]}]}
        }]);
        request["skillData"][0]["skillSlot"] = serde_json::json!("Awakening");
        request["skillData"][0]["cooldown"] = serde_json::json!(1.0);
        request["skillData"][0]["maxUses"] = serde_json::json!(5);
        request["character"]["skills"].as_array_mut().unwrap().push(serde_json::json!({
            "name": "초각성기", "level": 1, "rune": null, "tripods": []
        }));
        let mut super_awakening = request["skillData"][0].clone();
        super_awakening["name"] = serde_json::json!("초각성기");
        super_awakening["skillSlot"] = serde_json::json!("SuperAwakening");
        request["skillData"].as_array_mut().unwrap().push(super_awakening);

        let profiles = builder::build(&request.to_string()).unwrap();
        let awakening = profiles.skills.iter().find(|skill| skill.name == "프리즈매틱 미러").unwrap();
        let super_awakening = profiles.skills.iter().find(|skill| skill.name == "초각성기").unwrap();

        assert_eq!((awakening.cooldown_ms, awakening.max_uses), (440, 9));
        assert_eq!((super_awakening.cooldown_ms, super_awakening.max_uses), (1000, 5));
    }

    #[test]
    fn mana_flow_reduces_cooldown_after_ten_skill_casts() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["skillData"][0]["cooldown"] = serde_json::json!(0.1);
        request["skillData"][0]["castTimesMs"] = serde_json::json!([100]);
        request["character"]["engravings"] = serde_json::json!([{
            "name": "마나의 흐름", "grade": "유물", "level": 4,
            "featureData": {"grades": [{"grade": 5, "stages": [{
                "stage": 4, "featureType": 118, "values": [27, 522788],
                "skillBuffs": [{
                    "id": 522788, "duration": 10000, "overlap": 10,
                    "passiveOption": {"type": 2, "keyStat": 39, "value": 320}
                }, {
                    "id": 522888, "duration": -1,
                    "passiveOption": {"type": 35, "keyIndex": 36, "value": 625}
                }]
            }]}]}
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();
        let trace = engine::trace_one(&profiles, 1, 1_200).1;

        assert_eq!(trace.casts[10].time, 1.0);
        assert_eq!(trace.casts[11].time, 1.093);
    }

    #[test]
    fn ark_passive_builds_emperor_and_queen_card_pool() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["arkPassive"] = serde_json::json!({
            "classNodes": [
                {"name": "황제의 칙령", "level": 1},
                {"name": "황제의 하사품", "level": 2},
                {"name": "황후의 기사", "level": 1}
            ]
        });
        request["cardData"] = serde_json::json!([
            {"skillId": 19099, "name": "균형", "autoLearn": true},
            {"skillId": 19098, "name": "심판", "autoLearn": true},
            {"skillId": 19282, "name": "황제", "autoLearn": false},
            {"skillId": 19286, "name": "재상", "autoLearn": false},
            {"skillId": 19287, "name": "제후", "autoLearn": false},
            {"skillId": 19288, "name": "황후의 기사", "autoLearn": false}
        ]);

        let profiles = builder::build(&request.to_string()).unwrap();

        assert_eq!(
            profiles.arcana_card_pool.iter().map(|card| card.skill_id).collect::<Vec<_>>(),
            vec![19282, 19286, 19287, 19288]
        );
    }

    #[test]
    fn royal_card_fills_both_card_slots() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("셀레스티얼 레인")).unwrap();
        request["cardData"] = serde_json::json!([{
            "skillId": 19096,
            "name": "로열",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "fill_card_slots",
                "effectId": 190960,
                "maxCardSlots": 2
            }]
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();

        assert!(matches!(
            profiles.arcana_card_effects.get(&19096),
            Some(TriggerEffect::Sequence(effects))
                if effects.len() == 2 && effects.iter().all(|effect| matches!(effect, TriggerEffect::DrawCard))
        ));
    }

    #[test]
    fn balance_card_adds_one_stack_to_stacked_skill_hits() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("셀레스티얼 레인")).unwrap();
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 균형"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19099,
            "name": "균형",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "stacked_skill_extra_target_stack_buff",
                "buffId": 190990,
                "durationMs": 30000,
                "targetBuffKey": "arcana_ruin_stack",
                "extraStacks": 1
            }]
        }]);
        request["skillData"][0]["identityCategory"] = serde_json::json!(29);
        request["skillData"][0]["hits"][0]["targetBuffStackGain"] = serde_json::json!({
            "buffId": "arcana_ruin_stack",
            "stacks": 1,
            "maxStacks": 4,
            "durationMs": 10000
        });
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).unwrap();
        let trace = engine::trace_one(&profiles, 1, 2000).1;

        assert_eq!(trace.casts[1].target_debuff_details[0].stacks, 3);
    }

    #[test]
    fn judgment_card_forces_four_stack_ruin_damage_without_target_stacks() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_celestial_with_runtime_damage_input_request(),
        ).unwrap();
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 심판"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19098,
            "name": "심판",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "force_ruin_stack_damage_buff",
                "buffId": 190980,
                "durationMs": 4000,
                "targetBuffKey": "arcana_ruin_stack",
                "maxStacks": 4
            }]
        }]);
        request["skillData"][0]["runtimeDamageByTargetBuffStacks"] = serde_json::json!([
            {"targetBuffId": "arcana_ruin_stack", "stack": 1, "damageRatio": [0.0], "fixedDamage": [100.0]},
            {"targetBuffId": "arcana_ruin_stack", "stack": 2, "damageRatio": [0.0], "fixedDamage": [200.0]},
            {"targetBuffId": "arcana_ruin_stack", "stack": 3, "damageRatio": [0.0], "fixedDamage": [300.0]},
            {"targetBuffId": "arcana_ruin_stack", "stack": 4, "name": "루인 피해 (4스택)", "damageRatio": [0.0], "fixedDamage": [400.0]}
        ]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).unwrap();
        let trace = engine::trace_one(&profiles, 1, 1000).1;
        let ruin = trace.damage_events.iter().find(|event| {
            event.time == 1.0 && event.damage_source == "target_buff_stacks:arcana_ruin_stack:4"
        }).unwrap();

        assert_eq!(ruin.hit_name, "루인 피해 (4스택)");
        assert_eq!(ruin.result_damage, 400.0);
    }

    #[test]
    fn prince_card_increases_normal_skill_damage() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 운명의 부름\ncard_actions:\n  - card: 제후"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19287,
            "name": "제후",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "normal_skill_damage_buff",
                "buffId": 192834,
                "durationMs": 4000,
                "valuePercent": 50
            }]
        }]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).unwrap();
        let trace = engine::trace_one(&profiles, 1, 1000).1;

        assert_eq!(trace.card_uses[0].card_name, "제후");
        assert!((trace.damage_events[1].result_damage
            / trace.damage_events[0].result_damage
            - 1.5)
            .abs()
            < 1e-9);
    }

    #[test]
    fn chancellor_card_increases_normal_skill_crit_rate() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 운명의 부름\ncard_actions:\n  - card: 재상"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19286,
            "name": "재상",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "normal_skill_crit_rate_buff",
                "buffId": 192833,
                "durationMs": 12000,
                "critRatePercent": 100
            }]
        }]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).unwrap();
        let trace = engine::trace_one(&profiles, 1, 1000).1;

        assert!(!trace.damage_events[0].is_crit);
        assert!(trace.damage_events[1].is_crit);
        assert_eq!(trace.damage_events[1].result_damage, trace.damage_events[0].result_damage * 2.0);
    }

    #[test]
    fn wheel_card_resets_the_next_skill_cooldown_once() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        let mut target = request["skillData"][0].clone();
        request["enableAwakeningPotion"] = serde_json::json!(true);
        request["skillData"][0]["cooldown"] = serde_json::json!(10.0);
        request["skillData"][0]["castTimesMs"] = serde_json::json!([100]);
        target["name"] = serde_json::json!("리턴");
        target["cooldown"] = serde_json::json!(10.0);
        target["castTimesMs"] = serde_json::json!([100]);
        request["skillData"]
            .as_array_mut()
            .unwrap()
            .push(target);
        request["character"]["skills"] = serde_json::json!([
            {"name": "운명의 부름", "level": 1, "rune": null, "tripods": []},
            {"name": "리턴", "level": 1, "rune": null, "tripods": []}
        ]);
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 운명의 부름\n  - 리턴\ncard_actions:\n  - card: 운명의 수레바퀴\n    conditions:\n      - type: next_skill\n        skill: 리턴"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19095,
            "name": "운명의 수레바퀴",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "next_skill_cooldown_reset_buff",
                "buffId": 190950,
                "durationType": "until_skill_use"
            }]
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();
        let trace = engine::trace_one(&profiles, 1, 250).1;
        let casts: Vec<_> = trace
            .casts
            .iter()
            .map(|cast| (cast.skill_name.as_str(), cast.time))
            .collect();

        assert_eq!(casts, vec![("운명의 부름", 0.0), ("리턴", 0.1), ("리턴", 0.2)]);
        assert_eq!(trace.card_uses.len(), 1);
        assert_eq!(trace.card_uses[0].time, 0.0);
    }

    #[test]
    fn emperor_card_deals_raw_direct_damage() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 운명의 부름\ncard_actions:\n  - card: 황제"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19282,
            "name": "황제",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "direct_damage",
                "effectId": 192824,
                "hitId": "emperor_card_damage",
                "name": "황제 카드 피해",
                "damageRatio": 72.395,
                "fixedDamage": 7791
            }]
        }]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).unwrap();
        let trace = engine::trace_one(&profiles, 1, 1000).1;
        let damage = trace
            .damage_events
            .iter()
            .find(|event| event.damage_source == "card:19282")
            .unwrap();

        assert_eq!(damage.hit_id, "emperor_card_damage");
        assert_eq!(damage.hit_name, "황제 카드 피해");
        assert_eq!(damage.result_damage, 7791.0);
    }

    #[test]
    fn queens_knight_card_deals_damage_then_draws_one_card() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["cardData"] = serde_json::json!([{
            "skillId": 19288,
            "name": "황후의 기사",
            "autoLearn": false,
            "runtimeEffects": [{
                "kind": "direct_damage",
                "effectId": 192840,
                "hitId": "queens_knight_card_damage",
                "name": "황후의 기사 카드 피해",
                "damageRatio": 120.65,
                "fixedDamage": 12984.5,
                "drawCardCount": 1
            }]
        }]);

        let profiles = builder::build(&request.to_string()).unwrap();

        assert!(matches!(
            profiles.arcana_card_effects.get(&19288),
            Some(TriggerEffect::Sequence(effects))
                if matches!(effects.as_slice(), [
                    TriggerEffect::DealRuntimeDamage(hit),
                    TriggerEffect::DrawCard,
                ] if hit.hit_id == "queens_knight_card_damage"
                    && hit.name == "황후의 기사 카드 피해"
                    && hit.skill_modifier == 120.65
                    && hit.base_damage == 12984.5)
        ));
    }

    fn arcana_checkmate_request(card_increase: bool) -> String {
        let tripods = if card_increase {
            serde_json::json!([{"name": "카드 증가", "values": []}])
        } else {
            serde_json::json!([])
        };
        let mut hits = Vec::new();
        for effect_id in [190401, 190411, 190413, 190415] {
            for _ in 0..3 {
                let max_stacks = if card_increase && effect_id == 190415 { 4 } else { 3 };
                hits.push(serde_json::json!({
                    "effectId": effect_id,
                    "damageRatio": [1.0],
                    "fixedDamage": [10.0],
                    "phase": 0,
                    "critChanceBonus": 0.4,
                    "damageIncrease": 0.8,
                    "targetBuffStackGain": {
                        "buffId": "arcana_ruin_stack",
                        "stacks": 1,
                        "maxStacks": max_stacks,
                        "durationMs": 10000
                    },
                    "identityGain": null
                }));
            }
        }
        hits.push(serde_json::json!({
            "effectId": 190409,
            "damageRatio": [1.0],
            "fixedDamage": [10.0],
            "phase": 0,
            "identityGain": null
        }));

        serde_json::json!({
            "character": {
                "name": "test_arcana",
                "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [{
                    "name": "체크메이트",
                    "level": 1,
                    "rune": null,
                    "tripods": tripods
                }],
                "equipments": [],
                "accessories": [],
                "bracelet": null,
                "stone": null,
                "gems": [],
                "engravings": [],
                "arkPassive": null,
                "arkGrids": null,
                "arkPassiveKarma": null
            },
            "aplConfig": "",
            "skillData": [{
                "name": "체크메이트",
                "identityCategory": 29,
                "skillControlType": "Holding",
                "skillSlot": "Normal",
                "cooldown": 24.0,
                "castTimesMs": [6000],
                "hits": hits,
                "resourceCost": null,
                "identityGain": null,
                "properties": {"attackType": "", "element": ""}
            }]
        }).to_string()
    }

    fn arcana_celestial_with_runtime_damage_input_request() -> String {
        r#"{
            "character": {
                "name": "test_arcana",
                "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [
                    {"name": "셀레스티얼 레인", "level": 1, "rune": null, "tripods": []}
                ],
                "equipments": [],
                "accessories": [],
                "bracelet": null,
                "stone": null,
                "gems": [],
                "engravings": [],
                "arkPassive": null,
                "arkGrids": null,
                "arkPassiveKarma": null
            },
            "aplConfig": "",
            "skillData": [
                {
                    "name": "셀레스티얼 레인",
                    "identityCategory": 30,
                    "skillControlType": "Normal",
                    "skillSlot": "Normal",
                    "cooldown": 1.0,
                    "castTimesMs": [1000],
                    "hits": [
                        {"effectId": 191401, "damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "identityGain": null}
                    ],
                    "ruinTriggerEffectIds": [191401],
                    "runtimeDamageByTargetBuffStacks": [
                        {
                            "targetBuffId": "arcana_ruin_stack",
                            "stack": 1,
                            "damageRatio": [0.01],
                            "fixedDamage": [111.0]
                        },
                        {
                            "targetBuffId": "arcana_ruin_stack",
                            "stack": 2,
                            "damageRatio": [0.02],
                            "fixedDamage": [222.0]
                        }
                    ],
                    "resourceCost": null,
                    "identityGain": null,
                    "properties": {"attackType": "", "element": ""}
                }
            ]
        }"#.to_string()
    }

    #[test]
    fn arcana_empress_greed_reaches_ruin_skill_damage() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            r#"{
                        "commonNodes": [],
                        "classNodes": [
                            {"name": "황후의 탐욕", "level": 5, "values": [4.5, 20.0, 50.0]}
                        ]
                    }"#,
            "null",
        )).unwrap();
        request["skillData"][0]["identityGain"] =
            serde_json::json!({"name": "CardGauge", "value": 10.0});
        request["skillData"][0]["hits"][0]["identityGain"] =
            serde_json::json!({"name": "CardGauge", "value": 5.0});
        let mut normal = request["skillData"][0].clone();
        normal["name"] = serde_json::json!("운명의 부름");
        normal["gameSkillId"] = serde_json::json!(19180);
        normal["identityCategory"] = serde_json::json!(28);
        normal["properties"]["skillGroupIds"] = serde_json::json!([2190401]);
        request["skillData"].as_array_mut().unwrap().push(normal);
        request["character"]["skills"].as_array_mut().unwrap().push(serde_json::json!({
            "name": "운명의 부름", "level": 1, "rune": null, "tripods": []
        }));

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let celestial = profiles
            .skills
            .iter()
            .find(|skill| skill.name == "셀레스티얼 레인")
            .expect("Celestial Rain profile should exist");

        assert!((celestial.hits[0].damage_increase - 0.045).abs() < 1e-9);
        assert!((celestial.resource_gains["CardGauge"] - 12.0).abs() < 1e-9);
        assert!(celestial.hits[0].triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::GainResource { resource, amount }
                if resource == "CardGauge" && (*amount - 6.0).abs() < 1e-9
        )));
        let call = profiles.skills.iter().find(|skill| skill.name == "운명의 부름").unwrap();
        assert!((call.resource_gains["CardGauge"] - 5.0).abs() < 1e-9);
        assert!(call.hits[0].triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::GainResource { resource, amount }
                if resource == "CardGauge" && (*amount - 2.5).abs() < 1e-9
        )));
    }

    #[test]
    fn arcana_ark_grid_ruin_core_reaches_ruin_skill_damage() {
        let json = base_arcana_request(
            "[]",
            "null",
            r#"{
                        "gems": [],
                        "commonCores": [],
                        "classCores": [
                            {
                                "name": "루인 서브셋",
                                "points": 18,
                                "options": [
                                    {
                                        "pointRequirement": 10,
                                        "description": "루인 스킬의 피해량이 증가한다.",
                                        "rawOptionId": 3191000,
                                        "runtimeEffects": [
                                            {
                                                "kind": "static_skill_group_damage",
                                                "skillGroupId": 2190904,
                                                "valuePercent": 2.8
                                            }
                                        ]
                                    }
                                ]
                            }
                        ]
                    }"#,
        );

        let profiles = builder::build(&json).expect("build should succeed");
        let celestial = profiles
            .skills
            .iter()
            .find(|skill| skill.name == "셀레스티얼 레인")
            .expect("Celestial Rain profile should exist");

        assert!((celestial.hits[0].damage_increase - 0.028).abs() < 1e-9);
    }

    #[test]
    fn arcana_ark_grid_card_options_modify_the_exact_cards() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["arkGrids"] = serde_json::json!({
            "gems": [], "commonCores": [], "classCores": [{
                "name": "카드 옵션", "points": 17, "options": [
                    {"pointRequirement": 10, "rawOptionId": 3196500, "runtimeEffects": [
                        {"kind": "twisted_fate_override", "cardId": 19091, "durationMs": 6000}]},
                    {"pointRequirement": 14, "rawOptionId": 3196600, "runtimeEffects": [
                        {"kind": "card_crit_damage_bonus", "cardId": 19281, "valuePercent": 20}]},
                    {"pointRequirement": 17, "rawOptionId": 3196700, "runtimeEffects": [
                        {"kind": "card_target_damage_override", "cardId": 19093, "targetBuffId": "190931", "valuePercent": 12},
                        {"kind": "card_damage_multiplier", "cardId": 19282, "valuePercent": 15}]}
                ]
            }]
        });
        request["cardData"] = serde_json::json!([
            {"skillId": 19091, "name": "뒤틀린 운명", "autoLearn": true, "runtimeEffects": [
                {"kind": "random_self_damage_buff", "uniqueGroupId": 190900, "durationMs": 4000,
                 "resultBuffIds": [190913, 190914, 190915, 190916], "damagePercents": [0, 10, 20, 40]}]},
            {"skillId": 19281, "name": "도태", "autoLearn": true, "runtimeEffects": [
                {"kind": "self_stat_buff", "buffId": 192810, "durationMs": 4000,
                 "critRatePercent": 100, "critDamagePercent": 50}]},
            {"skillId": 19093, "name": "부식", "autoLearn": true, "runtimeEffects": [
                {"kind": "self_on_hit_target_damage_buff", "buffId": 190930, "durationMs": 30000,
                 "chancePercent": 30, "targetBuffId": 190931, "targetDurationMs": 5000, "valuePercent": 10}]},
            {"skillId": 19282, "name": "황제", "autoLearn": false, "runtimeEffects": [
                {"kind": "direct_damage", "hitId": "emperor", "name": "황제", "damageRatio": 10, "fixedDamage": 100}]}
        ]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert!(matches!(&profiles.arcana_card_effects[&19091],
            TriggerEffect::WeightedRandomChoice(effects)
                if effects.iter().map(|(weight, _)| *weight).collect::<Vec<_>>() == vec![1.02, 32.98, 33.0, 33.0]
                    && effects.iter().all(|(_, effect)| matches!(effect, TriggerEffect::ApplyBuff(buff) if buff.duration_ms == 6000))));
        assert!(matches!(&profiles.arcana_card_effects[&19281],
            TriggerEffect::Sequence(effects)
                if matches!(&effects[0], TriggerEffect::ApplyBuff(buff) if buff.effects[&StatField::CritDmg] == 70.0)));
        assert!(matches!(&profiles.arcana_card_effects[&19282],
            TriggerEffect::Sequence(effects)
                if matches!(&effects[0], TriggerEffect::DealRuntimeDamage(hit)
                    if (hit.base_damage - 115.0).abs() < 1e-9 && (hit.skill_modifier - 11.5).abs() < 1e-9)));
        assert!(profiles.global_triggers.iter().any(|trigger| matches!(&trigger.effect,
            TriggerEffect::Chance { effect, .. }
                if matches!(effect.as_ref(), TriggerEffect::ApplyTargetBuff(buff)
                    if buff.id == "190931" && buff.effects[&StatField::TargetDamageIncrease] == 0.12))));
    }

    #[test]
    fn arcana_ark_grid_card_and_stream_triggers_use_raw_groups_and_timing() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["skillData"][0]["gameSkillId"] = serde_json::json!(19150);
        request["skillData"][0]["properties"]["skillGroupIds"] = serde_json::json!([2190902]);
        request["character"]["arkGrids"] = serde_json::json!({
            "gems": [], "commonCores": [], "classCores": [{
                "name": "트리거", "points": 17, "options": [{
                    "pointRequirement": 17, "rawOptionId": 1, "runtimeEffects": [
                        {"kind": "on_card_use_delayed_skill_group_cooldown_reduction", "cardId": 19098,
                         "skillGroupId": 2190902, "delayMs": 6000, "cooldownReductionPercent": 15},
                        {"kind": "on_card_use_skill_group_buff", "cardId": 19099,
                         "skillGroupId": 2190902, "buffId": 3190700, "durationMs": 30000, "valuePercent": 2},
                        {"kind": "on_skill_cast_skill_group_buff", "skillId": 19150,
                         "skillGroupId": 2190902, "buffId": 3193200, "durationMs": 8000, "valuePercent": 5}
                    ]
                }]
            }]
        });
        request["cardData"] = serde_json::json!([
            {"skillId": 19098, "name": "심판", "autoLearn": true, "runtimeEffects": []},
            {"skillId": 19099, "name": "균형", "autoLearn": true, "runtimeEffects": []}
        ]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert!(profiles.skills[0].cast_buff_damage_bonuses.contains(&("3190700".to_string(), 0.02)));
        assert!(profiles.skills[0].cast_buff_damage_bonuses.contains(&("3193200".to_string(), 0.05)));
        assert!(matches!(&profiles.arcana_card_effects[&19098],
            TriggerEffect::Sequence(effects)
                if matches!(&effects[0], TriggerEffect::Schedule { delay_ms: 6000, effect }
                    if matches!(effect.as_ref(), TriggerEffect::ReduceCooldowns { percent, .. } if (*percent - 0.15).abs() < 1e-9))));
        assert!(profiles.global_triggers.iter().any(|trigger| matches!(
            (&trigger.filter, &trigger.effect),
            (TriggerFilter::SkillCastId(0), TriggerEffect::ApplyBuff(buff))
                if buff.id == "3193200" && buff.duration_ms == 8000)));
    }

    #[test]
    fn arcana_ark_passive_and_grid_damage_combine_multiplicatively() {
        let json = base_arcana_request(
            "[]",
            r#"{
                "commonNodes": [],
                "classNodes": [{"name": "황후의 탐욕", "level": 1, "values": [10.0]}]
            }"#,
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [{
                    "name": "루인 배율", "points": 10,
                    "options": [{
                        "pointRequirement": 10, "rawOptionId": 1,
                        "runtimeEffects": [{
                            "kind": "static_identity_category_damage",
                            "identityCategory": 30,
                            "valuePercent": 20.0
                        }]
                    }]
                }]
            }"#,
        );

        let profiles = builder::build(&json).expect("build should succeed");
        assert!((profiles.skills[0].hits[0].damage_increase - 0.32).abs() < 1e-9);
    }

    #[test]
    fn equipment_uses_the_exact_raw_item_level_row() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["equipments"] = serde_json::json!([
            {"type": "무기", "setName": "test weapon", "quality": 0,
             "itemLevel": 1680, "refinementLevel": 1, "advancedRefinementLevel": 0},
            {"type": "투구", "setName": "test armor", "quality": 0,
             "itemLevel": 1680, "refinementLevel": 1, "advancedRefinementLevel": 0}
        ]);
        request["equipmentTables"] = serde_json::json!({
            "test weapon": {"무기": [
                {"itemLevel": 1675, "weaponAttack": 124793},
                {"itemLevel": 1680, "weaponAttack": 128059}
            ]},
            "test armor": {"투구": [
                {"itemLevel": 1680, "mainStat": 30000, "vitality": 40000}
            ]}
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(profiles.stat.sum(StatField::WeaponAttackPower), 128059.0);
        assert_eq!(profiles.stat.base_main_stat, 30000.0);
        assert_eq!(profiles.stat.base_vitality, 40000.0);
    }

    #[test]
    fn esther_weapon_uses_only_its_raw_stage_stats() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["equipments"] = serde_json::json!([{
            "type": "무기", "setName": "세피로트 넬라", "quality": 0,
            "itemLevel": 1715, "refinementLevel": 8, "advancedRefinementLevel": 0
        }]);
        request["equipmentTables"] = serde_json::json!({
            "세피로트 넬라": {"무기": [{
                "itemLevel": 1715, "refinementLevel": 8, "weaponAttack": 147585
            }]}
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(profiles.stat.sum(StatField::WeaponAttackPower), 147585.0);
    }

    #[test]
    fn equipment_ignores_api_stats_and_rebuilds_advanced_honing_from_raw() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["equipments"] = serde_json::json!([
            {"type": "무기", "setName": "test weapon", "quality": 100,
             "itemLevel": 1680, "refinementLevel": 1, "advancedRefinementLevel": 40,
             "weaponAttack": 150000, "additionalDamage": 28.5},
            {"type": "투구", "setName": "test armor", "quality": 0,
             "itemLevel": 1680, "refinementLevel": 1, "advancedRefinementLevel": 40,
             "mainStat": 35000, "vitality": 45000},
            {"type": "완갑", "setName": "test bracer", "quality": -1,
             "itemLevel": 0, "refinementLevel": 25, "advancedRefinementLevel": 0,
             "weaponAttack": 25000, "mainStat": 75000, "vitality": 6500,
             "baseAttackPowerPercent": 3.5}
        ]);
        request["equipmentTables"] = serde_json::json!({
            "test weapon": {"무기": [
                {"itemLevel": 1680, "weaponAttack": 128059}
            ]},
            "test armor": {"투구": [
                {"itemLevel": 1680, "mainStat": 73754, "vitality": 12858}
            ]},
            "test bracer": {"완갑": [
                {"refinementLevel": 25, "weaponAttack": 22940,
                 "mainStat": 73710, "vitality": 6414, "baseAttackPowerPercent": 3}
            ]}
        });
        request["advancedHoningTables"] = serde_json::json!({
            "무기": [
                {"stage": 30, "stageBonusStatRate": 200},
                {"stage": 40, "stageBonusStatRate": 300}
            ],
            "투구": [
                {"stage": 30, "stageBonusStatRate": 200},
                {"stage": 40, "stageBonusStatRate": 300}
            ]
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(profiles.stat.sum(StatField::WeaponAttackPower), 157401.0);
        assert_eq!(profiles.stat.base_main_stat, 151151.0);
        assert_eq!(profiles.stat.base_vitality, 19914.0);
        assert_eq!(profiles.stat.sum(StatField::AdditionalDamage), 0.3);
        assert!((profiles.stat.base_ap_mul - 1.03).abs() < 1e-9);
    }

    #[test]
    fn bracer_uses_refinement_stats_and_base_attack_power_bonus() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["equipments"] = serde_json::json!([{
            "type": "완갑", "setName": "운명의 전율 완갑", "quality": 100,
            "itemLevel": 0, "refinementLevel": 25, "advancedRefinementLevel": 0
        }]);
        request["equipmentTables"] = serde_json::json!({
            "운명의 전율 완갑": {"완갑": [{
                "refinementLevel": 25,
                "weaponAttack": 22940,
                "mainStat": 73710,
                "vitality": 6414,
                "baseAttackPowerPercent": 3
            }]}
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(profiles.stat.sum(StatField::WeaponAttackPower), 22940.0);
        assert_eq!(profiles.stat.base_main_stat, 73710.0);
        assert_eq!(profiles.stat.base_vitality, 6414.0);
        assert!((profiles.stat.base_ap_mul - 1.03).abs() < 1e-9);
    }

    #[test]
    fn accessory_attack_stats_follow_the_final_attack_power_formula() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["equipments"] = serde_json::json!([
            {"type": "무기", "setName": "test weapon", "quality": 0,
             "itemLevel": 1680, "refinementLevel": 1, "advancedRefinementLevel": 0},
            {"type": "투구", "setName": "test armor", "quality": 0,
             "itemLevel": 1680, "refinementLevel": 1, "advancedRefinementLevel": 0}
        ]);
        request["character"]["accessories"] = serde_json::json!([{
            "baseStats": {"mainStat": 12000, "vitality": 0},
            "polishingEffects": [
                {"type": "공격력", "value": 390, "isPercentage": false},
                {"type": "무기 공격력", "value": 960, "isPercentage": false},
                {"type": "공격력", "value": 1.55, "isPercentage": true},
                {"type": "무기 공격력", "value": 3.0, "isPercentage": true}
            ]
        }]);
        request["equipmentTables"] = serde_json::json!({
            "test weapon": {"무기": [
                {"itemLevel": 1680, "weaponAttack": 128059}
            ]},
            "test armor": {"투구": [
                {"itemLevel": 1680, "mainStat": 30000, "vitality": 40000}
            ]}
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (_result, trace) = engine::trace_one(&profiles, 1, 1001);
        let expected_weapon_ap = (128059.0 + 960.0) * 1.03;
        let expected_attack_power =
            (((30000.0_f64 + 12000.0) * expected_weapon_ap / 6.0).sqrt() + 390.0)
                * 1.0155;

        assert!((trace.damage_events[0].formula.attack_power - expected_attack_power).abs() < 1e-9);
    }

    #[test]
    fn avatar_uses_the_exact_total_main_stat_percentage() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["avatarMainStatPercent"] = serde_json::json!(5.5);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");

        assert_eq!(profiles.stat.main_stat_mul, 1.055);
    }

    #[test]
    fn bracelet_stagger_damage_uses_both_raw_modifiers() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["bracelet"] = serde_json::json!({
            "lockedEffects": [{
                "name": "피해 증가 및 무력화 시 피해 증가",
                "values": [3.0, 5.0],
                "descriptions": []
            }],
            "changeableEffects": []
        });

        let normal = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(normal.stat.sum(StatField::DamageIncrease), 0.03);

        request["assumedTargetStaggered"] = serde_json::json!(true);
        let staggered = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(staggered.stat.sum(StatField::DamageIncrease), 0.08);
    }

    #[test]
    fn bracelet_non_directional_damage_excludes_awakening_skills() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["bracelet"] = serde_json::json!({
            "lockedEffects": [{
                "name": "비방향성 공격 스킬 피해 증가",
                "values": [3.5],
                "descriptions": []
            }],
            "changeableEffects": []
        });
        request["character"]["skills"].as_array_mut().unwrap().push(serde_json::json!({
            "name": "각성 테스트", "level": 1, "tripods": []
        }));
        let mut awakening = request["skillData"][0].clone();
        awakening["name"] = serde_json::json!("각성 테스트");
        awakening["skillSlot"] = serde_json::json!("Awakening");
        request["skillData"].as_array_mut().unwrap().push(awakening);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let normal = profiles.skills.iter().find(|skill| skill.name == "운명의 부름").unwrap();
        let awakening = profiles.skills.iter().find(|skill| skill.name == "각성 테스트").unwrap();

        assert!((normal.hits[0].damage_increase - 0.035).abs() < 1e-9);
        assert_eq!(awakening.hits[0].damage_increase, 0.0);
    }

    #[test]
    fn bracelet_demon_damage_requires_the_matching_target_type() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["bracelet"] = serde_json::json!({
            "lockedEffects": [{
                "name": "추가 피해 및 악마/대악마 피해 증가",
                "values": [3.5, 2.5],
                "descriptions": []
            }],
            "changeableEffects": []
        });

        let generic = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(generic.stat.sum(StatField::AdditionalDamage), 0.035);
        assert_eq!(generic.stat.sum(StatField::TypeDamage), 0.0);

        request["targetMonsterType"] = serde_json::json!("악마");
        let demon = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(demon.stat.sum(StatField::AdditionalDamage), 0.035);
        assert_eq!(demon.stat.sum(StatField::TypeDamage), 0.025);
    }

    #[test]
    fn bracelet_hit_stack_includes_both_raw_speed_stats() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["bracelet"] = serde_json::json!({
            "lockedEffects": [{
                "name": "공격 적중 시 무기 공격력 및 공이속 증가 (중첩)",
                "values": [1480],
                "descriptions": []
            }],
            "changeableEffects": []
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trigger = profiles.global_triggers.iter().find(|trigger| {
            matches!(trigger.filter, TriggerFilter::AnyHit)
                && trigger.cooldown_ms == Some(1_000)
        }).expect("bracelet should trigger on hit");
        let TriggerEffect::ApplyBuff(buff) = &trigger.effect else {
            panic!("bracelet should apply its stacking buff")
        };
        assert_eq!(buff.duration_ms, 10_000);
        assert_eq!(buff.max_stacks, 6);
        assert_eq!(buff.effects[&StatField::WeaponAttackPower], 1480.0);
        assert_eq!(buff.effects[&StatField::AttackSpeed], 0.01);
        assert_eq!(buff.effects[&StatField::MovementSpeed], 0.01);
    }

    #[test]
    fn bracelet_hp_condition_uses_the_existing_low_hp_assumption() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["bracelet"] = serde_json::json!({
            "lockedEffects": [{
                "name": "무기 공격력 증가 (체력 조건)",
                "values": [9000, 2400],
                "descriptions": []
            }],
            "changeableEffects": []
        });

        let full_hp = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(full_hp.stat.sum(StatField::WeaponAttackPower), 9000.0);
        assert!(full_hp.global_triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::ApplyBuff(buff)
                if buff.duration_ms == 5_000
                    && buff.effects[&StatField::WeaponAttackPower] == 2400.0
        )));

        request["assumedLowHp"] = serde_json::json!(true);
        let low_hp = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(low_hp.stat.sum(StatField::WeaponAttackPower), 9000.0);
        assert!(low_hp.global_triggers.is_empty());
    }

    #[test]
    fn bracelet_static_speed_applies_to_both_attack_and_movement() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["bracelet"] = serde_json::json!({
            "lockedEffects": [{
                "name": "공격 및 이동 속도 증가",
                "values": [6],
                "descriptions": []
            }],
            "changeableEffects": []
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(profiles.stat.sum(StatField::AttackSpeed), 0.06);
        assert_eq!(profiles.stat.sum(StatField::MovementSpeed), 0.06);
    }

    #[test]
    fn feast_uses_raw_skill_buff_100410424_stats() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();

        let disabled = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(disabled.stat.sum(StatField::WeaponAttackPower), 0.0);
        assert_eq!(disabled.stat.sum(StatField::AttackSpeed), 0.0);
        assert_eq!(disabled.stat.sum(StatField::MovementSpeed), 0.0);

        request["enableFeast"] = serde_json::json!(true);
        let enabled = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(enabled.stat.sum(StatField::WeaponAttackPower), 1800.0);
        assert_eq!(enabled.stat.sum(StatField::AttackSpeed), 0.05);
        assert_eq!(enabled.stat.sum(StatField::MovementSpeed), 0.05);
    }

    #[test]
    fn karma_uses_raw_stigma_stats_and_targets_only_super_awakening() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["arkPassiveKarma"] = serde_json::json!({
            "evolution": {"rank": 6, "level": 30},
            "enlightenment": {"rank": 6, "level": 30},
            "leap": {"rank": 6, "level": 30}
        });
        request["skillData"][0]["skillSlot"] =
            serde_json::json!("HyperAwakeningTechniques");

        let technique = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(technique.stat.base_vitality, 0.0);
        assert_eq!(technique.stat.sum(StatField::EvolutionDamage), 0.06);
        assert!((technique.stat.weapon_ap_mul - 1.03).abs() < 1e-9);
        assert_eq!(technique.skills[0].hits[0].damage_increase, 0.0);

        request["skillData"][0]["skillSlot"] = serde_json::json!("SuperAwakening");
        let super_awakening =
            builder::build(&request.to_string()).expect("build should succeed");
        assert!(super_awakening.skills[0]
            .tags
            .contains(&SkillTag::CategorySuperAwakening));
        assert!((super_awakening.skills[0].hits[0].damage_increase - 0.15).abs() < 1e-9);
    }

    #[test]
    fn bracelet_critical_options_keep_the_raw_on_crit_multiplier() {
        for (name, value, stat_field) in [
            ("치명타 적중률 및 적중 시 피해 증가", 5.0, StatField::CritRate),
            ("치명타 피해 및 적중 시 피해 증가", 10.0, StatField::CritDmg),
        ] {
            let mut request: serde_json::Value =
                serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
            request["character"]["bracelet"] = serde_json::json!({
                "lockedEffects": [{
                    "name": name,
                    "values": [value, 1.5],
                    "descriptions": []
                }],
                "changeableEffects": []
            });

            let profiles = builder::build(&request.to_string()).expect("build should succeed");
            assert_eq!(profiles.stat.sum(stat_field), value);
            assert_eq!(profiles.stat.on_crit_damage_bonus, 0.015);
        }
    }

    #[test]
    fn bracelet_long_weapon_power_stack_keeps_raw_timing_and_values() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["bracelet"] = serde_json::json!({
            "lockedEffects": [{
                "name": "무기 공격력 증가 (중첩)",
                "values": [8700, 150],
                "descriptions": []
            }],
            "changeableEffects": []
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(profiles.stat.sum(StatField::WeaponAttackPower), 8700.0);
        let trigger = profiles.global_triggers.iter().find(|trigger| {
            matches!(trigger.filter, TriggerFilter::AnyHit)
                && trigger.cooldown_ms == Some(30_000)
        }).expect("bracelet should trigger every thirty seconds");
        let TriggerEffect::ApplyBuff(buff) = &trigger.effect else {
            panic!("bracelet should apply its stacking buff")
        };
        assert_eq!(buff.duration_ms, 120_000);
        assert_eq!(buff.max_stacks, 30);
        assert_eq!(buff.effects[&StatField::WeaponAttackPower], 150.0);
    }

    #[test]
    fn bracelet_damage_tradeoff_keeps_both_damage_and_cooldown() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["bracelet"] = serde_json::json!({
            "lockedEffects": [{
                "name": "피해 증가 (쿨타임 패널티)",
                "values": [5.5],
                "descriptions": []
            }],
            "changeableEffects": []
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(profiles.stat.sum(StatField::DamageIncrease), 0.055);
        assert_eq!(profiles.stat.cooldown_penalty, 0.02);
    }

    #[test]
    fn chaos_core_applies_only_reached_static_options() {
        let json = base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [],
                "classCores": [],
                "commonCores": [{
                    "name": "흐르는 마나",
                    "points": 17,
                    "options": [
                        {"pointRequirement": 10, "simEffects": [{"kind": "flat", "field": "max_mana", "value": 80}]},
                        {"pointRequirement": 17, "simEffects": [{"kind": "flat", "field": "max_mana", "value": 240}]},
                        {"pointRequirement": 18, "simEffects": [{"kind": "flat", "field": "max_mana", "value": 999}]}
                    ]
                }]
            }"#,
        );

        let profiles = builder::build(&json).expect("build should succeed");
        assert_eq!(profiles.max_mp, 320.0);
    }

    #[test]
    fn rune_direct_effects_use_exact_skill_scope() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]", "null", "null",
        ))
        .expect("request should be valid JSON");
        request["character"]["skills"][0]["rune"] = serde_json::json!({
            "name": "raw rune",
            "grade": "legendary",
            "effects": [
                {"kind": "cast_speed", "value": 14},
                {"kind": "mana_cost_reduction", "value": 40},
                {"kind": "identity_hit_gain", "value": 40}
            ]
        });
        request["skillData"][0]["resourceCost"] =
            serde_json::json!({"name": "Mp", "cost": [100.0]});
        request["skillData"][0]["identityGain"] =
            serde_json::json!({"name": "CardGauge", "value": 10.0});
        request["skillData"][0]["hits"][0]["identityGain"] =
            serde_json::json!({"name": "CardGauge", "value": 5.0});

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let skill = profiles
            .skills
            .iter()
            .find(|skill| skill.name == "셀레스티얼 레인")
            .unwrap();

        assert_eq!(skill.cast_times_ms, vec![860]);
        assert_eq!(skill.resource_costs["Mp"], 60.0);
        assert_eq!(skill.resource_gains["CardGauge"], 10.0);
        assert!(skill.hits[0].triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::GainResource { resource, amount }
                if resource == "CardGauge" && (*amount - 7.0).abs() < 1e-9
        )));
    }

    #[test]
    fn rune_proc_effects_build_exact_cast_and_hit_triggers() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]", "null", "null",
        ))
        .expect("request should be valid JSON");
        request["character"]["skills"][0]["rune"] = serde_json::json!({
            "name": "raw rune",
            "grade": "legendary",
            "effects": [
                {"kind": "on_cast_cooldown_reduction", "chance": 1.0, "value": 16},
                {"kind": "on_cast_attack_speed_buff", "chance": 1.0, "buffId": "70000100", "durationMs": 6000, "value": 16},
                {"kind": "on_hit_buff", "chance": 1.0, "buffId": "70000060", "durationMs": 3000},
                {"kind": "on_hit_judgment", "chance": 1.0, "buffId": "70000062", "consumesBuffId": "70000060", "durationMs": 6000, "value": 15},
                {"kind": "on_hit_dot", "dotId": "70000140", "hitId": "rune_bleed_tick", "name": "출혈 룬 피해", "damageRatio": 2.906, "fixedDamage": 593.5, "firstTickMs": 500, "tickIntervalMs": 1000, "tickCount": 6}
            ]
        });
        request["buffData"] = serde_json::json!([{
            "id": "70000100", "name": "광분", "icon": "https://example.com/rage.png",
            "source": "룬 · 광분"
        }]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert!(profiles.global_triggers.iter().any(|trigger| matches!(
            (&trigger.filter, &trigger.effect),
            (
                crate::profile::TriggerFilter::SkillCastId(0),
                TriggerEffect::Chance { chance, effect }
            ) if (*chance - 1.0).abs() < 1e-9
                && matches!(effect.as_ref(), TriggerEffect::ReduceCooldowns { skill_ids, percent }
                    if skill_ids == &[0] && (*percent - 0.16).abs() < 1e-9)
        )));
        assert!(profiles.global_triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::Chance { effect, .. }
                if matches!(effect.as_ref(), TriggerEffect::ApplyBuff(spec)
                    if spec.id == "70000100"
                        && spec.duration_ms == 6000
                        && (spec.effects[&StatField::AttackSpeed] - 0.16).abs() < 1e-9
                        && (spec.effects[&StatField::MovementSpeed] - 0.16).abs() < 1e-9)
        )));

        let triggers = &profiles.skills[0].hits[0].triggers;
        assert!(triggers.iter().any(|trigger| {
            (trigger.chance - 1.0).abs() < 1e-9
                && matches!(&trigger.effect, TriggerEffect::ApplyBuff(spec)
                    if spec.id == "70000060" && spec.duration_ms == 3000)
        }));
        assert!(triggers.iter().any(|trigger| matches!(
            (&trigger.on, &trigger.effect),
            (
                crate::profile::HitTriggerOn::OnHitIf(crate::profile::HitCondition::BuffActive(id)),
                TriggerEffect::Sequence(effects)
            ) if id == "70000060"
                && effects.iter().any(|effect| matches!(effect,
                    TriggerEffect::ApplyBuff(spec)
                        if spec.id == "70000062"
                            && spec.duration_ms == 6000
                            && (spec.effects[&StatField::CooldownReduction] - 0.15).abs() < 1e-9))
        )));
        assert!(triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::ScheduleDot { dot_id, hit, first_tick_ms: 500, tick_interval_ms: 1000, tick_count: 6, refresh: true }
                if dot_id == "70000140"
                    && hit.hit_id == "rune_bleed_tick"
                    && (hit.skill_modifier - 2.906).abs() < 1e-9
                    && (hit.base_damage - 593.5).abs() < 1e-9
        )));

        let trace = engine::trace_one(&profiles, 1, 700).1;
        assert!(trace.damage_events.iter().any(|event| {
            event.time == 0.5 && event.hit_name == "출혈 룬 피해"
        }));
        assert!(trace.casts[0].active_buffs.iter().any(|id| id == "70000062"));
        assert!(!trace.casts[0].active_buffs.iter().any(|id| id == "70000060"));
        let rage = trace.casts[0]
            .active_buff_details
            .iter()
            .find(|buff| buff.id == "70000100")
            .expect("rage rune buff should be traced");
        assert_eq!(rage.name, "광분");
        assert_eq!(rage.icon, "https://example.com/rage.png");
        assert_eq!(rage.source, "룬 · 광분");
    }

    #[test]
    fn chaos_core_rotation_effects_use_exact_skill_and_resource_scope() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [],
                "classCores": [{
                    "name": "충전 테스트", "points": 10,
                    "options": [{
                        "pointRequirement": 10, "rawOptionId": 1,
                        "runtimeEffects": [{
                            "kind": "skill_charge_and_speed_buff", "skillId": 19140,
                            "maxCharges": 2, "rechargeMs": 10000,
                            "durationMs": 0, "valuePercent": 0
                        }]
                    }]
                }],
                "commonCores": [
                    {
                        "name": "신념의 강화", "points": 17,
                        "options": [
                            {"pointRequirement": 10, "simEffects": [{"kind": "identity_gain_multiplier", "value": 0.6}]},
                            {"pointRequirement": 17, "simEffects": [{"kind": "identity_gain_multiplier", "value": 1.4}]}
                        ]
                    },
                    {
                        "name": "흐르는 마나", "points": 20,
                        "options": [
                            {"pointRequirement": 14, "simEffects": [{"kind": "normal_skill_cooldown_reduction", "value": 0.4}]},
                            {"pointRequirement": 17, "simEffects": [{"kind": "normal_skill_cooldown_reduction", "value": 0.8}]},
                            {"pointRequirement": 18, "simEffects": [{"kind": "normal_skill_cooldown_reduction", "value": 0.13}]},
                            {"pointRequirement": 19, "simEffects": [{"kind": "normal_skill_cooldown_reduction", "value": 0.13}]},
                            {"pointRequirement": 20, "simEffects": [{"kind": "normal_skill_cooldown_reduction", "value": 0.13}]}
                        ]
                    }
                ]
            }"#,
        ))
        .expect("request should be valid JSON");
        request["skillData"][0]["cooldown"] = serde_json::json!(10.0);
        request["skillData"][0]["identityGain"] =
            serde_json::json!({"name": "CardGauge", "value": 10.0});
        request["skillData"][0]["hits"][0]["identityGain"] =
            serde_json::json!({"name": "CardGauge", "value": 5.0});
        request["character"]["skills"].as_array_mut().unwrap().push(serde_json::json!({
            "name": "각성 테스트", "level": 1, "rune": null, "tripods": []
        }));
        let mut awakening = request["skillData"][0].clone();
        awakening["name"] = serde_json::json!("각성 테스트");
        awakening["gameSkillId"] = serde_json::json!(99999);
        awakening["skillSlot"] = serde_json::json!("Awakening");
        request["skillData"].as_array_mut().unwrap().push(awakening);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let normal = profiles.skills.iter().find(|skill| skill.name == "셀레스티얼 레인").unwrap();
        let awakening = profiles.skills.iter().find(|skill| skill.name == "각성 테스트").unwrap();

        assert_eq!(normal.cooldown_ms, 9_841);
        assert_eq!(normal.charge_recovery_ms, 9_841);
        assert_eq!(awakening.cooldown_ms, 10_000);
        assert!((normal.resource_gains["CardGauge"] - 10.2).abs() < 1e-9);
        assert!(normal.hits[0].triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::GainResource { resource, amount }
                if resource == "CardGauge" && (*amount - 5.1).abs() < 1e-9
        )));
    }

    #[test]
    fn chaos_core_target_debuff_affects_hits_after_the_first() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [],
                "classCores": [],
                "commonCores": [{
                    "name": "강철의 흔적",
                    "points": 10,
                    "options": [{
                        "pointRequirement": 10,
                        "simEffects": [{
                            "kind": "on_hit_target_debuff",
                            "field": "target_defense_reduction",
                            "value": 10,
                            "durationMs": 6000
                        }]
                    }]
                }]
            }"#,
        )).expect("request should be valid JSON");
        request["targetDefense"] = serde_json::json!(6500.0);
        request["skillData"][0]["cooldown"] = serde_json::json!(0.1);
        request["skillData"][0]["castTimesMs"] = serde_json::json!([100]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 200).1;

        assert_eq!(trace.damage_events.len(), 3);
        let first = trace.damage_events[0].result_damage;
        let second = trace.damage_events[1].result_damage;
        assert!((second / first - (6500.0 / 12350.0) / 0.5).abs() < 1e-9);
    }

    #[test]
    fn chaos_core_burn_uses_raw_damage_and_refreshes_six_ticks() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [],
                "classCores": [],
                "commonCores": [{
                    "name": "불타는 일격",
                    "points": 17,
                    "options": [
                        {"pointRequirement": 10, "simEffects": [{"kind": "on_hit_dot", "dotId": "608211000", "hitId": "burning_strike_tick", "name": "불타는 일격 화상 피해", "damageRatio": 0.8625, "fixedDamage": 175.5, "firstTickMs": 500, "tickIntervalMs": 1000, "tickCount": 6}]},
                        {"pointRequirement": 17, "simEffects": [{"kind": "on_hit_dot", "dotId": "608211000", "hitId": "burning_strike_tick", "name": "불타는 일격 화상 피해", "damageRatio": 1.725, "fixedDamage": 351, "firstTickMs": 500, "tickIntervalMs": 1000, "tickCount": 6}]}
                    ]
                }]
            }"#,
        )).expect("request should be valid JSON");
        request["skillData"][0]["cooldown"] = serde_json::json!(100.0);
        request["skillData"][0]["castTimesMs"] = serde_json::json!([300]);
        request["skillData"][0]["hits"] = serde_json::json!([
            {"damageRatio": [0.0], "fixedDamage": [1.0], "phase": 0},
            {"damageRatio": [0.0], "fixedDamage": [1.0], "phase": 0, "fixedDelayMs": 300}
        ]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert!(profiles.global_triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::ScheduleDot { dot_id, hit, refresh: true, .. }
                if dot_id == "608211000"
                    && (hit.skill_modifier - 1.725).abs() < 1e-9
                    && (hit.base_damage - 351.0).abs() < 1e-9
        )));

        let trace = engine::trace_one(&profiles, 1, 5_900).1;
        let burn: Vec<_> = trace.damage_events.iter()
            .filter(|event| event.damage_source == "dot:608211000")
            .collect();
        assert_eq!(burn.len(), 6);
        assert_eq!(burn.iter().map(|event| event.time).collect::<Vec<_>>(), vec![0.8, 1.8, 2.8, 3.8, 4.8, 5.8]);
        assert!(burn.iter().all(|event| event.hit_name == "불타는 일격 화상 피해"));
    }

    #[test]
    fn arcana_ark_grid_exact_skill_core_uses_raw_game_skill_id() {
        let json = base_arcana_request(
            "[]",
            "null",
            r#"{
                        "gems": [],
                        "commonCores": [],
                        "classCores": [{
                            "name": "지정 스킬 테스트",
                            "points": 10,
                            "options": [{
                                "pointRequirement": 10,
                                "rawOptionId": 3192500,
                                "runtimeEffects": [{
                                    "kind": "static_skill_damage",
                                    "skillId": 19140,
                                    "valuePercent": 12.0
                                }]
                            }]
                        }]
                    }"#,
        );

        let profiles = builder::build(&json).expect("build should succeed");
        assert!((profiles.skills[0].hits[0].damage_increase - 0.12).abs() < 1e-9);
    }

    #[test]
    fn arcana_ark_grid_exact_skill_timing_uses_raw_game_skill_id() {
        let json = base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [{
                    "name": "타이밍 테스트", "points": 10,
                    "options": [{
                        "pointRequirement": 10, "rawOptionId": 1,
                        "runtimeEffects": [
                            {"kind": "static_skill_cooldown_change", "skillId": 19140, "valueMs": -500},
                            {"kind": "static_skill_cast_speed", "skillId": 19140, "valuePercent": 15}
                        ]
                    }]
                }]
            }"#,
        );

        let profiles = builder::build(&json).expect("build should succeed");
        assert_eq!(profiles.skills[0].cooldown_ms, 500);
        assert_eq!(profiles.skills[0].cast_times_ms, vec![850]);
    }

    #[test]
    fn arcana_ark_grid_target_stack_damage_reaches_normal_skill_hits() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [{
                    "name": "스택 홀드", "points": 17,
                    "options": [{
                        "pointRequirement": 17, "rawOptionId": 3195200,
                        "runtimeEffects": [{
                            "kind": "conditional_target_stack_damage",
                            "identityCategory": 28,
                            "targetBuffId": "arcana_ruin_stack",
                            "minimumStacks": 1,
                            "valuePercent": 3
                        }]
                    }]
                }]
            }"#,
        ))
        .expect("request should be valid JSON");
        request["skillData"][0]["identityCategory"] = serde_json::json!(28);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert!(profiles.skills[0].hits[0].triggers.iter().any(|trigger| matches!(
            (&trigger.on, &trigger.effect),
            (
                HitTriggerOn::OnHitIf(HitCondition::TargetBuffActive(buff_id)),
                TriggerEffect::DamageBonus(bonus)
            ) if buff_id == "arcana_ruin_stack" && (*bonus - 0.03).abs() < 1e-9
        )));
    }

    #[test]
    fn arcana_specialization_applies_card_gauge_and_ruin_damage_multipliers() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [{
                    "name": "체인 드로우", "points": 17,
                    "options": [{
                        "pointRequirement": 17, "rawOptionId": 3193700,
                        "runtimeEffects": [{
                            "kind": "card_gauge_gain_multiplier", "valuePercent": 15
                        }]
                    }]
                }]
            }"#,
        ))
        .expect("request should be valid JSON");
        request["skillData"][0]["hits"][0]["effectId"] = serde_json::json!(191401);
        request["skillData"][0]["ruinTriggerEffectIds"] = serde_json::json!([191401]);
        request["skillData"][0]["runtimeDamageByTargetBuffStacks"] = serde_json::json!([
            {"targetBuffId": "arcana_ruin_stack", "stack": 1, "damageRatio": [0.01], "fixedDamage": [111.0]},
            {"targetBuffId": "arcana_ruin_stack", "stack": 2, "damageRatio": [0.02], "fixedDamage": [222.0]}
        ]);
        request["skillData"][0]["identityGain"] =
            serde_json::json!({"name": "CardGauge", "value": 10.0});
        request["skillData"][0]["hits"][0]["identityGain"] =
            serde_json::json!({"name": "CardGauge", "value": 5.0});
        request["enablePetBuff"] = serde_json::json!(true);
        request["petBuffType"] = serde_json::json!("특화");
        request["petTalents"] = serde_json::json!({
            "additionalDamage": 0,
            "mainStatPercent": 0,
            "typeDamage": 0
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let specialization_multiplier = 1.0 + 160.0 / 699.0 * 0.25;
        let ruin_damage_multiplier = 1.0 + 160.0 / 699.0 * 0.35;
        assert!((profiles.skills[0].resource_gains["CardGauge"]
            - 11.5 * specialization_multiplier).abs() < 1e-9);
        assert!(profiles.skills[0].hits[0].triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::GainResource { resource, amount }
                if resource == "CardGauge"
                    && (*amount - 5.75 * specialization_multiplier).abs() < 1e-9
        )));
        let (_, trace) = engine::trace_one(&profiles, 1, 100);
        assert!((trace.casts[0].resource_gains[0].amount
            - 11.5 * specialization_multiplier).abs() < 1e-9);
        assert!((trace.damage_events[0].resource_gains[0].amount
            - 5.75 * specialization_multiplier).abs() < 1e-9);
        assert_eq!(profiles.skills[0].hits[0].damage_increase, 0.0);
        let ruin_damage_increases = profiles.skills[0].hits[0]
            .triggers
            .iter()
            .find_map(|trigger| match &trigger.effect {
                TriggerEffect::DealDamageByTargetBuffStacks { hits_by_stack, .. } => {
                    Some(hits_by_stack.iter().map(|hit| hit.damage_increase).collect::<Vec<_>>())
                }
                _ => None,
            })
            .expect("ruin stack damage trigger should exist");
        assert!(
            ruin_damage_increases
                .iter()
                .all(|value| (*value - (ruin_damage_multiplier - 1.0)).abs() < 1e-9),
            "unexpected ruin damage multipliers: {ruin_damage_increases:?}"
        );
    }

    #[test]
    fn ark_passive_combat_stats_reach_crit_speed_cooldown_and_specialization() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["arkPassive"] = serde_json::json!({
            "commonNodes": [
                {"name": "치명", "level": 1, "effectsByLevel": [[
                    {"kind": "flat", "field": "crit", "value": 2794}
                ]]},
                {"name": "특화", "level": 1, "effectsByLevel": [[
                    {"kind": "flat", "field": "specialization", "value": 699}
                ]]},
                {"name": "신속", "level": 1, "effectsByLevel": [[
                    {"kind": "flat", "field": "swiftness", "value": 5821}
                ]]}
            ],
            "classNodes": []
        });
        request["skillData"][0]["cooldown"] = serde_json::json!(10.0);
        request["skillData"][0]["castTimesMs"] = serde_json::json!([100]);
        request["skillData"][0]["hits"][0]["identityGain"] = serde_json::json!({
            "name": "CardGauge", "value": 100.0
        });

        let profiles = builder::build(&request.to_string()).unwrap();

        assert_eq!(profiles.stat.crit, 2794.0);
        assert_eq!(profiles.stat.specialization, 699.0);
        assert_eq!(profiles.stat.swiftness, 5821.0);
        assert!(profiles.skills[0].hits[0].triggers.iter().any(|trigger| matches!(
            trigger.effect,
            TriggerEffect::GainResource { ref resource, amount }
                if resource == "CardGauge" && (amount - 125.0).abs() < 1e-9
        )));

        let mut speed_profiles = profiles.clone();
        speed_profiles.skills[0].cooldown_ms = 0;
        let (_, speed_trace) = engine::trace_one(&speed_profiles, 1, 150);
        assert_eq!(
            speed_trace.casts.iter().map(|cast| cast.time).collect::<Vec<_>>(),
            vec![0.0, 0.071, 0.142]
        );

        let (_, trace) = engine::trace_one(&profiles, 1, 2_100);
        assert_eq!(trace.casts.iter().map(|cast| cast.time).collect::<Vec<_>>(), vec![0.0, 1.999]);
        assert!(trace.damage_events.iter().all(|event| event.is_crit));
        assert!(trace.damage_events.iter().all(|event| (event.formula.crit_rate - 1.0).abs() < 1e-9));
        let crit_factor = trace.damage_events[0].formula.factors.iter()
            .find(|factor| factor.name == "critRate")
            .expect("crit rate factor should be traced");
        assert_eq!(crit_factor.value, 1.0);
        assert!(crit_factor.sources.iter().any(|source|
            source.source == "stat:crit" && (source.value - 1.0).abs() < 1e-9
        ));
    }

    #[test]
    fn common_ark_passive_mana_and_blunt_thorn_use_the_hit_formula() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["arkPassive"] = serde_json::json!({
            "commonNodes": [
                {"name": "치명 보정", "level": 1, "effectsByLevel": [[
                    {"kind": "stat", "field": "crit_rate", "value": 100.0}
                ]]},
                {"name": "끝없는 마나", "level": 2, "effectsByLevel": [[], [
                    {"kind": "mana_skill_cooldown_reduction", "value": 14.0},
                    {"kind": "mana_cost_reduction", "value": 20.0}
                ]]},
                {"name": "마나 용광로", "level": 2, "effectsByLevel": [[], [
                    {"kind": "extra_max_mana_cost", "value": 2.0},
                    {"kind": "mana_furnace", "value": 0.5},
                    {"kind": "mana_furnace_cap", "value": 24.0}
                ]]},
                {"name": "뭉툭한 가시", "level": 2, "effectsByLevel": [[], [
                    {"kind": "stat", "field": "evolution_damage", "value": 15.0},
                    {"kind": "crit_rate_cap", "value": 80.0},
                    {"kind": "excess_crit_evolution", "value": 150.0},
                    {"kind": "excess_crit_evolution_cap", "value": 60.0}
                ]]}
            ],
            "classNodes": []
        });
        request["skillData"][0]["cooldown"] = serde_json::json!(10.0);
        request["skillData"][0]["resourceCost"] =
            serde_json::json!({"name": "Mp", "cost": [100.0]});
        request["skillData"][0]["baseManaCost"] = serde_json::json!(200.0);

        let profiles = builder::build(&request.to_string()).unwrap();
        let skill = &profiles.skills[0];
        assert_eq!(skill.cooldown_ms, 8_600);
        assert!((skill.resource_costs["Mp"] - 80.0).abs() < 1e-9);
        assert!((skill.extra_mp_cost_ratio - 0.02).abs() < 1e-9);
        assert_eq!(skill.evolution_damage_bonuses[0].1, 0.10);

        let (_, trace) = engine::trace_one(&profiles, 7, 100);
        let formula = &trace.damage_events[0].formula;
        assert!((formula.crit_rate - 0.8).abs() < 1e-9);
        assert!((formula.evolution_damage_multiplier - 1.55).abs() < 1e-9);
        let evolution = formula.factors.iter()
            .find(|factor| factor.name == "evolutionDamage").unwrap();
        assert!(evolution.sources.iter().any(|source|
            source.source == "ark_passive:뭉툭한 가시:초과치적"
                && (source.value - 0.30).abs() < 1e-9
        ));
    }

    #[test]
    fn sonic_breakthrough_uses_live_attack_and_move_speed() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["arkPassive"] = serde_json::json!({
            "commonNodes": [
                {"name": "신속", "level": 1, "effectsByLevel": [[
                    {"kind": "flat", "field": "swiftness", "value": 2328.4}
                ]]},
                {"name": "축복의 여신", "level": 3, "effectsByLevel": [[], [], [
                    {"kind": "stat", "field": "attack_speed", "value": 9.0},
                    {"kind": "stat", "field": "movement_speed", "value": 9.0}
                ]]},
                {"name": "음속 돌파", "level": 2, "effectsByLevel": [[], [
                    {"kind": "speed_evolution", "value": 10.0},
                    {"kind": "speed_overcap_evolution", "value": 8.0},
                    {"kind": "speed_overcap_coefficient", "value": 30.0},
                    {"kind": "speed_evolution_cap", "value": 24.0}
                ]]}
            ],
            "classNodes": []
        });

        let profiles = builder::build(&request.to_string()).unwrap();
        let (_, trace) = engine::trace_one(&profiles, 11, 100);
        let formula = &trace.damage_events[0].formula;
        assert!((formula.evolution_damage_multiplier - 1.232).abs() < 0.001,
            "got {}", formula.evolution_damage_multiplier);
        assert!(formula.factors.iter()
            .find(|factor| factor.name == "evolutionDamage").unwrap().sources.iter()
            .any(|source| source.source == "ark_passive:음속 돌파"));
    }

    #[test]
    fn arcana_specialization_applies_common_awakening_damage_only_to_regular_awakening() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["enablePetBuff"] = serde_json::json!(true);
        request["petBuffType"] = serde_json::json!("특화");
        request["petTalents"] = serde_json::json!({});
        request["skillData"][0]["skillSlot"] = serde_json::json!("Awakening");

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let expected = 160.0 / 699.0 * 0.1528;
        assert!((profiles.skills[0].hits[0].damage_increase - expected).abs() < 1e-9);

        request["skillData"][0]["skillSlot"] = serde_json::json!("SuperAwakening");
        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(profiles.skills[0].hits[0].damage_increase, 0.0);
    }

    #[test]
    fn card_set_damage_bonus_reaches_elemental_damage() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["cardDamageBonus"] = serde_json::json!(12.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(profiles.stat.sum(StatField::ElementalDamage), 0.12);
    }

    #[test]
    fn ark_grid_gem_uses_the_selected_effect_level() {
        let mut request: serde_json::Value =
            serde_json::from_str(&base_arcana_request("[]", "null", "null")).unwrap();
        request["character"]["arkGrids"] = serde_json::json!({
            "gems": [{
                "slot": 1,
                "level": 2,
                "effectsByLevel": [
                    [{"kind": "stat", "field": "attack_power_mul", "value": 0.03}],
                    [{"kind": "stat", "field": "attack_power_mul", "value": 0.07}]
                ]
            }],
            "commonCores": [],
            "classCores": []
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert!((profiles.stat.sum(StatField::AttackPowerMul) - 0.0007).abs() < 1e-9);
    }

    #[test]
    fn arcana_ark_grid_stream_charge_and_speed_buff_reach_runtime_profiles() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [{
                    "name": "스트림 오브 엣지", "points": 10,
                    "options": [{
                        "pointRequirement": 10, "rawOptionId": 3196000,
                        "runtimeEffects": [{
                            "kind": "skill_charge_and_speed_buff", "skillId": 19150,
                            "maxCharges": 2, "rechargeMs": 24000,
                            "durationMs": 8000, "valuePercent": 8
                        }]
                    }]
                }]
            }"#,
        ))
        .expect("request should be valid JSON");
        request["skillData"][0]["gameSkillId"] = serde_json::json!(19150);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(profiles.skills[0].max_stacks, 2);
        assert_eq!(profiles.skills[0].charge_recovery_ms, 24_000);
        assert!(profiles.global_triggers.iter().any(|trigger| matches!(
            (&trigger.filter, &trigger.effect),
            (
                crate::profile::TriggerFilter::SkillCastId(0),
                TriggerEffect::ApplyBuff(spec)
            ) if spec.id == "arcana_ark_grid_3196000_speed"
                && spec.duration_ms == 8_000
                && (spec.effects[&StatField::AttackSpeed] - 0.08).abs() < 1e-9
        )));
    }

    #[test]
    fn arcana_ark_grid_darkness_edge_adds_crit_damage_stack() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            r#"[{"name": "다크니스 엣지", "values": []}]"#,
            "null",
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [{
                    "name": "스트림 오브 엣지", "points": 17,
                    "options": [{
                        "pointRequirement": 17, "rawOptionId": 3196200,
                        "runtimeEffects": [{
                            "kind": "tripod_buff_crit_damage_per_stack", "skillId": 19150,
                            "requiredTripodName": "다크니스 엣지", "sourceBuffId": 191542,
                            "buffId": 3196200, "maxStacks": 5,
                            "durationMs": 3000, "valuePercent": 1.6
                        }]
                    }]
                }]
            }"#,
        ))
        .expect("request should be valid JSON");
        request["skillData"][0]["gameSkillId"] = serde_json::json!(19150);
        request["skillData"][0]["hits"][0]["selfCritRateStack"] = serde_json::json!({
            "buffId": "191542", "percentPerStack": 5.52,
            "maxStacks": 5, "durationMs": 3000
        });

        let mut inactive_request = request.clone();
        inactive_request["character"]["skills"][0]["tripods"] = serde_json::json!([]);
        let inactive_profiles =
            builder::build(&inactive_request.to_string()).expect("build should succeed");
        assert!(!inactive_profiles.skills[0].hits[0].triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::ApplyBuff(spec) if spec.id == "3196200"
        )));

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert!(profiles.skills[0].hits[0].triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::ApplyBuff(spec)
                if spec.id == "3196200"
                    && spec.max_stacks == 5
                    && spec.duration_ms == 3000
                    && (spec.effects[&StatField::CritDmg] - 1.6).abs() < 1e-9
        )));
    }

    #[test]
    fn arcana_ark_grid_tripod_effect_damage_handles_hits_and_dots() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [{
                    "name": "효과 피해 테스트", "points": 14,
                    "options": [{
                        "pointRequirement": 14, "rawOptionId": 3198000,
                        "runtimeEffects": [
                            {
                                "kind": "tripod_cooldown_and_effect_damage",
                                "skillId": 19200, "targetEffectId": 192011,
                                "valueMs": 20000, "valuePercent": 115
                            },
                            {
                                "kind": "tripod_effect_damage",
                                "skillId": 19200, "targetEffectId": 190509,
                                "valuePercent": 70
                            }
                        ]
                    }]
                }]
            }"#,
        ))
        .expect("request should be valid JSON");
        request["skillData"][0]["gameSkillId"] = serde_json::json!(19200);
        request["skillData"][0]["hits"][0]["effectId"] = serde_json::json!(192011);
        request["skillData"][0]["hits"][0]["dot"] = serde_json::json!({
            "dotId": "190509", "hitId": "fear_tick", "name": "죽음의 공포 피해",
            "chance": 1.0, "damageRatio": [2.0], "fixedDamage": [10.0],
            "firstTickMs": 500, "tickIntervalMs": 1000, "tickCount": 3
        });

        let mut inactive_request = request.clone();
        inactive_request["skillData"][0]["hits"][0]["effectId"] = serde_json::json!(0);
        inactive_request["skillData"][0]["hits"][0]["dot"]["dotId"] =
            serde_json::json!("other_dot");
        let inactive_profiles =
            builder::build(&inactive_request.to_string()).expect("build should succeed");
        assert_eq!(inactive_profiles.skills[0].cooldown_ms, 1_000);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let skill = &profiles.skills[0];
        assert_eq!(skill.cooldown_ms, 21_000);
        assert!((skill.hits[0].skill_modifier - 2.15).abs() < 1e-9);
        assert!((skill.hits[0].base_damage - 215.0).abs() < 1e-9);
        assert!(skill.hits[0].triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::ScheduleDot { dot_id, hit, .. }
                if dot_id == "190509"
                    && (hit.skill_modifier - 3.4).abs() < 1e-9
                    && (hit.base_damage - 17.0).abs() < 1e-9
        )));
    }

    #[test]
    fn arcana_ark_grid_fake_flip_draws_card_on_target_effect() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [{
                    "name": "셔플 댄스", "points": 14,
                    "options": [{
                        "pointRequirement": 14, "rawOptionId": 3198100,
                        "runtimeEffects": [{
                            "kind": "tripod_stack_and_card_draw", "skillId": 19200,
                            "targetEffectId": 192011, "stacks": 2,
                            "chancePercent": 33, "cards": 1
                        }]
                    }]
                }]
            }"#,
        ))
        .expect("request should be valid JSON");
        request["skillData"][0]["gameSkillId"] = serde_json::json!(19200);
        request["skillData"][0]["hits"][0]["effectId"] = serde_json::json!(192011);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert!(profiles.skills[0].hits[0].triggers.iter().any(|trigger| {
            matches!(trigger.effect, TriggerEffect::DrawCard)
                && (trigger.chance - 0.33).abs() < 1e-9
        }));
    }

    #[test]
    fn arcana_destiny_grants_and_consumes_edge_of_fate_stacks() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [
                    {"name": "엣지 오브 페이트", "points": 17, "options": [
                        {"pointRequirement": 14, "rawOptionId": 3190100,
                         "runtimeEffects": [{"kind": "on_destiny_ruin_consumable_damage", "skillGroupId": 2190902, "buffId": 3190100, "stacks": 3, "maxStacks": 3, "valuePercent": 8}]},
                        {"pointRequirement": 17, "rawOptionId": 3190200,
                         "runtimeEffects": [{"kind": "on_destiny_ruin_consumable_damage", "skillGroupId": 2190902, "buffId": 3190200, "stacks": 5, "maxStacks": 5, "valuePercent": 13}]}
                    ]},
                    {"name": "다크 메이트", "points": 14, "options": [
                        {"pointRequirement": 14, "rawOptionId": 3195600,
                         "runtimeEffects": [{"kind": "destiny_on_skill_cast", "skillId": 19040, "chancePercent": 100}]}
                    ]},
                    {"name": "인피니티 덱", "points": 14, "options": [
                        {"pointRequirement": 14, "rawOptionId": 3190600,
                         "runtimeEffects": [{"kind": "on_destiny_draw_cards", "cards": 1}]}
                    ]}
                ]
            }"#,
        ))
        .expect("request should be valid JSON");
        request["skillData"][0]["gameSkillId"] = serde_json::json!(19040);
        request["skillData"][0]["properties"]["skillGroupIds"] =
            serde_json::json!([2190902]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(
            profiles.skills[0].cast_buff_damage_bonuses,
            vec![("3190200".to_string(), 0.13)]
        );
        assert!(profiles.global_triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::Sequence(effects) if matches!(
                effects.as_slice(),
                [TriggerEffect::ApplyBuffStacks { spec, stacks }, TriggerEffect::DrawCard]
                    if spec.id == "3190200" && spec.max_stacks == 5 && *stacks == 5
            )
        )));

        let (result, _) = engine::trace_one(&profiles, 1, 2_500);
        assert!(profiles.global_triggers.iter().any(|trigger| matches!(
            (&trigger.condition, &trigger.effect),
            (
                crate::profile::TriggerCondition::BuffActive(id),
                TriggerEffect::ConsumeBuffStacks { buff_id, stacks }
            ) if id == "3190200" && buff_id == "3190200" && *stacks == 1
        )));
        assert!(
            (result.total_damage - 326.0).abs() < 1e-9,
            "unexpected total damage: {}",
            result.total_damage
        );
    }

    #[test]
    fn arcana_destiny_supports_random_casts_and_four_ruin_hits() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [
                    {"name": "엣지 오브 페이트", "points": 14, "options": [
                        {"pointRequirement": 14, "rawOptionId": 3190100,
                         "runtimeEffects": [{"kind": "on_destiny_ruin_consumable_damage", "skillGroupId": 2190902, "buffId": 3190100, "stacks": 3, "maxStacks": 3, "valuePercent": 8}]}
                    ]},
                    {"name": "루인 풀셋", "points": 14, "options": [
                        {"pointRequirement": 14, "rawOptionId": 3194100,
                         "runtimeEffects": [{"kind": "destiny_after_skill_group_hits", "skillGroupId": 2190402, "hitCount": 4}]}
                    ]}
                ]
            }"#,
        ))
        .expect("request should be valid JSON");
        request["skillData"][0]["properties"]["skillGroupIds"] =
            serde_json::json!([2190902, 2190402, 2192001]);
        let hit = request["skillData"][0]["hits"][0].clone();
        request["skillData"][0]["hits"] = serde_json::json!([
            hit.clone(), hit.clone(), hit.clone(), hit
        ]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (result, _) = engine::trace_one(&profiles, 1, 1_500);
        assert!((result.total_damage - 832.0).abs() < 1e-9);

        request["character"]["arkGrids"]["classCores"].as_array_mut().unwrap().push(
            serde_json::json!({"name": "다크 메이트", "points": 14, "options": [
                {"pointRequirement": 14, "rawOptionId": 3194600,
                 "runtimeEffects": [{"kind": "destiny_on_skill_group_cast", "skillGroupId": 2192001, "chancePercent": 18}]}
            ]}),
        );
        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert!(profiles.global_triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::Chance { chance, .. } if (*chance - 0.18).abs() < 1e-9
        )));
    }

    #[test]
    fn arcana_destiny_applies_timed_damage_and_cooldown_effects() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [
                    {"name": "루인 서브셋", "points": 17, "options": [
                        {"pointRequirement": 17, "rawOptionId": 3191200,
                         "runtimeEffects": [{"kind": "on_destiny_skill_group_buff", "skillGroupId": 2190904, "buffId": 3191200, "durationMs": 20000, "cooldownReductionPercent": 8, "valuePercent": 7}]}
                    ]},
                    {"name": "노말 인핸스", "points": 17, "options": [
                        {"pointRequirement": 14, "rawOptionId": 3192100,
                         "runtimeEffects": [{"kind": "on_destiny_skill_group_cooldown_reduction", "skillGroupId": 2192001, "cooldownReductionPercent": 10}]},
                        {"pointRequirement": 17, "rawOptionId": 3192200,
                         "runtimeEffects": [{"kind": "on_destiny_identity_category_damage_buff", "identityCategory": 28, "buffId": 3192200, "durationMs": 16000, "valuePercent": 3.8}]}
                    ]},
                    {"name": "임팩트 메이트", "points": 17, "options": [
                        {"pointRequirement": 17, "rawOptionId": 3192700,
                         "runtimeEffects": [{"kind": "on_destiny_damage_buff", "buffId": 3192700, "durationMs": 5000, "valuePercent": 15}]}
                    ]},
                    {"name": "다크 메이트", "points": 14, "options": [
                        {"pointRequirement": 14, "rawOptionId": 3195600,
                         "runtimeEffects": [{"kind": "destiny_on_skill_cast", "skillId": 19040, "chancePercent": 100}]}
                    ]}
                ]
            }"#,
        ))
        .expect("request should be valid JSON");
        request["skillData"][0]["gameSkillId"] = serde_json::json!(19040);
        request["skillData"][0]["identityCategory"] = serde_json::json!(28);
        request["skillData"][0]["properties"]["skillGroupIds"] =
            serde_json::json!([2190904, 2192001]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (result, trace) = engine::trace_one(&profiles, 1, 1_000);
        assert_eq!(trace.casts.len(), 2);
        assert!((trace.casts[1].time - 0.9).abs() < 1e-9);
        assert!(
            (result.total_damage - 254.84).abs() < 1e-9,
            "unexpected total damage: {}",
            result.total_damage
        );
    }

    #[test]
    fn arcana_empress_festival_reaches_four_stack_runtime_trigger() {
        let json = base_arcana_request(
            "[]",
            r#"{
                        "commonNodes": [],
                        "classNodes": [
                            {"name": "황후의 연회", "level": 3, "values": [22.0]}
                        ]
                    }"#,
            "null",
        );

        let profiles = builder::build(&json).expect("build should succeed");
        let celestial = profiles
            .skills
            .iter()
            .find(|skill| skill.name == "셀레스티얼 레인")
            .expect("Celestial Rain profile should exist");

        assert!(celestial.hits[0].triggers.iter().any(|trigger| {
            matches!(
                (&trigger.on, &trigger.effect),
                (
                    HitTriggerOn::OnHitIf(HitCondition::TargetBuffAtMaxStacks(buff_id)),
                    TriggerEffect::DamageBonus(value)
                ) if buff_id == "arcana_ruin_stack" && (*value - 0.22).abs() < 1e-9
            )
        }));
    }

    #[test]
    fn arcana_devil_leap_nodes_apply_raw_damage_crit_and_stack_preserve() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("더 데빌")).unwrap();
        request["character"]["arkPassive"] = serde_json::json!({
            "commonNodes": [],
            "classNodes": [
                {"name": "악마의 눈속임", "level": 3, "values": [6.0, 30.0, 40.0]},
                {"name": "쿼즈", "level": 3, "values": [20.0, 20.0]}
            ]
        });
        request["skillData"][0]["gameSkillId"] = serde_json::json!(19350);
        request["skillData"][0]["identityCategory"] = serde_json::json!(30);
        request["skillData"][0]["hits"][0]["effectId"] = serde_json::json!(193501);
        request["skillData"][0]["ruinTriggerEffectIds"] = serde_json::json!([193501]);
        request["skillData"][0]["runtimeDamageByTargetBuffStacks"] = serde_json::json!([{
            "targetBuffId": "arcana_ruin_stack",
            "stack": 4,
            "damageRatio": [1.0],
            "fixedDamage": [100.0]
        }]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let hit = &profiles.skills[0].hits[0];

        assert!((hit.damage_increase - 0.272).abs() < 1e-9);
        assert!((hit.crit_chance_bonus - 0.30).abs() < 1e-9);
        assert!(hit.triggers.iter().any(|trigger| matches!(
            (&trigger.on, &trigger.effect),
            (
                HitTriggerOn::OnHitIf(HitCondition::TargetBuffAtMaxStacks(buff_id)),
                TriggerEffect::DamageBonus(value)
            ) if buff_id == "arcana_ruin_stack" && (*value - 0.20).abs() < 1e-9
        )));
        assert!(hit.triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::ConsumeTargetBuff {
                buff_id,
                preserve_chance,
                preserve_required_stacks: 4,
                ..
            } if buff_id == "arcana_ruin_stack" && (*preserve_chance - 0.40).abs() < 1e-9
        )));
    }

    #[test]
    fn arcana_sun_leap_nodes_apply_raw_timing_mana_damage_and_draw() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("더 썬")).unwrap();
        request["character"]["arkPassive"] = serde_json::json!({
            "commonNodes": [],
            "classNodes": [
                {"name": "잠재력 해방", "level": 5, "values": [10.0]},
                {"name": "즉각적인 주문", "level": 3, "values": [12.0, 90.0]},
                {"name": "숨겨진 패", "level": 3, "values": [39.0, 100.0]},
                {"name": "폴스 딜", "level": 3, "values": [40.0, 30.0]}
            ]
        });
        request["skillData"][0]["gameSkillId"] = serde_json::json!(19340);
        request["skillData"][0]["skillSlot"] = serde_json::json!("HyperAwakeningTechniques");
        request["skillData"][0]["cooldown"] = serde_json::json!(80.0);
        request["skillData"][0]["resourceCost"] =
            serde_json::json!({"name": "Mp", "cost": [915.0]});
        request["skillData"][0]["hits"][0]["effectId"] = serde_json::json!(193402);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let skill = &profiles.skills[0];
        let hit = &skill.hits[0];

        assert_eq!(skill.cooldown_ms, 72_000);
        assert_eq!(skill.cast_times_ms, vec![892]);
        assert!(skill.tags.contains(&SkillTag::CategoryHyper));
        assert!(skill.tags.contains(&SkillTag::CategoryNormal));
        assert!((skill.resource_costs["Mp"] - 91.5).abs() < 1e-9);
        assert!((hit.damage_increase - 0.946).abs() < 1e-9);
        assert!((hit.crit_damage_bonus - 0.30).abs() < 1e-9);
        assert!(hit.triggers.iter().any(|trigger| {
            matches!(trigger.effect, TriggerEffect::DrawCard)
                && (trigger.chance - 1.0).abs() < 1e-9
        }));
    }

    #[test]
    fn arcana_enlightenment_nodes_modify_raw_card_effects() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름")).unwrap();
        request["character"]["arkPassive"] = serde_json::json!({
            "commonNodes": [],
            "classNodes": [
                {"name": "황제의 칙령", "level": 3, "values": [80.0]},
                {"name": "황후의 계략", "level": 2, "values": [2.0, 2.0]},
                {"name": "황후의 기사", "level": 5, "values": [400.0]},
                {"name": "또 다른 황제", "level": 3, "values": [16.0, 20.0, 40.0]}
            ]
        });
        request["cardData"] = serde_json::json!([
            {"skillId": 19282, "name": "황제", "autoLearn": false, "runtimeEffects": [{
                "kind": "direct_damage", "hitId": "emperor", "name": "황제", "damageRatio": 10.0, "fixedDamage": 100.0
            }]},
            {"skillId": 19288, "name": "황후의 기사", "autoLearn": false, "runtimeEffects": [{
                "kind": "direct_damage", "hitId": "queen", "name": "황후의 기사", "damageRatio": 20.0, "fixedDamage": 200.0
            }]},
            {"skillId": 19281, "name": "도태", "autoLearn": true, "runtimeEffects": [{
                "kind": "self_stat_buff", "buffId": 192810, "durationMs": 4000, "critRatePercent": 100.0, "critDamagePercent": 50.0
            }]},
            {"skillId": 19098, "name": "심판", "autoLearn": true, "runtimeEffects": [{
                "kind": "force_ruin_stack_damage_buff", "buffId": 190980, "durationMs": 4000, "targetBuffKey": "arcana_ruin_stack", "maxStacks": 4
            }]},
            {"skillId": 19280, "name": "로열", "autoLearn": true, "runtimeEffects": []}
        ]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let emperor = &profiles.arcana_card_effects[&19282];
        let queen = &profiles.arcana_card_effects[&19288];
        let cull = &profiles.arcana_card_effects[&19281];
        let judgment = &profiles.arcana_card_effects[&19098];
        let royal = &profiles.arcana_card_effects[&19280];

        assert!(matches!(emperor, TriggerEffect::Sequence(effects) if matches!(
            &effects[0], TriggerEffect::DealRuntimeDamage(hit)
                if (hit.base_damage - 220.0).abs() < 1e-9 && (hit.skill_modifier - 22.0).abs() < 1e-9
        )));
        assert!(matches!(queen, TriggerEffect::Sequence(effects) if matches!(
            &effects[0], TriggerEffect::DealRuntimeDamage(hit)
                if (hit.base_damage - 1000.0).abs() < 1e-9 && (hit.skill_modifier - 100.0).abs() < 1e-9
        )));
        assert!(matches!(cull, TriggerEffect::Sequence(effects) if matches!(
            &effects[0], TriggerEffect::ApplyBuff(buff) if buff.duration_ms == 6000
        )));
        assert!(matches!(judgment, TriggerEffect::Sequence(effects) if matches!(
            &effects[0], TriggerEffect::ApplyBuff(buff) if buff.duration_ms == 6000
        )));
        assert!(matches!(royal, TriggerEffect::Sequence(effects) if matches!(
            &effects[0], TriggerEffect::Chance { chance, effect }
                if (*chance - 0.20).abs() < 1e-9 && matches!(
                    effect.as_ref(), TriggerEffect::Sequence(inner) if matches!(
                        &inner[0], TriggerEffect::DealRuntimeDamage(hit) if (hit.base_damage - 220.0).abs() < 1e-9
                    )
                )
        )));
    }

    #[test]
    fn celestial_rain_prepared_tripod_effects_reach_skill_hit() {
        let json = base_arcana_request(
            r#"[
                {"name": "급소 타격", "values": [20.0]},
                {"name": "강화된 일격", "values": [30.0]},
                {"name": "파멸의 카드", "values": [25.0]}
            ]"#,
            "null",
            "null",
        );

        let mut request: serde_json::Value =
            serde_json::from_str(&json).expect("request should be valid");
        request["skillData"][0]["hits"][0]["effectId"] = serde_json::json!(191401);
        request["skillData"][0]["hits"][0]["critChanceBonus"] = serde_json::json!(0.20);
        request["skillData"][0]["hits"][0]["damageIncrease"] = serde_json::json!(0.30);
        request["skillData"][0]["ruinTriggerEffectIds"] = serde_json::json!([191401]);
        request["skillData"][0]["runtimeDamageByTargetBuffStacks"] = serde_json::json!([{
            "targetBuffId": "arcana_ruin_stack",
            "stack": 1,
            "damageRatio": [0.01],
            "fixedDamage": [100.0]
        }]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let celestial = profiles
            .skills
            .iter()
            .find(|skill| skill.name == "셀레스티얼 레인")
            .expect("Celestial Rain profile should exist");

        assert!((celestial.hits[0].crit_chance_bonus - 0.20).abs() < 1e-9);
        assert!((celestial.hits[0].damage_increase - 0.30).abs() < 1e-9);
        assert_eq!(celestial.hits[0].crit_damage_bonus, 0.0);
        assert!(celestial.hits[0].triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::ConsumeTargetBuff { buff_id, .. } if buff_id == "arcana_ruin_stack"
        )));
    }

    #[test]
    fn arcana_quadra_applies_target_ruin_stack_on_first_and_fourth_hit() {
        let profiles = builder::build(&arcana_quadra_request()).expect("build should succeed");
        let quadra = profiles
            .skills
            .iter()
            .find(|skill| skill.name == "쿼드라 엑셀레이트")
            .expect("Quadra Accelerate profile should exist");

        assert_eq!(quadra.category, SkillCategory::Stacked);
        assert!(matches!(
            quadra.hits[0].triggers.first().map(|trigger| &trigger.effect),
            Some(TriggerEffect::ApplyTargetBuffStacks { spec, stacks })
                if spec.id == "arcana_ruin_stack" && *stacks == 1
        ));
        assert!(quadra.hits[1].triggers.is_empty());
        assert!(quadra.hits[2].triggers.is_empty());
        assert!(matches!(
            quadra.hits[3].triggers.first().map(|trigger| &trigger.effect),
            Some(TriggerEffect::ApplyTargetBuffStacks { spec, stacks })
                if spec.id == "arcana_ruin_stack" && *stacks == 1
        ));
    }

    #[test]
    fn arcana_dancing_applies_one_ruin_stack_per_effect_group() {
        let mut request: serde_json::Value = serde_json::from_str(&arcana_quadra_request())
            .expect("request should be valid");
        request["character"]["skills"][0]["name"] =
            serde_json::json!("댄싱 오브 스파인플라워");
        request["skillData"][0]["name"] = serde_json::json!("댄싱 오브 스파인플라워");
        request["skillData"][0]["hits"] = serde_json::json!([
            {"effectId": 192000, "damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 1, "maxStacks": 4, "durationMs": 10000}},
            {"effectId": 192000, "damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0},
            {"effectId": 192007, "damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 1, "maxStacks": 4, "durationMs": 10000}},
            {"effectId": 192007, "damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0},
            {"effectId": 192008, "damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 1, "maxStacks": 4, "durationMs": 10000}},
            {"effectId": 192008, "damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0},
            {"effectId": 192009, "damageRatio": [1.0], "fixedDamage": [10.0], "phase": 0, "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 1, "maxStacks": 4, "durationMs": 10000}}
        ]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let dancing = &profiles.skills[0];
        let trigger_indices: Vec<_> = dancing.hits.iter().enumerate()
            .filter_map(|(index, hit)| (!hit.triggers.is_empty()).then_some(index))
            .collect();

        assert_eq!(dancing.category, SkillCategory::Stacked);
        assert_eq!(trigger_indices, vec![0, 2, 4, 6]);
    }

    #[test]
    fn arcana_back_flush_applies_two_target_ruin_stacks() {
        let profiles = builder::build(&arcana_single_hit_skill_request("백 플러쉬"))
            .expect("build should succeed");
        let back_flush = profiles
            .skills
            .iter()
            .find(|skill| skill.name == "백 플러쉬")
            .expect("Back Flush profile should exist");

        assert_eq!(back_flush.category, SkillCategory::Stacked);
        assert!(matches!(
            back_flush.hits[0].triggers.first().map(|trigger| &trigger.effect),
            Some(TriggerEffect::ApplyTargetBuffStacks { spec, stacks })
                if spec.id == "arcana_ruin_stack" && *stacks == 2
        ));

        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_single_hit_skill_request("백 플러쉬"),
        )
        .expect("request should be valid");
        request["character"]["skills"][0]["tripods"] =
            serde_json::json!([{"name": "카드 증가", "values": []}]);
        request["skillData"][0]["hits"][0]["targetBuffStackGain"]["stacks"] =
            serde_json::json!(3);
        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert!(matches!(
            profiles.skills[0].hits[0]
                .triggers
                .first()
                .map(|trigger| &trigger.effect),
            Some(TriggerEffect::ApplyTargetBuffStacks { stacks, .. }) if *stacks == 3
        ));

        request["character"]["skills"][0]["tripods"] =
            serde_json::json!([{"name": "페이크 플립", "values": []}]);
        request["skillData"][0]["identityCategory"] = serde_json::json!(28);
        request["skillData"][0]["hits"][0]["targetBuffStackGain"] = serde_json::Value::Null;
        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(profiles.skills[0].category, SkillCategory::Normal);
        assert!(profiles.skills[0].hits[0].triggers.is_empty());
    }

    #[test]
    fn arcana_fake_flip_becomes_stacked_and_applies_two_stacks() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_single_hit_skill_request("운명의 부름"),
        )
        .expect("request should be valid");
        request["character"]["skills"][0]["tripods"] =
            serde_json::json!([{"name": "페이크 플립", "values": []}]);
        request["skillData"][0]["identityCategory"] = serde_json::json!(29);
        request["skillData"][0]["hits"][0]["targetBuffStackGain"] = serde_json::json!({
            "buffId": "arcana_ruin_stack",
            "stacks": 2,
            "maxStacks": 4,
            "durationMs": 10000
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let fake_flip = &profiles.skills[0];

        assert_eq!(fake_flip.category, SkillCategory::Stacked);
        assert!(fake_flip.hits[0].triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::ApplyTargetBuffStacks { spec, stacks }
                if spec.id == "arcana_ruin_stack" && *stacks == 2
        )));
    }

    #[test]
    fn arcana_spiral_edge_applies_ruin_stack_on_both_combo_phases() {
        let profiles = builder::build(&arcana_spiral_edge_request()).expect("build should succeed");
        let spiral = profiles
            .skills
            .iter()
            .find(|skill| skill.name == "스파이럴 엣지")
            .expect("Spiral Edge profile should exist");

        assert_eq!(spiral.category, SkillCategory::Stacked);
        assert_eq!(spiral.hits.len(), 2);
        assert_eq!(spiral.hits[0].phase, 0);
        assert_eq!(spiral.hits[1].phase, 1);
        for hit in &spiral.hits {
            assert!(matches!(
                hit.triggers.first().map(|trigger| &trigger.effect),
                Some(TriggerEffect::ApplyTargetBuffStacks { spec, stacks })
                    if spec.id == "arcana_ruin_stack" && *stacks == 1
            ));
        }
    }

    #[test]
    fn arcana_spiral_edge_ruthless_shot_increases_damage_and_ruin_stacks() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_spiral_edge_request()).expect("request should be valid");
        request["character"]["skills"][0]["tripods"] =
            serde_json::json!([{"name": "무자비한 사격", "values": []}]);
        for hit in request["skillData"][0]["hits"]
            .as_array_mut()
            .expect("Spiral Edge hits should be an array")
        {
            hit["damageRatio"] = serde_json::json!([2.54]);
            hit["fixedDamage"] = serde_json::json!([25.4]);
            hit["targetBuffStackGain"]["stacks"] = serde_json::json!(2);
        }
        request["aplConfig"] = serde_json::json!(
            "actions:\n  - skill: 스파이럴 엣지\n    conditions:\n      - type: combo_phase\n        skill: 스파이럴 엣지\n        operator: \">=\"\n        value: 1\n  - skill: 스파이럴 엣지\n"
        );

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let spiral = profiles
            .skills
            .iter()
            .find(|skill| skill.name == "스파이럴 엣지")
            .expect("Spiral Edge profile should exist");

        assert_eq!(spiral.hits.len(), 2);
        for hit in &spiral.hits {
            assert!((hit.skill_modifier - 2.54).abs() < 1e-9);
            assert!((hit.base_damage - 25.4).abs() < 1e-9);
            assert!(matches!(
                hit.triggers.first().map(|trigger| &trigger.effect),
                Some(TriggerEffect::ApplyTargetBuffStacks { spec, stacks })
                    if spec.id == "arcana_ruin_stack" && *stacks == 2
            ));
        }

        let (_result, trace) = engine::trace_one(&profiles, 1, 1_250);
        let cast_stacks: Vec<u32> = trace
            .casts
            .iter()
            .filter(|cast| cast.skill_name == "스파이럴 엣지")
            .map(|cast| {
                cast.target_debuff_details
                    .iter()
                    .find(|debuff| debuff.id == "arcana_ruin_stack")
                    .map(|debuff| debuff.stacks)
                    .unwrap_or(0)
            })
            .collect();

        assert_eq!(cast_stacks, vec![2, 4]);
    }

    #[test]
    fn arcana_manual_phase_dataset_builds_and_traces_expected_phases() {
        let dataset: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/arcana-phase-test-dataset.json"
        ))
        .expect("manual phase dataset should be valid json");

        let cases = dataset["cases"]
            .as_array()
            .expect("manual phase dataset should contain cases");

        for case in cases {
            let case_id = case["id"].as_str().unwrap_or("<unknown>");
            let request = serde_json::to_string(&case["request"])
                .unwrap_or_else(|_| panic!("{case_id}: request should serialize"));
            let expect = &case["expect"];
            let skill_name = expect["skill"]
                .as_str()
                .unwrap_or_else(|| panic!("{case_id}: expected skill name is required"));
            let expected_type = expect["skillType"]
                .as_str()
                .unwrap_or_else(|| panic!("{case_id}: expected skill type is required"));
            let expected_last_phase = expect["lastPhase"]
                .as_u64()
                .unwrap_or_else(|| panic!("{case_id}: expected lastPhase is required"))
                as u32;
            let expected_hit_phases: Vec<u32> = expect["hitPhases"]
                .as_array()
                .unwrap_or_else(|| panic!("{case_id}: expected hitPhases is required"))
                .iter()
                .map(|value| value.as_u64().unwrap() as u32)
                .collect();

            let profiles = builder::build(&request)
                .unwrap_or_else(|err| panic!("{case_id}: build should succeed: {err}"));
            let skill = profiles
                .skills
                .iter()
                .find(|skill| skill.name == skill_name)
                .unwrap_or_else(|| panic!("{case_id}: expected skill profile should exist"));

            match (&skill.skill_type, expected_type) {
                (SkillType::Combo { last_phase, .. }, "Combo")
                | (SkillType::Chain { last_phase, .. }, "Chain") => {
                    assert_eq!(
                        *last_phase, expected_last_phase,
                        "{case_id}: last phase mismatch"
                    );
                }
                (SkillType::Normal, "Normal") => {}
                _ => panic!("{case_id}: unexpected skill type {:?}", skill.skill_type),
            }

            let hit_phases: Vec<u32> = skill.hits.iter().map(|hit| hit.phase).collect();
            assert_eq!(
                hit_phases, expected_hit_phases,
                "{case_id}: hit phases mismatch"
            );

            let duration_ms = expect["durationMs"]
                .as_u64()
                .unwrap_or_else(|| panic!("{case_id}: expected durationMs is required"))
                as u32;
            let min_casts = expect["minCasts"]
                .as_u64()
                .unwrap_or_else(|| panic!("{case_id}: expected minCasts is required"))
                as usize;
            let expected_damage_events = expect["damageEvents"]
                .as_u64()
                .unwrap_or_else(|| panic!("{case_id}: expected damageEvents is required"))
                as usize;

            let (_result, trace) = engine::trace_one(&profiles, 1, duration_ms);
            let skill_casts = trace
                .casts
                .iter()
                .filter(|cast| cast.skill_name == skill_name)
                .count();
            let skill_damage_events = trace
                .damage_events
                .iter()
                .filter(|event| event.skill_name == skill_name)
                .count();

            assert!(
                skill_casts >= min_casts,
                "{case_id}: expected at least {min_casts} casts, got {skill_casts}"
            );
            assert_eq!(
                skill_damage_events, expected_damage_events,
                "{case_id}: damage event count mismatch"
            );
        }
    }

    #[test]
    fn apl_can_explicitly_continue_combo_before_cooldown_starts() {
        let request = serde_json::json!({
            "character": {
                "name": "test_arcana",
                "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [
                    {"name": "리턴", "level": 1, "rune": null, "tripods": []}
                ],
                "equipments": [],
                "accessories": [],
                "bracelet": null,
                "stone": null,
                "gems": [],
                "engravings": [],
                "arkPassive": null,
                "arkGrids": null,
                "arkPassiveKarma": null
            },
            "aplConfig": "actions:\n  - skill: 리턴\n    conditions:\n      - type: combo_phase\n        skill: 리턴\n        operator: \">=\"\n        value: 1\n  - skill: 리턴\n",
            "skillData": [
                {
                    "name": "리턴",
                    "skillControlType": "Combo",
                    "skillSlot": "Normal",
                    "maxPhase": 2,
                    "cooldown": 12.0,
                    "castTimesMs": [100, 100],
                    "hits": [
                        {"damageRatio": [0.0], "fixedDamage": [10.0], "phase": 0, "identityGain": null},
                        {"damageRatio": [0.0], "fixedDamage": [20.0], "phase": 1, "identityGain": null}
                    ],
                    "resourceCost": null,
                    "identityGain": null,
                    "properties": {"attackType": "", "element": ""}
                }
            ]
        });
        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (_result, trace) = engine::trace_one(&profiles, 1, 12_250);
        let cast_times: Vec<f64> = trace
            .casts
            .iter()
            .filter(|cast| cast.skill_name == "리턴")
            .map(|cast| cast.time)
            .collect();

        assert_eq!(cast_times, vec![0.0, 0.1, 12.1, 12.2]);
    }

    #[test]
    fn expired_combo_restarts_from_first_phase() {
        let request = serde_json::json!({
            "character": {
                "name": "test_arcana", "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [
                    {"name": "리턴", "level": 1, "rune": null, "tripods": []},
                    {"name": "대기", "level": 1, "rune": null, "tripods": []}
                ],
                "equipments": [], "accessories": [], "bracelet": null, "stone": null,
                "gems": [], "engravings": [], "arkPassive": null, "arkGrids": null,
                "arkPassiveKarma": null
            },
            "aplConfig": "actions:\n  - skill: 리턴\n    conditions:\n      - type: combo_phase\n        skill: 리턴\n        operator: \"==\"\n        value: 0\n  - skill: 대기\n",
            "skillData": [
                {
                    "name": "리턴", "skillControlType": "Combo", "skillSlot": "Normal",
                    "maxPhase": 2, "cooldown": 12.0, "castTimesMs": [100, 100],
                    "hits": [
                        {"damageRatio": [0.0], "fixedDamage": [10.0], "phase": 0, "identityGain": null},
                        {"damageRatio": [0.0], "fixedDamage": [20.0], "phase": 1, "identityGain": null}
                    ],
                    "resourceCost": null, "identityGain": null,
                    "properties": {"attackType": "", "element": ""}
                },
                {
                    "name": "대기", "skillControlType": "Normal", "skillSlot": "Normal",
                    "cooldown": 10.0, "castTimesMs": [2500],
                    "hits": [], "resourceCost": null, "identityGain": null,
                    "properties": {"attackType": "", "element": ""}
                }
            ]
        });
        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (_result, trace) = engine::trace_one(&profiles, 1, 2_700);
        let damages: Vec<f64> = trace
            .damage_events
            .iter()
            .filter(|event| event.skill_name == "리턴")
            .map(|event| event.result_damage)
            .collect();

        assert_eq!(damages, vec![10.0, 10.0]);
    }

    #[test]
    fn arcana_empress_presets_keep_rotating_for_three_minutes() {
        let names = [
            "스트림 오브 엣지",
            "스크래치 딜러",
            "스파이럴 엣지",
            "운명의 부름",
            "더 데빌",
            "셀레스티얼 레인",
            "시크릿 가든",
            "포 카드",
            "세렌디피티",
        ];
        let character_skills = names
            .iter()
            .map(|name| serde_json::json!({"name": name, "level": 1, "tripods": []}))
            .collect::<Vec<_>>();
        let skill_data = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let effect_id = 900_000 + index as u32;
                let mut hit = serde_json::json!({
                    "effectId": effect_id, "damageRatio": [0.0], "fixedDamage": [1.0],
                    "phase": 0, "identityGain": {"name": "CardGauge", "value": 500.0}
                });
                if ["스트림 오브 엣지", "스파이럴 엣지"].contains(name) {
                    hit["targetBuffStackGain"] = serde_json::json!({
                        "buffId": "arcana_ruin_stack", "stacks": 2,
                        "maxStacks": 4, "durationMs": 10000
                    });
                }
                let mut skill = serde_json::json!({
                    "name": name, "skillControlType": "Normal", "skillSlot": "Normal",
                    "cooldown": 1.0, "castTimesMs": [100], "hits": [hit],
                    "resourceCost": null, "identityGain": null,
                    "properties": {"attackType": "", "element": ""}
                });
                if *name == "더 데빌" {
                    skill["skillSlot"] = serde_json::json!("HyperAwakeningTechniques");
                }
                if *name == "스크래치 딜러" {
                    skill["skillControlType"] = serde_json::json!("Combo");
                    skill["maxPhase"] = serde_json::json!(3);
                    skill["castTimesMs"] = serde_json::json!([100, 100, 100]);
                    skill["hits"] = serde_json::json!([
                        {
                            "effectId": effect_id, "damageRatio": [0.0], "fixedDamage": [1.0],
                            "phase": 0, "targetBuffStackGain": {
                                "buffId": "arcana_ruin_stack", "stacks": 1,
                                "maxStacks": 4, "durationMs": 10000
                            }
                        },
                        {
                            "effectId": effect_id, "damageRatio": [0.0], "fixedDamage": [1.0],
                            "phase": 1, "targetBuffStackGain": {
                                "buffId": "arcana_ruin_stack", "stacks": 1,
                                "maxStacks": 4, "durationMs": 10000
                            }
                        }
                    ]);
                }
                if ["더 데빌", "셀레스티얼 레인", "시크릿 가든", "포 카드", "세렌디피티"]
                    .contains(name)
                {
                    skill["ruinTriggerEffectIds"] = serde_json::json!([effect_id]);
                    skill["runtimeDamageByTargetBuffStacks"] = serde_json::json!([{
                        "targetBuffId": "arcana_ruin_stack", "stack": 4,
                        "damageRatio": [0.0], "fixedDamage": [100.0]
                    }]);
                }
                skill
            })
            .collect::<Vec<_>>();
        let request = serde_json::json!({
            "character": {
                "name": "empress_rotation", "className": "아르카나",
                "stats": {"maxhp": 100000}, "skills": character_skills,
                "equipments": [], "accessories": [], "bracelet": null, "stone": null,
                "gems": [], "engravings": [], "arkPassive": null, "arkGrids": null,
                "arkPassiveKarma": null
            },
            "skillData": skill_data
        });
        for (preset, apl) in [
            (
                "인콤스",
                include_str!("../tests/fixtures/BluntThornStream.yaml"),
            ),
            (
                "인풀마",
                include_str!("../tests/fixtures/BluntThornStreamInpulma.yaml"),
            ),
        ] {
            let mut request = request.clone();
            request["aplConfig"] = serde_json::json!(apl);
            request["cardData"] = serde_json::from_str::<serde_json::Value>(include_str!(
                "../tests/fixtures/cards.json"
            ))
            .unwrap()["cards"]
                .clone();

            let profiles = builder::build(&request.to_string()).expect("preset should build");
            let (result, trace) = engine::trace_one(&profiles, 1, 180_000);
            let stacked_casts = trace
                .casts
                .iter()
                .filter(|cast| {
                    ["스트림 오브 엣지", "스크래치 딜러", "스파이럴 엣지"]
                        .contains(&cast.skill_name.as_str())
                })
                .count();
            let ruin_hits = trace
                .damage_events
                .iter()
                .filter(|event| event.damage_source == "target_buff_stacks:arcana_ruin_stack:4")
                .count();

            assert!(stacked_casts > 2);
            assert!(ruin_hits > 2);
            assert_eq!(
                trace
                    .casts
                    .iter()
                    .take(5)
                    .map(|cast| cast.skill_name.as_str())
                    .collect::<Vec<_>>(),
                ["스트림 오브 엣지", "스크래치 딜러", "스크래치 딜러", "운명의 부름", "더 데빌"],
                "{preset}: 기본 사이클 순서"
            );
            assert!(
                !trace.casts.windows(3).any(|casts| casts.iter().all(|cast| cast.skill_name == "스크래치 딜러")),
                "{preset}: 스크래치 딜러 3번째 입력"
            );
            assert!(
                trace.casts.last().is_some_and(|cast| cast.time >= 170.0),
                "{preset}: 스킬 시전 정체: {:?}",
                trace.casts.last()
            );
            assert!(
                trace.card_uses.last().is_some_and(|card| card.time >= 150.0),
                "{preset}: 카드 소비 정체: {:?}",
                trace.card_uses.last()
            );
            assert!(result.total_damage.is_finite() && result.total_damage > 0.0);
        }
    }

    #[test]
    fn return_exposed_target_only_doubles_followup_combo_hits() {
        let request = serde_json::json!({
            "character": {
                "name": "test_arcana",
                "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [{"name": "리턴", "level": 1, "rune": null, "tripods": []}],
                "equipments": [], "accessories": [], "bracelet": null, "stone": null,
                "gems": [], "engravings": [], "arkPassive": null, "arkGrids": null,
                "arkPassiveKarma": null
            },
            "aplConfig": "actions:\n  - skill: 리턴\n    conditions:\n      - type: combo_phase\n        skill: 리턴\n        operator: \">=\"\n        value: 1\n  - skill: 리턴\n",
            "skillData": [{
                "name": "리턴",
                "skillControlType": "Combo",
                "skillSlot": "Normal",
                "maxPhase": 3,
                "cooldown": 12.0,
                "castTimesMs": [100, 100, 100],
                "hits": [
                    {"effectId": 192602, "damageRatio": [0.0], "fixedDamage": [10.0], "phase": 0, "identityGain": null},
                    {"effectId": 192603, "damageRatio": [0.0], "fixedDamage": [20.0], "phase": 1, "identityGain": null},
                    {"effectId": 192608, "damageRatio": [0.0], "fixedDamage": [30.0], "phase": 2, "identityGain": null}
                ],
                "runtimeTargetDamageBonuses": [
                    {"triggerEffectId": 192602, "targetEffectIds": [192603], "buffId": "192610", "durationMs": 2000, "damageIncrease": 1.0},
                    {"triggerEffectId": 192602, "targetEffectIds": [192608], "buffId": "192613", "durationMs": 2000, "damageIncrease": 1.0}
                ],
                "resourceCost": null,
                "identityGain": null,
                "properties": {"attackType": "", "element": ""}
            }]
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (result, trace) = engine::trace_one(&profiles, 1, 350);
        let damages: Vec<f64> = trace.damage_events.iter().map(|event| event.result_damage).collect();

        assert_eq!(damages, vec![10.0, 40.0, 60.0]);
        assert_eq!(result.total_damage, 110.0);
    }

    #[test]
    fn arcana_stacked_skill_applies_ruin_stack_and_ruin_can_preserve_it() {
        let request = serde_json::json!({
            "character": {
                "name": "test_arcana",
                "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [
                    {"name": "쿼드라 엑셀레이트", "level": 1, "rune": null, "tripods": []},
                    {"name": "셀레스티얼 레인", "level": 1, "rune": null, "tripods": []}
                ],
                "equipments": [],
                "accessories": [],
                "bracelet": null,
                "stone": null,
                "gems": [],
                "engravings": [],
                "arkPassive": null,
                "arkGrids": null,
                "arkPassiveKarma": null
            },
            "aplConfig": "[\"쿼드라 엑셀레이트\", \"셀레스티얼 레인\"]",
            "skillData": [
                {
                    "name": "쿼드라 엑셀레이트",
                    "skillControlType": "Normal",
                    "skillSlot": "Normal",
                    "cooldown": 10.0,
                    "castTimesMs": [100],
                    "hits": [
                        {"damageRatio": [0.0], "fixedDamage": [1.0], "phase": 0, "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 1, "maxStacks": 4, "durationMs": 10000}, "identityGain": null},
                        {"damageRatio": [0.0], "fixedDamage": [1.0], "phase": 0, "identityGain": null},
                        {"damageRatio": [0.0], "fixedDamage": [1.0], "phase": 0, "identityGain": null},
                        {"damageRatio": [0.0], "fixedDamage": [1.0], "phase": 0, "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 1, "maxStacks": 4, "durationMs": 10000}, "identityGain": null}
                    ],
                    "resourceCost": null,
                    "identityGain": null,
                    "properties": {"attackType": "", "element": ""}
                },
                {
                    "name": "셀레스티얼 레인",
                    "skillControlType": "Normal",
                    "skillSlot": "Normal",
                    "cooldown": 10.0,
                    "castTimesMs": [100],
                    "hits": [
                        {"effectId": 191401, "damageRatio": [0.0], "fixedDamage": [10.0], "phase": 0, "identityGain": null}
                    ],
                    "ruinTriggerEffectIds": [191401],
                    "runtimeDamageByTargetBuffStacks": [
                        {"targetBuffId": "arcana_ruin_stack", "stack": 1, "damageRatio": [0.0], "fixedDamage": [100.0]},
                        {"targetBuffId": "arcana_ruin_stack", "stack": 2, "damageRatio": [0.0], "fixedDamage": [200.0]},
                        {"targetBuffId": "arcana_ruin_stack", "stack": 3, "damageRatio": [0.0], "fixedDamage": [300.0]},
                        {"targetBuffId": "arcana_ruin_stack", "stack": 4, "damageRatio": [0.0], "fixedDamage": [400.0]}
                    ],
                    "targetBuffConsumePreserve": {
                        "buffId": "arcana_ruin_stack",
                        "chance": 1.0,
                        "requiredStacks": 2
                    },
                    "resourceCost": null,
                    "identityGain": null,
                    "properties": {"attackType": "", "element": ""}
                }
            ]
        });
        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (_result, trace) = engine::trace_one(&profiles, 1, 250);

        assert!(trace.damage_events.iter().any(|event| {
            event.skill_name == "셀레스티얼 레인"
                && event.damage_source == "target_buff_stacks:arcana_ruin_stack:2"
                && (event.result_damage - 200.0).abs() < 1e-9
        }));
        let quadra_cast = trace
            .casts
            .iter()
            .find(|cast| cast.skill_name == "쿼드라 엑셀레이트")
            .expect("Quadra Accelerate cast should be traced");
        assert!(quadra_cast.target_debuff_details.is_empty());
        assert!(quadra_cast.target_debuff_details_after_hit.iter().any(|debuff| {
            debuff.id == "arcana_ruin_stack" && debuff.name == "스택트" && debuff.stacks == 2
        }));
        let celestial_cast = trace
            .casts
            .iter()
            .find(|cast| cast.skill_name == "셀레스티얼 레인")
            .expect("Celestial Rain cast should be traced");
        assert!(celestial_cast.target_debuff_details.iter().any(|debuff| {
            debuff.id == "arcana_ruin_stack" && debuff.stacks == 2
        }));
        assert!(celestial_cast.target_debuff_details_after_hit.iter().any(|debuff| {
            debuff.id == "arcana_ruin_stack" && debuff.stacks == 2
        }));
    }

    #[test]
    fn scratch_dealer_safety_device_applies_both_speed_stats() {
        let request = serde_json::json!({
            "character": {
                "name": "test_arcana",
                "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [
                    {"name": "스크래치 딜러", "level": 1, "rune": null, "tripods": [{"name": "안전 장치", "values": []}]},
                    {"name": "후속 스킬", "level": 1, "rune": null, "tripods": []},
                    {"name": "확인 스킬", "level": 1, "rune": null, "tripods": []}
                ],
                "equipments": [], "accessories": [], "bracelet": null, "stone": null,
                "gems": [], "engravings": [{
                    "name": "돌격대장", "grade": "유물", "level": 4,
                    "featureData": {"grades": [{"grade": 5, "stages": [
                        {"stage": 4, "featureType": 121, "values": [5300]}
                    ]}]}
                }], "arkPassive": null, "arkGrids": null,
                "arkPassiveKarma": null
            },
            "aplConfig": "actions:\n  - skill: 스크래치 딜러\n  - skill: 후속 스킬\n  - skill: 확인 스킬\n",
            "buffData": [{
                "id": "193206", "name": "안전 장치", "icon": "https://example.com/buff.png",
                "source": "스크래치 딜러 · 안전 장치"
            }],
            "skillData": [
                {
                    "name": "스크래치 딜러", "cooldown": 100.0,
                    "castTimesMs": [1000], "maxStacks": 1,
                    "selfSpeedBuffOnCast": {
                        "buffId": "193206", "durationMs": 5000,
                        "attackSpeedPercent": 19.2, "moveSpeedPercent": 30.0
                    },
                    "hits": [{"damageRatio": [0.0], "fixedDamage": [100.0]}]
                },
                {
                    "name": "후속 스킬", "cooldown": 100.0,
                    "castTimesMs": [1000], "maxStacks": 1,
                    "hits": [{"damageRatio": [0.0], "fixedDamage": [100.0]}]
                },
                {
                    "name": "확인 스킬", "cooldown": 100.0,
                    "castTimesMs": [1000], "maxStacks": 1
                }
            ]
        });
        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (_result, trace) = engine::trace_one(&profiles, 1, 1900);
        let cast_times: Vec<f64> = trace.casts.iter().map(|cast| cast.time).collect();

        assert_eq!(cast_times, vec![0.0, 1.0, 1.838]);
        let safety_device = trace.casts[1]
            .active_buff_details
            .iter()
            .find(|buff| buff.id == "193206")
            .expect("safety device should be traced");
        assert_eq!(safety_device.name, "안전 장치");
        assert_eq!(safety_device.icon, "https://example.com/buff.png");
        assert_eq!(safety_device.source, "스크래치 딜러 · 안전 장치");
        assert!((trace.damage_events[0]
            .formula
            .movement_speed_damage_multiplier
            - 1.159)
            .abs()
            < 1e-9);
    }

    #[test]
    fn mana_shower_restores_twelve_percent_of_maximum_mp_on_hit() {
        let request = serde_json::json!({
            "character": {
                "name": "test_arcana", "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [
                    {"name": "백 플러쉬", "level": 1, "rune": null, "tripods": []},
                    {"name": "후속 스킬", "level": 1, "rune": null, "tripods": []}
                ],
                "equipments": [],
                "accessories": [{
                    "baseStats": {},
                    "polishingEffects": [{"type": "최대 마나", "value": 200, "isPercentage": false}]
                }],
                "bracelet": null, "stone": null, "gems": [], "engravings": [],
                "arkPassive": null, "arkGrids": null, "arkPassiveKarma": null
            },
            "aplConfig": "actions:\n  - skill: 백 플러쉬\n  - skill: 후속 스킬\n",
            "skillData": [
                {
                    "name": "백 플러쉬", "cooldown": 100.0, "castTimesMs": [100],
                    "resourceCost": {"name": "Mp", "cost": [200]},
                    "hits": [{
                        "damageRatio": [0.0], "fixedDamage": [1.0],
                        "mpRestorePercent": 0.12
                    }]
                },
                {
                    "name": "후속 스킬", "cooldown": 100.0, "castTimesMs": [100],
                    "resourceCost": {"name": "Mp", "cost": [24]},
                    "hits": [{"damageRatio": [0.0], "fixedDamage": [1.0]}]
                }
            ]
        });
        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (_result, trace) = engine::trace_one(&profiles, 1, 250);

        let cast_names: Vec<&str> = trace.casts.iter().map(|cast| cast.skill_name.as_str()).collect();
        assert_eq!(cast_names, vec!["백 플러쉬", "후속 스킬"]);
    }

    #[test]
    fn resource_cost_waiver_skips_the_cast_cost_once() {
        let request = serde_json::json!({
            "character": {
                "name": "test_arcana", "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [
                    {"name": "셀레스티얼 레인", "level": 1, "rune": null, "tripods": []},
                    {"name": "후속 스킬", "level": 1, "rune": null, "tripods": []}
                ],
                "equipments": [],
                "accessories": [{
                    "baseStats": {},
                    "polishingEffects": [{"type": "최대 마나", "value": 200, "isPercentage": false}]
                }],
                "bracelet": null, "stone": null, "gems": [], "engravings": [],
                "arkPassive": null, "arkGrids": null, "arkPassiveKarma": null
            },
            "aplConfig": "actions:\n  - skill: 셀레스티얼 레인\n  - skill: 후속 스킬\n",
            "skillData": [
                {
                    "name": "셀레스티얼 레인", "cooldown": 100.0, "castTimesMs": [100],
                    "resourceCost": {"name": "Mp", "cost": [200]},
                    "mpCostWaiverChance": 1.0
                },
                {
                    "name": "후속 스킬", "cooldown": 100.0, "castTimesMs": [100],
                    "resourceCost": {"name": "Mp", "cost": [200]}
                }
            ]
        });
        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (_result, trace) = engine::trace_one(&profiles, 1, 250);

        let cast_names: Vec<&str> = trace.casts.iter().map(|cast| cast.skill_name.as_str()).collect();
        assert_eq!(cast_names, vec!["셀레스티얼 레인", "후속 스킬"]);
    }

    #[test]
    fn empress_grace_refunds_only_mp_paid_by_that_ruin_cast() {
        let request = |waiver: f64| serde_json::json!({
            "character": {
                "name": "test_arcana", "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [
                    {"name": "셀레스티얼 레인", "level": 1, "rune": null, "tripods": []},
                    {"name": "마나 소진", "level": 1, "rune": null, "tripods": []},
                    {"name": "환급 확인", "level": 1, "rune": null, "tripods": []}
                ],
                "equipments": [],
                "accessories": [{
                    "baseStats": {},
                    "polishingEffects": [{"type": "최대 마나", "value": 200, "isPercentage": false}]
                }],
                "bracelet": null, "stone": null, "gems": [], "engravings": [],
                "arkPassive": {"commonNodes": [], "classNodes": [
                    {"name": "황후의 은총", "level": 3, "values": [50.0]}
                ]},
                "arkGrids": null, "arkPassiveKarma": null
            },
            "aplConfig": "actions:\n  - skill: 셀레스티얼 레인\n  - skill: 마나 소진\n  - skill: 환급 확인\n",
            "skillData": [
                {
                    "name": "셀레스티얼 레인", "identityCategory": 30,
                    "cooldown": 100.0, "castTimesMs": [100],
                    "resourceCost": {"name": "Mp", "cost": [200]},
                    "mpCostWaiverChance": waiver,
                    "ruinTriggerEffectIds": [1],
                    "runtimeDamageByTargetBuffStacks": [{
                        "targetBuffId": "arcana_ruin_stack", "stack": 1,
                        "damageRatio": [0.0], "fixedDamage": [1.0]
                    }],
                    "hits": [{
                        "effectId": 1, "actionDelayMs": 150,
                        "damageRatio": [0.0], "fixedDamage": [1.0]
                    }]
                },
                {
                    "name": "마나 소진", "cooldown": 100.0, "castTimesMs": [100],
                    "resourceCost": {"name": "Mp", "cost": [200]}
                },
                {
                    "name": "환급 확인", "cooldown": 100.0, "castTimesMs": [100],
                    "resourceCost": {"name": "Mp", "cost": [100]}
                }
            ]
        });

        let paid = builder::build(&request(0.0).to_string()).unwrap();
        assert!(paid.skills[0].hits[0].triggers.iter().any(|trigger| matches!(
            trigger.effect,
            TriggerEffect::RefundCastMp { percent } if (percent - 0.5).abs() < 1e-9
        )));
        let (_, paid_trace) = engine::trace_one(&paid, 1, 250);
        assert_eq!(
            paid_trace.casts.iter().map(|cast| cast.skill_name.as_str()).collect::<Vec<_>>(),
            vec!["셀레스티얼 레인", "환급 확인"]
        );

        let waived = builder::build(&request(1.0).to_string()).unwrap();
        let (_, waived_trace) = engine::trace_one(&waived, 1, 250);
        assert_eq!(
            waived_trace.casts.iter().map(|cast| cast.skill_name.as_str()).collect::<Vec<_>>(),
            vec!["셀레스티얼 레인", "마나 소진"]
        );
    }

    #[test]
    fn arcana_scratch_dealer_applies_or_sets_stacks_once_per_combo_phase() {
        let request = serde_json::json!({
            "character": {
                "name": "test_arcana",
                "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [
                    {"name": "스크래치 딜러", "level": 1, "rune": null, "tripods": [{"name": "연속 공격", "values": []}]}
                ],
                "equipments": [],
                "accessories": [],
                "bracelet": null,
                "stone": null,
                "gems": [],
                "engravings": [],
                "arkPassive": null,
                "arkGrids": null,
                "arkPassiveKarma": null
            },
            "aplConfig": "actions:\n  - skill: 스크래치 딜러\n    conditions:\n      - type: combo_phase\n        skill: 스크래치 딜러\n        operator: \">=\"\n        value: 1\n  - skill: 스크래치 딜러\n",
            "skillData": [
                {
                    "name": "스크래치 딜러",
                    "skillControlType": "Combo",
                    "skillSlot": "Normal",
                    "maxPhase": 3,
                    "cooldown": 10.0,
                    "castTimesMs": [100, 100, 100],
                    "hits": [
                        {"damageRatio": [0.0], "fixedDamage": [10.0], "phase": 0, "identityGain": null},
                        {"damageRatio": [0.0], "fixedDamage": [10.0], "phase": 0, "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 1, "maxStacks": 4, "durationMs": 10000}, "identityGain": null},
                        {"damageRatio": [0.0], "fixedDamage": [20.0], "phase": 1, "targetBuffStackGain": {"buffId": "arcana_ruin_stack", "stacks": 1, "maxStacks": 4, "durationMs": 10000}, "identityGain": null},
                        {
                            "damageRatio": [0.0],
                            "fixedDamage": [30.0],
                            "phase": 2,
                            "targetBuffStackSet": {
                                "buffId": "arcana_ruin_stack",
                                "stacks": 2,
                                "maxStacks": 4,
                                "durationMs": 10000
                            },
                            "identityGain": null
                        }
                    ],
                    "resourceCost": null,
                    "identityGain": null,
                    "properties": {"attackType": "", "element": ""}
                }
            ]
        });
        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (_result, trace) = engine::trace_one(&profiles, 1, 250);
        let scratch_cast_stacks: Vec<u32> = trace
            .casts
            .iter()
            .filter(|cast| cast.skill_name == "스크래치 딜러")
            .map(|cast| {
                cast.target_debuff_details
                    .iter()
                    .find(|debuff| debuff.id == "arcana_ruin_stack")
                    .map(|debuff| debuff.stacks)
                    .unwrap_or(0)
            })
            .collect();

        assert_eq!(scratch_cast_stacks, vec![1, 2, 2]);
    }

    #[test]
    fn runtime_damage_by_target_buff_stacks_is_recorded_as_skill_damage() {
        let stack_buff = BuffSpec {
            id: "arcana_ruin_stack".to_string(),
            effects: HashMap::new(),
            max_stacks: 4,
            stack_type: StackType::Refresh,
            duration_ms: 10_000,
        };
        let stack_skill = SkillProfile {
            skill_id: 0,
            name: "stack".to_string(),
            skill_type: SkillType::Normal,
            category: SkillCategory::Stacked,
            slot: SkillSlot::Normal,
            tags: vec![SkillTag::CategoryStacked],
            cooldown_ms: 100_000,
            cast_times_ms: vec![1000],
            max_uses: 0,
            max_stacks: 1,
            charge_recovery_ms: 0,
            resource_costs: HashMap::new(),
            extra_mp_cost_ratio: 0.0,
            mp_cost_waiver_chance: 0.0,
            resource_gains: HashMap::new(),
            cast_buff_damage_bonuses: Vec::new(),
            cast_buff_crit_rate_bonuses: Vec::new(),
            evolution_damage_bonuses: Vec::new(),
            hits: vec![SkillHit {
                effect_id: 0,
                hit_id: "stack_hit".to_string(),
                name: "스택 타격".to_string(),
                base_damage: 0.0,
                skill_modifier: 0.0,
                action_delay_ms: 0,
                fixed_delay_ms: 0,
                counter_attack: false,
                phase: 0,
                crit_chance_bonus: 0.0,
                crit_damage_bonus: 0.0,
                defense_ignore: 0.0,
                defense_ignore_chance: 0.0,
                damage_increase: 0.0,
                stagger: 0.0,
                destruction: 0,
                use_snapshot: false,
                triggers: vec![HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::ApplyTargetBuffStacks {
                        spec: stack_buff,
                        stacks: 2,
                    },
                    chance: 1.0,
                }],
            }],
        };
        let ruin_skill = SkillProfile {
            skill_id: 1,
            name: "ruin".to_string(),
            skill_type: SkillType::Normal,
            category: SkillCategory::Ruin,
            slot: SkillSlot::Normal,
            tags: vec![SkillTag::CategoryRuin],
            cooldown_ms: 100_000,
            cast_times_ms: vec![1000],
            max_uses: 0,
            max_stacks: 1,
            charge_recovery_ms: 0,
            resource_costs: HashMap::new(),
            extra_mp_cost_ratio: 0.0,
            mp_cost_waiver_chance: 0.0,
            resource_gains: HashMap::new(),
            cast_buff_damage_bonuses: Vec::new(),
            cast_buff_crit_rate_bonuses: Vec::new(),
            evolution_damage_bonuses: Vec::new(),
            hits: vec![SkillHit {
                effect_id: 0,
                hit_id: "ruin_trigger".to_string(),
                name: "루인 발동 타격".to_string(),
                base_damage: 0.0,
                skill_modifier: 0.0,
                action_delay_ms: 0,
                fixed_delay_ms: 0,
                counter_attack: false,
                phase: 0,
                crit_chance_bonus: 0.0,
                crit_damage_bonus: 0.0,
                defense_ignore: 0.0,
                defense_ignore_chance: 0.0,
                damage_increase: 0.0,
                stagger: 0.0,
                destruction: 0,
                use_snapshot: false,
                triggers: vec![HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::DealDamageByTargetBuffStacks {
                        buff_id: "arcana_ruin_stack".to_string(),
                        hits_by_stack: vec![
                            RuntimeDamageSpec {
                                hit_id: "ruin_damage_stack_1".to_string(),
                                name: "루인 피해 (1스택)".to_string(),
                                base_damage: 100.0,
                                skill_modifier: 0.0,
                                damage_increase: 0.0,
                                crit_chance_bonus: 0.0,
                                crit_damage_bonus: 0.0,
                                random_crit_damage_chance: 0.0,
                                random_crit_damage_bonus: 0.0,
                                defense_ignore: 0.0,
                                defense_ignore_chance: 0.0,
                            },
                            RuntimeDamageSpec {
                                hit_id: "ruin_damage_stack_2".to_string(),
                                name: "루인 피해 (2스택)".to_string(),
                                base_damage: 250.0,
                                skill_modifier: 0.0,
                                damage_increase: 0.0,
                                crit_chance_bonus: 1.0,
                                crit_damage_bonus: 0.0,
                                random_crit_damage_chance: 1.0,
                                random_crit_damage_bonus: 5.04,
                                defense_ignore: 0.0,
                                defense_ignore_chance: 0.0,
                            },
                        ],
                        forced_stacks_while_buff: None,
                    },
                    chance: 1.0,
                }],
            }],
        };
        let profiles = CharacterProfiles {
            name: "runtime_damage_test".to_string(),
            class_name: "아르카나".to_string(),
            stat: StatProfile::default(),
            skills: vec![stack_skill, ruin_skill],
            max_hp: 1000.0,
            max_mp: 0.0,
            target_defense: 0.0,
            global_triggers: Vec::new(),
            apl_order: vec![0, 1],
            apl_actions: Vec::new(),
            card_apl_actions: Vec::new(),
            arcana_card_pool: Vec::new(),
            arcana_card_names: HashMap::new(),
            initial_card_draws: 0,
            arcana_card_effects: HashMap::new(),
            arcana_card_blocking_buffs: HashMap::new(),
            buff_metadata: HashMap::new(),
        };

        let (result, trace) = engine::trace_one(&profiles, 1, 2500);
        let ruin_stat = result.skill_stats.get(&1).expect("ruin skill should hit");
        let ruin_damage = trace
            .damage_events
            .iter()
            .find(|event| event.hit_id == "ruin_damage_stack_2")
            .expect("ruin damage name tag should reach trace");

        assert_eq!(ruin_stat.total_damage, 1760.0);
        assert_eq!(ruin_damage.hit_name, "루인 피해 (2스택)");
    }

    #[test]
    fn trace_one_records_damage_formula_for_each_hit() {
        let mut profiles = builder::build(&arcana_single_hit_skill_request("셀레스티얼 레인"))
            .expect("build should succeed");
        profiles
            .stat
            .insert(StatField::DamageIncrease, "test_damage_source", 0.2);
        let trace = engine::trace_one(&profiles, 1, 1500).1;
        let damage = trace
            .damage_events
            .first()
            .expect("trace should record a damage event");

        assert_eq!(damage.skill_name, "셀레스티얼 레인");
        assert_eq!(damage.hit_index, 0);
        assert_eq!(damage.formula.base_damage, 10.0);
        assert_eq!(damage.formula.base_term, 10.0);
        assert_eq!(damage.result_damage, 12.0);
        assert!(damage.formula.factors.iter().any(|factor| {
            factor.name == "damageIncrease"
                && (factor.value - 1.2).abs() < 1e-9
                && factor.sources.iter().any(|source| {
                    source.source == "test_damage_source" && (source.value - 0.2).abs() < 1e-9
                })
        }));
    }

    #[test]
    fn target_defense_and_skill_defense_ignore_change_damage() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("운명의 부름"))
                .expect("request should parse");
        request["targetDefense"] = serde_json::json!(1606.0);
        request["skillData"][0]["hits"][0]["defenseIgnore"] = serde_json::json!(0.6);
        request["skillData"][0]["hits"][0]["defenseIgnoreChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let damage = engine::trace_one(&profiles, 1, 1500)
            .1
            .damage_events
            .remove(0);
        let expected_multiplier = 6500.0 / (6500.0 + 1606.0 * 0.4);

        assert!((damage.result_damage - 10.0 * expected_multiplier).abs() < 1e-9);
        assert!((damage.formula.defense_multiplier - expected_multiplier).abs() < 1e-9);
    }

    #[test]
    fn installed_hit_keeps_fixed_delay_after_actor_is_idle() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("다크 리저렉션"))
                .expect("request should parse");
        let skill = &mut request["skillData"][0];
        skill["cooldown"] = serde_json::json!(10.0);
        skill["hits"][0]["actionDelayMs"] = serde_json::json!(500);
        skill["hits"][0]["fixedDelayMs"] = serde_json::json!(1000);

        let mut profiles = builder::build(&request.to_string()).expect("build should succeed");
        profiles
            .stat
            .insert(StatField::AttackSpeed, "test_attack_speed", 1.0);
        let trace = engine::trace_one(&profiles, 1, 1400).1;
        let damage = trace
            .damage_events
            .first()
            .expect("installed hit should remain scheduled");

        // raw 공속 상한 140%: 500ms Action delay / 1.4 + fixed 1000ms.
        assert!((damage.time - 1.357).abs() < 1e-9);
    }

    #[test]
    fn skill_data_runtime_damage_by_target_buff_stacks_reaches_hit_trigger() {
        let profiles = builder::build(&arcana_celestial_with_runtime_damage_input_request())
            .expect("build should succeed");
        let celestial = profiles
            .skills
            .iter()
            .find(|skill| skill.name == "셀레스티얼 레인")
            .expect("Celestial Rain profile should exist");

        assert!(celestial.hits[0].triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::DealDamageByTargetBuffStacks { buff_id, hits_by_stack, .. }
                if buff_id == "arcana_ruin_stack"
                    && hits_by_stack.len() == 2
                    && (hits_by_stack[1].base_damage - 222.0).abs() < 1e-9
        )));
    }

    #[test]
    fn skill_data_draw_card_reaches_hit_trigger() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_celestial_with_runtime_damage_input_request(),
        ).expect("request should be valid JSON");
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(2);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(0.4);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let triggers = &profiles.skills[0].hits[0].triggers;
        assert_eq!(triggers.iter().filter(|trigger| matches!(
            trigger.effect,
            TriggerEffect::DrawCard
        ) && (trigger.chance - 0.4).abs() < 1e-9).count(), 2);
    }

    #[test]
    fn trace_keeps_cards_drawn_by_each_cast() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_single_hit_skill_request("셀레스티얼 레인"),
        ).expect("request should be valid JSON");
        request["aplConfig"] = serde_json::json!("skills:\n  - 셀레스티얼 레인");
        request["cardData"] = serde_json::json!([{
            "skillId": 19098, "name": "심판", "autoLearn": true, "drawWeight": 1
        }]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 100).1;

        assert!(trace.casts[0].held_cards.is_empty());
        assert_eq!(trace.damage_events[0].held_cards_after_hit[0].card_name, "심판");
    }

    #[test]
    fn card_apl_uses_a_drawn_card_without_delaying_skill_casts() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_single_hit_skill_request("셀레스티얼 레인"),
        ).expect("request should be valid JSON");
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 심판"
        );
        request["cardData"] = serde_json::json!([
            {"skillId": 19098, "name": "심판", "autoLearn": true}
        ]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 1000).1;

        assert_eq!(trace.card_uses.len(), 2);
        assert_eq!(trace.card_uses[0].card_name, "심판");
        assert_eq!(trace.card_uses[0].time, 0.0);
        assert_eq!(trace.card_uses[1].time, 1.0);
        assert_eq!(trace.casts.len(), 2);
    }

    #[test]
    fn card_apl_can_wait_for_another_card_in_hand() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("셀레스티얼 레인"))
                .expect("request should be valid JSON");
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 심판\n    conditions:\n      - type: card_held\n        card: 균형"
        );
        request["cardData"] = serde_json::json!([
            {"skillId": 19098, "name": "심판", "autoLearn": true, "drawWeight": 1},
            {"skillId": 19097, "name": "균형", "autoLearn": true, "drawWeight": 0.000001}
        ]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 1000).1;

        assert_eq!(trace.card_uses.len(), 1);
        assert_eq!(trace.card_uses[0].card_name, "심판");
        assert_eq!(trace.card_uses[0].time, 1.0);
    }

    #[test]
    fn card_apl_uses_cull_and_judgment_together_at_four_stacks() {
        let mut request: serde_json::Value =
            serde_json::from_str(&arcana_single_hit_skill_request("스크래치 딜러"))
                .expect("request should be valid JSON");
        request["enableAwakeningPotion"] = serde_json::json!(true);
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 스크래치 딜러\ncard_actions:\n  - card: 도태\n    conditions:\n      - type: card_held\n        card: 심판\n      - type: target_debuff_stacks\n        debuff: ruin\n        operator: '>='\n        value: 4\n  - card: 심판\n    conditions:\n      - type: buff_active\n        buff: '192810'"
        );
        request["skillData"][0]["hits"][0]["targetBuffStackGain"] = serde_json::json!({
            "buffId": "arcana_ruin_stack", "stacks": 4, "maxStacks": 4, "durationMs": 10000
        });
        request["cardData"] = serde_json::json!([
            {
                "skillId": 19281, "name": "도태", "autoLearn": true, "drawWeight": 1,
                "runtimeEffects": [{
                    "kind": "self_stat_buff", "buffId": 192810, "durationMs": 4000,
                    "critRatePercent": 100, "critDamagePercent": 50
                }]
            },
            {
                "skillId": 19098, "name": "심판", "autoLearn": true, "drawWeight": 1,
                "runtimeEffects": [{
                    "kind": "force_ruin_stack_damage_buff", "buffId": 190980,
                    "durationMs": 4000, "targetBuffKey": "arcana_ruin_stack", "maxStacks": 4
                }]
            }
        ]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 1000).1;

        assert_eq!(trace.card_uses.len(), 2);
        assert_eq!(trace.card_uses[0].card_name, "도태");
        assert_eq!(trace.card_uses[1].card_name, "심판");
        assert_eq!(trace.card_uses[0].time, trace.card_uses[1].time);
    }

    #[test]
    fn awakening_potion_draws_two_arcana_cards_before_combat() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_single_hit_skill_request("셀레스티얼 레인"),
        ).expect("request should be valid JSON");
        request["enableAwakeningPotion"] = serde_json::json!(true);
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 심판\n  - card: 균형"
        );
        request["cardData"] = serde_json::json!([
            {"skillId": 19098, "name": "심판", "autoLearn": true, "drawWeight": 1},
            {"skillId": 19097, "name": "균형", "autoLearn": true, "drawWeight": 1}
        ]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 0).1;

        assert_eq!(profiles.initial_card_draws, 2);
        assert_eq!(trace.card_uses.len(), 2);
        assert!(trace.card_uses.iter().all(|card| card.time == 0.0));
    }

    #[test]
    fn star_card_waives_mana_cost_then_restores_half_max_mana() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_single_hit_skill_request("셀레스티얼 레인"),
        )
        .expect("request should be valid JSON");
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\n  - 운명의 부름\ncard_actions:\n  - card: 별"
        );
        request["character"]["skills"] = serde_json::json!([
            {"name": "셀레스티얼 레인", "level": 1, "rune": null, "tripods": []},
            {"name": "운명의 부름", "level": 1, "rune": null, "tripods": []}
        ]);
        request["skillData"][0]["cooldown"] = serde_json::json!(100.0);
        request["skillData"][0]["resourceCost"] =
            serde_json::json!({"name": "Mp", "cost": [100.0]});
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);
        let mut second = request["skillData"][0].clone();
        second["name"] = serde_json::json!("운명의 부름");
        second["cooldown"] = serde_json::json!(1.0);
        second["resourceCost"] = serde_json::json!({"name": "Mp", "cost": [50.0]});
        second["hits"][0]["drawCardCount"] = serde_json::json!(0);
        request["skillData"].as_array_mut().unwrap().push(second);
        request["cardData"] = serde_json::json!([{
            "skillId": 19094,
            "name": "별",
            "autoLearn": true,
            "runtimeEffects": [
                {"kind": "mana_cost_freeze_buff", "buffId": 190941, "durationMs": 3000},
                {"kind": "restore_max_mana_percent", "delayMs": 3000, "valuePercent": 50}
            ]
        }]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 5000).1;
        let destiny_casts = trace
            .casts
            .iter()
            .filter(|cast| cast.skill_name == "운명의 부름")
            .map(|cast| cast.time)
            .collect::<Vec<_>>();

        assert_eq!(trace.card_uses[0].time, 0.0);
        assert_eq!(destiny_casts, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn card_use_can_trigger_destiny_effects() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [{
                    "name": "체인 드로우", "points": 14,
                    "options": [{
                        "pointRequirement": 14, "rawOptionId": 3193600,
                        "runtimeEffects": [
                            {"kind": "destiny_on_card_use", "cardSkillGroupId": 2190903, "chancePercent": 100},
                            {"kind": "on_destiny_damage_buff", "buffId": 3192600, "durationMs": 5000, "valuePercent": 10}
                        ]
                    }]
                }]
            }"#,
        )).expect("request should be valid JSON");
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 심판"
        );
        request["cardData"] = serde_json::json!([
            {"skillId": 19098, "name": "심판", "autoLearn": true}
        ]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 1000).1;

        assert_eq!(trace.card_uses.len(), 2);
        assert!((trace.damage_events[1].result_damage
            / trace.damage_events[0].result_damage - 1.1).abs() < 1e-9);
    }

    #[test]
    fn stack_hold_triggers_destiny_on_every_second_card_use() {
        let mut request: serde_json::Value = serde_json::from_str(&base_arcana_request(
            "[]",
            "null",
            r#"{
                "gems": [], "commonCores": [],
                "classCores": [{
                    "name": "스택 홀드", "points": 14,
                    "options": [{
                        "pointRequirement": 14, "rawOptionId": 3195100,
                        "runtimeEffects": [
                            {"kind": "destiny_after_card_uses", "hitCount": 2},
                            {"kind": "on_destiny_damage_buff", "buffId": 3192600, "durationMs": 5000, "valuePercent": 10}
                        ]
                    }]
                }]
            }"#,
        )).expect("request should be valid JSON");
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 심판"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19098, "name": "심판", "autoLearn": true
        }]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 2000).1;

        assert_eq!(trace.card_uses.len(), 3);
        assert_eq!(trace.damage_events[1].result_damage, trace.damage_events[0].result_damage);
        assert!((trace.damage_events[2].result_damage
            / trace.damage_events[0].result_damage - 1.1).abs() < 1e-9);
    }

    #[test]
    fn cull_card_effect_applies_to_the_next_hit() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_single_hit_skill_request("셀레스티얼 레인"),
        ).expect("request should be valid JSON");
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 도태"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19281,
            "name": "도태",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "self_stat_buff",
                "buffId": 192810,
                "durationMs": 4000,
                "critRatePercent": 100,
                "critDamagePercent": 50
            }]
        }]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 1000).1;

        assert!(trace.damage_events[0].formula.crit_multiplier.is_none());
        assert_eq!(trace.damage_events[1].formula.crit_multiplier, Some(2.5));
        assert!((trace.damage_events[1].result_damage
            / trace.damage_events[0].result_damage - 2.5).abs() < 1e-9);
    }

    #[test]
    fn moon_card_reduces_cooldowns_started_while_active() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_single_hit_skill_request("셀레스티얼 레인"),
        ).expect("request should be valid JSON");
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 달"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19092,
            "name": "달",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "self_cooldown_reduction_buff",
                "buffId": 190920,
                "durationMs": 30000,
                "cooldownReductionPercent": 20
            }]
        }]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 1800).1;

        assert_eq!(
            trace.casts.iter().map(|cast| cast.time).collect::<Vec<_>>(),
            vec![0.0, 1.0, 1.8]
        );
    }

    #[test]
    fn corrosion_card_debuff_affects_hits_after_the_first_proc() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_single_hit_skill_request("셀레스티얼 레인"),
        ).expect("request should be valid JSON");
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 부식"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19093,
            "name": "부식",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "self_on_hit_target_damage_buff",
                "buffId": 190930,
                "durationMs": 30000,
                "chancePercent": 100,
                "targetBuffId": 190931,
                "targetDurationMs": 5000,
                "valuePercent": 10
            }]
        }]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 2000).1;

        assert!((trace.damage_events[1].result_damage
            / trace.damage_events[0].result_damage - 1.0).abs() < 1e-9);
        assert!((trace.damage_events[2].result_damage
            / trace.damage_events[1].result_damage - 1.1).abs() < 1e-9);
    }

    #[test]
    fn madness_card_stacks_cast_speed_for_following_casts() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_single_hit_skill_request("셀레스티얼 레인"),
        ).expect("request should be valid JSON");
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 광기"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19280,
            "name": "광기",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "self_on_skill_cast_stack_buff",
                "buffId": 192800,
                "durationMs": 30000,
                "stackBuffId": 192801,
                "stackDurationMs": 4000,
                "maxStacks": 3,
                "attackSpeedPercent": 5
            }]
        }]);
        request["skillData"][0]["cooldown"] = serde_json::json!(0.1);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 2952).1;

        assert_eq!(
            trace.casts.iter().map(|cast| cast.time).collect::<Vec<_>>(),
            vec![0.0, 1.0, 2.0, 2.952]
        );
    }

    #[test]
    fn twisted_fate_card_applies_one_exclusive_damage_result() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_single_hit_skill_request("셀레스티얼 레인"),
        ).expect("request should be valid JSON");
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 뒤틀린 운명"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19091,
            "name": "뒤틀린 운명",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "random_self_damage_buff",
                "uniqueGroupId": 190900,
                "durationMs": 4000,
                "resultBuffIds": [190916],
                "damagePercents": [40]
            }]
        }]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 1000).1;

        assert!((trace.damage_events[1].result_damage
            / trace.damage_events[0].result_damage - 1.4).abs() < 1e-9);
    }

    #[test]
    fn joy_card_reduces_current_normal_skill_cooldowns() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_single_hit_skill_request("셀레스티얼 레인"),
        ).expect("request should be valid JSON");
        request["aplConfig"] = serde_json::json!(
            "skills:\n  - 셀레스티얼 레인\ncard_actions:\n  - card: 환희"
        );
        request["cardData"] = serde_json::json!([{
            "skillId": 19285,
            "name": "환희",
            "autoLearn": true,
            "runtimeEffects": [{
                "kind": "random_reduce_cooldowns",
                "effectIds": [192838],
                "cooldownReductionPercents": [30]
            }]
        }]);
        request["skillData"][0]["hits"][0]["drawCardCount"] = serde_json::json!(1);
        request["skillData"][0]["hits"][0]["drawCardChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 700).1;

        assert_eq!(
            trace.casts.iter().map(|cast| cast.time).collect::<Vec<_>>(),
            vec![0.0, 0.7]
        );
    }

    #[test]
    fn target_crit_rate_debuff_affects_the_next_hit() {
        let mut request: serde_json::Value = serde_json::from_str(
            &base_arcana_request("[]", "null", "null"),
        ).expect("request should be valid JSON");
        request["skillData"][0]["cooldown"] = serde_json::json!(10.0);
        request["skillData"][0]["castTimesMs"] = serde_json::json!([200]);
        request["skillData"][0]["hits"] = serde_json::json!([
            {
                "effectId": 1,
                "damageRatio": [0.0],
                "fixedDamage": [100.0],
                "fixedDelayMs": 0,
                "targetCritRateDebuff": {
                    "buffId": "test_crit_debuff",
                    "percent": 100.0,
                    "durationMs": 1000
                },
                "identityGain": null
            },
            {
                "effectId": 2,
                "damageRatio": [0.0],
                "fixedDamage": [100.0],
                "fixedDelayMs": 100,
                "targetCritRateDebuff": {
                    "buffId": "test_crit_debuff",
                    "percent": 100.0,
                    "durationMs": 1000
                },
                "identityGain": null
            },
            {
                "effectId": 3,
                "damageRatio": [0.0],
                "fixedDamage": [100.0],
                "fixedDelayMs": 1050,
                "identityGain": null
            }
        ]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (result, trace) = engine::trace_one(&profiles, 1, 1100);

        assert_eq!(trace.damage_events.len(), 3);
        assert!(!trace.damage_events[0].is_crit);
        assert!(trace.damage_events[1].is_crit);
        assert!(trace.damage_events[2].is_crit);
        assert_eq!(result.crit_count, 2);
    }

    #[test]
    fn skill_data_self_crit_rate_stack_reaches_hit_trigger() {
        let mut request: serde_json::Value = serde_json::from_str(
            &base_arcana_request("[]", "null", "null"),
        ).expect("request should be valid JSON");
        request["skillData"][0]["hits"][0]["selfCritRateStack"] = serde_json::json!({
            "buffId": "191542",
            "percentPerStack": 5.52,
            "maxStacks": 5,
            "durationMs": 3000
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert!(profiles.skills[0].hits[0].triggers.iter().any(|trigger| matches!(
            &trigger.effect,
            TriggerEffect::ApplyBuff(spec)
                if spec.id == "191542"
                    && spec.max_stacks == 5
                    && spec.duration_ms == 3000
                    && (spec.effects[&StatField::CritRate] - 5.52).abs() < 1e-9
        )));
    }

    #[test]
    fn self_crit_damage_buff_affects_the_next_hit() {
        let mut request: serde_json::Value = serde_json::from_str(
            &base_arcana_request("[]", "null", "null"),
        ).expect("request should be valid JSON");
        request["skillData"][0]["cooldown"] = serde_json::json!(10.0);
        request["skillData"][0]["hits"] = serde_json::json!([
            {
                "effectId": 1,
                "damageRatio": [0.0],
                "fixedDamage": [100.0],
                "selfCritDamageBuff": {
                    "buffId": "191826",
                    "percent": 77.5,
                    "durationMs": 5000
                },
                "identityGain": null
            },
            {
                "effectId": 2,
                "damageRatio": [0.0],
                "fixedDamage": [100.0],
                "fixedDelayMs": 1,
                "identityGain": null
            }
        ]);

        let mut profiles = builder::build(&request.to_string()).expect("build should succeed");
        profiles.stat.insert(StatField::CritRate, "test", 100.0);
        let trace = engine::trace_one(&profiles, 1, 1).1;

        assert!((trace.damage_events[0].result_damage - 200.0).abs() < 1e-9);
        assert!((trace.damage_events[1].result_damage - 277.5).abs() < 1e-9);
    }

    #[test]
    fn skill_data_cooldown_reset_allows_the_next_cast() {
        let mut request: serde_json::Value = serde_json::from_str(
            &base_arcana_request("[]", "null", "null"),
        ).expect("request should be valid JSON");
        request["skillData"][0]["cooldown"] = serde_json::json!(10.0);
        request["skillData"][0]["castTimesMs"] = serde_json::json!([100]);
        request["skillData"][0]["hits"][0]["resetCooldownChance"] = serde_json::json!(1.0);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 250).1;
        let casts = trace.casts.iter()
            .filter(|cast| cast.skill_name == "셀레스티얼 레인")
            .count();

        assert_eq!(casts, 3);
    }

    #[test]
    fn skill_data_additional_damage_sets_target_stacks() {
        let mut request: serde_json::Value = serde_json::from_str(
            &base_arcana_request("[]", "null", "null"),
        ).expect("request should be valid JSON");
        request["skillData"][0]["cooldown"] = serde_json::json!(10.0);
        request["skillData"][0]["hits"][0]["additionalDamage"] = serde_json::json!({
            "effectId": 191704,
            "hitId": "card_increase_additional_hit_1",
            "name": "카드 증가 추가 피해 1",
            "chance": 1.0,
            "damageRatio": [0.0],
            "fixedDamage": [100.0],
            "targetBuffId": "arcana_ruin_stack",
            "targetBuffStacks": 4,
            "targetBuffMaxStacks": 4,
            "targetBuffDurationMs": 30000
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (result, trace) = engine::trace_one(&profiles, 1, 0);

        assert_eq!(trace.damage_events.len(), 2);
        assert_eq!(trace.damage_events[1].damage_source, "additional_effect:191704");
        assert_eq!(trace.damage_events[1].hit_id, "card_increase_additional_hit_1");
        assert_eq!(trace.damage_events[1].hit_name, "카드 증가 추가 피해 1");
        assert!((trace.damage_events[1].result_damage - 100.0).abs() < 1e-9);
        assert!((result.total_damage
            - trace.damage_events.iter().map(|event| event.result_damage).sum::<f64>())
            .abs() < 1e-9);
        assert_eq!(trace.casts[0].target_debuff_details[0].stacks, 4);
    }

    #[test]
    fn skill_data_dot_uses_raw_damage_and_first_tick_time() {
        let mut request: serde_json::Value = serde_json::from_str(
            &base_arcana_request("[]", "null", "null"),
        ).expect("request should be valid JSON");
        request["skillData"][0]["cooldown"] = serde_json::json!(10.0);
        request["skillData"][0]["hits"][0]["dot"] = serde_json::json!({
            "dotId": "190330",
            "hitId": "mana_addiction_tick",
            "name": "마나 중독 피해",
            "chance": 1.0,
            "damageRatio": [0.0],
            "fixedDamage": [10.0],
            "firstTickMs": 500,
            "tickIntervalMs": 1000,
            "tickCount": 3
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let trace = engine::trace_one(&profiles, 1, 2500).1;
        let dots: Vec<_> = trace.damage_events.iter()
            .filter(|event| event.damage_source == "dot:190330")
            .collect();

        assert_eq!(dots.len(), 3);
        assert!(dots.iter().all(|event| event.hit_id == "mana_addiction_tick"));
        assert!(dots.iter().all(|event| event.hit_name == "마나 중독 피해"));
        assert_eq!(dots.iter().map(|event| event.time).collect::<Vec<_>>(), vec![0.5, 1.5, 2.5]);
        assert!(dots.iter().all(|event| (event.result_damage - 10.0).abs() < 1e-9));
    }

    #[test]
    fn arcana_checkmate_stacks_on_each_card_and_card_increase_raises_the_cap() {
        for card_increase in [false, true] {
            let profiles = builder::build(&arcana_checkmate_request(card_increase))
                .expect("build should succeed");
            let checkmate = profiles
                .skills
                .iter()
                .find(|skill| skill.name == "체크메이트")
                .expect("Checkmate profile should exist");

            assert_eq!(checkmate.hits.len(), 13);
            for (index, hit) in checkmate.hits[..12].iter().enumerate() {
                let expected_cap = if card_increase && index >= 9 { 4 } else { 3 };
                assert!((hit.crit_chance_bonus - 0.4).abs() < 1e-9);
                assert!((hit.damage_increase - 0.8).abs() < 1e-9);
                assert!(matches!(
                    hit.triggers.first().map(|trigger| &trigger.effect),
                    Some(TriggerEffect::ApplyTargetBuffStacks { spec, stacks })
                        if spec.id == "arcana_ruin_stack"
                            && spec.max_stacks == expected_cap
                            && *stacks == 1
                ));
            }
            assert!(checkmate.hits[12].triggers.is_empty());
        }
    }

    #[test]
    fn checkmate_runtime_damage_stack_affects_the_next_hit() {
        for trigger_on in ["hit", "crit"] {
            let mut request: serde_json::Value = serde_json::from_str(
                &arcana_checkmate_request(false),
            ).expect("request should parse");
            for (index, hit) in request["skillData"][0]["hits"]
                .as_array_mut()
                .expect("hits should be an array")
                .iter_mut()
                .enumerate()
            {
                hit["actionDelayMs"] = serde_json::json!(index * 10);
                hit["damageIncrease"] = serde_json::json!(0.0);
                hit["critChanceBonus"] = serde_json::json!(
                    if trigger_on == "crit" { 1.0 } else { 0.0 }
                );
            }
            request["skillData"][0]["runtimeSkillDamageStacks"] = serde_json::json!([{
                "buffId": "checkmate_test",
                "damageIncreasePerStack": 0.1,
                "maxStacks": 16,
                "durationMs": 2500,
                "triggerOn": trigger_on,
                "triggerEffectIds": [190401, 190411, 190413, 190415]
            }]);

            let profiles = builder::build(&request.to_string()).expect("build should succeed");
            let (_result, trace) = engine::trace_one(&profiles, 1, 200);
            let damage = trace.damage_events
                .iter()
                .map(|event| event.result_damage)
                .collect::<Vec<_>>();

            assert_eq!(damage.len(), 13);
            let first = damage[0];
            for (index, value) in damage.iter().enumerate() {
                let expected_stack = index.min(12) as f64;
                assert!((value / first - (1.0 + expected_stack * 0.1)).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn ruin_damage_and_consume_are_attached_only_to_matching_effect_hits() {
        let mut request: serde_json::Value = serde_json::from_str(
            &arcana_celestial_with_runtime_damage_input_request(),
        ).expect("request should parse");
        let skill = &mut request["skillData"][0];
        let mut first = skill["hits"][0].clone();
        first["effectId"] = serde_json::json!(192400);
        let mut second = first.clone();
        second["effectId"] = serde_json::json!(192401);
        skill["hits"] = serde_json::json!([first, second]);
        skill["ruinTriggerEffectIds"] = serde_json::json!([192401]);

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let celestial = profiles.skills.iter()
            .find(|profile| profile.name == "셀레스티얼 레인")
            .expect("Celestial Rain profile should exist");

        assert!(celestial.hits[0].triggers.iter().all(|trigger| !matches!(
            &trigger.effect,
            TriggerEffect::DealDamageByTargetBuffStacks { .. }
                | TriggerEffect::ConsumeTargetBuff { .. }
        )));
        assert!(matches!(
            &celestial.hits[1].triggers[0].effect,
            TriggerEffect::DealDamageByTargetBuffStacks { .. }
        ));
        assert!(matches!(
            &celestial.hits[1].triggers[1].effect,
            TriggerEffect::ConsumeTargetBuff { .. }
        ));
    }

    #[test]
    fn awakening_use_limit_and_amplifier_are_enforced() {
        let request = serde_json::json!({
            "character": {
                "name": "test_arcana", "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [{"name": "프리즈매틱 미러", "level": 1, "tripods": []}],
                "equipments": [], "accessories": [], "bracelet": null, "stone": null,
                "gems": [], "engravings": [],
                "arkPassive": {"commonNodes": [], "classNodes": [
                    {"name": "각성 증폭기", "level": 3, "values": [3.0]}
                ]},
                "arkGrids": null, "arkPassiveKarma": null
            },
            "aplConfig": "",
            "skillData": [{
                "name": "프리즈매틱 미러", "gameSkillId": 19110,
                "skillControlType": "Point", "skillSlot": "Awakening",
                "maxUses": 5, "cooldown": 0.1, "castTimesMs": [100],
                "hits": [{"effectId": 191101, "damageRatio": [1.0], "fixedDamage": [1.0]}],
                "properties": {}
            }]
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        assert_eq!(profiles.skills[0].max_uses, 8);
        let (result, trace) = engine::trace_one(&profiles, 1, 2_000);
        assert_eq!(result.cast_count, 8);
        assert_eq!(trace.casts.len(), 8);
    }

    #[test]
    fn charged_fury_reduces_the_remaining_awakening_cooldown_when_the_gauge_fills() {
        let request = serde_json::json!({
            "character": {
                "name": "test_arcana", "className": "아르카나",
                "stats": {"maxhp": 100000},
                "skills": [
                    {"name": "프리즈매틱 미러", "level": 1, "tripods": []},
                    {"name": "게이지 타격", "level": 1, "tripods": []}
                ],
                "equipments": [], "accessories": [], "bracelet": null, "stone": null,
                "gems": [], "engravings": [],
                "arkPassive": {"commonNodes": [], "classNodes": [
                    {"name": "충전된 분노", "level": 5, "values": [50.0]}
                ]},
                "arkGrids": null, "arkPassiveKarma": null
            },
            "aplConfig": "",
            "skillData": [{
                "name": "프리즈매틱 미러", "gameSkillId": 19110,
                "skillControlType": "Point", "skillSlot": "Awakening",
                "cooldown": 100.0, "castTimesMs": [100],
                "hits": [{"effectId": 191101, "damageRatio": [1.0], "fixedDamage": [1.0]}],
                "properties": {}
            }, {
                "name": "게이지 타격", "gameSkillId": 19030,
                "skillControlType": "Normal", "skillSlot": "Normal", "maxUses": 1,
                "cooldown": 1.0, "castTimesMs": [100],
                "hits": [{
                    "effectId": 190301, "damageRatio": [1.0], "fixedDamage": [1.0],
                    "ultimatePointGain": 10000.0
                }],
                "properties": {}
            }]
        });

        let profiles = builder::build(&request.to_string()).expect("build should succeed");
        let (_result, trace) = engine::trace_one(&profiles, 1, 60_000);
        let awakening_casts = trace.casts.iter()
            .filter(|cast| cast.skill_name == "프리즈매틱 미러")
            .map(|cast| cast.time)
            .collect::<Vec<_>>();

        assert_eq!(awakening_casts, vec![0.0, 50.05]);
    }

    #[test]
    fn disabling_pet_turns_off_the_buff_and_talents_together() {
        let request = |enabled| {
            serde_json::json!({
                "character": {
                    "name": "test_arcana", "className": "아르카나", "stats": {},
                    "bracelet": null, "stone": null, "arkPassive": null,
                    "arkGrids": null, "arkPassiveKarma": null
                },
                "enablePetBuff": enabled,
                "petBuffType": "치명",
                "petTalents": {
                    "additionalDamage": 2.0,
                    "typeDamage": 3.0,
                    "mainStatPercent": 4.0
                }
            })
        };

        let disabled = builder::build(&request(false).to_string()).expect("build should succeed");
        assert_eq!(disabled.stat.crit, 0.0);
        assert_eq!(disabled.stat.base_main_stat, 0.0);
        assert_eq!(disabled.stat.main_stat_mul, 1.0);
        assert!(disabled.stat.additional_damage.is_empty());
        assert!(disabled.stat.type_damage.is_empty());

        let enabled = builder::build(&request(true).to_string()).expect("build should succeed");
        assert_eq!(enabled.stat.crit, 160.0);
        assert!((enabled.stat.main_stat_mul - 1.04).abs() < 1e-9);
        assert_eq!(enabled.stat.additional_damage["pet:추가피해"], 0.02);
        assert_eq!(enabled.stat.type_damage["pet:계열피해"], 0.03);
    }
}
