//! Engine-owned route selection. Clients supply choices, never class coefficients.
use crate::profile::{
    valkyrie::{ValkyrieIdentity, ValkyriePassives},
    valkyrie_cores::{CoreEffect, CoreRuntime, CoreSelection},
    SkillProfile,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    schema: u32,
    character_level: u32,
    #[serde(default = "yes")]
    ignore_mana: bool,
    #[serde(default = "yes")]
    perfect: bool,
    #[serde(default = "input_time")]
    perfect_input_ms: u32,
    #[serde(default)]
    cores: Vec<CoreSelection>,
}
fn yes() -> bool {
    true
}
fn input_time() -> u32 {
    600
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    variants: Vec<Route>,
    special: HashMap<u32, Vec<Special>>,
    skill_categories: hashbrown::HashMap<u32, u32>,
    sky_damage: Vec<f64>,
    conviction_damage: Vec<f64>,
    leap: HashMap<String, Vec<Vec<f64>>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Route {
    skill_id: u32,
    choices: Vec<u32>,
    tripods: Vec<String>,
    data: Value,
    mp_costs: Vec<f64>,
    category: u32,
    windows: Vec<crate::profile::valkyrie::ValkyrieLinkWindow>,
    debuffs: Vec<Debuff>,
    reductions: Vec<Reduction>,
}
#[derive(Deserialize)]
struct Special {
    mode: String,
    data: Value,
    windows: Vec<crate::profile::valkyrie::ValkyrieLinkWindow>,
}
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Debuff {
    effect_ids: Vec<u32>,
    duration_ms: u32,
    value: f64,
}
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Reduction {
    effect_id: u32,
    amount_ms: u64,
    cap_per_effect_ms: u64,
}
fn catalog() -> &'static Catalog {
    static DATA: OnceLock<Catalog> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../skill-data/valkyrie/backend/build-runtime.json"
        ))
        .expect("invalid Valkyrie build catalog")
    })
}
pub struct Resolved {
    pub identity: ValkyrieIdentity,
    debuffs: Vec<Debuff>,
    reductions: Vec<Reduction>,
    source_cast_times: HashMap<u32, u32>,
}
fn node(nodes: &[Value], name: &str, max: u32) -> Result<u32, String> {
    let selected: Vec<_> = nodes.iter().filter(|n| n["name"] == name).collect();
    if selected.len() > 1 {
        return Err(format!("중복 발키리 패시브: {name}"));
    }
    let Some(n) = selected.first() else {
        return Ok(0);
    };
    let level = n["level"]
        .as_u64()
        .ok_or_else(|| format!("잘못된 패시브 레벨: {name}"))?;
    if level > max as u64 {
        return Err(format!("패시브 레벨 범위 밖: {name}"));
    }
    Ok(level as u32)
}
pub fn resolve(input: &mut Value) -> Result<Option<Resolved>, String> {
    if input["character"]["className"] != "발키리" {
        return Ok(None);
    }
    if input.get("valkyrieBuild").is_none() {
        return Err("지원하지 않는 직업: 발키리 (전체 빌드 선택 계약 없음)".into());
    }
    let selection: Selection = serde_json::from_value(input["valkyrieBuild"].clone())
        .map_err(|e| format!("발키리 빌드 선택: {e}"))?;
    if selection.schema != 1 || !(600..1100).contains(&selection.perfect_input_ms) {
        return Err("발키리 빌드 버전/퍼펙트 입력 범위 밖".into());
    }
    let cat = catalog();
    let specials = cat
        .special
        .get(&selection.character_level)
        .ok_or("발키리 원본 캐릭터 레벨 키 없음")?;
    let mut nodes = input["character"]["arkPassive"]["classNodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let allowed = [
        "빛의 기사",
        "검술 훈련",
        "성검 개방",
        "최후의 빛",
        "삼위일체",
        "연결되는 빛",
        "신념의 검",
        "창공의 검",
    ];
    for n in &nodes {
        if !allowed.contains(&n["name"].as_str().unwrap_or(""))
            && !cat.leap.contains_key(n["name"].as_str().unwrap_or(""))
        {
            return Err(format!("미지원 발키리 패시브: {}", n["name"]));
        }
    }
    for (name, levels) in &cat.leap {
        let level = node(&nodes, name, levels.len() as u32)?;
        for n in &mut nodes {
            if n["name"] == *name {
                n["values"] = if level == 0 {
                    json!([])
                } else {
                    json!(levels[level as usize - 1])
                };
            }
        }
    }
    let base = node(&nodes, "빛의 기사", 3)?;
    if base == 0 {
        return Err("빛의 기사 패시브가 필요합니다".into());
    }
    let p = ValkyriePassives {
        training: node(&nodes, "검술 훈련", 3)?,
        release: node(&nodes, "성검 개방", 3)?,
        last_light: node(&nodes, "최후의 빛", 5)?,
        trinity: node(&nodes, "삼위일체", 3)?,
        linked_light: node(&nodes, "연결되는 빛", 5)?,
    };
    let sky = node(&nodes, "창공의 검", 3)?;
    let conviction = node(&nodes, "신념의 검", 3)?;
    if sky > 0 && conviction > 0 {
        return Err("개벽의 두 대체 경로 중복".into());
    }
    let has_passives = p.training + p.release + p.last_light + p.trinity + p.linked_light > 0;
    if base != 3 && (has_passives || sky + conviction > 0) {
        return Err("상위 패시브는 빛의 기사3레벨이 필요합니다".into());
    }
    let mut reference = ValkyrieIdentity {
        light_per_use: 667,
        light_max: 10000,
        light_sword_grant: if p.trinity > 0 { 3 } else { 1 },
        light_sword_damage_bonus: [0.10, 0.225, 0.35][base as usize - 1],
        skill_categories: cat.skill_categories.clone(),
        initial_light: 0,
        passives: has_passives.then_some(p),
        linked_windows: Default::default(),
        cores: selection.cores,
    };
    reference.validate()?;
    let selected = input["character"]["skills"]
        .as_array()
        .ok_or("발키리 스킬 목록 없음")?;
    let mut data = Vec::new();
    let mut seen = HashSet::new();
    let mut ordinary = 0;
    let mut debuffs = Vec::new();
    let mut reductions = Vec::new();
    for skill in selected {
        let name = skill["name"].as_str().ok_or("발키리 스킬 이름 없음")?;
        if !seen.insert(name) {
            return Err("중복 발키리 스킬".into());
        }
        let level = skill["level"].as_u64().ok_or("잘못된 발키리 스킬 레벨")?;
        let mut names: Vec<_> = skill["tripods"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| t["name"].as_str().unwrap_or("").to_string())
            .collect();
        names.sort();
        if names.windows(2).any(|w| w[0] == w[1]) {
            return Err("중복 트라이포드".into());
        }
        let route = cat.variants.iter().find(|r| {
            r.data["name"] == name && {
                let mut ts = r.tripods.clone();
                ts.sort();
                ts == names
            }
        });
        let mut row;
        if let Some(r) = route {
            ordinary += 1;
            if !(1..=20).contains(&level) {
                return Err("발키리 스킬 레벨은1~20입니다".into());
            }
            if r.choices.iter().any(|id| {
                level
                    < if *id < 4 {
                        4
                    } else if *id < 7 {
                        7
                    } else {
                        10
                    }
            }) {
                return Err("선택한 트라이포드의 스킬 레벨이 부족합니다".into());
            }
            row = r.data.clone();
            let cost = r.mp_costs[level as usize - 1];
            row["baseManaCost"] = json!(cost);
            row["resourceCost"] = if selection.ignore_mana {
                Value::Null
            } else {
                json!({"name":"Mp","cost":r.mp_costs})
            };
            reference.skill_categories.insert(r.skill_id, r.category);
            if reference
                .passives
                .as_ref()
                .is_some_and(|p| p.linked_light > 0)
            {
                reference
                    .linked_windows
                    .insert(r.skill_id, r.windows.clone());
            }
            debuffs.extend(r.debuffs.clone());
            reductions.extend(r.reductions.clone());
        } else {
            let mode = if name == "개벽" {
                if sky > 0 {
                    if selection.perfect {
                        "sky"
                    } else {
                        "skyFail"
                    }
                } else if conviction > 0 {
                    "conviction"
                } else {
                    "base"
                }
            } else {
                "base"
            };
            let special = specials
                .iter()
                .find(|s| s.data["name"] == name && s.mode == mode)
                .ok_or_else(|| format!("보류 또는 미지원 발키리 조합: {name}"))?;
            if level != 1 || !names.is_empty() {
                return Err("아이덴티티/각성/이동기는1레벨·트라이포드 없음입니다".into());
            }
            if !skill["rune"].is_null() {
                return Err("아이덴티티/각성/이동기에는 룬을 장착할 수 없습니다".into());
            }
            row = special.data.clone();
            let mut windows = special.windows.clone();
            if name == "개벽" {
                let factor = if sky > 0 {
                    (1. + cat.sky_damage[sky as usize - 1]) / (1. + cat.sky_damage[0])
                } else if conviction > 0 {
                    (1. + cat.conviction_damage[conviction as usize - 1])
                        / (1. + cat.conviction_damage[0])
                } else {
                    1.
                };
                for hit in row["hits"].as_array_mut().unwrap() {
                    for field in ["damageRatio", "fixedDamage"] {
                        for v in hit[field].as_array_mut().unwrap() {
                            *v = json!(v.as_f64().unwrap() * factor);
                        }
                    }
                }
                if sky > 0 && selection.perfect {
                    let delta = selection.perfect_input_ms - 600;
                    row["castTimesMs"][0] =
                        json!(row["castTimesMs"][0].as_u64().unwrap() + delta as u64);
                    row["hits"][1]["actionDelayMs"] =
                        json!(row["hits"][1]["actionDelayMs"].as_u64().unwrap() + delta as u64);
                    for w in &mut windows {
                        w.start_ms += delta;
                        w.end_ms += delta;
                    }
                }
                if reference
                    .passives
                    .as_ref()
                    .is_some_and(|p| p.linked_light > 0)
                {
                    reference.linked_windows.insert(48500, windows);
                }
            }
            if selection.ignore_mana && row["resourceCost"]["name"] == "Mp" {
                row["resourceCost"] = Value::Null;
            }
        }
        data.push(row);
    }
    if ordinary > 8 {
        return Err("발키리 일반 스킬은 최대8개입니다".into());
    }
    let leap_value = |name: &str, index: usize| -> f64 {
        nodes
            .iter()
            .find(|n| n["name"] == name)
            .and_then(|n| n["values"][index].as_f64())
            .unwrap_or(0.)
    };
    if let Some(row) = data.iter_mut().find(|s| s["gameSkillId"] == 48500) {
        row["cooldown"] =
            json!(row["cooldown"].as_f64().unwrap() * (1. - leap_value("잠재력 해방", 0) / 100.));
        let ratio = 1. / (1. + leap_value("즉각적인 주문", 0) / 100.);
        for v in row["castTimesMs"].as_array_mut().unwrap() {
            *v = json!((v.as_f64().unwrap() * ratio) as u32);
        }
        for h in row["hits"].as_array_mut().unwrap() {
            h["actionDelayMs"] = json!((h["actionDelayMs"].as_f64().unwrap() * ratio) as u32);
        }
        for w in reference
            .linked_windows
            .get_mut(&48500)
            .into_iter()
            .flatten()
        {
            w.start_ms = (w.start_ms as f64 * ratio) as u32;
            w.end_ms = (w.end_ms as f64 * ratio) as u32;
        }
        if let Some(costs) = row
            .get_mut("resourceCost")
            .and_then(|v| v.get_mut("cost"))
            .and_then(Value::as_array_mut)
        {
            for v in costs {
                *v = json!(v.as_f64().unwrap() * (1. - leap_value("즉각적인 주문", 1) / 100.));
            }
        }
    }
    for effect in CoreRuntime::from_selected(&reference.cores)?.effects {
        let (id, kind) = match &effect {
            CoreEffect::Speed { skill_id, .. }
            | CoreEffect::Cooldown { skill_id, .. }
            | CoreEffect::Mana { skill_id, .. }
            | CoreEffect::Category { skill_id, .. } => (*skill_id, &effect),
            _ => continue,
        };
        if let CoreEffect::Category { category, .. } = kind {
            reference.skill_categories.insert(id, *category);
            if *category != 68 {
                reference.linked_windows.remove(&id);
            }
        }
        let Some(row) = data.iter_mut().find(|s| s["gameSkillId"] == id) else {
            continue;
        };
        match kind {
            CoreEffect::Speed { ratio, .. } => {
                for v in row["castTimesMs"].as_array_mut().unwrap() {
                    *v = json!((v.as_f64().unwrap() * ratio).round() as u32);
                }
                for h in row["hits"].as_array_mut().unwrap() {
                    h["actionDelayMs"] =
                        json!((h["actionDelayMs"].as_f64().unwrap() * ratio).round() as u32);
                }
                for w in reference.linked_windows.get_mut(&id).into_iter().flatten() {
                    w.start_ms = (w.start_ms as f64 * ratio).round() as u32;
                    w.end_ms = (w.end_ms as f64 * ratio).round() as u32;
                }
            }
            CoreEffect::Cooldown { amount_ms, .. } => {
                row["cooldown"] =
                    json!(row["cooldown"].as_f64().unwrap() + *amount_ms as f64 / 1000.)
            }
            CoreEffect::Mana { ratio, .. } => {
                if let Some(costs) = row
                    .get_mut("resourceCost")
                    .and_then(|v| v.get_mut("cost"))
                    .and_then(Value::as_array_mut)
                {
                    for v in costs {
                        *v = json!(v.as_f64().unwrap() * ratio);
                    }
                }
            }
            _ => {}
        }
    }
    reference.validate()?;
    let source_cast_times = data
        .iter()
        .map(|s| {
            (
                s["gameSkillId"].as_u64().unwrap() as u32,
                s["castTimesMs"][0].as_u64().unwrap() as u32,
            )
        })
        .collect();
    input["skillData"] = json!(data);
    input["character"]["arkPassive"]["classNodes"] = json!(nodes);
    Ok(Some(Resolved {
        identity: reference,
        debuffs,
        reductions,
        source_cast_times,
    }))
}

