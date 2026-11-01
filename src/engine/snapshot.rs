use crate::constants::{
    DamageFactorTrace, DamageSourceTrace, BASE_CRIT_MULTIPLIER, CRIT_STAT_CONVERSION,
    MAX_ATTACK_SPEED_BONUS, MAX_COOLDOWN_REDUCTION, MAX_MOVEMENT_SPEED_BONUS,
    SWIFTNESS_ATTACK_SPEED_BONUS, SWIFTNESS_COOLDOWN_REDUCTION,
};
use crate::engine::state::ActorState;
use crate::profile::{CharacterProfiles, SkillProfile, SkillSlot, StatField};

#[cfg(test)]
mod movement_comparison_tests {
    #[test]
    fn valkyrie_release_recaptures_global_crit_and_excludes_skill_specific_crit() {
        use super::*;
        use crate::profile::{valkyrie::{ValkyrieIdentity,ValkyriePassives},BuffSpec,StackType,SkillTag};
        let mut p=crate::builder::build(include_str!("../../fixtures/reaper-thirst-reference-request.json")).unwrap();
        p.stat.crit=0.;p.stat.crit_rate.clear();
        let reference=ValkyrieIdentity {light_per_use:667,light_max:10000,light_sword_grant:1,light_sword_damage_bonus:0.35,
            initial_light:0,cores:Vec::new(),linked_windows:hashbrown::HashMap::new(),skill_categories:hashbrown::HashMap::from([(48160,68)]),passives:Some(ValkyriePassives {release:3,..Default::default()})};
        let mut state=ActorState::new(100.,Box::new(crate::engine::state::class::valkyrie::ValkyrieState::new(100.,reference)));
        let mut skill=p.skills[0].clone();skill.tags.retain(|t|!matches!(t,SkillTag::SkillId(_)));skill.tags.push(SkillTag::SkillId(48160));
        skill.cast_buff_crit_rate_bonuses.clear();skill.cast_resource_crit_rate_bonuses.clear();
        assert_eq!(capture(&p,&skill,0,&state,false).valkyrie_passive_damage,1.);
        let buff=BuffSpec {id:"crit".into(),effects:hashbrown::HashMap::from([(StatField::CritRate,50.)]),max_stacks:1,stack_type:StackType::Refresh,duration_ms:1000};
        state.buff_manager.apply(buff,1000,1);skill.cast_buff_crit_rate_bonuses.push(("crit".into(),0.25));
        let snap=capture(&p,&skill,0,&state,false);assert_eq!(snap.uncapped_crit_rate,0.75);assert_eq!(snap.valkyrie_passive_damage,1.275);
        state.current_time=1000;state.buff_manager.expire_if("crit",1000);assert_eq!(capture(&p,&skill,0,&state,false).valkyrie_passive_damage,1.);
    }
    #[test]
    fn target_critical_damage_taken_multiplies_bracelet_and_expires() {
        use super::*;
        use crate::profile::{BuffSpec, StackType};
        let mut p = crate::builder::build(include_str!("../../fixtures/reaper-thirst-reference-request.json")).unwrap();
        p.stat.on_crit_damage_bonus = 0.015;
        let mut state = ActorState::new(100., Box::new(crate::engine::state::class::reaper::ReaperState::from_reference(100., p.reaper_reference.as_ref().unwrap())));
        let initial = capture(&p, &p.skills[0], 0, &state, false);
        assert_eq!(initial.on_crit_damage_bonus, 0.015);
        let buff = BuffSpec { id: "weakness".into(), effects: hashbrown::HashMap::from([(StatField::OnCritDamageTaken, 0.08)]), max_stacks: 1, stack_type: StackType::Refresh, duration_ms: 8000 };
        state.target_buff_manager.apply(buff.clone(), 8000, 1);
        let active = capture(&p, &p.skills[0], 0, &state, false);
        assert_eq!(active.on_crit_damage_bonus, 0.015);
        assert_eq!(active.target_on_crit_damage_taken, 0.08);
        assert_eq!(active.crit_damage, initial.crit_damage);
        assert_eq!(active.target_damage_increase, initial.target_damage_increase);
        state.current_time = 4000;
        state.target_buff_manager.apply(buff, 12000, 1);
        state.target_buff_manager.expire_if("weakness", 8000);
        assert!((capture(&p, &p.skills[0], 0, &state, false).target_on_crit_damage_taken - active.target_on_crit_damage_taken).abs() < 1e-12);
        state.current_time = 12000;
        state.target_buff_manager.expire_if("weakness", 12000);
        assert_eq!(capture(&p, &p.skills[0], 0, &state, false).target_on_crit_damage_taken, 0.0);
    }

