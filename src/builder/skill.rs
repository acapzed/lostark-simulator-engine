use hashbrown::HashMap;

use super::dto::{CharacterSkillInput, SkillDataInput};
use super::modifier::CompiledModifiers;
use crate::constants::{BACK_HEAD_ATTACK_BONUS, COMBO_PHASE_TIMEOUT_MS};
use crate::profile::{
    BuffSpec, GlobalTrigger, HitCondition, HitTrigger, HitTriggerOn, RuntimeDamageSpec,
    SkillCategory, SkillHit, SkillProfile, SkillSlot, SkillTag, SkillType, StackType, StatField,
    TriggerCondition, TriggerEffect, TriggerFilter,
};

// ---------------------------------------------------------------------------
// SkillPipelineMap — 스킬별 파이프라인 큐 묶음
// ---------------------------------------------------------------------------

/// 스킬 이름 → SkillPipeline 매핑.
/// 각 소스(트라이포드/보석/아크패시브 등)가 외부에서 register한 변경점을 보관한다.
/// 모든 register 완료 후 build_all()로 Vec<SkillProfile>을 확정한다.
pub struct SkillPipelineMap {
    /// skill_name → pipeline
    pipelines: HashMap<String, SkillPipeline>,
}

impl SkillPipelineMap {
    /// 스킬 이름 목록을 받아 빈 파이프라인 맵을 생성한다.
    pub fn new(skill_names: impl IntoIterator<Item = String>) -> Self {
        Self {
            pipelines: skill_names
                .into_iter()
                .map(|name| (name, SkillPipeline::new()))
                .collect(),
        }
    }

    /// 특정 스킬의 파이프라인을 반환한다.
    /// 존재하지 않는 스킬이면 None (APL에 없는 스킬은 건너뜀).
    pub fn get_mut(&mut self, skill_name: &str) -> Option<&mut SkillPipeline> {
        self.pipelines.get_mut(skill_name)
    }

    /// 모든 파이프라인을 실행해 Vec<SkillProfile>을 확정한다.
    ///
    /// - `skill_data`: Frontend가 게임 DB에서 조회해 전달한 스킬 기본 데이터
    /// - `char_skills`: 캐릭터가 장착한 스킬 목록 (레벨, 트라이포드 포함)
    ///
    /// char_skills 순서 기준으로 skill_id를 부여해 결정적(deterministic) ID를 보장한다.
    pub fn build_all(
        mut self,
        skill_data: &[SkillDataInput],
        char_skills: &[CharacterSkillInput],
    ) -> Result<Vec<SkillProfile>, String> {
        let data_by_name: HashMap<&str, &SkillDataInput> =
            skill_data.iter().map(|s| (s.name.as_str(), s)).collect();

        let mut profiles = Vec::with_capacity(char_skills.len());

        for (skill_id, char_skill) in char_skills.iter().enumerate() {
            let data = data_by_name
                .get(char_skill.name.as_str())
                .ok_or_else(|| format!("스킬 데이터 없음: {}", char_skill.name))?;

            // 레벨은 1-indexed → 0-indexed 변환
            let level_idx = char_skill.level.saturating_sub(1) as usize;

            // 해당 스킬의 파이프라인 (없으면 빈 파이프라인)
            let pipeline = self
                .pipelines
                .remove(&char_skill.name)
                .unwrap_or_else(SkillPipeline::new);

            let mut profile =
                build_base_profile(skill_id as u32, &char_skill.name, data, level_idx);
            pipeline.run(&mut profile)?;
            profiles.push(profile);
        }

        Ok(profiles)
    }
}

