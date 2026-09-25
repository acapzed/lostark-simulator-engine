use crate::builder::dto::{BraceletEffectInput, BuildRequest};
use crate::builder::modifier::{Modifier, ModifierManager, OpType};
use crate::builder::stat::StatBuilder;
use crate::profile::{
    BuffSpec, GlobalTrigger, SkillTag, StackType, StatField,
    TriggerCondition, TriggerEffect, TriggerFilter,
};

pub fn register(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    req: &BuildRequest,
) {
    let Some(bracelet) = &req.character.bracelet else { return };

    let target_type = req.target_monster_type.as_str();

    for effect in bracelet.locked_effects.iter().chain(&bracelet.changeable_effects) {
        apply_effect(
            stat,
            modifier,
            global_triggers,
            effect,
            target_type,
            req.assumed_target_staggered,
            req.assumed_low_hp,
        );
    }
}

fn apply_effect(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    effect: &BraceletEffectInput,
    target_type: &str,
    target_staggered: bool,
    assumed_low_hp: bool,
) {
    let Some(&val) = effect.values.first() else { return };

    // ── name 기반 매칭 ────────────────────────────────────────────────────
    match effect.name.as_str() {

        // ── 기본 효과 ─────────────────────────────────────────────────────
        "힘, 민첩, 지능" => {
            stat.add_flat("bracelet:주스탯", move |s| s.base_main_stat += val);
            return;
        }
        "체력" => {
            stat.add_flat("bracelet:체력", move |s| s.base_vitality += val);
            return;
        }

        // ── 특수 효과 — StatBuilder ────────────────────────────────────────
        "무기 공격력" => {
            stat.add_stat(StatField::WeaponAttackPower, "bracelet:무기공격력", val);
            return;
        }
        "추가 피해" => {
            stat.add_stat(StatField::AdditionalDamage, "bracelet:추가피해", val / 100.0);
            return;
        }
        "추가 피해 및 악마/대악마 피해 증가" => {
            stat.add_stat(StatField::AdditionalDamage, "bracelet:추가피해", val / 100.0);
            // values[1]: 악마/대악마 계열 피해 증가 → type_damage 독립 곱산 버킷
            let type_val = effect.values.get(1).copied().unwrap_or(0.0);
            if type_val > 0.0 && matches!(target_type, "악마" | "대악마") {
                stat.add_stat(StatField::TypeDamage, "bracelet:계열피해(악마)", type_val / 100.0);
            }
            return;
        }
        "치명타 적중률" => {
            stat.add_stat(StatField::CritRate, "bracelet:치적률", val);
            return;
        }
        "치명타 적중률 및 적중 시 피해 증가" => {
            stat.add_stat(StatField::CritRate, "bracelet:치적률", val);
            // values[1]: 치명타 적중 시 피해 증가 (고정 1.5%)
            let bonus = effect.values.get(1).copied().unwrap_or(0.0);
            if bonus > 0.0 {
                stat.add_flat("bracelet:치적시피해", move |s| s.on_crit_damage_bonus += bonus / 100.0);
            }
            return;
        }
        "치명타 피해" => {
            stat.add_stat(StatField::CritDmg, "bracelet:치적피해", val);
            return;
        }
        "치명타 피해 및 적중 시 피해 증가" => {
            stat.add_stat(StatField::CritDmg, "bracelet:치적피해", val);
            // values[1]: 치명타 적중 시 피해 증가 (고정 1.5%)
            let bonus = effect.values.get(1).copied().unwrap_or(0.0);
            if bonus > 0.0 {
                stat.add_flat("bracelet:치적시피해", move |s| s.on_crit_damage_bonus += bonus / 100.0);
            }
            return;
        }
        "최대 생명력" => {
            stat.add_flat("bracelet:최대HP", move |s| s.max_hp += val);
            return;
        }
        "공격 및 이동 속도 증가" => {
            stat.add_stat(StatField::AttackSpeed, "bracelet:공격속도", val / 100.0);
            stat.add_stat(StatField::MovementSpeed, "bracelet:이동속도", val / 100.0);
            return;
        }
        // ── 특수 효과 — ModifierManager ───────────────────────────────────
        "백어택 스킬 피해 증가" => {
            modifier.add_modifier(Modifier {
                target_tag: SkillTag::AttackBack,
                stat_name:  "DamageMultiplier".to_string(),
                value:      val / 100.0,
                op_type:    OpType::Multiply,
                source:     "bracelet:백어택".to_string(),
            });
            return;
        }
        "헤드어택 스킬 피해 증가" => {
            modifier.add_modifier(Modifier {
                target_tag: SkillTag::AttackHead,
                stat_name:  "DamageMultiplier".to_string(),
                value:      val / 100.0,
                op_type:    OpType::Multiply,
                source:     "bracelet:헤드어택".to_string(),
            });
            return;
        }
        "비방향성 공격 스킬 피해 증가" => {
            modifier.add_modifier(Modifier {
                target_tag: SkillTag::NonDirectionalNonAwakening,
                stat_name:  "DamageMultiplier".to_string(),
                value:      val / 100.0,
                op_type:    OpType::Multiply,
                source:     "bracelet:비방향성".to_string(),
            });
            return;
        }
        "피해 증가" => {
            stat.add_stat(StatField::DamageIncrease, "bracelet:피해증가", val / 100.0);
            return;
        }
        "피해 증가 및 무력화 시 피해 증가" => {
            stat.add_stat(StatField::DamageIncrease, "bracelet:피해증가", val / 100.0);
            if target_staggered {
                let stagger_bonus = effect.values.get(1).copied().unwrap_or(0.0);
                stat.add_stat(
                    StatField::DamageIncrease,
                    "bracelet:무력화피해증가",
                    stagger_bonus / 100.0,
                );
            }
            return;
        }
        "피해 증가 (쿨타임 패널티)" => {
            stat.add_stat(StatField::DamageIncrease, "bracelet:피해증가_쿨패널티", val / 100.0);
            stat.add_flat("bracelet:쿨타임패널티", |s| s.cooldown_penalty += 0.02);
            return;
        }

        // ── 조건부/중첩 무기공격력 ────────────────────────────────────────
        "무기 공격력 증가 (중첩)" => {
            // values[0]: 기본 무기공격력, values[1]: 중첩당 무기공격력
            stat.add_stat(StatField::WeaponAttackPower, "bracelet:무기공격력(중첩기본)", val);
            let stack_val = effect.values.get(1).copied().unwrap_or(0.0);
            if stack_val > 0.0 {
                global_triggers.push(GlobalTrigger {
                    filter:      TriggerFilter::AnyHit,
                    condition:   TriggerCondition::Always,
                    effect:      TriggerEffect::ApplyBuff(BuffSpec {
                        id:         "bracelet_weapon_ap_stack".into(),
                        effects:    hashbrown::HashMap::from([
                            (StatField::WeaponAttackPower, stack_val),
                        ]),
                        max_stacks: 30,
                        stack_type: StackType::Refresh,
                        duration_ms: 120_000,
                    }),
                    cooldown_ms: Some(30_000),
                });
            }
            return;
        }
        "무기 공격력 증가 (체력 조건)" => {
            // values[0]: 기본 무기공격력, values[1]: 체력 50% 이상 시 버프값
            stat.add_stat(StatField::WeaponAttackPower, "bracelet:무기공격력(체력조건기본)", val);
            let cond_val = effect.values.get(1).copied().unwrap_or(0.0);
            if cond_val > 0.0 && !assumed_low_hp {
                global_triggers.push(GlobalTrigger {
                    filter:      TriggerFilter::AnyHit,
                    condition:   TriggerCondition::Always,
                    effect:      TriggerEffect::ApplyBuff(BuffSpec {
                        id:         "bracelet_weapon_ap_hp_cond".into(),
                        effects:    hashbrown::HashMap::from([
                            (StatField::WeaponAttackPower, cond_val),
                        ]),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: 5_000,
                    }),
                    cooldown_ms: None,
                });
            }
            return;
        }
        "공격 적중 시 무기 공격력 및 공이속 증가 (중첩)" => {
            // values[0]: 중첩당 무기공격력. 공이속 1% 고정. 최대 6중첩, 10초, 매 초마다
            global_triggers.push(GlobalTrigger {
                filter:      TriggerFilter::AnyHit,
                condition:   TriggerCondition::Always,
                effect:      TriggerEffect::ApplyBuff(BuffSpec {
                    id:         "bracelet_weapon_ap_speed_stack".into(),
                    effects:    hashbrown::HashMap::from([
                        (StatField::WeaponAttackPower, val),
                        (StatField::AttackSpeed, 0.01),
                        (StatField::MovementSpeed, 0.01),
                    ]),
                    max_stacks: 6,
                    stack_type: StackType::Refresh,
                    duration_ms: 10_000,
                }),
                cooldown_ms: Some(1_000),
            });
            return;
        }

        // ── 무시 — 서폿 전용 ──────────────────────────────────────────────
        "아군 공격력 강화 효과"
        | "아군 피해량 강화 효과"
        | "파티원 보호 및 회복 효과"
        | "방어력 감소 및 아군 공격력 강화"
        | "보호/회복 효과 적용 대상 피해 증가 및 아군 공격력 강화"
        | "치명타 저항 감소 및 아군 공격력 강화"
        | "치명타 피해 저항 감소 및 아군 공격력 강화" => {
            return;
        }

        // ── 무시 — DPS 무관 (주석으로 식별 가능하게 유지) ────────────────
        "물리 방어력" | "마법 방어력" => {
            // TODO(무시): 방어력 — DPS 무관
            return;
        }
        "전투 중 생명력 회복량" => {
            // TODO(무시): 생명력 회복 — DPS 무관
            return;
        }
        "경직 및 피격 이상 면역" => {
            // TODO(무시): 경직/피격 이상 면역 — DPS 무관
            return;
        }
        "이동기 및 기상기 재사용 대기 시간" => {
            // TODO(무시): 이동기/기상기 쿨감 — DPS 무관
            return;
        }
        "시드 등급 이하 몬스터 피해 감소" | "시드 등급 이하 몬스터 피해 증가" => {
            // TODO(무시): 특정 컨텐츠 한정 효과
            return;
        }

        _ => {}
    }

    // ── description 기반 매칭 — 전투 특성 (name이 빈 문자열인 경우) ──────
    let desc = effect.descriptions.first().map(|s| s.as_str()).unwrap_or("");

    if      desc.contains("치명") { stat.add_flat("bracelet:치명", move |s| s.crit           += val); }
    else if desc.contains("특화") { stat.add_flat("bracelet:특화", move |s| s.specialization += val); }
    else if desc.contains("신속") { stat.add_flat("bracelet:신속", move |s| s.swiftness       += val); }
    else if desc.contains("제압") { stat.add_flat("bracelet:제압", move |s| s.domination      += val); }
    else if desc.contains("인내") { stat.add_flat("bracelet:인내", move |s| s.endurance       += val); }
    else if desc.contains("숙련") { stat.add_flat("bracelet:숙련", move |s| s.expertise       += val); }
    else {
        #[cfg(debug_assertions)]
        eprintln!("[bracelet] 알 수 없는 효과: name={:?} desc={:?}", effect.name, desc);
    }
}