    #[test]
    fn weaker_buff_resumes_after_stronger_expires_and_old_refresh_token_is_ignored() {
        use super::*;
        use crate::profile::{BuffSpec,StackType};
        let mut profile=crate::builder::build(include_str!("../../fixtures/reaper-thirst-reference-request.json")).unwrap();
        profile.stat.swiftness=0.;profile.stat.attack_move_speed_percent=0.;profile.stat.movement_speed.clear();profile.stat.movement_speed_damage_coefficient=0.5;
        let mut state=ActorState::new(100.,Box::new(crate::engine::state::class::reaper::ReaperState::from_reference(100.,profile.reaper_reference.as_ref().unwrap())));
        let specs=[("weak",0.192),("strong",0.246)].map(|(id,value)|BuffSpec {id:id.into(),effects:hashbrown::HashMap::from([(StatField::MovementSpeedMax,value)]),max_stacks:1,stack_type:StackType::Refresh,duration_ms:3000});
        let bonus=|state:&ActorState|capture(&profile,&profile.skills[0],0,state,false).movement_speed_damage;
        state.buff_manager.apply(specs[0].clone(),6000,1);state.buff_manager.apply(specs[1].clone(),4000,1);
        assert!((bonus(&state)-1.123).abs()<1e-12);
        state.current_time=4000;state.buff_manager.expire_if("strong",4000);
        assert!((bonus(&state)-1.096).abs()<1e-12);
        state.buff_manager.apply(specs[0].clone(),7000,1);state.buff_manager.expire_if("weak",6000);
        state.current_time=6000;assert!((bonus(&state)-1.096).abs()<1e-12);
        state.current_time=7000;assert_eq!(bonus(&state),1.);
    }
}