/// `SkillDataInput`으로부터 파이프라인 적용 전 기본 `SkillProfile`을 생성한다.
fn build_base_profile(
    skill_id: u32,
    name: &str,
    data: &SkillDataInput,
    level_idx: usize,
) -> SkillProfile {
    let phase_timeout_ms = if data.phase_timeout_ms > 0 {
        data.phase_timeout_ms
    } else {
        COMBO_PHASE_TIMEOUT_MS as u32
    };
    let skill_type = match data.skill_control_type.as_str() {
        "Combo" => SkillType::Combo {
            last_phase: data.max_phase.saturating_sub(1),
            phase_timeout_ms,
        },
        "Chain" => SkillType::Chain {
            last_phase: data.max_phase.saturating_sub(1),
            phase_timeout_ms,
        },
        "Holding" => SkillType::Holding { max_hold_ms: 0 },
        "Point" => SkillType::Point,
        _ => SkillType::Normal,
    };

    let slot = match data.skill_slot.as_str() {
        "Awakening" => SkillSlot::Awakening,
        "HyperAwakeningTechniques" => SkillSlot::HyperAwakeningTechniques,
        "SuperAwakening" => SkillSlot::SuperAwakening,
        _ => SkillSlot::Normal,
    };

    let mut resource_costs = HashMap::new();
    if let Some(cost) = &data.resource_cost {
        let v = cost.cost.get(level_idx).copied().unwrap_or(0.0);
        if v > 0.0 {
            resource_costs.insert(cost.name.clone(), v);
        }
    }

    // 스킬 시전 단위 자원 획득 (아이덴티티 등)
    let mut resource_gains = HashMap::new();
    if let Some(gain) = &data.identity_gain {
        if gain.value > 0.0 {
            resource_gains.insert(gain.name.clone(), gain.value);
        }
    }

    let mut hits: Vec<SkillHit> = data
        .hits
        .iter()
        .map(|h| {
            let base_damage = h.fixed_damage.get(level_idx).copied().unwrap_or(0.0);
            let skill_modifier = h.damage_ratio.get(level_idx).copied().unwrap_or(0.0);

            // 히트 단위 아이덴티티 획득 → HitTrigger로 인코딩
            let mut triggers = Vec::new();
            if let Some(gain) = &h.identity_gain {
                if gain.value > 0.0 {
                    triggers.push(HitTrigger {
                        on: HitTriggerOn::OnHit,
                        effect: TriggerEffect::GainResource {
                            resource: gain.name.clone(),
                            amount: gain.value,
                        },
                        chance: 1.0,
                    });
                }
            }
            if h.ultimate_point_gain > 0.0 {
                triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::GainResource {
                        resource: "UltimatePoint".to_string(),
                        amount: h.ultimate_point_gain,
                    },
                    chance: 1.0,
                });
            }
            if h.mp_restore_percent > 0.0 {
                triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::GainResourcePercent {
                        resource: "Mp".to_string(),
                        percent: h.mp_restore_percent,
                    },
                    chance: 1.0,
                });
            }
            for _ in 0..h.draw_card_count {
                triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::DrawCard,
                    chance: h.draw_card_chance,
                });
            }
            if let Some(debuff) = &h.target_crit_rate_debuff {
                triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::ApplyTargetBuff(BuffSpec {
                        id: debuff.buff_id.clone(),
                        effects: HashMap::from([(StatField::CritRate, debuff.percent)]),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: debuff.duration_ms,
                    }),
                    chance: 1.0,
                });
            }
            if let Some(buff) = &h.self_crit_rate_stack {
                triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::ApplyBuff(BuffSpec {
                        id: buff.buff_id.clone(),
                        effects: HashMap::from([(StatField::CritRate, buff.percent_per_stack)]),
                        max_stacks: buff.max_stacks,
                        stack_type: StackType::Refresh,
                        duration_ms: buff.duration_ms,
                    }),
                    chance: 1.0,
                });
            }
            if let Some(buff) = &h.self_crit_damage_buff {
                triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::ApplyBuff(BuffSpec {
                        id: buff.buff_id.clone(),
                        effects: HashMap::from([(StatField::CritDmg, buff.percent)]),
                        max_stacks: 1,
                        stack_type: StackType::Refresh,
                        duration_ms: buff.duration_ms,
                    }),
                    chance: 1.0,
                });
            }
            if h.reset_cooldown_chance > 0.0 {
                triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::ResetSkillCooldown,
                    chance: h.reset_cooldown_chance,
                });
            }
            if let Some(stack) = &h.target_buff_stack_gain {
                triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::ApplyTargetBuffStacks {
                        spec: BuffSpec {
                            id: stack.buff_id.clone(),
                            effects: HashMap::new(),
                            max_stacks: stack.max_stacks,
                            stack_type: StackType::Refresh,
                            duration_ms: stack.duration_ms,
                        },
                        stacks: stack.stacks,
                    },
                    chance: 1.0,
                });
            }
            if let Some(stack) = &h.target_buff_stack_set {
                triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::SetTargetBuffStacks {
                        spec: BuffSpec {
                            id: stack.buff_id.clone(),
                            effects: HashMap::new(),
                            max_stacks: stack.max_stacks,
                            stack_type: StackType::Refresh,
                            duration_ms: stack.duration_ms,
                        },
                        stacks: stack.stacks,
                    },
                    chance: 1.0,
                });
            }
            if let Some(additional) = &h.additional_damage {
                triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::DealDamageAndSetTargetBuffStacks {
                        effect_id: additional.effect_id,
                        hit: RuntimeDamageSpec {
                            hit_id: additional.hit_id.clone(),
                            name: additional.name.clone(),
                            base_damage: additional
                                .fixed_damage
                                .get(level_idx)
                                .copied()
                                .unwrap_or(0.0),
                            skill_modifier: additional
                                .damage_ratio
                                .get(level_idx)
                                .copied()
                                .unwrap_or(0.0),
                            damage_increase: 0.0,
                            crit_chance_bonus: 0.0,
                            crit_damage_bonus: 0.0,
                            random_crit_damage_chance: 0.0,
                            random_crit_damage_bonus: 0.0,
                            defense_ignore: h.defense_ignore,
                            defense_ignore_chance: h.defense_ignore_chance,
                        },
                        spec: BuffSpec {
                            id: additional.target_buff_id.clone(),
                            effects: HashMap::new(),
                            max_stacks: additional.target_buff_max_stacks,
                            stack_type: StackType::Refresh,
                            duration_ms: additional.target_buff_duration_ms,
                        },
                        stacks: additional.target_buff_stacks,
                    },
                    chance: additional.chance,
                });
            }
            if let Some(dot) = &h.dot {
                triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::ScheduleDot {
                        dot_id: dot.dot_id.clone(),
                        hit: RuntimeDamageSpec {
                            hit_id: dot.hit_id.clone(),
                            name: dot.name.clone(),
                            base_damage: dot.fixed_damage.get(level_idx).copied().unwrap_or(0.0),
                            skill_modifier: dot.damage_ratio.get(level_idx).copied().unwrap_or(0.0),
                            damage_increase: 0.0,
                            crit_chance_bonus: 0.0,
                            crit_damage_bonus: 0.0,
                            random_crit_damage_chance: 0.0,
                            random_crit_damage_bonus: 0.0,
                            defense_ignore: h.defense_ignore,
                            defense_ignore_chance: h.defense_ignore_chance,
                        },
                        first_tick_ms: dot.first_tick_ms,
                        tick_interval_ms: dot.tick_interval_ms,
                        tick_count: dot.tick_count,
                        refresh: false,
                    },
                    chance: dot.chance,
                });
            }

            SkillHit {
                effect_id: h.effect_id,
                hit_id: h.hit_id.clone(),
                name: h.name.clone(),
                base_damage,
                skill_modifier,
                action_delay_ms: h.action_delay_ms,
                fixed_delay_ms: h.fixed_delay_ms,
                counter_attack: h.counter_attack,
                phase: h.phase,
                crit_chance_bonus: h.crit_chance_bonus,
                crit_damage_bonus: h.crit_damage_bonus,
                defense_ignore: h.defense_ignore,
                defense_ignore_chance: h.defense_ignore_chance,
                damage_increase: h.damage_increase,
                stagger: 0.0,
                destruction: 0,
                use_snapshot: false,
                triggers,
            }
        })
        .collect();

    for stack in &data.runtime_skill_damage_stacks {
        let buff = BuffSpec {
            id: stack.buff_id.clone(),
            effects: HashMap::new(),
            max_stacks: stack.max_stacks,
            stack_type: StackType::Refresh,
            duration_ms: stack.duration_ms,
        };
        for hit in &mut hits {
            hit.triggers.push(HitTrigger {
                on: HitTriggerOn::OnHit,
                effect: TriggerEffect::DamageBonusPerBuffStack {
                    buff_id: stack.buff_id.clone(),
                    per_stack: stack.damage_increase_per_stack,
                },
                chance: 1.0,
            });
            if stack.trigger_effect_ids.is_empty()
                || stack.trigger_effect_ids.contains(&hit.effect_id)
            {
                hit.triggers.push(HitTrigger {
                    on: if stack.trigger_on == "crit" {
                        HitTriggerOn::OnCrit
                    } else {
                        HitTriggerOn::OnHit
                    },
                    effect: TriggerEffect::ApplyBuff(buff.clone()),
                    chance: 1.0,
                });
            }
        }
    }

    for bonus in &data.runtime_target_damage_bonuses {
        let buff = BuffSpec {
            id: bonus.buff_id.clone(),
            effects: HashMap::new(),
            max_stacks: 1,
            stack_type: StackType::Refresh,
            duration_ms: bonus.duration_ms,
        };
        for hit in &mut hits {
            if hit.effect_id == bonus.trigger_effect_id {
                hit.triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHit,
                    effect: TriggerEffect::ApplyTargetBuff(buff.clone()),
                    chance: 1.0,
                });
            }
            if bonus.target_effect_ids.contains(&hit.effect_id) {
                hit.triggers.push(HitTrigger {
                    on: HitTriggerOn::OnHitIf(HitCondition::TargetBuffActive(
                        bonus.buff_id.clone(),
                    )),
                    effect: TriggerEffect::DamageBonus(bonus.damage_increase),
                    chance: 1.0,
                });
            }
        }
    }

    let runtime_damage_specs = runtime_damage_specs_by_target_buff(data, level_idx);
    for hit in &mut hits {
        if !data.ruin_trigger_effect_ids.contains(&hit.effect_id) {
            continue;
        }
        for (target_buff_id, runtime_hits) in &runtime_damage_specs {
            hit.triggers.push(HitTrigger {
                on: HitTriggerOn::OnHit,
                effect: TriggerEffect::DealDamageByTargetBuffStacks {
                    buff_id: target_buff_id.clone(),
                    hits_by_stack: runtime_hits.clone(),
                    forced_stacks_while_buff: None,
                },
                chance: 1.0,
            });
            hit.triggers.push(HitTrigger {
                on: HitTriggerOn::OnHit,
                effect: TriggerEffect::ConsumeTargetBuff {
                    buff_id: target_buff_id.clone(),
                    preserve_chance: data
                        .target_buff_consume_preserve
                        .as_ref()
                        .filter(|preserve| preserve.buff_id == *target_buff_id)
                        .map(|preserve| preserve.chance)
                        .unwrap_or(0.0),
                    preserve_required_stacks: data
                        .target_buff_consume_preserve
                        .as_ref()
                        .filter(|preserve| preserve.buff_id == *target_buff_id)
                        .map(|preserve| preserve.required_stacks)
                        .unwrap_or(0),
                    preserve_resource_gain: data
                        .target_buff_consume_preserve
                        .as_ref()
                        .and_then(|preserve| preserve.preserve_identity_gain.as_ref())
                        .map(|gain| (gain.name.clone(), gain.value)),
                },
                chance: 1.0,
            });
        }
    }

    // 슬롯 기반 기본 태그 — meta phase 클로저로 덮어쓸 수 있다.
    let mut tags = match &slot {
        SkillSlot::Awakening => vec![SkillTag::CategoryAwakening],
        SkillSlot::HyperAwakeningTechniques => vec![SkillTag::CategoryHyper],
        SkillSlot::SuperAwakening => vec![
            SkillTag::CategoryAwakening,
            SkillTag::CategorySuperAwakening,
        ],
        SkillSlot::Normal => vec![SkillTag::CategoryNormal],
    };
    if data.game_skill_id != 0 {
        tags.push(SkillTag::SkillId(data.game_skill_id));
    }
    let category = match data.identity_category {
        29 => SkillCategory::Stacked,
        30 => SkillCategory::Ruin,
        _ => SkillCategory::Normal,
    };
    match category {
        SkillCategory::Stacked => {
            tags.retain(|tag| !matches!(tag, SkillTag::CategoryNormal));
            tags.push(SkillTag::CategoryStacked);
        }
        SkillCategory::Ruin => {
            tags.retain(|tag| !matches!(tag, SkillTag::CategoryNormal));
            tags.push(SkillTag::CategoryRuin);
        }
        SkillCategory::Normal => {
            if data.identity_category == 28 && !tags.contains(&SkillTag::CategoryNormal) {
                tags.push(SkillTag::CategoryNormal);
            }
        }
    }
    tags.extend(
        data.properties
            .skill_group_ids
            .iter()
            .copied()
            .map(SkillTag::SkillGroup),
    );

    // 공격 방향 태그
    match data.properties.attack_type.as_str() {
        "백 어택" => tags.push(SkillTag::AttackBack),
        "헤드 어택" => tags.push(SkillTag::AttackHead),
        _ => {
            tags.push(SkillTag::NonDirectional);
            if !matches!(slot, SkillSlot::Awakening | SkillSlot::SuperAwakening) {
                tags.push(SkillTag::NonDirectionalNonAwakening);
            }
        }
    };
    if data.skill_control_type == "Holding" {
        tags.push(SkillTag::HoldingCasting);
    }
    if data.skill_control_type == "Charge" {
        tags.push(SkillTag::Charging);
    }
    if resource_costs.contains_key("Mp") || data.base_mana_cost > 0.0 {
        tags.push(SkillTag::UsesMana);
    }

    // 속성 태그
    match data.properties.element.as_str() {
        "화" => tags.push(SkillTag::ElementFire),
        "수" => tags.push(SkillTag::ElementWater),
        "전" => tags.push(SkillTag::ElementElectricity),
        "풍" => tags.push(SkillTag::ElementWind),
        "암" => tags.push(SkillTag::ElementDark),
        "성" => tags.push(SkillTag::ElementHoly),
        _ => {}
    };

    SkillProfile {
        skill_id,
        name: name.to_string(),
        skill_type,
        category,
        slot,
        tags,
        cooldown_ms: (data.cooldown * 1000.0) as u32,
        cast_times_ms: data.cast_times_ms.clone(),
        max_uses: data.max_uses,
        max_stacks: data.max_stacks,
        charge_recovery_ms: 0,
        resource_costs,
        extra_mp_cost_ratio: 0.0,
        mp_cost_waiver_chance: data.mp_cost_waiver_chance,
        resource_gains,
        cast_buff_damage_bonuses: Vec::new(),
        cast_buff_crit_rate_bonuses: Vec::new(),
        evolution_damage_bonuses: Vec::new(),
        hits,
    }
}

