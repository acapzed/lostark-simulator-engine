use hashbrown::HashMap;
use serde::{Deserialize, Serialize};

use crate::constants::{
    MAX_COOLDOWN_REDUCTION,
    SWIFTNESS_ATTACK_SPEED_BONUS, SWIFTNESS_COOLDOWN_REDUCTION,
};

// ---------------------------------------------------------------------------
// StatField — 소스 기반 버킷의 필드 선택자
// ---------------------------------------------------------------------------
//
// 두 가지 역할:
//   1. StatProfile::insert/sum/field_mut 의 라우터 — 어느 HashMap에 넣을지 선택
//   2. BuffSpec.effects 의 키 — "이 버프는 어느 필드에 delta를 줄 것인가"
//
// StatProfile의 소스 기반 필드(HashMap<String, f64>)와 1:1 대응한다.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StatField {
    /// 무기 공격력 고정값 합산 (base 0.0)
    WeaponAttackPower,
    /// 치명타 적중률 직접 부여 % 합산 (base 0.0, /100 후 비율로 변환)
    CritRate,
    /// 치명타 피해 % 합산 (base 0.0)
    CritDmg,
    /// 공격력 배율 delta 합산 (1.0 + sum → 최종 배율)
    AttackPowerMul,
    /// 추가 피해 delta 합산 (1.0 + sum → 최종 배율)
    AdditionalDamage,
    /// 진화형 피해 delta 합산 (1.0 + sum → 최종 배율)
    EvolutionDamage,
    /// 적에게 주는 피해 delta 합산 (1.0 + sum → 최종 배율)
    DamageIncrease,
    /// 악세서리 연마 "적에게 주는 피해" delta 합산 (1.0 + sum → 최종 배율)
    TargetDamageIncrease,
    /// 공격 속도 추가 보너스 합산 (base 0.0, 버프 전용)
    AttackSpeed,
    /// 이동 속도 추가 보너스 합산 (0.20 = 20%).
    MovementSpeed,
    /// 재사용 대기시간 감소율 합산 (0.15 = 15%)
    CooldownReduction,
    /// 대상 방어력 감소율 합산 (0.01 = 1%)
    TargetDefenseReduction,
    /// 계열(종족) 피해 증가 delta 합산 (1.0 + sum → 최종 배율)
    TypeDamage,
    /// 속성 피해 증가 delta 합산 (1.0 + sum → 최종 배율)
    ElementalDamage,
}

