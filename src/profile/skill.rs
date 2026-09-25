use hashbrown::HashMap;
use serde::{Deserialize, Serialize};

use crate::profile::stat::StatField;


// ---------------------------------------------------------------------------
// 스킬 분류
// ---------------------------------------------------------------------------

/// Go의 MaxPhase 암묵 의존 제거 — phase 정보를 variant 안으로 흡수
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SkillType {
    Normal,
    Point,
    Combo {
        /// 0-indexed 마지막 페이즈 (3타 콤보 = 2)
        last_phase: u32,
        /// 단계 유지 타임아웃 ms (기본값: COMBO_PHASE_TIMEOUT_MS)
        phase_timeout_ms: u32,
    },
    Chain {
        last_phase: u32,
        phase_timeout_ms: u32,
    },
    Holding {
        max_hold_ms: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillCategory {
    Normal,
    Stacked,
    Ruin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSlot {
    Normal,
    Awakening,
    HyperAwakeningTechniques,
    SuperAwakening,
}

// ---------------------------------------------------------------------------
// ModifierManager 태그 (스킬 분류 키)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkillTag {
    SkillId(u32),
    SkillGroup(u32),
    CategoryNormal,
    CategoryRuin,
    CategoryStacked,
    CategoryAwakening,
    CategoryHyper,
    CategorySuperAwakening,
    AttackBack,
    AttackHead,
    AttackFront,
    Charging,
    HoldingCasting,
    UsesMana,
    SourceArkPassive,
    SourceArkGrid,
    SourceEquipment,
    // 속성
    ElementFire,
    ElementWater,
    ElementElectricity,
    ElementWind,
    ElementDark,
    ElementHoly,
    /// 전 스킬 적용 (원한, 팔찌 피해 증가 등)
    All,
    /// 백·헤드 판정이 없는 스킬.
    NonDirectional,
    /// 타격의 대가 적용 대상: 방향성이 없고 각성기가 아닌 스킬.
    NonDirectionalNonAwakening,
}

// ---------------------------------------------------------------------------
// 버프 효과 정의
// ---------------------------------------------------------------------------
// BuffEffect enum 제거 — StatKey HashMap으로 대체.
// BuffSpec.effects: HashMap<StatKey, f64> 사용.

/// 버프 중첩 방식 — Go StackType 대응
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StackType {
    /// 기존 스택에 합산 + 시간 갱신
    Refresh,
    /// 시간 연장
    Extend,
    /// 각 스택이 독립적으로 시간 관리
    Independent,
}

/// 버프의 완전한 명세 — 프로필에 self-contained로 포함된다.
/// Go의 Buff 구조(Effect map + Stacks + Duration 자체 보유)와 동일한 철학.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuffSpec {
    /// 버프 식별자 (BuffManager 키, dedup용)
    pub id: String,
    /// 스탯 효과 (스택당 적용). StatField → 스택당 delta.
    pub effects: HashMap<StatField, f64>,
    pub max_stacks: u32,
    pub stack_type: StackType,
    pub duration_ms: u32,
}

// ---------------------------------------------------------------------------
// HitTrigger 시스템
// ---------------------------------------------------------------------------

/// 히트 시점에 평가되는 조건
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HitCondition {
    Always,
    IsCrit,
    TargetHpBelow(f64),
    BuffActive(String),
    TargetBuffActive(String),
    TargetBuffAtMaxStacks(String),
}

/// 트리거 발생 시점
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HitTriggerOn {
    OnHit,
    OnCrit,
    OnHitIf(HitCondition),
}

/// 트리거 효과 — builder가 주입, engine이 실행
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TriggerEffect {
    /// 한 번의 확률 판정으로 여러 효과를 함께 실행한다.
    Sequence(Vec<TriggerEffect>),
    /// 성공했을 때만 안쪽 효과를 실행한다.
    Chance {
        chance: f64,
        effect: Box<TriggerEffect>,
    },
    /// 후보 중 하나를 같은 확률로 실행한다.
    RandomChoice(Vec<TriggerEffect>),
    /// 후보 중 하나를 지정 가중치로 실행한다. 가중치는 합계와 무관하게 정규화한다.
    WeightedRandomChoice(Vec<(f64, TriggerEffect)>),
    /// 지정 시간이 지난 뒤 안쪽 효과를 이벤트 큐에서 실행한다.
    Schedule {
        delay_ms: u32,
        effect: Box<TriggerEffect>,
    },
    /// 자원 획득 (Mp, Battery, CardGauge, Fury 등 문자열 키)
    GainResource {
        resource: String,
        amount: f64,
    },
    /// 최대 자원 기준 비율 획득 (0.12 = 최대치의 12%).
    GainResourcePercent {
        resource: String,
        percent: f64,
    },
    /// 이 캐스트에서 실제 소모한 MP의 비율만큼 적중 시 환급한다.
    RefundCastMp { percent: f64 },
    /// 버프 적용. BuffSpec이 효과·스택·지속시간을 모두 자체 보유한다.
    ApplyBuff(BuffSpec),
    /// 자신에게 지정 중첩 수만큼 Buff를 적용한다.
    ApplyBuffStacks {
        spec: BuffSpec,
        stacks: u32,
    },
    /// 자신의 Buff를 지정 중첩 수만큼 소비한다.
    ConsumeBuffStacks {
        buff_id: String,
        stacks: u32,
    },
    /// 이 스킬에 방금 설정된 쿨다운 또는 충전 회복 시간을 줄인다.
    ReduceSkillCooldown { percent: f64 },
    /// 지정 스킬들의 현재 남은 쿨다운 또는 충전 회복 시간을 줄인다.
    ReduceCooldowns { skill_ids: Vec<u32>, percent: f64 },
    /// 자원이 처음 최대치에 도달한 순간 지정 스킬들의 남은 쿨다운을 줄인다.
    ReduceCooldownsOnResourceFull {
        resource: String,
        skill_ids: Vec<u32>,
        percent: f64,
    },
    /// 대상 버프/디버프 적용. 루인 스택처럼 적에게 쌓이는 상태에 사용한다.
    ApplyTargetBuff(BuffSpec),
    /// 대상 버프/디버프를 지정 스택만큼 적용한다.
    ApplyTargetBuffStacks {
        spec: BuffSpec,
        stacks: u32,
    },
    /// 대상 버프/디버프를 기존 중첩과 무관하게 지정 중첩으로 설정한다.
    SetTargetBuffStacks {
        spec: BuffSpec,
        stacks: u32,
    },
    /// 대상 버프/디버프 제거. 루인 스킬이 대상 스택을 소비할 때 사용한다.
    ConsumeTargetBuff {
        buff_id: String,
        preserve_chance: f64,
        preserve_required_stacks: u32,
        preserve_resource_gain: Option<(String, f64)>,
    },
    /// 조건부 피해 증가 (Go ConditionalDamageModifier)
    DamageBonus(f64),
    /// 현재 캐릭터 Buff 중첩당 이 Hit의 피해 증가.
    DamageBonusPerBuffStack {
        buff_id: String,
        per_stack: f64,
    },
    /// 대상 버프 스택 수에 따라 별도 피해를 발생시킨다.
    DealDamageByTargetBuffStacks {
        buff_id: String,
        hits_by_stack: Vec<RuntimeDamageSpec>,
        forced_stacks_while_buff: Option<(String, u32)>,
    },
    /// 카드 등 스킬 Hit 밖에서 발생하는 직접 피해.
    DealRuntimeDamage(RuntimeDamageSpec),
    /// DoT 예약
    ScheduleDot {
        dot_id: String,
        hit: RuntimeDamageSpec,
        first_tick_ms: u32,
        tick_interval_ms: u32,
        tick_count: u32,
        refresh: bool,
    },
    /// 카드 뽑기 (아르카나 전용, 비아르카나 직업은 no-op)
    DrawCard,
    /// 가장 최근에 사용한 비복제 카드를 손패에 다시 추가한다.
    DrawLastUsedCard,
    /// 이 Hit을 발생시킨 스킬의 재사용 대기시간 초기화.
    ResetSkillCooldown,
    /// 버프 활성 중 방금 소비된 스킬 쿼다운/충전 1회를 복구하고 버프를 제거한다.
    ResetConsumedSkillCooldown {
        buff_id: String,
        max_charges: u32,
    },
    /// 별도 피해 Effect를 실행하고 대상 Buff를 지정 중첩까지 채운다.
    DealDamageAndSetTargetBuffStacks {
        effect_id: u32,
        hit: RuntimeDamageSpec,
        spec: BuffSpec,
        stacks: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeDamageSpec {
    pub hit_id: String,
    pub name: String,
    pub base_damage: f64,
    pub skill_modifier: f64,
    pub damage_increase: f64,
    pub crit_chance_bonus: f64,
    pub crit_damage_bonus: f64,
    pub random_crit_damage_chance: f64,
    pub random_crit_damage_bonus: f64,
    pub defense_ignore: f64,
    pub defense_ignore_chance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitTrigger {
    pub on: HitTriggerOn,
    pub effect: TriggerEffect,
    /// 발동 확률 (0.0–1.0). 1.0 = 항상.
    pub chance: f64,
}

// ---------------------------------------------------------------------------
// GlobalTrigger (런타임 조건부 — CharacterProfiles에 보관)
// ---------------------------------------------------------------------------

/// 어느 스킬/히트에 반응할지
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TriggerFilter {
    SkillTag(SkillTag),
    SkillId(u32),
    SkillCastId(u32),
    BeforeSkillCastId(u32),
    AnyHit,
    /// 치명타 적중 시 (타격의 대가 등)
    AnyCrit,
    /// 이동기·기본공격을 제외한 모든 스킬 캐스트 시점 (아드레날린 등)
    SkillCast,
    CardUse,
}

/// 런타임 조건 — State를 보고 발동 여부 결정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TriggerCondition {
    Always,
    Chance(f64),
    BuffActive(String),
    ResourceAbove { resource: String, threshold: f64 },
    AfterSkill(u32),
    /// 지정 버프가 최대 중첩에 도달했을 때
    BuffAtMaxStacks(String),
}

/// 빌드타임에 bake 불가한 조건부 트리거
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalTrigger {
    pub filter: TriggerFilter,
    pub condition: TriggerCondition,
    pub effect: TriggerEffect,
    /// 트리거 재발동 최소 간격 ms. None = 제한 없음.
    /// 예: "30초마다 1중첩" 팔찌 효과 → Some(30_000)
    pub cooldown_ms: Option<u32>,
}

// ---------------------------------------------------------------------------
// 스킬 히트 / 프로필
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillHit {
    /// raw SkillEffect ID. 데이터 기반 발동 경로 매칭에 사용한다.
    pub effect_id: u32,
    pub hit_id: String,
    pub name: String,
    pub base_damage: f64,
    pub skill_modifier: f64,
    /// 캐스트 시작부터 설치기·투사체 생성까지의 공속 영향 구간.
    pub action_delay_ms: u32,
    /// 생성 이후 실제 타격까지의 공속 비영향 구간.
    pub fixed_delay_ms: u32,
    pub counter_attack: bool,
    /// 콤보/체인에서 이 히트가 속하는 페이즈 (Normal = 0)
    pub phase: u32,
    pub crit_chance_bonus: f64,
    pub crit_damage_bonus: f64,
    pub defense_ignore: f64,
    pub defense_ignore_chance: f64,
    /// 스킬 고유 피해 증가 (Go DamageIncrese 오타 수정)
    pub damage_increase: f64,
    pub stagger: f64,
    pub destruction: u32,
    /// false: 히트 시점에 snapshot 재캡처 (버프 반영)
    /// true:  캐스트 시점 snapshot 고정 (DoT 등)
    pub use_snapshot: bool,
    /// builder가 주입한 런타임 트리거 목록
    pub triggers: Vec<HitTrigger>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillProfile {
    pub skill_id: u32,
    pub name: String,
    pub skill_type: SkillType,
    pub category: SkillCategory,
    pub slot: SkillSlot,
    /// ModifierManager 조회용 태그
    pub tags: Vec<SkillTag>,
    pub cooldown_ms: u32,
    /// 단계별 시전 시간 (Normal = [cast_ms], Combo = [phase0_ms, phase1_ms, ...])
    pub cast_times_ms: Vec<u32>,
    /// 전투 중 총 사용 가능 횟수. 0이면 제한 없음.
    pub max_uses: u32,
    /// 최대 충전 횟수 (1 = 일반 쿨타임, 2+ = 충전형)
    pub max_stacks: u32,
    /// 충전 1회 회복 간격 ms (max_stacks <= 1이면 0)
    pub charge_recovery_ms: u32,
    /// 스킬 사용 시 소모 자원 ("Mp", "Battery", "CardGauge" 등)
    pub resource_costs: HashMap<String, f64>,
    /// 마나 용광로처럼 최대 마나에 비례해 추가로 소모하는 비율.
    pub extra_mp_cost_ratio: f64,
    /// 시전 시 MP 비용을 소모하지 않을 확률.
    pub mp_cost_waiver_chance: f64,
    /// 스킬 사용 시 획득 자원 ("CardGauge", "Fury" 등)
    pub resource_gains: HashMap<String, f64>,
    /// 캐스트 시점에 Buff가 있으면 이 스킬에만 적용하는 피해 증가.
    pub cast_buff_damage_bonuses: Vec<(String, f64)>,
    /// 캐스트 시점에 Buff가 있으면 이 스킬에만 적용하는 치명타 적중률.
    pub cast_buff_crit_rate_bonuses: Vec<(String, f64)>,
    /// 스킬 한정 진화형 피해. 전체 진화형 피해 버킷에 합산한다.
    pub evolution_damage_bonuses: Vec<(String, f64)>,
    pub hits: Vec<SkillHit>,
}