fn runtime_damage_specs_by_target_buff(
    data: &SkillDataInput,
    level_idx: usize,
) -> Vec<(String, Vec<RuntimeDamageSpec>)> {
    let mut grouped: HashMap<String, Vec<(u32, RuntimeDamageSpec)>> = HashMap::new();
    for row in &data.runtime_damage_by_target_buff_stacks {
        if row.target_buff_id.is_empty() || row.stack == 0 {
            continue;
        }
        grouped
            .entry(row.target_buff_id.clone())
            .or_default()
            .push((
                row.stack,
                RuntimeDamageSpec {
                    hit_id: row.hit_id.clone(),
                    name: row.name.clone(),
                    base_damage: row.fixed_damage.get(level_idx).copied().unwrap_or(0.0),
                    skill_modifier: row.damage_ratio.get(level_idx).copied().unwrap_or(0.0),
                    damage_increase: row.damage_increase,
                    crit_chance_bonus: row.crit_chance_bonus,
                    crit_damage_bonus: row.crit_damage_bonus,
                    random_crit_damage_chance: row.random_crit_damage_chance,
                    random_crit_damage_bonus: row.random_crit_damage_bonus,
                    defense_ignore: row.defense_ignore,
                    defense_ignore_chance: row.defense_ignore_chance,
                },
            ));
    }

    grouped
        .into_iter()
        .map(|(target_buff_id, mut specs)| {
            specs.sort_by_key(|(stack, _)| *stack);
            let max_stack = specs.iter().map(|(stack, _)| *stack).max().unwrap_or(0) as usize;
            let mut by_stack = vec![
                RuntimeDamageSpec {
                    hit_id: String::new(),
                    name: String::new(),
                    base_damage: 0.0,
                    skill_modifier: 0.0,
                    damage_increase: 0.0,
                    crit_chance_bonus: 0.0,
                    crit_damage_bonus: 0.0,
                    random_crit_damage_chance: 0.0,
                    random_crit_damage_bonus: 0.0,
                    defense_ignore: 0.0,
                    defense_ignore_chance: 0.0,
                };
                max_stack
            ];
            for (stack, spec) in specs {
                by_stack[stack as usize - 1] = spec;
            }
            (target_buff_id, by_stack)
        })
        .collect()
}

