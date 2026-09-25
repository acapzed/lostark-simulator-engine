use crate::constants::{DamageFactorTrace, DamageFormulaTrace, DamageSourceTrace};
use crate::engine::rng::Rng;
use crate::engine::snapshot::CastSnapshot;
use crate::profile::skill::{RuntimeDamageSpec, SkillHit};

pub struct HitResult {
    pub damage: f64,
    pub is_crit: bool,
}

/// 단일 히트 데미지 계산.
///
/// 공식 (docs/damage-formula.md 기준):
/// ```text
/// 1. base_damage + skill_modifier × attack_power
/// 2. × hit.damage_increase          // 스킬 고유 피해 증가 (modifier bake-in 포함)
/// 3. × snap.additional_damage       // 추가 피해 multiplier (이미 곱산 완료)
/// 4. × snap.evolution_damage        // 진화형 피해 multiplier
/// 5. × snap.target_damage_increase  // 타겟 디버프 multiplier
/// 6. × defense_multiplier           // 대상 방어력과 스킬 방어력 무시
/// 7. × crit_multiplier              // 치명타 발생 시
/// 8. × back_head_bonus              // 백어택/헤드어택
/// ```
pub fn calculate_hit(hit: &SkillHit, snap: &CastSnapshot, rng: &mut Rng) -> HitResult {
    calculate_hit_with_runtime_damage_bonus(hit, snap, rng, 0.0)
}

pub fn calculate_hit_with_runtime_damage_bonus(
    hit: &SkillHit,
    snap: &CastSnapshot,
    rng: &mut Rng,
    runtime_damage_bonus: f64,
) -> HitResult {
    // 1. 기본 피해
    let mut damage = hit.base_damage + hit.skill_modifier * snap.attack_power;

    // 2. 스킬 고유 피해 증가 (트라이포드 + modifier bake-in)
    damage *= 1.0 + hit.damage_increase + runtime_damage_bonus;

    // 3~8. 피해 증가 multiplier (모두 1.0 기준 곱산 완료값, H. 주는 피해 배율 공식 순서)
    damage *= snap.additional_damage;
    let (crit_rate, evolution_damage, _) =
        effective_crit_and_evolution(snap, hit.crit_chance_bonus);
    damage *= evolution_damage;
    damage *= snap.damage_increase;
    damage *= snap.movement_speed_damage;
    damage *= snap.target_damage_increase;
    damage *= snap.type_damage;
    damage *= snap.elemental_damage;

    // 6. 대상 방어력. 무시 확률은 피해 판정마다 독립적으로 굴린다.
    damage *= defense_multiplier(
        snap.target_defense,
        hit.defense_ignore,
        hit.defense_ignore_chance,
        rng,
    );

    // 7. 치명타
    let is_crit = rng.is_crit(crit_rate);
    if is_crit {
        damage *= snap.crit_damage + hit.crit_damage_bonus;
        // 치명타 적중 시 피해 증가 (팔찌 등)
        if snap.on_crit_damage_bonus > 0.0 {
            damage *= 1.0 + snap.on_crit_damage_bonus;
        }
    }

    // 7. 백어택/헤드어택 보너스 — 빌드타임에 hit.damage_increase에 bake 완료
    damage *= random_damage_multiplier(snap, rng);

    HitResult { damage, is_crit }
}

pub fn calculate_hit_with_trace(
    hit: &SkillHit,
    snap: &CastSnapshot,
    rng: &mut Rng,
    runtime_damage_bonus: f64,
) -> (HitResult, DamageFormulaTrace) {
    let base_term = hit.base_damage + hit.skill_modifier * snap.attack_power;
    let skill_damage_multiplier = 1.0 + hit.damage_increase + runtime_damage_bonus;
    let mut factors = snap.damage_factors.clone();
    factors.push(DamageFactorTrace {
        name: "skillDamage".to_string(),
        value: skill_damage_multiplier,
        operation: "multiplier".to_string(),
        sources: vec![
            DamageSourceTrace {
                source: "skill_hit.damage_increase".to_string(),
                value: hit.damage_increase,
                stacks: None,
            },
            DamageSourceTrace {
                source: "runtime_damage_bonus".to_string(),
                value: runtime_damage_bonus,
                stacks: None,
            },
        ],
    });
    calculate_with_trace(
        base_term,
        hit.base_damage,
        hit.skill_modifier,
        hit.crit_chance_bonus,
        hit.crit_damage_bonus,
        hit.defense_ignore,
        hit.defense_ignore_chance,
        skill_damage_multiplier,
        factors,
        snap,
        rng,
    )
}