/// 캐릭터 스탯 최종 확정값.
///
/// ## 필드 분류
///
/// ### flat f64 (순수 정적, 빌드타임 확정)
/// 전투 특성, 주스탯/체력 고정값, 배율 필드 등 동적 기여분이 없는 값.
///
/// ### 소스 기반 HashMap<String, f64>
/// 출처(source)가 키. 정적 기여분을 소스별로 보관.
/// 동적(버프) 기여분은 snapshot 시 별도 합산 후 버린다.
/// snapshot에서 `1.0 + sum()` 또는 `sum()` 형태로 소비.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatProfile {
    // -----------------------------------------------------------------------
    // flat — 전투 특성
    // -----------------------------------------------------------------------
    pub crit:           f64,
    pub specialization: f64,
    pub swiftness:      f64,
    pub domination:     f64,
    pub endurance:      f64,
    pub expertise:      f64,

    // -----------------------------------------------------------------------
    // flat — 주스탯 / 체력
    // -----------------------------------------------------------------------
    pub base_main_stat: f64,
    pub base_vitality:  f64,

    // -----------------------------------------------------------------------
    // flat — 공격력 (기반 계산용)
    // -----------------------------------------------------------------------
    pub base_attack_power: f64,

    // -----------------------------------------------------------------------
    // flat — 치명타 직접 부여
    // -----------------------------------------------------------------------
    /// 치명타 적중 시 피해 증가 (0.015 = 1.5%). 크리티컬 히트에만 추가 곱산.
    pub on_crit_damage_bonus: f64,
    /// 매 타격 독립 판정 피해 감소 확률과 감소율.
    pub random_damage_reduction_chance: f64,
    pub random_damage_reduction: f64,
    pub movement_speed_damage_coefficient: f64,
    /// 뭉툭한 가시: 치명타 상한과 초과 치적의 진화형 피해 전환.
    pub crit_rate_cap: f64,
    pub excess_crit_evolution_coefficient: f64,
    pub excess_crit_evolution_cap: f64,
    /// 음속 돌파: 속도 증가량 기반 진화형 피해 전환.
    pub speed_evolution_coefficient: f64,
    pub speed_overcap_evolution: f64,
    pub speed_overcap_coefficient: f64,
    pub speed_evolution_cap: f64,
    /// 쿨타임 패널티 (0.02 = +2%).
    pub cooldown_penalty: f64,

    // -----------------------------------------------------------------------
    // flat — 최대 HP / 마나
    // -----------------------------------------------------------------------
    pub max_hp: f64,
    pub max_mana: f64,

    // -----------------------------------------------------------------------
    // flat — 속도
    // -----------------------------------------------------------------------
    /// 팔찌 "공격 및 이동 속도 증가" 등 고정 % (0.03 = 3%)
    pub attack_move_speed_percent: f64,

    // -----------------------------------------------------------------------
    // flat additive multiplier (기본값 1.0, += 합산, 동적 기여분 없음)
    // -----------------------------------------------------------------------
    pub main_stat_mul:  f64,
    pub vitality_mul:   f64,
    /// 무기 공격력 배율. 동적 기여분 없으므로 plain f64 유지.
    pub weapon_ap_mul:  f64,
    /// 순수 공격력(√(주스탯×무기공격력÷6)) 배율.
    /// D. 기본 공격력 = 순수 공격력 × base_ap_mul.
    /// 보석 "기본 공격력 증가" 및 어빌리티 스톤 기본 공격력 증가가 이 버킷에 누적된다.
    pub base_ap_mul:    f64,

    // -----------------------------------------------------------------------
    // 소스 기반 additive 버킷 — HashMap<source, delta>
    // -----------------------------------------------------------------------
    pub weapon_attack_power:    HashMap<String, f64>, // sum → 무기 공격력 합산값
    pub crit_rate:              HashMap<String, f64>, // sum → 치적률 % 합산 (÷100 후 비율)
    pub crit_dmg:               HashMap<String, f64>, // sum → 치적배 % 합산
    pub attack_power_mul:       HashMap<String, f64>, // 1.0 + sum → 공격력 배율
    pub additional_damage:      HashMap<String, f64>, // 1.0 + sum → 추가 피해 배율
    pub evolution_damage:       HashMap<String, f64>, // 1.0 + sum → 진화형 피해 배율
    pub damage_increase:        HashMap<String, f64>, // 1.0 + sum → 피해 증가 배율
    pub target_damage_increase: HashMap<String, f64>, // 1.0 + sum → 적주피 배율
    pub attack_speed:           HashMap<String, f64>, // sum → 추가 공격 속도 (버프 전용)
    pub movement_speed:         HashMap<String, f64>, // sum → 추가 이동 속도
    pub cooldown_reduction:     HashMap<String, f64>, // sum → 추가 재사용 대기시간 감소
    pub target_defense_reduction: HashMap<String, f64>, // sum → 대상 방어력 감소율
    pub type_damage:            HashMap<String, f64>, // 1.0 + sum → 계열(종족) 피해 배율
    pub elemental_damage:       HashMap<String, f64>, // 1.0 + sum → 속성 피해 배율

    // -----------------------------------------------------------------------
    // baked 파생값 — bake() 호출 후 유효
    // -----------------------------------------------------------------------
    pub baked_swiftness_cd_reduction: f64,
    pub baked_swiftness_attack_speed: f64,
}