/// 태그 기반 modifier를 매칭 스킬의 각 히트에 적용한다.
///
/// - `DamageMultiplier`: 컴파일 결과(multiplier)에서 1을 뺀 값을 `hit.damage_increase`에 누적
/// - `CooldownReduction`: 컴파일 결과(multiplier)를 `cooldown_ms`에 직접 곱함
///
/// 반드시 `build_all()` 완료 후 호출해야 한다 (태그 확정 필요).
pub fn apply_modifiers(skills: &mut Vec<SkillProfile>, compiled: &CompiledModifiers) {
    for skill in skills.iter_mut() {
        // 태그별 DamageMultiplier는 독립 곱산 후 히트에 적용
        // (예: BackAttack×1.12 * CategoryNormal×1.15 = 1.288, 합산 아님)
        let mut dmg_multiplier = compiled.get_multiplier(&SkillTag::All, "DamageMultiplier");
        let mut cd_multiplier = compiled.get_multiplier(&SkillTag::All, "CooldownReduction");
        let mut cast_multiplier = compiled.get_multiplier(&SkillTag::All, "CastTimeReduction");

        // 백/헤드 어택 기본 보너스 (+5%)
        if skill.tags.contains(&SkillTag::AttackBack) || skill.tags.contains(&SkillTag::AttackHead)
        {
            dmg_multiplier *= 1.0 + BACK_HEAD_ATTACK_BONUS;
        }

        for tag in &skill.tags {
            let dmg_mul = compiled.get_multiplier(tag, "DamageMultiplier");
            dmg_multiplier *= dmg_mul;

            let cd_mul = compiled.get_multiplier(tag, "CooldownReduction");
            if (cd_mul - 1.0).abs() > f64::EPSILON {
                cd_multiplier *= cd_mul;
            }
            let cast_mul = compiled.get_multiplier(tag, "CastTimeReduction");
            if (cast_mul - 1.0).abs() > f64::EPSILON {
                cast_multiplier *= cast_mul;
            }
        }

        if (dmg_multiplier - 1.0).abs() > f64::EPSILON {
            for hit in &mut skill.hits {
                hit.damage_increase += dmg_multiplier - 1.0;
            }
        }

        if (cd_multiplier - 1.0).abs() > f64::EPSILON {
            skill.cooldown_ms = (skill.cooldown_ms as f64 * cd_multiplier) as u32;
        }
        if (cast_multiplier - 1.0).abs() > f64::EPSILON {
            for duration in &mut skill.cast_times_ms {
                *duration = (*duration as f64 * cast_multiplier).round() as u32;
            }
        }
    }
}