pub fn calculate_runtime_damage(
    spec: &RuntimeDamageSpec,
    snap: &CastSnapshot,
    rng: &mut Rng,
) -> HitResult {
    let mut damage = spec.base_damage + spec.skill_modifier * snap.attack_power;
    damage *= 1.0 + spec.damage_increase;

    damage *= snap.additional_damage;
    let (crit_rate, evolution_damage, _) =
        effective_crit_and_evolution(snap, spec.crit_chance_bonus);
    damage *= evolution_damage;
    damage *= snap.damage_increase;
    damage *= snap.movement_speed_damage;
    damage *= snap.target_damage_increase;
    damage *= snap.type_damage;
    damage *= snap.elemental_damage;
    damage *= defense_multiplier(
        snap.target_defense,
        spec.defense_ignore,
        spec.defense_ignore_chance,
        rng,
    );

    let is_crit = rng.is_crit(crit_rate);
    if is_crit {
        damage *= snap.crit_damage + spec.crit_damage_bonus;
        if snap.on_crit_damage_bonus > 0.0 {
            damage *= 1.0 + snap.on_crit_damage_bonus;
        }
    }

    damage *= random_damage_multiplier(snap, rng);

    HitResult { damage, is_crit }
}

pub fn calculate_runtime_damage_with_trace(
    spec: &RuntimeDamageSpec,
    snap: &CastSnapshot,
    rng: &mut Rng,
) -> (HitResult, DamageFormulaTrace) {
    let base_term = spec.base_damage + spec.skill_modifier * snap.attack_power;
    let skill_damage_multiplier = 1.0 + spec.damage_increase;
    let mut factors = snap.damage_factors.clone();
    factors.push(DamageFactorTrace {
        name: "skillDamage".to_string(),
        value: skill_damage_multiplier,
        operation: "multiplier".to_string(),
        sources: vec![DamageSourceTrace {
            source: "runtime_damage_spec.damage_increase".to_string(),
            value: spec.damage_increase,
            stacks: None,
        }],
    });
    calculate_with_trace(
        base_term,
        spec.base_damage,
        spec.skill_modifier,
        spec.crit_chance_bonus,
        spec.crit_damage_bonus,
        spec.defense_ignore,
        spec.defense_ignore_chance,
        skill_damage_multiplier,
        factors,
        snap,
        rng,
    )
}