/// These four nodes have the same source contracts across classes. The two
/// Dawn-only cooldown/speed nodes are resolved alongside its linked windows.
pub fn register_leap(
    stat: &mut super::stat::StatBuilder,
    modifier: &mut super::modifier::ModifierManager,
    triggers: &mut Vec<crate::profile::GlobalTrigger>,
    pipelines: &mut super::skill::SkillPipelineMap,
    nodes: &[super::dto::ArkPassiveClassNodeInput],
    req: &super::dto::BuildRequest,
) {
    for n in nodes.iter().filter(|n| {
        n.level > 0 && ["초월적인 힘", "각성 증폭기", "풀려난 힘"].contains(&n.name.as_str())
    }) {
        super::class::arcana::apply_ark_passive(
            stat,
            modifier,
            triggers,
            pipelines,
            std::slice::from_ref(n),
            req,
        );
    }
    if let Some(node) = nodes
        .iter()
        .find(|n| n.name == "충전된 분노" && n.level > 0)
    {
        triggers.push(crate::profile::GlobalTrigger {
            filter: crate::profile::TriggerFilter::AnyHit,
            condition: crate::profile::TriggerCondition::Always,
            effect: crate::profile::TriggerEffect::ReduceCooldownsOnResourceFull {
                resource: "UltimatePoint".into(),
                skill_ids: req
                    .character
                    .skills
                    .iter()
                    .enumerate()
                    .filter_map(|(i, s)| {
                        ["브뢴디아의 계시", "브뢴디아의 화신"]
                            .contains(&s.name.as_str())
                            .then_some(i as u32)
                    })
                    .collect(),
                percent: node.values[0] / 100.,
            },
            cooldown_ms: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Value {
        json!({"valkyrieBuild":{"schema":1,"characterLevel":70},"character":{"name":"빌더 검증","className":"발키리","stats":{"maxhp":100000},"skills":[{"name":"간파 베기","level":12,"tripods":[]}],"arkPassive":{"commonNodes":[],"classNodes":[{"name":"빛의 기사","level":3,"values":[999]}]}},"aplConfig":""})
    }
    #[test]
    fn canonical_routes_replace_untrusted_damage_and_preserve_common_equipment() {
        let mut v = request();
        v["skillData"] = json!([{"name":"간파 베기","cooldown":0,"hits":[]}]);
        v["character"]["equipments"] = json!([{"type":"무기","setName":"검증","quality":0,"itemLevel":1800,"refinementLevel":25,"advancedRefinementLevel":0},{"type":"투구","setName":"검증","quality":0,"itemLevel":1800,"refinementLevel":25,"advancedRefinementLevel":0}]);
        v["equipmentTables"] = json!({"검증":{"무기":[{"itemLevel":1800,"weaponAttack":60000}],"투구":[{"itemLevel":1800,"mainStat":100000}]}});
        let p = super::super::build_raid(&v.to_string()).unwrap();
        assert_eq!(p.skills.len(), 1);
        assert!(!p.skills[0].hits.is_empty());
        assert_eq!(
            p.valkyrie_identity
                .as_ref()
                .unwrap()
                .light_sword_damage_bonus,
            0.35
        );
        assert_eq!(p.stat.base_main_stat, 100000.);
        assert_eq!(p.stat.weapon_attack_power.values().sum::<f64>(), 60000.);
    }
    #[test]
    fn every_supported_route_and_character_level_deserializes_through_public_builder() {
        for r in &catalog().variants {
            let minimum = r
                .choices
                .iter()
                .map(|id| {
                    if *id < 4 {
                        4
                    } else if *id < 7 {
                        7
                    } else {
                        10
                    }
                })
                .max()
                .unwrap_or(1);
            for level in [minimum, 12, 20] {
                let mut v = request();
                v["character"]["skills"] = json!([{"name":r.data["name"],"level":level,"tripods":r.tripods.iter().map(|n|json!({"name":n})).collect::<Vec<_>>() }]);
                let p = super::super::build_raid(&v.to_string()).unwrap();
                assert_eq!(p.skills.len(), 1);
            }
        }
        for level in catalog().special.keys() {
            for name in [
                "빛의 기사",
                "종언의 빛",
                "진입",
                "개벽",
                "브뢴디아의 계시",
                "브뢴디아의 화신",
            ] {
                let mut v = request();
                v["valkyrieBuild"]["characterLevel"] = json!(level);
                v["character"]["skills"] = json!([{"name":name,"level":1,"tripods":[]}]);
                super::super::build_raid(&v.to_string()).unwrap();
            }
        }
    }
    #[test]
    fn held_combos_invalid_levels_and_duplicate_nodes_fail_closed() {
        let raw: Value = serde_json::from_str(include_str!(
            "../../skill-data/valkyrie/backend/build-runtime.json"
        ))
        .unwrap();
        for held in raw["held"].as_array().unwrap() {
            let id = held["skillId"].as_u64().unwrap();
            let name = &catalog()
                .variants
                .iter()
                .find(|r| r.skill_id as u64 == id)
                .unwrap()
                .data["name"];
            let mut v = request();
            v["character"]["skills"] = json!([{"name":name,"level":12,"tripods":held["tripods"].as_array().unwrap().iter().map(|n|json!({"name":n})).collect::<Vec<_>>()}]);
            assert!(resolve(&mut v).is_err(), "held {held}");
        }
        let mut v = request();
        v["valkyrieBuild"]["characterLevel"] = json!(69);
        assert!(resolve(&mut v).is_err());
        let mut v = request();
        v["character"]["skills"][0]["level"] = json!(21);
        assert!(resolve(&mut v).is_err());
        let mut v = request();
        v["character"]["arkPassive"]["classNodes"] =
            json!([{"name":"빛의 기사","level":3},{"name":"빛의 기사","level":3}]);
        assert!(resolve(&mut v).is_err());
        let mut v = request();
        v["valkyrieBuild"]["arbitraryMultiplier"] = json!(99);
        assert!(resolve(&mut v).is_err());
    }
    #[test]
    fn six_leap_nodes_use_source_values_and_awakening_limits() {
        let mut v = request();
        v["valkyrieBuild"]["ignoreMana"] = json!(false);
        v["character"]["skills"] = json!([{"name":"개벽","level":1,"tripods":[]},{"name":"브뢴디아의 계시","level":1,"tripods":[]},{"name":"브뢴디아의 화신","level":1,"tripods":[]}]);
        for (name, levels) in &catalog().leap {
            v["character"]["arkPassive"]["classNodes"]
                .as_array_mut()
                .unwrap()
                .push(json!({"name":name,"level":levels.len(),"values":[999999]}));
        }
        let p = super::super::build_raid(&v.to_string()).unwrap();
        let dawn = &p.skills[0];
        assert_eq!(dawn.cooldown_ms, 58500);
        assert_eq!(dawn.cast_times_ms[0], (2900_f64 / 1.12) as u32);
        assert!((dawn.resource_costs["Mp"] - 89.4).abs() < 1e-10);
        assert!((dawn.hits[0].damage_increase - 0.15).abs() < 1e-10);
        assert_eq!(p.skills[1].max_uses, 8);
        assert_eq!(p.skills[2].max_uses, 1);
        assert_eq!(p.skills[2].resource_costs["UltimatePoint"], 10000.);
        assert!((p.skills[2].hits[0].damage_increase - 0.5).abs() < 1e-10);
        assert!(p.global_triggers.iter().any(|t|matches!(t.effect,crate::profile::TriggerEffect::ReduceCooldownsOnResourceFull{percent,..}if percent==0.5)));
    }
    #[test]
    fn full_builder_cycles_reinitialize_with_exact_trace_and_plain_damage() {
        let mut v = request();
        let names = [
            "빛의 기사",
            "종언의 빛",
            "간파 베기",
            "계시의 검",
            "성휘의 심판",
        ];
        // Use source names for two distinct radiance skills.
        let a = &catalog()
            .variants
            .iter()
            .find(|r| r.skill_id == 48160 && r.choices.is_empty())
            .unwrap()
            .data["name"];
        let b = &catalog()
            .variants
            .iter()
            .find(|r| r.skill_id == 48180 && r.choices.is_empty())
            .unwrap()
            .data["name"];
        let chosen = vec![
            json!({"name":names[0],"level":1,"tripods":[]}),
            json!({"name":names[1],"level":1,"tripods":[]}),
            json!({"name":names[2],"level":12,"tripods":[]}),
            json!({"name":a,"level":12,"tripods":[]}),
            json!({"name":b,"level":12,"tripods":[]}),
        ];
        v["aplConfig"]=json!(json!({"name":"기본 순환","class":"발키리","actions":chosen.iter().map(|s|json!({"skill":s["name"]})).collect::<Vec<_>>()}).to_string());
        v["character"]["skills"] = json!(chosen);
        v["character"]["arkPassive"]["classNodes"]
            .as_array_mut()
            .unwrap()
            .extend([
                json!({"name":"삼위일체","level":3}),
                json!({"name":"연결되는 빛","level":5}),
            ]);
        v["valkyrieBuild"]["cores"] = json!([{"key":"origin","grade":3,"points":20},{"key":"finalKnight","grade":3,"points":20}]);
        let p = super::super::build_raid(&v.to_string()).unwrap();
        let (result, trace) = crate::engine::trace_one(&p, 42, 180000);
        assert!(result.total_damage > 0.);
        assert!(trace
            .valkyrie_events
            .iter()
            .any(|e| e.game_skill_id == 48070));
        assert_eq!(
            result.total_damage,
            crate::engine::simulate(&p, 1, 180000, 42, None).total_damage
        );
        let events = serde_json::to_value(&trace.valkyrie_events).unwrap();
        for _ in 0..100 {
            let (again, t) = crate::engine::trace_one(&p, 42, 180000);
            assert_eq!(result.total_damage, again.total_damage);
            assert_eq!(serde_json::to_value(t.valkyrie_events).unwrap(), events);
        }
    }
    #[test]
    fn ultimate_contact_gain_cost_and_shared_awakening_cooldown() {
        let mut v = request();
        v["character"]["skills"] = json!([{"name":"간파 베기","level":12,"tripods":[]},{"name":"브뢴디아의 화신","level":1,"tripods":[]},{"name":"브뢴디아의 계시","level":1,"tripods":[]}]);
        v["aplConfig"]=json!(json!({"name":"초각성 경계","class":"발키리","actions":[{"skill":"브뢴디아의 화신"},{"skill":"간파 베기"},{"skill":"브뢴디아의 계시"}]}).to_string());
        let mut p = super::super::build_raid(&v.to_string()).unwrap();
        assert!(p.skills[0].hits[0].triggers.iter().any(|t|matches!(&t.effect,crate::profile::TriggerEffect::GainResource{resource,amount}if resource=="UltimatePoint"&&*amount==10.)));
        p.skills[0].max_uses = 1;
        // Force a full bar in the engine fixture to exercise the gate without a
        // thousand repetitive source contacts. Production still starts at zero.
        p.skills[0].hits[0]
            .triggers
            .push(crate::profile::HitTrigger {
                on: crate::profile::HitTriggerOn::OnHit,
                chance: 1.,
                effect: crate::profile::TriggerEffect::GainResource {
                    resource: "UltimatePoint".into(),
                    amount: 10000.,
                },
            });
        let (_, trace) = crate::engine::trace_one(&p, 42, 610000);
        let hyper: Vec<_> = trace.casts.iter().filter(|c| c.skill_id == 1).collect();
        let regular: Vec<_> = trace.casts.iter().filter(|c| c.skill_id == 2).collect();
        assert_eq!(hyper.len(), 1);
        assert!(!regular.is_empty());
        assert!(regular[0].time >= hyper[0].time + 300.);
    }
    #[test]
    fn gale_and_cast_modifiers_scale_contacts_and_link_windows_together() {
        let r = catalog()
            .variants
            .iter()
            .find(|r| r.skill_id == 48160 && r.choices.is_empty())
            .unwrap();
        let mut v = request();
        v["character"]["skills"] = json!([{"name":r.data["name"],"level":12,"tripods":[],"rune":{"name":"질풍","grade":"전설","effects":[{"kind":"cast_speed","value":14.}]}}]);
        v["character"]["arkPassive"]["classNodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"연결되는 빛","level":5}));
        let p = super::super::build_raid(&v.to_string()).unwrap();
        let s = &p.skills[0];
        let base = r.data["castTimesMs"][0].as_u64().unwrap();
        assert_eq!(s.cast_times_ms[0], (base as f64 * 0.86).round() as u32);
        assert_eq!(
            s.hits[0].action_delay_ms,
            (r.data["hits"][0]["actionDelayMs"].as_u64().unwrap() * s.cast_times_ms[0] as u64
                / base) as u32
        );
        let windows = &p.valkyrie_identity.unwrap().linked_windows[&48160];
        for (a, b) in windows.iter().zip(&r.windows) {
            assert_eq!(
                a.start_ms,
                (b.start_ms as u64 * s.cast_times_ms[0] as u64 / base) as u32
            );
        }
    }
    #[test]
    fn rain_internal_contacts_preserve_one_ultimate_proc_per_effect_call() {
        let mut v = request();
        v["character"]["skills"] = json!([{"name":"안식의 비","level":12,"tripods":[]}]);
        let p = super::super::build_raid(&v.to_string()).unwrap();
        let amounts: Vec<_> = p.skills[0]
            .hits
            .iter()
            .flat_map(|h| h.triggers.iter())
            .filter_map(|t| match &t.effect {
                crate::profile::TriggerEffect::GainResource { resource, amount }
                    if resource == "UltimatePoint" =>
                {
                    Some(*amount)
                }
                _ => None,
            })
            .collect();
        // One opening Effect482001 and two separate Effect482002 Notify calls;
        // the latter each split into three damage contacts, not three procs.
        assert_eq!(p.skills[0].hits.len(), 7);
        assert_eq!(amounts, vec![12., 14., 14.]);
    }
}
impl Resolved {
    pub fn attach(
        &mut self,
        skills: &mut [SkillProfile],
        specialization: f64,
    ) -> Result<(), String> {
        use crate::profile::{
            BuffSpec, HitTrigger, HitTriggerOn, StackType, StatField, TriggerEffect,
        };
        for skill in skills.iter_mut() {
            let id = skill
                .tags
                .iter()
                .find_map(|tag| match tag {
                    crate::profile::SkillTag::SkillId(id) => Some(*id),
                    _ => None,
                })
                .ok_or("발키리 스킬 ID 없음")?;
            let base = self.source_cast_times[&id];
            if base > 0 && skill.cast_times_ms[0] != base {
                let scale = |t: u32| t as u64 * skill.cast_times_ms[0] as u64 / base as u64;
                for h in &mut skill.hits {
                    h.action_delay_ms = scale(h.action_delay_ms) as u32;
                }
                for w in self
                    .identity
                    .linked_windows
                    .get_mut(&id)
                    .into_iter()
                    .flatten()
                {
                    w.start_ms = scale(w.start_ms) as u32;
                    w.end_ms = scale(w.end_ms) as u32;
                }
            }
            for d in &self.debuffs {
                for hit in &mut skill.hits {
                    if d.effect_ids.contains(&hit.effect_id) {
                        hit.triggers.push(HitTrigger {
                            on: HitTriggerOn::OnHit,
                            chance: 1.,
                            effect: TriggerEffect::ApplyTargetBuff(BuffSpec {
                                id: crate::engine::state::class::valkyrie::WEAKNESS_BUFF.into(),
                                effects: hashbrown::HashMap::from([(
                                    StatField::OnCritDamageTaken,
                                    d.value,
                                )]),
                                max_stacks: 1,
                                stack_type: StackType::Refresh,
                                duration_ms: d.duration_ms,
                            }),
                        });
                    }
                }
            }
            for r in &self.reductions {
                let mut assigned = 0;
                for hit in &mut skill.hits {
                    if hit.effect_id == r.effect_id {
                        let amount = r
                            .amount_ms
                            .min(r.cap_per_effect_ms.saturating_sub(assigned));
                        assigned += amount;
                        if amount > 0 {
                            hit.triggers.push(HitTrigger {
                                on: HitTriggerOn::OnHit,
                                chance: 1.,
                                effect: TriggerEffect::ReduceCooldownsFlat {
                                    skill_ids: vec![skill.skill_id],
                                    amount_ms: amount,
                                },
                            });
                        }
                    }
                }
            }
        }
        super::valkyrie::attach(skills, &self.identity)?;
        super::valkyrie::apply_specialization(skills, specialization)
    }
}