/// 캐스트 시점에 캡처하는 스냅샷.
///
#[derive(Debug, Clone)]
pub struct CastSnapshot {
    pub source_damage_multiplier: f64,
    pub forced_back_attack: bool,
    pub cast_buff_crit_rate_bonus: f64,
    pub reaper_boost: Option<crate::engine::state::class::reaper::ReaperBoost>,
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
    pub target_on_crit_damage_taken: f64,
    pub valkyrie_passive_damage: f64,
    /// Extra own-on-critical rate; included in on_crit_damage_bonus, retained for trace attribution.
    pub valkyrie_core_crit_damage:f64,
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
    pub actual_charge_recovery_ms: u32,
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
    let reaper_boost = state.class_state.as_reaper().map(|reaper| {
        let game_id = skill.tags.iter().find_map(|tag| match tag {
            crate::profile::SkillTag::SkillId(id) => Some(*id), _ => None,
        }).unwrap_or(0);
        reaper.boost(game_id, skill.tags.contains(&crate::profile::SkillTag::AttackBack))
    });

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
        total(StatField::CooldownReduction) + profiles.reaper_reference.as_ref()
            .and_then(|reference|reference.metadata.get("shabel_mark_policy"))
            .filter(|model|model["skillIds"].as_array().is_some_and(|ids|ids.iter().any(|id|id.as_u64().is_some_and(|id|skill.tags.contains(&crate::profile::SkillTag::SkillId(id as u32))))))
            .filter(|model|model["buffId"].as_str().is_some_and(|id|state.buff_manager.active_buffs(state.current_time).any(|(spec,_)|spec.id==id)))
            .and_then(|model|model["value"].as_f64()).unwrap_or(0.0)
    } else {
        0.0
    };
    let cd_reduction = (stat.swiftness * SWIFTNESS_COOLDOWN_REDUCTION + buff_cd_reduction)
        .min(MAX_COOLDOWN_REDUCTION);
    let raw_attack_speed =
        stat.swiftness * SWIFTNESS_ATTACK_SPEED_BONUS + total(StatField::AttackSpeed)
            + reaper_boost.map_or(0.0, |boost| boost.attack_speed);
    let attack_speed = raw_attack_speed.clamp(-MAX_ATTACK_SPEED_BONUS, MAX_ATTACK_SPEED_BONUS);

    let actual_cooldown_ms =
        (skill.cooldown_ms as f64 * (1.0 - cd_reduction) * (1.0 + stat.cooldown_penalty)) as u32;
    let actual_charge_recovery_ms =
        (skill.charge_recovery_ms as f64 * (1.0 - cd_reduction) * (1.0 + stat.cooldown_penalty)) as u32;
    let base_cast_ms = skill
        .cast_times_ms
        .get(phase as usize)
        .or_else(|| skill.cast_times_ms.first())
        .copied()
        .unwrap_or(0);
    let source_cast_ms = skill.cast_buff_clock_multipliers.iter()
        .filter(|(id,_)| state.buff_manager.is_active(id,state.current_time))
        .fold(base_cast_ms as f64,|duration,(_,factor)| duration*factor);
    let actual_cast_time_ms = (source_cast_ms / (1.0 + attack_speed)) as u32;

    // ── 치명타 ───────────────────────────────────────────────────────────
    let cast_buff_crit_rate_bonus: f64 = skill
        .cast_buff_crit_rate_bonuses
        .iter()
        .filter(|(id, _)| state.buff_manager.is_active(id, state.current_time))
        .map(|(_, value)| value)
        .chain(skill.cast_resource_crit_rate_bonuses.iter()
            .filter(|(resource, _)| state.get_resource(resource)>0.0).map(|(_,value)|value))
        .sum();
    let uncapped_crit_rate = stat.crit / CRIT_STAT_CONVERSION
        + total(StatField::CritRate) / 100.0
        + cast_buff_crit_rate_bonus;
    let crit_rate = uncapped_crit_rate.clamp(0.0, stat.crit_rate_cap);
    let crit_damage = BASE_CRIT_MULTIPLIER + total(StatField::CritDmg) / 100.0
        + skill.buff_crit_damage_bonuses.iter()
            .filter(|(id,_)|state.buff_manager.is_active(id,state.current_time))
            .map(|(_,bonus)|bonus).sum::<f64>();

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
                    source: format!("cast:buff:{id}"),
                    value: *value,
                    stacks: None,
                }),
        );
        factor.sources.extend(skill.cast_resource_crit_rate_bonuses.iter()
            .filter(|(resource,_)| state.get_resource(resource)>0.0)
            .map(|(resource,value)| DamageSourceTrace { source:format!("cast:resource:{resource}"),value:*value,stacks:None }));
        if uncapped_crit_rate > stat.crit_rate_cap {
            factor.sources.push(DamageSourceTrace {
                source: "ark_passive:뭉툭한 가시:치명타상한".to_string(),
                value: stat.crit_rate_cap - uncapped_crit_rate,
                stacks: None,
            });
        }
        factor
    });

    // All-critical chance: global stat/buffs before the skill-specific cast/hit bonuses.
    let valkyrie_factors=state.class_state.as_valkyrie().map_or_else(Vec::new,|v| {
        let id=skill.tags.iter().find_map(|tag|match tag {crate::profile::SkillTag::SkillId(id)=>Some(*id),_=>None}).unwrap_or(0);
        v.passive_factors(id,stat.crit/CRIT_STAT_CONVERSION+total(StatField::CritRate)/100.)
    });
    let game_id=skill.tags.iter().find_map(|t|if let crate::profile::SkillTag::SkillId(id)=t {Some(*id)}else{None}).unwrap_or(0);
    let core_factor=state.class_state.as_valkyrie().map_or(1.,|v|v.core_damage_factor(game_id,false));
    let valkyrie_core_crit_damage=state.class_state.as_valkyrie().map_or(0.,|v|v.core_damage_factor(game_id,true)-1.);
    let valkyrie_passive_damage=valkyrie_factors.iter().fold(core_factor,|value,(_,factor)|value*factor);
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
        .map(|(_, value)| *value)
        .chain(skill.cast_buff_stack_damage_bonuses.iter()
            .map(|(id, value)| state.buff_manager.stacks(id, state.current_time) as f64 * value))
        .chain(skill.cast_resource_damage_bonuses.iter()
            .filter(|(resource, _)| state.get_resource(resource) > 0.0).map(|(_, value)| *value))
        .chain(skill.cast_resource_scaled_damage_bonuses.iter()
            .filter(|e|e.cap.is_finite()&&e.cap>0.0&&e.bonus.is_finite()&&e.buff_id.as_ref().is_none_or(|id|state.buff_manager.is_active(id,state.current_time)))
            .map(|e|(state.get_resource(&e.resource)/e.cap).clamp(0.0,1.0)*e.bonus))
        .sum();
    let damage_increase = 1.0 + total(StatField::DamageIncrease) + cast_buff_damage_bonus
        + skill.buff_damage_bonuses.iter()
            .filter(|(id,_)|state.buff_manager.is_active(id,state.current_time))
            .map(|(_,bonus)|bonus).sum::<f64>();
    let raw_movement_speed_bonus = stat.swiftness * SWIFTNESS_ATTACK_SPEED_BONUS
        + stat.attack_move_speed_percent
        + total(StatField::MovementSpeed)
        + state.buff_manager.active_buffs(state.current_time)
            .filter_map(|(spec, _)| spec.effects.get(&StatField::MovementSpeedMax).copied())
            .chain(stat.movement_speed_max.values().copied())
            .fold(0.0_f64, f64::max);
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
            {
                let mut value=factor("critDamage",StatField::CritDmg,crit_damage,"additivePercent");
                value.sources.extend(skill.buff_crit_damage_bonuses.iter()
                    .filter(|(id,_)|state.buff_manager.is_active(id,state.current_time))
                    .map(|(id,bonus)|DamageSourceTrace{source:format!("skill:buff:{id}"),value:*bonus*100.0,stacks:None}));
                value
            },
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
            {
                let mut value = factor("damageIncrease", StatField::DamageIncrease, damage_increase, "multiplier");
                value.sources.extend(skill.cast_buff_damage_bonuses.iter()
                    .filter(|(id, _)| state.buff_manager.is_active(id, state.current_time))
                    .map(|(id, bonus)| DamageSourceTrace { source: format!("cast:buff:{id}"), value: *bonus, stacks: None }));
                value.sources.extend(skill.cast_resource_damage_bonuses.iter()
                    .filter(|(resource, _)| state.get_resource(resource) > 0.0)
                    .map(|(resource, bonus)| DamageSourceTrace { source: format!("cast:resource:{resource}"), value: *bonus, stacks: None }));
                value.sources.extend(skill.cast_buff_stack_damage_bonuses.iter()
                    .filter_map(|(id, bonus)| {let stacks=state.buff_manager.stacks(id,state.current_time);(stacks>0).then(||DamageSourceTrace {
                        source:format!("cast:buff_stacks:{id}"),value:*bonus*stacks as f64,stacks:Some(stacks)})}));
                value.sources.extend(skill.cast_resource_scaled_damage_bonuses.iter()
                    .filter(|e|e.cap.is_finite()&&e.cap>0.0&&e.bonus.is_finite()&&state.get_resource(&e.resource)>0.0&&e.buff_id.as_ref().is_none_or(|id|state.buff_manager.is_active(id,state.current_time)))
                    .map(|e|DamageSourceTrace{source:format!("cast:resource_scaled:{}",e.resource),value:(state.get_resource(&e.resource)/e.cap).clamp(0.0,1.0)*e.bonus,stacks:None}));
                value.sources.extend(skill.buff_damage_bonuses.iter()
                    .filter(|(id,_)|state.buff_manager.is_active(id,state.current_time))
                    .map(|(id,bonus)|DamageSourceTrace{source:format!("skill:buff:{id}"),value:*bonus,stacks:None}));
                value
            },
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

    if include_source_breakdown {
        if core_factor!=1. {damage_factors.push(DamageFactorTrace {name:"ark_grid:발키리:상시".into(),value:core_factor,operation:"multiplier".into(),sources:vec![DamageSourceTrace {source:"ark_grid:source_options".into(),value:core_factor-1.,stacks:None}]});}
        for (source,value) in valkyrie_factors {
            damage_factors.push(DamageFactorTrace {name:source.into(),value,operation:"multiplier".into(),
                sources:vec![DamageSourceTrace {source:source.into(),value:value-1.,stacks:None}]});
        }
    }
    if include_source_breakdown && skill.source_damage_multiplier != 1.0 {
        damage_factors.push(DamageFactorTrace {name:"sourceDamage".into(),value:skill.source_damage_multiplier,operation:"multiplier".into(),
            sources:vec![DamageSourceTrace {source:format!("skill:source:{}",skill.skill_id),value:skill.source_damage_multiplier,stacks:None}]});
    }
    let mut snapshot = CastSnapshot {
        source_damage_multiplier: skill.source_damage_multiplier,
        forced_back_attack: false,
        cast_buff_crit_rate_bonus,
        reaper_boost: None,
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
        on_crit_damage_bonus: stat.on_crit_damage_bonus+valkyrie_core_crit_damage,
        target_on_crit_damage_taken: total(StatField::OnCritDamageTaken),
        valkyrie_passive_damage,
        valkyrie_core_crit_damage,
        random_damage_reduction_chance: stat.random_damage_reduction_chance,
        random_damage_reduction: stat.random_damage_reduction,
        target_defense: profiles.target_defense
            * (1.0 - total(StatField::TargetDefenseReduction).clamp(0.0, 1.0)),
        damage_factors,
        actual_cast_time_ms,
        actual_cooldown_ms,
        actual_charge_recovery_ms,
    };
    snapshot.replace_reaper_boost(reaper_boost);
    snapshot
}