fn calculate_with_trace(
    base_term: f64,
    base_damage: f64,
    skill_modifier: f64,
    crit_chance_bonus: f64,
    crit_damage_bonus: f64,
    defense_ignore: f64,
    defense_ignore_chance: f64,
    skill_damage_multiplier: f64,
    mut factors: Vec<DamageFactorTrace>,
    snap: &CastSnapshot,
    rng: &mut Rng,
) -> (HitResult, DamageFormulaTrace) {
    let mut damage = base_term;

    damage *= skill_damage_multiplier;
    damage *= snap.additional_damage;
    let (crit_rate, evolution_damage, blunt_thorn_bonus) =
        effective_crit_and_evolution(snap, crit_chance_bonus);
    damage *= evolution_damage;
    if let Some(factor) = factors
        .iter_mut()
        .find(|factor| factor.name == "evolutionDamage")
    {
        factor.value = evolution_damage;
        if blunt_thorn_bonus > 0.0 {
            factor.sources.push(DamageSourceTrace {
                source: "ark_passive:뭉툭한 가시:초과치적".to_string(),
                value: blunt_thorn_bonus,
                stacks: None,
            });
        }
    }
    damage *= snap.damage_increase;
    damage *= snap.movement_speed_damage;
    damage *= snap.target_damage_increase;
    damage *= snap.type_damage;
    damage *= snap.elemental_damage;
    let defense_multiplier = defense_multiplier(
        snap.target_defense,
        defense_ignore,
        defense_ignore_chance,
        rng,
    );
    damage *= defense_multiplier;
    factors.push(DamageFactorTrace {
        name: "defense".to_string(),
        value: defense_multiplier,
        operation: "multiplier".to_string(),
        sources: vec![DamageSourceTrace {
            source: "hit.defense_ignore".to_string(),
            value: defense_ignore,
            stacks: None,
        }],
    });

    let is_crit = rng.is_crit(crit_rate);
    let mut crit_multiplier = None;
    let mut on_crit_damage_multiplier = None;
    if crit_chance_bonus != 0.0 {
        factors.push(DamageFactorTrace {
            name: "hitCritRate".to_string(),
            value: crit_chance_bonus,
            operation: "additiveRate".to_string(),
            sources: vec![DamageSourceTrace {
                source: "hit.crit_chance_bonus".to_string(),
                value: crit_chance_bonus,
                stacks: None,
            }],
        });
    }

    if is_crit {
        let multiplier = snap.crit_damage + crit_damage_bonus;
        damage *= multiplier;
        crit_multiplier = Some(multiplier);
        factors.push(DamageFactorTrace {
            name: "hitCritDamage".to_string(),
            value: multiplier,
            operation: "multiplier".to_string(),
            sources: vec![DamageSourceTrace {
                source: "hit.crit_damage_bonus".to_string(),
                value: crit_damage_bonus,
                stacks: None,
            }],
        });

        if snap.on_crit_damage_bonus > 0.0 {
            let multiplier = 1.0 + snap.on_crit_damage_bonus;
            damage *= multiplier;
            on_crit_damage_multiplier = Some(multiplier);
            factors.push(DamageFactorTrace {
                name: "onCritDamage".to_string(),
                value: multiplier,
                operation: "multiplier".to_string(),
                sources: vec![DamageSourceTrace {
                    source: "stat.on_crit_damage_bonus".to_string(),
                    value: snap.on_crit_damage_bonus,
                    stacks: None,
                }],
            });
        }
    }

    let random_damage_multiplier = random_damage_multiplier(snap, rng);
    damage *= random_damage_multiplier;
    if random_damage_multiplier != 1.0 {
        factors.push(DamageFactorTrace {
            name: "randomDamageReduction".to_string(),
            value: random_damage_multiplier,
            operation: "multiplier".to_string(),
            sources: vec![DamageSourceTrace {
                source: "stat.random_damage_reduction".to_string(),
                value: snap.random_damage_reduction,
                stacks: None,
            }],
        });
    }

    let formula = DamageFormulaTrace {
        base_damage,
        skill_modifier,
        attack_power: snap.attack_power,
        base_term,
        skill_damage_multiplier,
        additional_damage_multiplier: snap.additional_damage,
        evolution_damage_multiplier: evolution_damage,
        damage_increase_multiplier: snap.damage_increase,
        movement_speed_damage_multiplier: snap.movement_speed_damage,
        target_damage_multiplier: snap.target_damage_increase,
        type_damage_multiplier: snap.type_damage,
        elemental_damage_multiplier: snap.elemental_damage,
        defense_multiplier,
        crit_rate,
        crit_multiplier,
        on_crit_damage_multiplier,
        factors,
    };

    (HitResult { damage, is_crit }, formula)
}

fn effective_crit_and_evolution(snap: &CastSnapshot, hit_crit_bonus: f64) -> (f64, f64, f64) {
    let raw_crit_rate = snap.uncapped_crit_rate + hit_crit_bonus;
    let crit_rate = raw_crit_rate.clamp(0.0, snap.crit_rate_cap);
    let converted = if snap.excess_crit_evolution_coefficient > 0.0 {
        ((raw_crit_rate - snap.crit_rate_cap).max(0.0) * snap.excess_crit_evolution_coefficient)
            .min(snap.excess_crit_evolution_cap)
    } else {
        0.0
    };
    (crit_rate, snap.evolution_damage + converted, converted)
}

fn random_damage_multiplier(snap: &CastSnapshot, rng: &mut Rng) -> f64 {
    if snap.random_damage_reduction_chance > 0.0
        && rng.next_f64() < snap.random_damage_reduction_chance
    {
        1.0 - snap.random_damage_reduction
    } else {
        1.0
    }
}

const DEFENSE_CONSTANT: f64 = 6500.0;

fn defense_multiplier(
    target_defense: f64,
    defense_ignore: f64,
    defense_ignore_chance: f64,
    rng: &mut Rng,
) -> f64 {
    let ignore_applies = defense_ignore > 0.0
        && (defense_ignore_chance >= 1.0
            || (defense_ignore_chance > 0.0 && rng.next_f64() < defense_ignore_chance));
    let remaining_defense = target_defense
        * (1.0
            - if ignore_applies {
                defense_ignore.clamp(0.0, 1.0)
            } else {
                0.0
            });
    DEFENSE_CONSTANT / (DEFENSE_CONSTANT + remaining_defense.max(0.0))
}
