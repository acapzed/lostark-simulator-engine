use crate::constants::{
    DamageFactorTrace, DamageSourceTrace, BASE_CRIT_MULTIPLIER, CRIT_STAT_CONVERSION,
    MAX_ATTACK_SPEED_BONUS, MAX_COOLDOWN_REDUCTION, MAX_MOVEMENT_SPEED_BONUS,
    SWIFTNESS_ATTACK_SPEED_BONUS, SWIFTNESS_COOLDOWN_REDUCTION,
};
use crate::engine::state::ActorState;
use crate::profile::{CharacterProfiles, SkillProfile, SkillSlot, StatField};

/// 캐스트 시점에 캡처하는 스냅샷.
///
#[derive(Debug, Clone)]
pub struct CastSnapshot {
    /// 비용 면제까지 반영한 이 캐스트의 실제 MP 소모량.
    pub paid_mp: f64,
    /// 유효 공격력 = (√(주스탯 × 무기공격력 ÷ 6) + base_attack_power) × attack_power_mul
    pub attack_power: f64,
    /// 상한 적용 전 기본 치적률. 스킬 고유 보너스는 히트 단위에서 합산.
    pub uncapped_crit_rate: f64,
    pub crit_rate_cap: f64,
    pub excess_crit_evolution_coefficient: f64,
    pub excess_crit_evolution_cap: f64,
    /// 기본 치적배. 스킬 고유 보너스는 히트 단위에서 합산.
    pub crit_damage: f64,
    /// 추가 피해 multiplier (1.0 기준)
    pub additional_damage: f64,
    /// 진화형 피해 multiplier
    pub evolution_damage: f64,
    /// 타겟 디버프 피해 multiplier
    pub target_damage_increase: f64,
    /// 전체 피해 증가 multiplier
    pub damage_increase: f64,
    /// 돌격대장: 캐스트 시점 이동속도 증가량으로 계산한 배율.
    pub movement_speed_damage: f64,
    /// 스킬 전용 Buff 피해 증가는 캐스트 시점 조건을 유지한다.
    pub cast_buff_damage_bonus: f64,
    /// 계열(종족) 피해 multiplier
    pub type_damage: f64,
    /// 속성 피해 multiplier
    pub elemental_damage: f64,
    /// 치명타 적중 시 추가 피해 (0.015 = 1.5%). calculate_hit에서 is_crit일 때만 곱산.
    pub on_crit_damage_bonus: f64,
    pub random_damage_reduction_chance: f64,
    pub random_damage_reduction: f64,
    /// 시뮬레이션 대상의 방어력.
    pub target_defense: f64,
    /// trace 모드에서 데미지 식 출처를 보여주기 위한 source breakdown.
    pub damage_factors: Vec<DamageFactorTrace>,
    /// 신속 보정 후 실제 시전 시간 (ms)
    pub actual_cast_time_ms: u32,
    /// 신속 보정 후 실제 쿨타임 (ms)
    pub actual_cooldown_ms: u32,
}

