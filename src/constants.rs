use hashbrown::HashMap;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// 공통 열거형
// TODO: lib.rs로 이동 예정 (파이프라인 제어용)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PipelineStep {
    Prepare,
    Simulate,
    Trace,
}

// ---------------------------------------------------------------------------
// 전투 상수
// ---------------------------------------------------------------------------

/// 신속 → 쿨타임 감소. PCLevel 70 RapidityCoefficient 0.5821 × 10000,
/// PCStatCoefficient rapidity→cooldown_reduction 1.25.
pub const SWIFTNESS_COOLDOWN_REDUCTION: f64 = 1.25 / 5821.0;
/// 신속 → 공격·이동 속도. rapidity→attack/move_speed_rate 1.0.
pub const SWIFTNESS_ATTACK_SPEED_BONUS: f64 = 1.0 / 5821.0;
/// 치명 스탯 → 치적률 환산 분모
pub const CRIT_STAT_CONVERSION: f64 = 2794.0;
/// 기본 치명타 배율 (200%)
pub const BASE_CRIT_MULTIPLIER: f64 = 2.0;
/// 쿨타임 감소 최대 한도 (80%)
pub const MAX_COOLDOWN_REDUCTION: f64 = 0.8;
/// 기본 공격 속도 대비 증감 한도 (60~140%).
pub const MAX_ATTACK_SPEED_BONUS: f64 = 0.4;
/// 기본 이동 속도 대비 증가분 상한 (140%).
pub const MAX_MOVEMENT_SPEED_BONUS: f64 = 0.4;
/// 콤보/체인 단계 타임아웃 (Go 하드코딩 2000ms 명시화)
pub const COMBO_PHASE_TIMEOUT_MS: u64 = 2000;
/// 백어택 / 헤드어택 추가 피해 (5%)
pub const BACK_HEAD_ATTACK_BONUS: f64 = 0.05;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swiftness_uses_the_level_70_raw_conversion() {
        assert!((1800.0 * SWIFTNESS_ATTACK_SPEED_BONUS - 0.3092252190345301).abs() < 1e-15);
        assert!((1800.0 * SWIFTNESS_COOLDOWN_REDUCTION - 0.3865315237931627).abs() < 1e-15);
    }
}

// ---------------------------------------------------------------------------
// 시뮬레이션 결과
// TODO: Stage 6에서 results/ 모듈 생성 시 results/collector.rs 로 이동 예정
// ---------------------------------------------------------------------------

/// 단일 워커 실행 결과 (FastCollector가 생성)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimResult {
    pub total_damage: f64,
    pub dps: f64,
    pub cast_count: u32,
    pub crit_count: u32,
    pub skill_stats: HashMap<u32, SkillStat>,
    /// 이터레이션별 (DPS, seed) 목록 — 중앙값(Median) 재현용
    pub distribution: Vec<(f64, u64)>,
}

/// trace 모드에서 UI에 노출하는 버프/디버프 상세.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuffDebuffTraceDetail {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub stacks: u32,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceGainTrace {
    pub resource: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldCardTrace {
    pub card_id: u32,
    pub card_name: String,
}

/// 단일 캐스트 이벤트 (trace 모드에서만 수집)
/// UI 형식: `{ time, skillName, isAwakening, activeBuffDetails, targetDebuffDetails }`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CastEvent {
    /// 캐스트 시각 (초)
    pub time: f64,
    pub skill_id: u32,
    pub skill_name: String,
    pub is_awakening: bool,
    pub active_buffs: Vec<String>,
    pub target_debuffs: Vec<String>,
    pub active_buff_details: Vec<BuffDebuffTraceDetail>,
    pub target_debuff_details: Vec<BuffDebuffTraceDetail>,
    pub active_buff_details_after_hit: Vec<BuffDebuffTraceDetail>,
    pub target_debuff_details_after_hit: Vec<BuffDebuffTraceDetail>,
    pub resource_gains: Vec<ResourceGainTrace>,
    pub held_cards: Vec<HeldCardTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardUseEvent {
    pub time: f64,
    pub card_id: u32,
    pub card_name: String,
}

/// 단일 데미지 이벤트 (trace 모드에서만 수집)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DamageTraceEvent {
    /// 타격 시각 (초)
    pub time: f64,
    pub skill_id: u32,
    pub skill_name: String,
    /// 스킬 hits 배열 기준 0-indexed 타수
    pub hit_index: usize,
    pub hit_id: String,
    pub hit_name: String,
    pub damage_source: String,
    pub result_damage: f64,
    pub is_crit: bool,
    pub resource_gains: Vec<ResourceGainTrace>,
    pub held_cards_after_hit: Vec<HeldCardTrace>,
    pub formula: DamageFormulaTrace,
}

/// 사람이 데미지 식을 역산할 수 있도록 보존하는 중간 계산값.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DamageFormulaTrace {
    pub base_damage: f64,
    pub skill_modifier: f64,
    pub attack_power: f64,
    /// base_damage + skill_modifier * attack_power
    pub base_term: f64,
    /// 1.0 + 스킬 피해 증가 + 런타임 피해 증가
    pub skill_damage_multiplier: f64,
    pub additional_damage_multiplier: f64,
    pub evolution_damage_multiplier: f64,
    pub damage_increase_multiplier: f64,
    pub movement_speed_damage_multiplier: f64,
    pub target_damage_multiplier: f64,
    pub type_damage_multiplier: f64,
    pub elemental_damage_multiplier: f64,
    pub defense_multiplier: f64,
    pub crit_rate: f64,
    pub crit_multiplier: Option<f64>,
    pub on_crit_damage_multiplier: Option<f64>,
    pub factors: Vec<DamageFactorTrace>,
}

/// 데미지 식의 한 항목과 그 항목을 만든 출처 목록.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DamageFactorTrace {
    /// 예: "damageIncrease", "additionalDamage", "critRate"
    pub name: String,
    /// 식에 사용된 최종값. multiplier 항목은 1.0 기준 배율.
    pub value: f64,
    /// 예: "multiplier", "additiveRate"
    pub operation: String,
    pub sources: Vec<DamageSourceTrace>,
}

/// 데미지 식 항목에 기여한 단일 source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DamageSourceTrace {
    /// 예: "engraving:원한", "buff:아드레날린"
    pub source: String,
    pub value: f64,
    pub stacks: Option<u32>,
}

/// trace 모드에서 수집하는 상세 이벤트 묶음.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TraceData {
    pub casts: Vec<CastEvent>,
    pub card_uses: Vec<CardUseEvent>,
    pub damage_events: Vec<DamageTraceEvent>,
}

/// 스킬 단위 집계
/// TODO: Stage 6에서 results/collector.rs 로 이동 예정
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillStat {
    pub count: u32,
    pub total_damage: f64,
    pub hit_count: u32,
    pub crit_count: u32,
    /// Damage Per Cast
    pub dpc: f64,
}