impl CastSnapshot {
    pub fn replace_cast_crit_bonus(&mut self, cast: &Self) {
        let delta = cast.cast_buff_crit_rate_bonus - self.cast_buff_crit_rate_bonus;
        self.uncapped_crit_rate += delta;
        self.cast_buff_crit_rate_bonus = cast.cast_buff_crit_rate_bonus;
        if let Some(factor) = self.damage_factors.iter_mut().find(|factor| factor.name == "critRate") {
            let uncapped = self.uncapped_crit_rate - self.reaper_boost.map_or(0.0, |boost| boost.crit_rate);
            factor.value = uncapped.clamp(0.0, self.crit_rate_cap);
            factor.sources.retain(|source| !source.source.starts_with("cast:")
                && source.source != "ark_passive:뭉툭한 가시:치명타상한");
            if uncapped > self.crit_rate_cap {
                factor.sources.push(DamageSourceTrace { source: "ark_passive:뭉툭한 가시:치명타상한".into(),
                    value: self.crit_rate_cap - uncapped, stacks: None });
            }
            if let Some(original) = cast.damage_factors.iter().find(|factor| factor.name == "critRate") {
                factor.sources.extend(original.sources.iter().filter(|source| source.source.starts_with("cast:")).cloned());
            }
        }
    }