/// 스킬 데이터에 명시된 시전 시점 버프를 수집한다.
pub fn apply_triggers(
    skill_data: &[SkillDataInput],
    char_skills: &[CharacterSkillInput],
) -> Vec<GlobalTrigger> {
    char_skills
        .iter()
        .enumerate()
        .filter_map(|(skill_id, skill)| {
            let buff = skill_data
                .iter()
                .find(|data| data.name == skill.name)?
                .self_speed_buff_on_cast
                .as_ref()?;
            Some(GlobalTrigger {
                filter: TriggerFilter::SkillCastId(skill_id as u32),
                condition: TriggerCondition::Always,
                effect: TriggerEffect::ApplyBuff(BuffSpec {
                    id: buff.buff_id.clone(),
                    effects: HashMap::from([
                        (StatField::AttackSpeed, buff.attack_speed_percent / 100.0),
                        (StatField::MovementSpeed, buff.move_speed_percent / 100.0),
                    ]),
                    max_stacks: 1,
                    stack_type: StackType::Refresh,
                    duration_ms: buff.duration_ms,
                }),
                cooldown_ms: None,
            })
        })
        .collect()
}

/// 확정된 Arcana raw 특화 공식을 카드 게이지와 루인 스택 피해에 적용한다.
pub fn apply_arcana_specialization(skills: &mut [SkillProfile], specialization: f64) {
    let card_gauge_multiplier = 1.0 + specialization / 699.0 * 0.25;
    let ruin_damage_multiplier = 1.0 + specialization / 699.0 * 0.35;
    let awakening_damage_multiplier = 1.0 + specialization / 699.0 * 0.1528;
    for skill in skills {
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
                    TriggerEffect::DealDamageByTargetBuffStacks { hits_by_stack, .. }
                        if skill.category == SkillCategory::Ruin =>
                    {
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

// ---------------------------------------------------------------------------
// SkillPipeline
// ---------------------------------------------------------------------------

/// 스킬 하나를 변환하는 단계별 파이프라인.
///
/// ```text
/// Phase 0 structure  — 히트 배열 재구성 (타수, 페이즈 구조 변경)
/// Phase 1 stats      — 수치 수정 (damage, crit, cooldown, cast_time)
/// Phase 2 triggers   — HitTrigger 등록
/// Phase 3 meta       — 카테고리/태그 확정
/// ```
pub struct SkillPipeline {
    structure: Vec<Box<dyn FnOnce(&mut SkillProfile) -> Result<(), String>>>,
    stats: Vec<Box<dyn FnOnce(&mut SkillProfile) -> Result<(), String>>>,
    triggers: Vec<Box<dyn FnOnce(&mut SkillProfile) -> Result<(), String>>>,
    meta: Vec<Box<dyn FnOnce(&mut SkillProfile) -> Result<(), String>>>,
}

impl SkillPipeline {
    pub fn new() -> Self {
        Self {
            structure: Vec::new(),
            stats: Vec::new(),
            triggers: Vec::new(),
            meta: Vec::new(),
        }
    }

    pub fn add_structure<F>(&mut self, f: F)
    where
        F: FnOnce(&mut SkillProfile) -> Result<(), String> + 'static,
    {
        self.structure.push(Box::new(f));
    }

    pub fn add_stats<F>(&mut self, f: F)
    where
        F: FnOnce(&mut SkillProfile) -> Result<(), String> + 'static,
    {
        self.stats.push(Box::new(f));
    }

    pub fn add_trigger<F>(&mut self, f: F)
    where
        F: FnOnce(&mut SkillProfile) -> Result<(), String> + 'static,
    {
        self.triggers.push(Box::new(f));
    }

    pub fn add_meta<F>(&mut self, f: F)
    where
        F: FnOnce(&mut SkillProfile) -> Result<(), String> + 'static,
    {
        self.meta.push(Box::new(f));
    }

    /// 4단계 순서대로 모든 step을 실행한다.
    pub fn run(self, skill: &mut SkillProfile) -> Result<(), String> {
        for step in self.structure {
            step(skill)?;
        }
        for step in self.stats {
            step(skill)?;
        }
        for step in self.triggers {
            step(skill)?;
        }
        for step in self.meta {
            step(skill)?;
        }
        Ok(())
    }
}
