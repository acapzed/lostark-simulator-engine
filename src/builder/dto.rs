use serde::Deserialize;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// 시뮬레이션 요청 (JS → WASM 진입점)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRequest {
    pub character: CharacterInput,
    #[serde(default)]
    pub apl_config: String,
    // HP 상태 가정 플래그 (달인의 저력 / 안정된 상태 등 조건부 각인 활성화 여부)
    // Frontend에서 빌드 설정으로 입력. 시뮬레이션 루프와 무관하게 정적으로 평가됨.
    #[serde(default)]
    pub assumed_low_hp: bool, // true = HP 50% 이하 가정 (달인의 저력, 아드로핀 빌드)
    #[serde(default)]
    pub assumed_high_hp: bool, // true = HP 65% 이상 가정 (안정된 상태)
    #[serde(default = "default_target_hp_percent")]
    pub target_hp_percent: f64,
    #[serde(default)]
    pub assumed_target_staggered: bool,
    #[serde(default)]
    pub assumed_shield_active: bool,
    #[serde(default)]
    pub assumed_counter_success: bool,
    #[serde(default)]
    pub sight_focus_skill_name: String,
    #[serde(default)]
    pub ether_pickup_delay_ms: Option<u32>,
    #[serde(default)]
    pub necromancy_travel_delay_ms: Option<u32>,
    #[serde(default)]
    pub enable_feast: bool,
    #[serde(default)]
    pub enable_awakening_potion: bool,
    #[serde(default)]
    pub enable_pet_buff: bool,
    // Go의 JSON 키가 PascalCase이므로 명시적 rename
    #[serde(default, rename = "CollectionStats")]
    pub collection_stats: f64,
    #[serde(default)]
    pub avatar_main_stat_percent: f64,
    #[serde(default)]
    pub card_damage_bonus: f64,
    #[serde(default)]
    pub pet_buff_type: String,
    #[serde(default)]
    pub pet_talents: PetTalents,
    /// 캐릭터가 장착한 스킬들의 게임 DB 데이터.
    /// Frontend가 Backend에서 조회한 뒤 캐릭터 데이터와 합쳐서 전달한다.
    #[serde(default)]
    pub skill_data: Vec<SkillDataInput>,
    #[serde(default)]
    pub card_data: Vec<ArcanaCardInput>,
    #[serde(default)]
    pub buff_data: Vec<BuffMetadataInput>,
    /// 장비 세트별 강화 단계 스탯 테이블.
    /// Frontend가 Backend에서 캐싱한 뒤 전달한다.
    /// key1: 세트 이름 (예: "세르카", "에기르")
    /// key2: 슬롯 이름 (예: "무기", "투구", "어깨", "상의", "하의", "장갑")
    /// value: stage 1~25 행 배열 (index = stage - 1 로 직접 접근 가능)
    #[serde(default)]
    pub equipment_tables: HashMap<String, HashMap<String, Vec<EquipmentStageRow>>>,
    #[serde(default)]
    pub advanced_honing_tables: HashMap<String, Vec<AdvancedHoningRow>>,
    /// 시뮬레이션 대상 몬스터 타입. 종류별 피해 증가 적용에 사용.
    /// 예: "악마", "대악마", "인간", "기계", "" (범용 — 종류별 보너스 미적용)
    #[serde(default)]
    pub target_monster_type: String,
    /// 레벨 70 PvE NPC 기본 방어력은 raw NpcStat 기준 1606.
    #[serde(default)]
    pub target_defense: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuffMetadataInput {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub source: String,
}

fn default_target_hp_percent() -> f64 {
    100.0
}