    pub fn replace_cast_damage_bonus(&mut self, cast: &Self) {
        let delta = cast.cast_buff_damage_bonus - self.cast_buff_damage_bonus;
        self.damage_increase += delta * self.reaper_boost.map_or(1.0, |boost| boost.damage);
        self.cast_buff_damage_bonus = cast.cast_buff_damage_bonus;
        if let Some(factor) = self.damage_factors.iter_mut().find(|factor| factor.name == "damageIncrease") {
            factor.value += delta;
            factor.sources.retain(|source| !source.source.starts_with("cast:"));
            if let Some(original) = cast.damage_factors.iter().find(|factor| factor.name == "damageIncrease") {
                factor.sources.extend(original.sources.iter().filter(|source| source.source.starts_with("cast:")).cloned());
            }
        }
    }

    /// Only class effects are frozen; live equipment/target effects still recapture.
    pub fn replace_reaper_boost(&mut self, boost: Option<crate::engine::state::class::reaper::ReaperBoost>) {
        if self.reaper_boost.is_none() && boost.is_none() { return; }
        let old = self.reaper_boost.unwrap_or_default();
        let new = boost.unwrap_or_default();
        self.attack_power *= new.attack_power / old.attack_power;
        self.damage_increase *= new.damage / old.damage;
        self.uncapped_crit_rate += new.crit_rate - old.crit_rate;
        self.crit_damage += new.crit_damage - old.crit_damage;
        self.damage_factors.retain(|factor| !factor.name.starts_with("reaperReference"));
        if boost.is_some() && !self.damage_factors.is_empty() {
            for (name, value, operation) in [
                ("reaperReferenceDamage", new.damage, "multiplier"),
                ("reaperReferenceAttackPower", new.attack_power, "multiplier"),
                ("reaperReferenceCritRate", new.crit_rate, "additiveRate"),
            ] {
                self.damage_factors.push(DamageFactorTrace {
                    name: name.into(), value, operation: operation.into(),
                    sources: vec![DamageSourceTrace { source: "reaper_reference:provisional".into(), value, stacks: None }],
                });
            }
        }
        if boost.is_some() && new.crit_damage != 0.0 && !self.damage_factors.is_empty() {
            self.damage_factors.push(DamageFactorTrace { name: "reaperReferenceCritDamage".into(), value: new.crit_damage,
                operation: "additiveRate".into(), sources: vec![DamageSourceTrace { source: "reaper_passive:조건부 치명타 피해".into(), value: new.crit_damage, stacks: None }] });
        }
        self.reaper_boost = boost;
    }
}

