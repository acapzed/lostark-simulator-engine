use hashbrown::HashMap;
use serde::Deserialize;

use crate::builder::dto::ArcanaCardInput;
use crate::profile::{AplAction, AplCondition, CardAplAction, CompareOperator, SkillProfile};

#[derive(Debug)]
pub struct BuiltApl {
    pub order: Vec<u32>,
    pub actions: Vec<AplAction>,
    pub card_actions: Vec<CardAplAction>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApl {
    #[serde(default, rename = "name")]
    _name: String,
    #[serde(default, rename = "class")]
    _class: String,
    #[serde(default, rename = "build")]
    _build: String,
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    actions: Vec<RawAction>,
    #[serde(default)]
    card_actions: Vec<RawCardAction>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCardAction {
    card: String,
    #[serde(default)]
    conditions: Vec<RawCondition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAction {
    skill: String,
    #[serde(default)]
    conditions: Vec<RawCondition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCondition {
    #[serde(rename = "type")]
    condition_type: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    skill: String,
    #[serde(default)]
    card: String,
    #[serde(default)]
    buff: String,
    #[serde(default)]
    debuff: String,
    #[serde(default)]
    operator: String,
    #[serde(default)]
    value: f64,
}

/// APL 설정 문자열을 파싱해 실행 가능한 우선순위 액션으로 컴파일한다.
///
/// 기존 JSON 문자열 배열(`["스킬명"]`)은 계속 지원한다.
/// YAML/JSON 객체 APL은 `actions`의 조건을 평가하고, 잘못된 참조나 문법은 오류로 반환한다.
pub fn build(
    apl_config: &str,
    skills: &[SkillProfile],
    cards: &[ArcanaCardInput],
) -> Result<BuiltApl, String> {
    let default_order: Vec<u32> = skills.iter().map(|s| s.skill_id).collect();
    let trimmed = apl_config.trim();
    if trimmed.is_empty() {
        return Ok(BuiltApl {
            order: default_order,
            actions: Vec::new(),
            card_actions: Vec::new(),
        });
    }

    let name_to_id: HashMap<&str, u32> = skills
        .iter()
        .map(|s| (s.name.as_str(), s.skill_id))
        .collect();
    let card_name_to_id: HashMap<&str, u32> = cards
        .iter()
        .map(|card| (card.name.as_str(), card.skill_id))
        .collect();

    if let Ok(names) = serde_json::from_str::<Vec<String>>(trimmed) {
        let order = names_to_order(&names, &name_to_id)?;
        return Ok(BuiltApl {
            order: if order.is_empty() {
                default_order
            } else {
                order
            },
            actions: Vec::new(),
            card_actions: Vec::new(),
        });
    }

    let raw = serde_yaml::from_str::<RawApl>(trimmed)
        .map_err(|error| format!("APL YAML 파싱 실패: {error}"))?;
    if raw.skills.is_empty() && raw.actions.is_empty() && raw.card_actions.is_empty() {
        return Err("APL에 실행할 skills, actions 또는 card_actions가 필요합니다".to_string());
    }
    let mut actions = Vec::new();
    for raw_action in raw.actions {
        let skill_id = name_to_id
            .get(raw_action.skill.as_str())
            .copied()
            .ok_or_else(|| format!("APL에 존재하지 않는 스킬: {}", raw_action.skill))?;
        let conditions = raw_action
            .conditions
            .iter()
            .map(|condition| compile_condition(condition, skill_id, &name_to_id, &card_name_to_id))
            .collect::<Result<Vec<_>, _>>()?;
        actions.push(AplAction {
            skill_id,
            conditions,
        });
    }

    let card_actions = raw
        .card_actions
        .iter()
        .map(|raw_action| {
            let card_id = card_name_to_id
                .get(raw_action.card.as_str())
                .copied()
                .ok_or_else(|| format!("APL에 존재하지 않는 카드: {}", raw_action.card))?;
            Ok(CardAplAction {
                card_id,
                card_name: raw_action.card.clone(),
                conditions: raw_action
                    .conditions
                    .iter()
                    .map(|condition| compile_condition(condition, 0, &name_to_id, &card_name_to_id))
                    .collect::<Result<Vec<_>, String>>()?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let order = if !actions.is_empty() {
        actions.iter().map(|action| action.skill_id).collect()
    } else {
        let order = names_to_order(&raw.skills, &name_to_id)?;
        if order.is_empty() {
            default_order
        } else {
            order
        }
    };

    Ok(BuiltApl {
        order,
        actions,
        card_actions,
    })
}

fn names_to_order(names: &[String], name_to_id: &HashMap<&str, u32>) -> Result<Vec<u32>, String> {
    names
        .iter()
        .map(|name| {
            name_to_id
                .get(name.as_str())
                .copied()
                .ok_or_else(|| format!("APL에 존재하지 않는 스킬: {name}"))
        })
        .collect()
}

fn compile_condition(
    raw: &RawCondition,
    current_skill_id: u32,
    name_to_id: &HashMap<&str, u32>,
    card_name_to_id: &HashMap<&str, u32>,
) -> Result<AplCondition, String> {
    let op = || {
        if raw.operator.is_empty() {
            Ok(CompareOperator::Gte)
        } else {
            CompareOperator::parse(raw.operator.as_str())
                .ok_or_else(|| format!("APL에 지원하지 않는 비교 연산자: {}", raw.operator))
        }
    };
    Ok(match raw.condition_type.as_str() {
        "cooldown_ready" => AplCondition::CooldownReady,
        "skill_cooldown_ready" => name_to_id
            .get(required_field(raw, "skill", &raw.skill)?)
            .copied()
            .map(AplCondition::SkillCooldownReady)
            .ok_or_else(|| format!("APL에 존재하지 않는 스킬: {}", raw.skill))?,
        "skill_cooldown_remaining" => {
            let skill_id = name_to_id
                .get(required_field(raw, "skill", &raw.skill)?)
                .copied()
                .ok_or_else(|| format!("APL에 존재하지 않는 스킬: {}", raw.skill))?;
            AplCondition::SkillCooldownRemaining {
                skill_id,
                operator: op()?,
                value_ms: seconds_to_ms(raw.value),
            }
        }
        "resource" => AplCondition::Resource {
            resource: normalize_resource(required_field(raw, "name", &raw.name)?),
            operator: op()?,
            value: raw.value,
        },
        "card_held" => card_name_to_id
            .get(required_field(raw, "card", &raw.card)?)
            .copied()
            .map(AplCondition::CardHeld)
            .ok_or_else(|| format!("APL에 존재하지 않는 카드: {}", raw.card))?,
        "card_count" => AplCondition::CardCount {
            card_id: card_name_to_id
                .get(required_field(raw, "card", &raw.card)?)
                .copied()
                .ok_or_else(|| format!("APL에 존재하지 않는 카드: {}", raw.card))?,
            operator: op()?,
            value: raw.value,
        },
        "next_skill" if current_skill_id == 0 => name_to_id
            .get(required_field(raw, "skill", &raw.skill)?)
            .copied()
            .map(AplCondition::NextSkill)
            .ok_or_else(|| format!("APL에 존재하지 않는 스킬: {}", raw.skill))?,
        "next_skill" => {
            return Err("APL next_skill 조건은 card_actions에서만 사용할 수 있습니다".to_string())
        }
        "buff_active" => {
            AplCondition::BuffActive(normalize_buff_id(required_field(raw, "buff", &raw.buff)?))
        }
        "buff_inactive" => {
            AplCondition::BuffInactive(normalize_buff_id(required_field(raw, "buff", &raw.buff)?))
        }
        "buff_remaining" => AplCondition::BuffRemaining {
            buff_id: normalize_buff_id(required_field(raw, "buff", &raw.buff)?),
            operator: op()?,
            value_ms: seconds_to_ms(raw.value),
        },
        "target_debuff_stacks" => AplCondition::TargetDebuffStacks {
            debuff_id: normalize_target_debuff_id(required_field(raw, "debuff", &raw.debuff)?),
            operator: op()?,
            value: raw.value,
        },
        "combo_phase" => AplCondition::ComboPhase {
            skill_id: named_skill_or_current(raw, current_skill_id, name_to_id)?,
            operator: op()?,
            value: raw.value,
        },
        "chain_phase" => AplCondition::ChainPhase {
            skill_id: named_skill_or_current(raw, current_skill_id, name_to_id)?,
            operator: op()?,
            value: raw.value,
        },
        "time_remaining" => AplCondition::TimeRemaining {
            operator: op()?,
            value_ms: seconds_to_ms(raw.value),
        },
        other => return Err(format!("APL에 지원하지 않는 조건: {other}")),
    })
}

fn required_field<'a>(raw: &RawCondition, field: &str, value: &'a str) -> Result<&'a str, String> {
    if value.is_empty() {
        Err(format!(
            "APL {} 조건에 {} 값이 필요합니다",
            raw.condition_type, field
        ))
    } else {
        Ok(value)
    }
}

fn named_skill_or_current(
    raw: &RawCondition,
    current_skill_id: u32,
    name_to_id: &HashMap<&str, u32>,
) -> Result<u32, String> {
    if raw.skill.is_empty() {
        Ok(current_skill_id)
    } else {
        name_to_id
            .get(raw.skill.as_str())
            .copied()
            .ok_or_else(|| format!("APL에 존재하지 않는 스킬: {}", raw.skill))
    }
}

fn seconds_to_ms(value: f64) -> u64 {
    (value.max(0.0) * 1000.0).round() as u64
}

fn normalize_resource(name: &str) -> String {
    match name {
        "hp" => "Hp",
        "mp" => "Mp",
        other => other,
    }
    .to_string()
}

fn normalize_buff_id(id: &str) -> String {
    id.to_string()
}

fn normalize_target_debuff_id(id: &str) -> String {
    match id {
        "ruin" => "arcana_ruin_stack",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{SkillCategory, SkillSlot, SkillType};

    fn skill(id: u32, name: &str) -> SkillProfile {
        SkillProfile {
            skill_id: id,
            name: name.to_string(),
            skill_type: SkillType::Normal,
            category: SkillCategory::Normal,
            slot: SkillSlot::Normal,
            tags: Vec::new(),
            cooldown_ms: 1000,
            cast_times_ms: vec![100],
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
            hits: Vec::new(),
        }
    }

    #[test]
    fn builds_yaml_actions_with_conditions() {
        let skills = vec![skill(10, "스택"), skill(20, "루인")];
        let apl = build(
            r#"
skills:
  - "스택"
  - "루인"
actions:
  - skill: "루인"
    conditions:
      - type: "target_debuff_stacks"
        debuff: "ruin"
        operator: ">="
        value: 4
"#,
            &skills,
            &[],
        )
        .unwrap();

        assert_eq!(apl.order, vec![20]);
        assert!(matches!(
            &apl.actions[0].conditions[0],
            AplCondition::TargetDebuffStacks { debuff_id, value, .. }
                if debuff_id == "arcana_ruin_stack" && (*value - 4.0).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn keeps_json_name_array_compatibility() {
        let skills = vec![skill(10, "A"), skill(20, "B")];
        let apl = build(r#"["B", "A"]"#, &skills, &[]).unwrap();

        assert_eq!(apl.order, vec![20, 10]);
        assert!(apl.actions.is_empty());
    }

    #[test]
    fn builds_card_actions_from_raw_card_names() {
        let cards = [(19098, "심판"), (19097, "도태")]
            .into_iter()
            .map(|(skill_id, name)| ArcanaCardInput {
                skill_id,
                name: name.to_string(),
                auto_learn: true,
                draw_weight: 1.0,
                runtime_effects: Vec::new(),
            })
            .collect::<Vec<_>>();
        let apl = build(
            "card_actions:\n  - card: 심판\n    conditions:\n      - type: card_held\n        card: 도태\n      - type: card_count\n        card: 심판\n        operator: '>='\n        value: 2",
            &[],
            &cards,
        ).unwrap();

        assert_eq!(apl.card_actions[0].card_id, 19098);
        assert_eq!(apl.card_actions[0].card_name, "심판");
        assert!(matches!(apl.card_actions[0].conditions[0], AplCondition::CardHeld(19097)));
        assert!(matches!(
            apl.card_actions[0].conditions[1],
            AplCondition::CardCount { card_id: 19098, value: 2.0, .. }
        ));
    }

    #[test]
    fn arcana_empress_presets_compile_every_action_and_condition() {
        let skills = [
            "스트림 오브 엣지",
            "스크래치 딜러",
            "스파이럴 엣지",
            "운명의 부름",
            "더 데빌",
            "셀레스티얼 레인",
            "시크릿 가든",
            "포 카드",
            "세렌디피티",
        ]
        .iter()
        .enumerate()
        .map(|(id, name)| skill(id as u32, name))
        .collect::<Vec<_>>();
        let cards = [
            "삼두사",
            "광기",
            "부식",
            "환희",
            "뒤틀린 운명",
            "유령",
            "도태",
            "균형",
            "심판",
            "달",
            "별",
            "로열",
            "운명의 수레바퀴",
            "황제",
            "재상",
            "제후",
            "황후의 기사",
            "광대",
        ]
        .iter()
        .enumerate()
        .map(|(id, name)| ArcanaCardInput {
            skill_id: id as u32,
            name: (*name).to_string(),
            auto_learn: true,
            draw_weight: 1.0,
            runtime_effects: Vec::new(),
        })
        .collect::<Vec<_>>();
        let presets = [
            (
                include_str!("../../tests/fixtures/BluntThornStream.yaml"),
                16,
                26,
            ),
            (
                include_str!("../../tests/fixtures/BluntThornStreamInpulma.yaml"),
                15,
                26,
            ),
            (
                include_str!("../../tests/fixtures/TestNoCond.yaml"),
                8,
                18,
            ),
        ];

        for (preset, expected_actions, expected_card_actions) in presets {
            let raw = serde_yaml::from_str::<RawApl>(preset).unwrap();
            if raw._build.starts_with("empress-") {
                assert_eq!(raw.skills.len(), 9); // 일반 스킬 8개 + 초각성 스킬 더 데빌
                assert!(!raw.skills.iter().any(|skill| skill == "리턴"));
            }
            let apl = build(preset, &skills, &cards).unwrap();
            assert_eq!(apl.actions.len(), expected_actions);
            assert_eq!(apl.card_actions.len(), expected_card_actions);
            if raw._build.starts_with("empress-") {
                let devil_id = skills.iter().find(|skill| skill.name == "더 데빌").unwrap().skill_id;
                let twisted = apl.card_actions.iter().find(|action| action.card_name == "뒤틀린 운명").unwrap();
                assert!(matches!(twisted.conditions.as_slice(), [AplCondition::NextSkill(id)] if *id == devil_id));
                let clowns = apl.card_actions.iter().filter(|action| action.card_name == "광대").collect::<Vec<_>>();
                assert_eq!(clowns.len(), 4);
                assert_eq!(clowns.iter().filter(|action| action.conditions.is_empty()).count(), 1);
            }
        }
    }

    #[test]
    fn rejects_invalid_references_and_conditions() {
        let skills = vec![skill(10, "A")];

        assert_eq!(
            build("actions:\n  - skill: 없는 스킬", &skills, &[]).unwrap_err(),
            "APL에 존재하지 않는 스킬: 없는 스킬"
        );
        assert_eq!(
            build(
                "actions:\n  - skill: A\n    conditions:\n      - type: invented",
                &skills,
                &[],
            )
            .unwrap_err(),
            "APL에 지원하지 않는 조건: invented"
        );
        assert_eq!(
            build(
                "actions:\n  - skill: A\n    conditions:\n      - type: time_remaining\n        operator: ~=\n        value: 1",
                &skills,
                &[],
            )
            .unwrap_err(),
            "APL에 지원하지 않는 비교 연산자: ~="
        );
        assert_eq!(
            build(
                "actions:\n  - skill: A\n    conditions:\n      - type: next_skill\n        skill: A",
                &skills,
                &[],
            )
            .unwrap_err(),
            "APL next_skill 조건은 card_actions에서만 사용할 수 있습니다"
        );
        assert!(build(
            "actions:\n  - skill: A\n    conditions:\n      - type: time_remaining\n        operators: '<='\n        value: 1",
            &skills,
            &[],
        )
        .unwrap_err()
        .starts_with("APL YAML 파싱 실패:"));
    }
}