impl Default for StatProfile {
    fn default() -> Self {
        Self {
            crit: 0.0, specialization: 0.0, swiftness: 0.0,
            domination: 0.0, endurance: 0.0, expertise: 0.0,
            base_main_stat: 0.0, base_vitality: 0.0,
            base_attack_power: 0.0,
            on_crit_damage_bonus: 0.0,
            random_damage_reduction_chance: 0.0,
            random_damage_reduction: 0.0,
            movement_speed_damage_coefficient: 0.0,
            crit_rate_cap: 1.0,
            excess_crit_evolution_coefficient: 0.0,
            excess_crit_evolution_cap: 0.0,
            speed_evolution_coefficient: 0.0,
            speed_overcap_evolution: 0.0,
            speed_overcap_coefficient: 0.0,
            speed_evolution_cap: 0.0,
            cooldown_penalty: 0.0,
            max_hp: 0.0, max_mana: 0.0,
            attack_move_speed_percent: 0.0,
            main_stat_mul: 1.0, vitality_mul: 1.0, weapon_ap_mul: 1.0, base_ap_mul: 1.0,
            weapon_attack_power:    HashMap::new(),
            crit_rate:              HashMap::new(),
            crit_dmg:               HashMap::new(),
            attack_power_mul:       HashMap::new(),
            additional_damage:      HashMap::new(),
            evolution_damage:       HashMap::new(),
            damage_increase:        HashMap::new(),
            target_damage_increase: HashMap::new(),
            attack_speed:           HashMap::new(),
            movement_speed:         HashMap::new(),
            cooldown_reduction:     HashMap::new(),
            target_defense_reduction: HashMap::new(),
            type_damage:            HashMap::new(),
            elemental_damage:       HashMap::new(),
            baked_swiftness_cd_reduction: 0.0,
            baked_swiftness_attack_speed: 0.0,
        }
    }
}

impl StatProfile {
    /// 소스 기반 버킷에 delta를 삽입한다. 같은 source 키는 덮어쓴다.
    #[inline]
    pub fn insert(&mut self, field: StatField, source: impl Into<String>, delta: f64) {
        self.field_mut(field).insert(source.into(), delta);
    }

    /// 해당 버킷의 모든 delta 합산값을 반환한다.
    #[inline]
    pub fn sum(&self, field: StatField) -> f64 {
        self.field_ref(field).values().copied().sum()
    }

    pub fn field_mut(&mut self, field: StatField) -> &mut HashMap<String, f64> {
        match field {
            StatField::WeaponAttackPower    => &mut self.weapon_attack_power,
            StatField::CritRate             => &mut self.crit_rate,
            StatField::CritDmg              => &mut self.crit_dmg,
            StatField::AttackPowerMul       => &mut self.attack_power_mul,
            StatField::AdditionalDamage     => &mut self.additional_damage,
            StatField::EvolutionDamage      => &mut self.evolution_damage,
            StatField::DamageIncrease       => &mut self.damage_increase,
            StatField::TargetDamageIncrease => &mut self.target_damage_increase,
            StatField::AttackSpeed          => &mut self.attack_speed,
            StatField::MovementSpeed        => &mut self.movement_speed,
            StatField::CooldownReduction    => &mut self.cooldown_reduction,
            StatField::TargetDefenseReduction => &mut self.target_defense_reduction,
            StatField::TypeDamage           => &mut self.type_damage,
            StatField::ElementalDamage      => &mut self.elemental_damage,
        }
    }

    pub fn field_ref(&self, field: StatField) -> &HashMap<String, f64> {
        match field {
            StatField::WeaponAttackPower    => &self.weapon_attack_power,
            StatField::CritRate             => &self.crit_rate,
            StatField::CritDmg              => &self.crit_dmg,
            StatField::AttackPowerMul       => &self.attack_power_mul,
            StatField::AdditionalDamage     => &self.additional_damage,
            StatField::EvolutionDamage      => &self.evolution_damage,
            StatField::DamageIncrease       => &self.damage_increase,
            StatField::TargetDamageIncrease => &self.target_damage_increase,
            StatField::AttackSpeed          => &self.attack_speed,
            StatField::MovementSpeed        => &self.movement_speed,
            StatField::CooldownReduction    => &self.cooldown_reduction,
            StatField::TargetDefenseReduction => &self.target_defense_reduction,
            StatField::TypeDamage           => &self.type_damage,
            StatField::ElementalDamage      => &self.elemental_damage,
        }
    }

    /// 원본값으로부터 파생값을 계산한다.
    /// `StatBuilder::build()` 마지막에 한 번 호출.
    pub fn bake(&mut self) {
        self.baked_swiftness_cd_reduction = (self.swiftness * SWIFTNESS_COOLDOWN_REDUCTION)
            .min(MAX_COOLDOWN_REDUCTION);
        self.baked_swiftness_attack_speed = self.swiftness * SWIFTNESS_ATTACK_SPEED_BONUS;
    }
}