#[cfg(test)]
mod shabel_mark_scope_tests {
#[test]
fn reaper_shabel_mark_snapshot_is_timed_and_limited_to_the_seven_raw_group_skills() {
    use super::*;use crate::engine::state::ActorState;use crate::engine::state::class::reaper::ReaperState;use crate::profile::{SkillTag,SkillSlot};
    let mut p=crate::builder::build(include_str!("../../fixtures/reaper-thirst-reference-request.json")).unwrap();p.stat.swiftness=0.;p.stat.cooldown_penalty=0.;p.stat.cooldown_reduction.clear();p.reaper_reference.as_mut().unwrap().metadata["shabel_mark_policy"]=serde_json::json!({"buffId":"reaper:shabel_mark","value":0.21,"durationMs":3000,"skillIds":[26050,26060,26070,26080,26110,26120,26140]});
    let buff=crate::profile::BuffSpec {id:"reaper:shabel_mark".into(),effects:hashbrown::HashMap::new(),max_stacks:1,stack_type:crate::profile::StackType::Refresh,duration_ms:3000};
    let mut state=ActorState::new(100.,Box::new(ReaperState::from_reference(100.,p.reaper_reference.as_ref().unwrap())));let mut skill=p.skills[0].clone();skill.cooldown_ms=10000;skill.slot=SkillSlot::Normal;
    for id in [26050,26060,26070,26080,26110,26120,26140,26090,26100,26560,26600,26810] {skill.tags.retain(|t|!matches!(t,SkillTag::SkillId(_)));skill.tags.push(SkillTag::SkillId(id));state.current_time=0;assert_eq!(capture(&p,&skill,0,&state,false).actual_cooldown_ms,10000);state.buff_manager.apply(buff.clone(),3000,1);let scoped=matches!(id,26050|26060|26070|26080|26110|26120|26140);assert_eq!(capture(&p,&skill,0,&state,false).actual_cooldown_ms,if scoped {7900} else {10000});state.current_time=3000;state.buff_manager.expire_if(&buff.id,3000);assert_eq!(capture(&p,&skill,0,&state,false).actual_cooldown_ms,10000);}
    skill.tags.retain(|t|!matches!(t,SkillTag::SkillId(_)));skill.tags.push(SkillTag::SkillId(26110));state.current_time=0;state.buff_manager.apply(buff.clone(),3000,1);state.current_time=1000;state.buff_manager.apply(buff.clone(),4000,1);state.current_time=3000;state.buff_manager.expire_if(&buff.id,3000);assert_eq!(capture(&p,&skill,0,&state,false).actual_cooldown_ms,7900);state.current_time=3999;assert_eq!(capture(&p,&skill,0,&state,false).actual_cooldown_ms,7900);state.current_time=4000;state.buff_manager.expire_if(&buff.id,4000);assert_eq!(capture(&p,&skill,0,&state,false).actual_cooldown_ms,10000);
    state.current_time=0;state.buff_manager.apply(buff,3000,1);p.stat.cooldown_reduction.insert("test".into(),0.75);assert_eq!(capture(&p,&skill,0,&state,false).actual_cooldown_ms,1999);skill.slot=SkillSlot::Awakening;assert_eq!(capture(&p,&skill,0,&state,false).actual_cooldown_ms,10000);
}

}

