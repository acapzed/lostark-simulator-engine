use hashbrown::HashMap;
use serde::{Deserialize, Serialize};

use crate::profile::apl::{AplAction, CardAplAction};
use crate::profile::skill::{GlobalTrigger, SkillProfile, TriggerEffect};
use crate::profile::stat::StatProfile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuffMetadata {
    pub name: String,
    pub icon: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArcanaCardPoolEntry {
    pub skill_id: u32,
    pub draw_weight: f64,
}

/// 시뮬레이션 중 절대 변하지 않는 캐릭터 설계도.
/// prepare() 완료 후 SharedArrayBuffer에 단 하나만 존재하며
/// 모든 워커가 복사 없이 동일 주소를 참조한다.
///
/// CompiledModifiers는 build 과정의 중간 산물이므로 여기에 포함하지 않는다.
/// 모든 수치는 build 완료 시점에 SkillProfile / StatProfile 안으로 구워진다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterProfiles {
    pub name: String,
    pub class_name: String,
    pub stat: StatProfile,
    pub skills: Vec<SkillProfile>,
    pub max_hp: f64,
    pub max_mp: f64,
    pub target_defense: f64,
    /// 런타임 조건부 트리거 — 빌드타임에 bake 불가한 경우만 여기 보관
    pub global_triggers: Vec<GlobalTrigger>,
    /// APL 우선순위 (skill_id 순서). BuildRequest.apl_config 파싱 결과.
    /// 빈 경우 skills 순서가 기본값.
    pub apl_order: Vec<u32>,
    /// 조건까지 컴파일된 APL 액션. 비어 있으면 apl_order만 사용한다.
    pub apl_actions: Vec<AplAction>,
    /// 스킬 GCD와 독립적으로 평가하는 아이덴티티 카드 사용 우선순위.
    pub card_apl_actions: Vec<CardAplAction>,
    /// 카드 ID와 커뮤니티 표본 기반 추첨 가중치.
    pub arcana_card_pool: Vec<ArcanaCardPoolEntry>,
    #[serde(default)]
    pub arcana_card_names: HashMap<u32, String>,
    /// 전투 시작 전 뽑아 둘 카드 수. 현재 아르카나 각성 물약은 2장.
    #[serde(default)]
    pub initial_card_draws: u32,
    pub arcana_card_effects: HashMap<u32, TriggerEffect>,
    /// 해당 Buff 활성 중에는 같은 카드를 다시 사용하지 못한다.
    pub arcana_card_blocking_buffs: HashMap<u32, String>,
    pub buff_metadata: HashMap<String, BuffMetadata>,
}