// ---------------------------------------------------------------------------
// 캐릭터
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterInput {
    pub name: String,
    pub class_name: String,
    // 프로필에는 저장하지 않음
    // pub item_level: f64,
    pub stats: CharacterStatsInput,
    #[serde(default)]
    pub equipments: Vec<EquipmentInput>,
    #[serde(default)]
    pub accessories: Vec<AccessoryInput>,
    pub bracelet: Option<BraceletInput>,
    pub stone: Option<StoneInput>,
    #[serde(default)]
    pub gems: Vec<GemInput>,
    #[serde(default)]
    pub skills: Vec<CharacterSkillInput>,
    #[serde(default)]
    pub engravings: Vec<EngravingInput>,
    pub ark_passive: Option<ArkPassiveInput>,
    pub ark_grids: Option<ArkGridInput>,
    pub ark_passive_karma: Option<ArkPassiveKarmaInput>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CharacterStatsInput {
    #[serde(rename = "maxhp", default)]
    pub max_hp: f64,
    // 검증용 데이터 - 모든 값은 source로부터 build하여 생성함
    // #[serde(default)]
    // pub crit: f64,
    // #[serde(default)]
    // pub specialization: f64,
    // #[serde(default)]
    // pub domination: f64,
    // #[serde(default)]
    // pub swiftness: f64,
    // #[serde(default)]
    // pub endurance: f64,
    // #[serde(default)]
    // pub expertise: f64,
    // #[serde(rename = "attackPower", default)]
    // pub attack_power: f64,
    // #[serde(default)]
    // pub strength: f64,
    // #[serde(default)]
    // pub dexterity: f64,
    // #[serde(default)]
    // pub intelligence: f64,
    // #[serde(default)]
    // pub vitality: f64,
}

// ---------------------------------------------------------------------------
// 장비 / 악세서리
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EquipmentInput {
    #[serde(rename = "type")]
    pub item_slot: String,
    pub set_name: String,
    pub quality: i32,
    pub item_level: u32,
    pub refinement_level: u32,
    pub advanced_refinement_level: u32,
}

/// 장비 세트 강화 단계별 스탯 행.
/// item_slot별 값은 Optional — 완갑은 재련 단계와 네 스탯을 함께 사용한다.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EquipmentStageRow {
    #[serde(default)]
    pub item_level: u32,
    #[serde(default)]
    pub refinement_level: u32,
    #[serde(default)]
    pub weapon_attack: f64,
    #[serde(default)]
    pub main_stat: f64,
    #[serde(default)]
    pub vitality: f64,
    #[serde(default)]
    pub base_attack_power_percent: f64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedHoningRow {
    pub stage: u32,
    #[serde(default)]
    pub stage_bonus_stat_rate: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessoryInput {
    // 시뮬레이션에서는 쓰지 않음
    // pub slot: String,
    // pub name: String,
    // pub grade: String,
    // pub quality: u32,
    #[serde(default)]
    pub base_stats: AccessoryBaseStats,
    #[serde(default)]
    pub polishing_effects: Vec<PolishingEffectInput>,
    // 카르마 수치는 검증하지 않음
    // pub ark_passive_karma: Option<ArkPassiveKarmaPointInput>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccessoryBaseStats {
    #[serde(default)]
    pub main_stat: f64,
    #[serde(default)]
    pub vitality: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolishingEffectInput {
    #[serde(rename = "type")]
    pub effect_type: String,
    pub value: f64,
    pub is_percentage: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BraceletInput {
    #[serde(default)]
    pub locked_effects: Vec<BraceletEffectInput>,
    #[serde(default)]
    pub changeable_effects: Vec<BraceletEffectInput>,
    // 카르마값은 검증하지 않음
    // pub ark_passive_karma: Option<ArkPassiveKarmaPointInput>,
}

#[derive(Debug, Deserialize)]
pub struct BraceletEffectInput {
    /// 효과 이름 (특수 효과 이름 기반 매칭용). 미전달 시 descriptions 텍스트로 폴백.
    #[serde(default)]
    pub name: String,
    pub values: Vec<f64>,
    pub descriptions: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoneInput {
    #[serde(default)]
    pub base_stats: AccessoryBaseStats,
    #[serde(default)]
    pub bonus_stats: AccessoryBaseStats,
    #[serde(default)]
    pub engravings: Vec<EngravingItemInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngravingItemInput {
    pub name: String,
    pub level: u32,
    #[serde(default)]
    pub is_negative: bool,
}

// ---------------------------------------------------------------------------
// 보석 / 스킬 / 각인
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GemInput {
    pub level: u32,
    #[serde(rename = "type", default)]
    pub gem_type: String,
    pub skill_name: String,
    pub skill_effect: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterSkillInput {
    pub name: String,
    pub level: u32,
    pub rune: Option<RuneInput>,
    #[serde(default)]
    pub tripods: Vec<TripodChoiceInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TripodChoiceInput {
    pub name: String,
    /// Backend 게임 DB에서 조회한 트라이포드 수치값 배열.
    /// Frontend가 캐릭터 데이터와 게임 DB를 합쳐서 전달한다.
    #[serde(default)]
    pub values: Vec<f64>,
}

#[derive(Debug, Deserialize)]
pub struct RuneInput {
    pub name: String,
    pub grade: String,
    #[serde(default)]
    pub effects: Vec<SimEffect>,
}

// ---------------------------------------------------------------------------
// 각인 simEffects — 시뮬레이터용 레벨별 효과 데이터
// ---------------------------------------------------------------------------

/// 단일 각인 효과 항목.
///
/// `kind` 기본값은 "stat".
/// - `"stat"`:          field + value → stat.add_stat() 직접 bake
/// - `"conditional"`:   field + value + condition → BuildRequest 플래그 확인 후 bake
/// - `"stack_trigger"`: trigger_event + per_stack + on_max_stack → GlobalTrigger 등록
///
/// tag 필드가 있으면 kind와 무관하게 ModifierManager(tag 기반 DamageMultiplier)로 라우팅.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SimEffect {
    #[serde(default = "default_sim_effect_kind")]
    pub kind: String,
    // "stat" / "conditional" / modifier(tag 존재 시 자동)
    #[serde(default)]
    pub field: String,
    #[serde(default)]
    pub value: f64,
    #[serde(default)]
    pub chance: f64,
    #[serde(default)]
    pub buff_id: String,
    #[serde(default)]
    pub consumes_buff_id: String,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u32>,
    #[serde(default)]
    pub cooldown_ms: Option<u32>,
    #[serde(default)]
    pub dot_id: String,
    #[serde(default)]
    pub hit_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub damage_ratio: f64,
    #[serde(default)]
    pub fixed_damage: f64,
    #[serde(default)]
    pub first_tick_ms: u32,
    #[serde(default)]
    pub tick_interval_ms: u32,
    #[serde(default)]
    pub tick_count: u32,
    // "stack_trigger" 전용
    #[serde(default)]
    pub trigger_event: Option<String>,
    #[serde(default)]
    pub max_stacks: Option<u32>,
    /// 스택 지속 시간 (초 단위). register에서 ×1000 → ms 변환.
    #[serde(default)]
    pub stack_duration: Option<f64>,
    /// 스택당 적용 효과
    #[serde(default)]
    pub per_stack: Option<Vec<SimEffect>>,
    /// 최대 중첩 달성 시 추가 효과
    #[serde(default)]
    pub on_max_stack: Option<Vec<SimEffect>>,
}

fn default_sim_effect_kind() -> String {
    "stat".to_string()
}

/// 각인 simEffects — 등급별 레벨 효과 + 공통 스톤 효과
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngravingSimEffects {
    /// 등급별 레벨 효과. 키: 등급 문자열 (예: "기본", "전설", "유물")
    #[serde(default)]
    pub grades: HashMap<String, EngravingGradeEffects>,
    /// 어빌리티 스톤 활성화 효과 배열 (등급 무관, 0-indexed)
    #[serde(default)]
    pub stone: Vec<Vec<SimEffect>>,
}

/// 특정 등급의 각인 레벨 효과
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngravingGradeEffects {
    /// 각인 레벨별 효과 배열 (0-indexed)
    #[serde(default)]
    pub levels: Vec<Vec<SimEffect>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngravingInput {
    pub name: String,
    #[serde(default)]
    pub icon: String,
    pub grade: String,
    pub level: u32,
    #[serde(default)]
    pub ability_level: u32,
    #[serde(default)]
    pub feature_data: Option<EngravingFeatureDataInput>,
    #[serde(default)]
    pub sim_effects: Option<EngravingSimEffects>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngravingFeatureDataInput {
    #[serde(default)]
    pub base_levels: Vec<EngravingFeatureRowInput>,
    #[serde(default)]
    pub grades: Vec<EngravingGradeInput>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngravingFeatureRowInput {
    #[serde(default)]
    pub stage: u32,
    #[serde(default)]
    pub feature_type: u32,
    #[serde(default)]
    pub values: Vec<f64>,
    #[serde(default)]
    pub cooldown: u32,
    #[serde(default)]
    pub base_ratio: f64,
    #[serde(default)]
    pub combat_effect: Option<EngravingCombatEffectInput>,
    #[serde(default)]
    pub skill_buff: Option<EngravingSkillBuffInput>,
    #[serde(default)]
    pub skill_buffs: Vec<EngravingSkillBuffInput>,
    #[serde(default)]
    pub skill_effects: Vec<EngravingSkillEffectInput>,
    #[serde(default)]
    pub drop_ethers: Vec<EngravingDropEtherInput>,
    #[serde(default)]
    pub summon_npc: Option<EngravingSummonNpcInput>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct EngravingSummonNpcInput {
    pub id: u32,
    pub coefficient: f64,
    pub skills: Vec<EngravingSummonSkillInput>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct EngravingSummonSkillInput {
    pub id: u32,
    pub probability: f64,
    pub action_effects: Vec<EngravingSummonActionEffectInput>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct EngravingSummonActionEffectInput {
    pub time_ms: u32,
    pub id: u32,
    pub key: u32,
    pub value_a: f64,
    pub value_b: f64,
    pub value_f: f64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct EngravingDropEtherInput {
    pub id: u32,
    pub audience: String,
    pub delay_time: u32,
    pub effect: Option<EngravingDropEtherEffectInput>,
    pub skill_buff: Option<EngravingSkillBuffInput>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct EngravingDropEtherEffectInput {
    pub key: u32,
    pub value_a: f64,
    pub value_b: f64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct EngravingSkillEffectInput {
    pub id: u32,
    pub key: u32,
    pub value_a: f64,
    pub value_b: f64,
    pub value_f: f64,
    pub multi_hit_count: u32,
    pub multi_hit_time: u32,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngravingCombatEffectInput {
    pub id: u32,
    pub action_type: u32,
    pub action_actor: u32,
    #[serde(default)]
    pub args: Vec<f64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct EngravingSkillBuffInput {
    pub id: u32,
    pub duration: i32,
    pub overlap: u32,
    pub unique_group: u32,
    pub visible_filter_type: u32,
    pub visible_filter_value: f64,
    pub key: u32,
    pub values: Vec<f64>,
    pub passive_option: EngravingPassiveOptionInput,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct EngravingPassiveOptionInput {
    pub r#type: u32,
    pub key_stat: u32,
    pub key_index: u32,
    pub value: f64,
    pub duplex: i32,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngravingGradeInput {
    pub grade: u32,
    #[serde(default)]
    pub stages: Vec<EngravingFeatureRowInput>,
}

// ---------------------------------------------------------------------------
// 아크 패시브 / 아크 그리드
// ---------------------------------------------------------------------------

/// 아크 패시브 입력.
/// 프론트엔드가 className 기준으로 분류하여 전달한다.
/// - common_nodes: className == null (진화 + 도약 공통) — 데이터 드리븐
/// - class_nodes:  className == 클래스명 (깨달음 + 도약 클래스) — 클래스 코드
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArkPassiveInput {
    #[serde(default)]
    pub common_nodes: Vec<ArkPassiveCommonNodeInput>,
    #[serde(default)]
    pub class_nodes: Vec<ArkPassiveClassNodeInput>,
}

/// 공통 아크패시브 노드 — 데이터 드리븐 (simEffects)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArkPassiveCommonNodeInput {
    pub name: String,
    pub level: u32,
    /// 레벨별 simEffects. index = level - 1.
    #[serde(default)]
    pub effects_by_level: Vec<Vec<SimEffect>>,
}

/// 클래스 전용 아크패시브 노드 — 클래스 코드에서 처리
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArkPassiveClassNodeInput {
    pub name: String,
    pub level: u32,
    /// 해당 레벨의 수치값 배열. 클래스 코드에서 참조.
    #[serde(default)]
    pub values: Vec<f64>,
}

/// 아크 그리드 입력.
/// - gems:         공통 젬 (데이터 드리븐)
/// - common_cores: 혼돈 코어 (공통, 데이터 드리븐)
/// - class_cores:  질서 코어 (클래스 전용, 클래스 코드)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArkGridInput {
    #[serde(default)]
    pub gems: Vec<ArkGridGemInput>,
    #[serde(default)]
    pub common_cores: Vec<ArkGridCommonCoreInput>,
    #[serde(default)]
    pub class_cores: Vec<ArkGridClassCoreInput>,
}

/// 혼돈 코어 — 데이터 드리븐 (simEffects)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArkGridCommonCoreInput {
    pub name: String,
    /// 해당 슬롯의 젬 포인트 합산
    pub points: u32,
    /// 옵션별 (임계값 + simEffects)
    #[serde(default)]
    pub options: Vec<ArkGridCommonCoreOption>,
}

/// 혼돈 코어 개별 옵션 — 포인트 임계값 + simEffects
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArkGridCommonCoreOption {
    pub point_requirement: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub sim_effects: Vec<SimEffect>,
}

/// 질서 코어 — 클래스 코드에서 처리
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArkGridClassCoreInput {
    pub name: String,
    /// 해당 슬롯의 젬 포인트 합산
    pub points: u32,
    /// Backend 게임 데이터에서 조회한 raw 코어 옵션 목록
    #[serde(default)]
    pub options: Vec<ArkGridClassCoreOption>,
}

/// 질서 코어 개별 옵션 — raw ID와 해석이 끝난 실행 효과
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArkGridClassCoreOption {
    pub point_requirement: u32,
    #[serde(default)]
    pub raw_option_id: u32,
    #[serde(default)]
    pub runtime_effects: Vec<ArkGridClassCoreRuntimeEffect>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArcanaCardInput {
    pub skill_id: u32,
    pub name: String,
    pub auto_learn: bool,
    #[serde(default = "default_card_draw_weight")]
    pub draw_weight: f64,
    #[serde(default)]
    pub runtime_effects: Vec<ArcanaCardRuntimeEffect>,
}

fn default_card_draw_weight() -> f64 {
    1.0
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArcanaCardRuntimeEffect {
    pub kind: String,
    #[serde(default)]
    pub buff_id: u32,
    #[serde(default)]
    pub duration_ms: u32,
    #[serde(default)]
    pub delay_ms: u32,
    #[serde(default)]
    pub crit_rate_percent: f64,
    #[serde(default)]
    pub crit_damage_percent: f64,
    #[serde(default)]
    pub cooldown_reduction_percent: f64,
    #[serde(default)]
    pub target_buff_id: u32,
    #[serde(default)]
    pub target_duration_ms: u32,
    #[serde(default)]
    pub value_percent: f64,
    #[serde(default)]
    pub chance_percent: f64,
    #[serde(default)]
    pub stack_buff_id: u32,
    #[serde(default)]
    pub stack_duration_ms: u32,
    #[serde(default)]
    pub max_stacks: u32,
    #[serde(default)]
    pub attack_speed_percent: f64,
    #[serde(default)]
    pub move_speed_percent: f64,
    #[serde(default)]
    pub unique_group_id: u32,
    #[serde(default)]
    pub result_buff_ids: Vec<u32>,
    #[serde(default)]
    pub damage_percents: Vec<f64>,
    #[serde(default)]
    pub effect_ids: Vec<u32>,
    #[serde(default)]
    pub cooldown_reduction_percents: Vec<f64>,
    #[serde(default)]
    pub max_card_slots: u32,
    #[serde(default)]
    pub target_buff_key: String,
    #[serde(default)]
    pub extra_stacks: u32,
    #[serde(default)]
    pub effect_id: u32,
    #[serde(default)]
    pub hit_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub damage_ratio: f64,
    #[serde(default)]
    pub fixed_damage: f64,
    #[serde(default)]
    pub draw_card_count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArkGridClassCoreRuntimeEffect {
    pub kind: String,
    #[serde(default)]
    pub card_id: u32,
    #[serde(default)]
    pub skill_group_id: u32,
    #[serde(default)]
    pub skill_id: u32,
    #[serde(default)]
    pub target_effect_id: u32,
    #[serde(default)]
    pub identity_category: u32,
    #[serde(default)]
    pub target_buff_id: String,
    #[serde(default)]
    pub required_tripod_name: String,
    #[serde(default)]
    pub source_buff_id: u32,
    #[serde(default)]
    pub buff_id: u32,
    #[serde(default)]
    pub stacks: u32,
    #[serde(default)]
    pub hit_count: u32,
    #[serde(default)]
    pub minimum_stacks: u32,
    #[serde(default)]
    pub max_stacks: u32,
    #[serde(default)]
    pub cards: u32,
    #[serde(default)]
    pub chance_percent: f64,
    #[serde(default)]
    pub cooldown_reduction_percent: f64,
    #[serde(default)]
    pub max_charges: u32,
    #[serde(default)]
    pub recharge_ms: u32,
    #[serde(default)]
    pub duration_ms: u32,
    #[serde(default)]
    pub delay_ms: u32,
    #[serde(default)]
    pub value_ms: i32,
    #[serde(default)]
    pub value_percent: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArkGridGemInput {
    pub slot: u32,
    pub level: u32,
    /// 레벨별 simEffects. index = level - 1.
    /// Frontend가 게임 데이터에서 전 레벨 효과를 넘기고, 시뮬레이터가 level로 선택.
    #[serde(default)]
    pub effects_by_level: Vec<Vec<SimEffect>>,
}

#[derive(Debug, Deserialize)]
pub struct ArkPassiveKarmaInput {
    pub evolution: ArkPassiveKarmaNodeInput,
    pub enlightenment: ArkPassiveKarmaNodeInput,
    pub leap: ArkPassiveKarmaNodeInput,
}

#[derive(Debug, Deserialize, Default)]
pub struct ArkPassiveKarmaNodeInput {
    /// 총 수치는 검증용이므로 쓰지 않음
    // #[serde(default)]
    // pub value: f64,
    #[serde(default)]
    pub rank: u32,
    #[serde(default)]
    pub level: u32,
}

/// 카르마값은 검증하지 않음
// #[derive(Debug, Deserialize, Default)]
// pub struct ArkPassiveKarmaPointInput {
//     #[serde(rename = "type", default)]
//     pub karma_type: String,
//     #[serde(default)]
//     pub value: f64,
// }

// ---------------------------------------------------------------------------
// 펫 특기
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PetTalents {
    #[serde(default)]
    pub additional_damage: f64,
    #[serde(default)]
    pub main_stat_percent: f64,
    #[serde(default)]
    pub type_damage: f64,
}

// ---------------------------------------------------------------------------
// 스킬 게임 DB 데이터 (Frontend가 Backend에서 조회하여 전달)
// ---------------------------------------------------------------------------

/// 스킬 하나의 게임 DB 데이터.
/// Backend의 ISkillModel에 대응하며, build_all()에서 기본 SkillProfile 생성에 사용된다.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDataInput {
    pub name: String,
    #[serde(default)]
    pub game_skill_id: u32,
    #[serde(default)]
    pub identity_category: u32,
    #[serde(default)]
    pub skill_control_type: String,
    #[serde(default)]
    pub skill_slot: String,
    /// 전투 중 총 사용 가능 횟수. 0이면 제한 없음.
    #[serde(default)]
    pub max_uses: u32,
    #[serde(default)]
    pub max_stacks: u32,
    #[serde(default)]
    pub max_phase: u32,
    #[serde(default)]
    pub phase_timeout_ms: u32,
    /// 쿨타임 (초 단위)
    pub cooldown: f64,
    /// 시전 시간 (ms). 일반 스킬은 1개, 콤보/체인은 단계별 배열.
    #[serde(default)]
    pub cast_times_ms: Vec<u32>,
    #[serde(default)]
    pub hits: Vec<HitDataInput>,
    #[serde(default)]
    pub ruin_trigger_effect_ids: Vec<u32>,
    #[serde(default)]
    pub runtime_damage_by_target_buff_stacks: Vec<RuntimeDamageByTargetBuffStackInput>,
    pub target_buff_consume_preserve: Option<TargetBuffConsumePreserveInput>,
    #[serde(default)]
    pub runtime_skill_damage_stacks: Vec<RuntimeSkillDamageStackInput>,
    #[serde(default)]
    pub runtime_target_damage_bonuses: Vec<RuntimeTargetDamageBonusInput>,
    pub self_speed_buff_on_cast: Option<SelfSpeedBuffOnCastInput>,
    pub resource_cost: Option<ResourceCostInput>,
    /// 증감 전 기본 마나 소모량. 마나 무시 모드에서도 피해식에 사용한다.
    #[serde(default)]
    pub base_mana_cost: f64,
    #[serde(default)]
    pub mp_cost_waiver_chance: f64,
    pub identity_gain: Option<IdentityGainInput>,
    #[serde(default)]
    pub properties: SkillPropertiesInput,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDamageByTargetBuffStackInput {
    #[serde(default)]
    pub hit_id: String,
    #[serde(default)]
    pub name: String,
    pub target_buff_id: String,
    pub stack: u32,
    #[serde(default)]
    pub damage_ratio: Vec<f64>,
    #[serde(default)]
    pub fixed_damage: Vec<f64>,
    #[serde(default)]
    pub damage_increase: f64,
    #[serde(default)]
    pub crit_chance_bonus: f64,
    #[serde(default)]
    pub crit_damage_bonus: f64,
    #[serde(default)]
    pub random_crit_damage_chance: f64,
    #[serde(default)]
    pub random_crit_damage_bonus: f64,
    #[serde(default)]
    pub defense_ignore: f64,
    #[serde(default)]
    pub defense_ignore_chance: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfSpeedBuffOnCastInput {
    pub buff_id: String,
    pub duration_ms: u32,
    pub attack_speed_percent: f64,
    #[serde(default)]
    pub move_speed_percent: f64,
}

/// 스킬 히트 하나의 게임 DB 데이터.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HitDataInput {
    #[serde(default)]
    pub effect_id: u32,
    #[serde(default)]
    pub hit_id: String,
    #[serde(default)]
    pub name: String,
    /// 이 hit가 몇번째인지는 index로도 처리 가능함
    // #[serde(default)]
    // pub hit_number: u32,
    #[serde(default)]
    pub phase: u32,
    /// 레벨별 스킬 계수 (%)
    #[serde(default)]
    pub damage_ratio: Vec<f64>,
    /// 레벨별 고정 피해
    #[serde(default)]
    pub fixed_damage: Vec<f64>,
    /// 캐스트 시작부터 설치기·투사체 생성까지의 공속 영향 구간.
    #[serde(default)]
    pub action_delay_ms: u32,
    /// 생성 이후 실제 타격까지의 공속 비영향 구간.
    #[serde(default)]
    pub fixed_delay_ms: u32,
    #[serde(default)]
    pub counter_attack: bool,
    #[serde(default)]
    pub crit_chance_bonus: f64,
    #[serde(default)]
    pub crit_damage_bonus: f64,
    #[serde(default)]
    pub defense_ignore: f64,
    #[serde(default)]
    pub defense_ignore_chance: f64,
    #[serde(default)]
    pub draw_card_count: u32,
    #[serde(default)]
    pub draw_card_chance: f64,
    pub target_buff_stack_gain: Option<TargetBuffStackInput>,
    pub target_buff_stack_set: Option<TargetBuffStackInput>,
    pub target_crit_rate_debuff: Option<TargetCritRateDebuffInput>,
    pub self_crit_rate_stack: Option<SelfCritRateStackInput>,
    pub self_crit_damage_buff: Option<SelfCritDamageBuffInput>,
    #[serde(default)]
    pub reset_cooldown_chance: f64,
    pub additional_damage: Option<AdditionalDamageInput>,
    pub dot: Option<DotInput>,
    #[serde(default)]
    pub damage_increase: f64,
    #[serde(default)]
    pub mp_restore_percent: f64,
    pub identity_gain: Option<IdentityGainInput>,
    #[serde(default)]
    pub ultimate_point_gain: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetBuffConsumePreserveInput {
    pub buff_id: String,
    pub chance: f64,
    pub required_stacks: u32,
    pub preserve_identity_gain: Option<IdentityGainInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetBuffStackInput {
    pub buff_id: String,
    pub stacks: u32,
    pub max_stacks: u32,
    pub duration_ms: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdditionalDamageInput {
    pub effect_id: u32,
    #[serde(default)]
    pub hit_id: String,
    #[serde(default)]
    pub name: String,
    pub chance: f64,
    #[serde(default)]
    pub damage_ratio: Vec<f64>,
    #[serde(default)]
    pub fixed_damage: Vec<f64>,
    pub target_buff_id: String,
    pub target_buff_stacks: u32,
    pub target_buff_max_stacks: u32,
    pub target_buff_duration_ms: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DotInput {
    pub dot_id: String,
    #[serde(default)]
    pub hit_id: String,
    #[serde(default)]
    pub name: String,
    pub chance: f64,
    #[serde(default)]
    pub damage_ratio: Vec<f64>,
    #[serde(default)]
    pub fixed_damage: Vec<f64>,
    pub first_tick_ms: u32,
    pub tick_interval_ms: u32,
    pub tick_count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetCritRateDebuffInput {
    pub buff_id: String,
    pub percent: f64,
    pub duration_ms: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfCritRateStackInput {
    pub buff_id: String,
    pub percent_per_stack: f64,
    pub max_stacks: u32,
    pub duration_ms: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfCritDamageBuffInput {
    pub buff_id: String,
    pub percent: f64,
    pub duration_ms: u32,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSkillDamageStackInput {
    pub buff_id: String,
    pub damage_increase_per_stack: f64,
    pub max_stacks: u32,
    pub duration_ms: u32,
    pub trigger_on: String,
    #[serde(default)]
    pub trigger_effect_ids: Vec<u32>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTargetDamageBonusInput {
    pub trigger_effect_id: u32,
    #[serde(default)]
    pub target_effect_ids: Vec<u32>,
    pub buff_id: String,
    pub duration_ms: u32,
    pub damage_increase: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceCostInput {
    pub name: String,
    /// 레벨별 비용
    #[serde(default)]
    pub cost: Vec<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityGainInput {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillPropertiesInput {
    /// 무력화랑 무력파괴는 hit들의 sum이라 나중에 hit에서 처리함
    // #[serde(default)]
    // pub stagger: f64,
    // #[serde(default)]
    // pub part_break: u32,
    #[serde(default)]
    pub element: String,
    #[serde(default)]
    pub attack_type: String,
    #[serde(default)]
    pub skill_group_ids: Vec<u32>,
}