#[cfg(test)]
mod cast_resource_scope_tests {
    use super::*;
    #[test]
    fn frozen_cast_bonus_respects_class_multiplier_and_sources_after_persona_consumption() {
        use crate::engine::state::class::reaper::{ReaperState,ReaperBoost};
        let p=crate::builder::build(include_str!("../../fixtures/reaper-moon-reference-request.json")).unwrap();let mut skill=p.skills.iter().find(|s|s.name=="레이지 스피어").unwrap().clone();skill.cast_resource_damage_bonuses=vec![("ReaperPersona".into(),0.043)];
        let mut class=ReaperState::from_reference(p.max_mp,p.reaper_reference.as_ref().unwrap());class.persona=true;let mut state=ActorState::new(100.,Box::new(class));let cast=capture(&p,&skill,0,&state,true);assert_eq!(cast.cast_buff_damage_bonus,0.043);state.class_state.as_reaper_mut().unwrap().cast(26810);
        for damage in [0.5,1.,2.69] {let mut live=capture(&p,&skill,0,&state,true);assert_eq!(live.cast_buff_damage_bonus,0.);live.replace_reaper_boost(Some(ReaperBoost {damage,..Default::default()}));live.replace_cast_damage_bonus(&cast);live.replace_reaper_boost(cast.reaper_boost);assert!((live.damage_increase-cast.damage_increase).abs()<1e-12);let source=live.damage_factors.iter().find(|f|f.name=="damageIncrease").unwrap().sources.iter().find(|s|s.source=="cast:resource:ReaperPersona").unwrap();assert_eq!(source.value,0.043);}
        let legacy=serde_json::to_value(&p.skills[0]).unwrap();assert!(legacy.get("cast_resource_damage_bonuses").is_none());let restored:SkillProfile=serde_json::from_value(legacy).unwrap();assert!(restored.cast_resource_damage_bonuses.is_empty());
    }
}