/// 캐스트 시점에 스냅샷을 캡처한다.
pub fn capture(
    profiles: &CharacterProfiles,
    skill: &SkillProfile,
    phase: u32,
    state: &ActorState,
    include_source_breakdown: bool,
) -> CastSnapshot {
    let stat = &profiles.stat;

    // ── 버프 delta 계산 ───────────────────────────────────────────────────
    // 활성 버프의 (field → stacks × delta) 를 합산한다.
    // StatProfile은 불변이므로 delta를 별도로 계산 후 합산.
    let buff_delta = |field: StatField| -> f64 {
        state
            .buff_manager
            .active_buffs(state.current_time)
            .chain(state.target_buff_manager.active_buffs(state.current_time))
            .filter_map(|(spec, stacks)| spec.effects.get(&field).map(|v| v * stacks as f64))
            .sum::<f64>()
    };

    // ── 소스 합산 헬퍼 ────────────────────────────────────────────────────
    // 정적(빌드타임) 기여분 + 동적(버프) 기여분
    let total = |field: StatField| stat.sum(field) + buff_delta(field);
    let factor = |name: &str, field: StatField, value: f64, operation: &str| -> DamageFactorTrace {
        let mut sources: Vec<DamageSourceTrace> = stat
            .field_ref(field)
            .iter()
            .map(|(source, value)| DamageSourceTrace {
                source: source.clone(),
                value: *value,
                stacks: None,
            })
            .collect();

        for (spec, stacks) in state.buff_manager.active_buffs(state.current_time) {
            if let Some(value) = spec.effects.get(&field) {
                sources.push(DamageSourceTrace {
                    source: format!("buff:{}", spec.id),
                    value: *value * stacks as f64,
                    stacks: Some(stacks),
                });
            }
        }
        for (spec, stacks) in state.target_buff_manager.active_buffs(state.current_time) {
            if let Some(value) = spec.effects.get(&field) {
                sources.push(DamageSourceTrace {
                    source: format!("target_debuff:{}", spec.id),
                    value: *value * stacks as f64,
                    stacks: Some(stacks),
                });
            }
        }

        DamageFactorTrace {
            name: name.to_string(),
            value,
            operation: operation.to_string(),
            sources,
        }
    };

    // ── 유효 공격력 ──────────────────────────────────────────────────────
    let main_stat = stat.base_main_stat * stat.main_stat_mul;
    let weapon_ap = total(StatField::WeaponAttackPower) * stat.weapon_ap_mul;
    // C. 순수 공격력 = √(주스탯 × 무기공격력 ÷ 6)
    let pure_ap = if main_stat > 0.0 && weapon_ap > 0.0 {
        (main_stat * weapon_ap / 6.0).sqrt()
    } else {
        0.0
    };
    // D. 기본 공격력 = 순수 공격력 × (1 + 보석/스톤 기본공격력 증가율)
    let base_ap = pure_ap * stat.base_ap_mul;
    // E. 최종 공격력 = (D + 공격력 (+)) × (1 + 공격력 증가율%)
    let attack_power_mul = 1.0 + total(StatField::AttackPowerMul);
    let attack_power = (base_ap + stat.base_attack_power) * attack_power_mul;

    // ── 신속 → 쿨타임 감소 / 공격 속도 ──────────────────────────────────
    let buff_cd_reduction = if skill.slot == SkillSlot::Normal {
        total(StatField::CooldownReduction)
    } else {
        0.0
    };
    let cd_reduction = (stat.swiftness * SWIFTNESS_COOLDOWN_REDUCTION + buff_cd_reduction)
        .min(MAX_COOLDOWN_REDUCTION);
    let raw_attack_speed =
        stat.swiftness * SWIFTNESS_ATTACK_SPEED_BONUS + total(StatField::AttackSpeed);
    let attack_speed = raw_attack_speed.clamp(-MAX_ATTACK_SPEED_BONUS, MAX_ATTACK_SPEED_BONUS);

    let actual_cooldown_ms =
        (skill.cooldown_ms as f64 * (1.0 - cd_reduction) * (1.0 + stat.cooldown_penalty)) as u32;
    let base_cast_ms = skill
        .cast_times_ms
        .get(phase as usize)
        .or_else(|| skill.cast_times_ms.first())
        .copied()
        .unwrap_or(0);
    let actual_cast_time_ms = (base_cast_ms as f64 / (1.0 + attack_speed)) as u32;

    // ── 치명타 ───────────────────────────────────────────────────────────
    let cast_buff_crit_rate_bonus: f64 = skill
        .cast_buff_crit_rate_bonuses
        .iter()
        .filter(|(id, _)| state.buff_manager.is_active(id, state.current_time))
        .map(|(_, value)| value)
        .sum();
    let uncapped_crit_rate = stat.crit / CRIT_STAT_CONVERSION
        + total(StatField::CritRate) / 100.0
        + cast_buff_crit_rate_bonus;
    let crit_rate = uncapped_crit_rate.clamp(0.0, stat.crit_rate_cap);
    let crit_damage = BASE_CRIT_MULTIPLIER + total(StatField::CritDmg) / 100.0;

    let crit_rate_factor = include_source_breakdown.then(|| {
        let mut factor = factor("critRate", StatField::CritRate, crit_rate, "additiveRate");
        for source in &mut factor.sources {
            source.value /= 100.0;
        }
        if stat.crit != 0.0 {
            factor.sources.insert(
                0,
                DamageSourceTrace {
                    source: "stat:crit".to_string(),
                    value: stat.crit / CRIT_STAT_CONVERSION,
                    stacks: None,
                },
            );
        }
        factor.sources.extend(
            skill
                .cast_buff_crit_rate_bonuses
                .iter()
                .filter(|(id, _)| state.buff_manager.is_active(id, state.current_time))
                .map(|(id, value)| DamageSourceTrace {
                    source: format!("buff:{id}"),
                    value: *value,
                    stacks: None,
                }),
        );
        if uncapped_crit_rate > stat.crit_rate_cap {
            factor.sources.push(DamageSourceTrace {
                source: "ark_passive:뭉툭한 가시:치명타상한".to_string(),
                value: stat.crit_rate_cap - uncapped_crit_rate,
                stacks: None,
            });
        }
        factor
    });

    let additional_damage = 1.0 + total(StatField::AdditionalDamage);
    let skill_evolution_damage: f64 = skill
        .evolution_damage_bonuses
        .iter()
        .map(|(_, value)| value)
        .sum();
    let target_damage_increase = 1.0 + total(StatField::TargetDamageIncrease);
    let cast_buff_damage_bonus: f64 = skill
        .cast_buff_damage_bonuses
        .iter()
        .filter(|(id, _)| state.buff_manager.is_active(id, state.current_time))
        .map(|(_, value)| value)
        .sum();
    let damage_increase = 1.0 + total(StatField::DamageIncrease) + cast_buff_damage_bonus;
    let raw_movement_speed_bonus = stat.swiftness * SWIFTNESS_ATTACK_SPEED_BONUS
        + stat.attack_move_speed_percent
        + total(StatField::MovementSpeed);
    let movement_speed_bonus = raw_movement_speed_bonus.clamp(0.0, MAX_MOVEMENT_SPEED_BONUS);
    let speed_evolution_damage = if stat.speed_evolution_coefficient > 0.0 {
        let base = (raw_attack_speed.max(0.0) + raw_movement_speed_bonus.max(0.0))
            * stat.speed_evolution_coefficient;
        let overcap = if raw_attack_speed > MAX_ATTACK_SPEED_BONUS
            && raw_movement_speed_bonus > MAX_MOVEMENT_SPEED_BONUS
        {
            stat.speed_overcap_evolution
                + ((raw_attack_speed - MAX_ATTACK_SPEED_BONUS)
                    + (raw_movement_speed_bonus - MAX_MOVEMENT_SPEED_BONUS))
                    * stat.speed_overcap_coefficient
        } else {
            0.0
        };
        (base + overcap).min(stat.speed_evolution_cap)
    } else {
        0.0
    };
    let evolution_damage =
        1.0 + total(StatField::EvolutionDamage) + skill_evolution_damage + speed_evolution_damage;
    let movement_speed_damage = 1.0 + movement_speed_bonus * stat.movement_speed_damage_coefficient;
    let type_damage = 1.0 + total(StatField::TypeDamage);
    let elemental_damage = 1.0 + total(StatField::ElementalDamage);

    let mut damage_factors = if include_source_breakdown {
        vec![
            factor(
                "weaponAttackPower",
                StatField::WeaponAttackPower,
                weapon_ap,
                "additive",
            ),
            factor(
                "attackPowerMultiplier",
                StatField::AttackPowerMul,
                attack_power_mul,
                "multiplier",
            ),
            crit_rate_factor.expect("source breakdown is enabled"),
            factor(
                "critDamage",
                StatField::CritDmg,
                crit_damage,
                "additivePercent",
            ),
            factor(
                "additionalDamage",
                StatField::AdditionalDamage,
                additional_damage,
                "multiplier",
            ),
            {
                let mut factor = factor(
                    "evolutionDamage",
                    StatField::EvolutionDamage,
                    evolution_damage,
                    "multiplier",
                );
                factor
                    .sources
                    .extend(
                        skill
                            .evolution_damage_bonuses
                            .iter()
                            .map(|(source, value)| DamageSourceTrace {
                                source: source.clone(),
                                value: *value,
                                stacks: None,
                            }),
                    );
                if speed_evolution_damage > 0.0 {
                    factor.sources.push(DamageSourceTrace {
                        source: "ark_passive:음속 돌파".to_string(),
                        value: speed_evolution_damage,
                        stacks: None,
                    });
                }
                factor
            },
            factor(
                "damageIncrease",
                StatField::DamageIncrease,
                damage_increase,
                "multiplier",
            ),
            factor(
                "targetDamageIncrease",
                StatField::TargetDamageIncrease,
                target_damage_increase,
                "multiplier",
            ),
            factor(
                "typeDamage",
                StatField::TypeDamage,
                type_damage,
                "multiplier",
            ),
            factor(
                "elementalDamage",
                StatField::ElementalDamage,
                elemental_damage,
                "multiplier",
            ),
        ]
    } else {
        Vec::new()
    };
    if include_source_breakdown && stat.movement_speed_damage_coefficient != 0.0 {
        damage_factors.push(DamageFactorTrace {
            name: "movementSpeedDamage".to_string(),
            value: movement_speed_damage,
            operation: "multiplier".to_string(),
            sources: vec![
                DamageSourceTrace {
                    source: "movement_speed_bonus".to_string(),
                    value: movement_speed_bonus,
                    stacks: None,
                },
                DamageSourceTrace {
                    source: "engraving:raid_captain".to_string(),
                    value: stat.movement_speed_damage_coefficient,
                    stacks: None,
                },
            ],
        });
    }

    CastSnapshot {
        paid_mp: 0.0,
        attack_power,
        uncapped_crit_rate,
        crit_rate_cap: stat.crit_rate_cap,
        excess_crit_evolution_coefficient: stat.excess_crit_evolution_coefficient,
        excess_crit_evolution_cap: stat.excess_crit_evolution_cap,
        crit_damage,
        additional_damage,
        evolution_damage,
        target_damage_increase,
        damage_increase,
        movement_speed_damage,
        cast_buff_damage_bonus,
        type_damage,
        elemental_damage,
        on_crit_damage_bonus: stat.on_crit_damage_bonus,
        random_damage_reduction_chance: stat.random_damage_reduction_chance,
        random_damage_reduction: stat.random_damage_reduction,
        target_defense: profiles.target_defense
            * (1.0 - total(StatField::TargetDefenseReduction).clamp(0.0, 1.0)),
        damage_factors,
        actual_cast_time_ms,
        actual_cooldown_ms,
    }
}