#[cfg(test)]
mod conditional_skill_bonus_tests {
    use super::*;
    use crate::engine::state::{ActorState,class::berserker::BerserkerState};
    use crate::profile::{BuffSpec,StackType};
    use crate::profile::skill::ResourceScaledDamage;
    #[test]
    fn source_multiplier_covers_direct_and_runtime_damage_without_changing_other_skills() {
        use crate::engine::{mechanics::damage, rng::Rng};
        let(p,mut skill,state)=fixture();let original=capture(&p,&skill,0,&state,true);
        assert!(serde_json::to_value(&skill).unwrap().get("source_damage_multiplier").is_none());
        skill.source_damage_multiplier=1.37;let scaled=capture(&p,&skill,0,&state,true);
        let hit=&skill.hits[0];
        let direct=|s:&CastSnapshot|damage::calculate_hit(hit,s,&mut Rng::new(42)).damage;
        let runtime=crate::profile::RuntimeDamageSpec {destruction:0,hit_id:"owned_dot".into(),name:"owned_dot".into(),base_damage:100.,skill_modifier:0.7,
            damage_increase:0.2,crit_chance_bonus:0.,crit_damage_bonus:0.,random_crit_damage_chance:0.,random_crit_damage_bonus:0.,defense_ignore:0.,defense_ignore_chance:0.};
        let dot=|s:&CastSnapshot|damage::calculate_runtime_damage(&runtime,s,&mut Rng::new(42)).damage;
        assert!((direct(&scaled)/direct(&original)-1.37).abs()<1e-12);
        assert!((dot(&scaled)/dot(&original)-1.37).abs()<1e-12);
        assert_eq!(capture(&p,&p.skills[0],0,&state,false).source_damage_multiplier,1.);
        assert!(scaled.damage_factors.iter().any(|f|f.name=="sourceDamage"&&f.value==1.37));
    }
    #[test]
    fn stacked_cast_bonus_caps_expires_and_survives_consumption_in_frozen_bucket() {
        let(p,mut skill,mut state)=fixture();skill.cast_buff_stack_damage_bonuses=vec![("stacked".into(),0.22)];
        let spec=BuffSpec{id:"stacked".into(),effects:Default::default(),max_stacks:3,stack_type:StackType::Refresh,duration_ms:1000};
        for n in 1..=5 {state.buff_manager.apply(spec.clone(),1000,1);let cast=capture(&p,&skill,0,&state,true);assert!((cast.cast_buff_damage_bonus-n.min(3)as f64*0.22).abs()<1e-12);}
        let cast=capture(&p,&skill,0,&state,true);state.buff_manager.consume_stacks("stacked",3,0);
        let mut live=capture(&p,&skill,0,&state,true);assert_eq!(live.cast_buff_damage_bonus,0.);live.replace_cast_damage_bonus(&cast);
        assert_eq!(live.damage_increase,cast.damage_increase);
        assert!(live.damage_factors.iter().flat_map(|f|&f.sources).any(|s|s.source=="cast:buff_stacks:stacked"&&s.stacks==Some(3)));
        state.buff_manager.apply(spec,1000,1);state.current_time=1000;assert_eq!(capture(&p,&skill,0,&state,false).cast_buff_damage_bonus,0.);
    }
    fn fixture()->(CharacterProfiles,SkillProfile,ActorState){
        let p=crate::builder::build(include_str!("../../fixtures/reaper-thirst-reference-request.json")).unwrap();
        let skill=p.skills[0].clone();let state=ActorState::new(100.,Box::new(BerserkerState::new(100.,200.)));
        (p,skill,state)
    }
    fn apply(state:&mut ActorState){state.buff_manager.apply(BuffSpec{id:"scoped".into(),effects:hashbrown::HashMap::new(),max_stacks:1,stack_type:StackType::Refresh,duration_ms:1000},1000,1);}
    #[test]
    fn skill_scoped_live_damage_and_crit_expire_without_affecting_another_skill(){
        let (p,mut skill,mut state)=fixture();let base=capture(&p,&skill,0,&state,true);let other=skill.clone();
        skill.buff_damage_bonuses=vec![("scoped".into(),0.56)];skill.buff_crit_damage_bonuses=vec![("scoped".into(),0.6)];
        apply(&mut state);let active=capture(&p,&skill,0,&state,true);assert!((active.damage_increase-base.damage_increase-0.56).abs()<1e-12);assert!((active.crit_damage-base.crit_damage-0.6).abs()<1e-12);
        let unaffected=capture(&p,&other,0,&state,true);assert_eq!(unaffected.damage_increase,base.damage_increase);assert_eq!(unaffected.crit_damage,base.crit_damage);
        assert!(active.damage_factors.iter().flat_map(|f|&f.sources).any(|s|s.source=="skill:buff:scoped"));
        state.current_time=1000;let expired=capture(&p,&skill,0,&state,true);assert_eq!(expired.damage_increase,base.damage_increase);assert_eq!(expired.crit_damage,base.crit_damage);
    }
    #[test]
    fn scaled_resource_requires_buff_caps_and_remains_after_consumption(){
        let(p,mut skill,mut state)=fixture();skill.cast_resource_scaled_damage_bonuses=vec![ResourceScaledDamage{resource:"Fury".into(),cap:160.,bonus:0.55,buff_id:Some("scoped".into())}];
        state.class_state.set_resource("Fury",200.);assert_eq!(capture(&p,&skill,0,&state,true).cast_buff_damage_bonus,0.);apply(&mut state);
        for(value,expected)in[(0.,0.),(80.,0.275),(160.,0.55),(200.,0.55)]{state.class_state.set_resource("Fury",value);assert!((capture(&p,&skill,0,&state,true).cast_buff_damage_bonus-expected).abs()<1e-12);}
        let cast=capture(&p,&skill,0,&state,true);state.class_state.set_resource("Fury",0.);state.buff_manager.consume_stacks("scoped",1,0);
        let mut live=capture(&p,&skill,0,&state,true);assert_eq!(live.cast_buff_damage_bonus,0.);live.replace_cast_damage_bonus(&cast);assert_eq!(live.cast_buff_damage_bonus,0.55);
        assert!(live.damage_factors.iter().flat_map(|f|&f.sources).any(|s|s.source=="cast:resource_scaled:Fury"&&(s.value-0.55).abs()<1e-12));
    }
    #[test]
    fn legacy_profiles_omit_and_restore_empty_new_scopes(){
        let(_,skill,_)=fixture();let value=serde_json::to_value(&skill).unwrap();
        for key in ["buff_damage_bonuses","buff_crit_damage_bonuses","cast_resource_scaled_damage_bonuses"]{assert!(value.get(key).is_none());}
        let restored:SkillProfile=serde_json::from_value(value).unwrap();assert!(restored.buff_damage_bonuses.is_empty()&&restored.buff_crit_damage_bonuses.is_empty()&&restored.cast_resource_scaled_damage_bonuses.is_empty());
    }
}
