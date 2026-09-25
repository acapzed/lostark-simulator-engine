use hashbrown::{HashMap, HashSet};

use crate::engine::state::buff::BuffManager;
use crate::engine::state::class::ClassState;

/// 워커별 독립 가변 상태 — 시뮬레이션 중 매 순간 변하는 데이터
#[derive(Debug, Clone)]
pub struct ActorState {
    /// 모든 직업 공통
    pub hp: f64,
    pub max_hp: f64,

    /// 직업별 자원 및 상태 (Mp/Battery/Fury/CardGauge 등 + Vec 데이터)
    pub class_state: Box<dyn ClassState>,

    /// 지연 연산(Lazy) 자원 회복을 위한 마지막 갱신 시각
    pub last_resource_update: u64,
    pub current_time: u64,

    // 쿨타임 / 충전
    /// skill_id → 쿨타임 종료 시각 (ms)
    pub skill_cooldowns: HashMap<u32, u64>,
    /// skill_id → 현재 충전 횟수
    pub skill_charges: HashMap<u32, u32>,
    /// skill_id → 다음 충전 완료 시각 (ms)
    pub charge_recovery_times: HashMap<u32, u64>,
    /// skill_id → 전투 중 사용 횟수
    pub skill_use_counts: HashMap<u32, u32>,

    // 콤보/체인 단계
    /// skill_id → 현재 콤보 페이즈 (0-indexed)
    pub combo_phases: HashMap<u32, u32>,
    /// skill_id → 콤보 타임아웃 시각 (ms)
    pub combo_timeouts: HashMap<u32, u64>,
    /// skill_id → 현재 체인 페이즈
    pub chain_phases: HashMap<u32, u32>,
    /// skill_id → 체인 타임아웃 시각 (ms)
    pub chain_timeouts: HashMap<u32, u64>,

    pub buff_manager: BuffManager,
    pub target_buff_manager: BuffManager,

    /// GlobalTrigger 쿨타임 추적 — profiles.global_triggers 인덱스 → 마지막 발동 시각 (ms)
    pub trigger_cooldowns: HashMap<usize, u64>,
    /// 전투 중 이미 처리한 자원 완충 이벤트.
    pub full_resource_triggers: HashSet<String>,
}

impl ActorState {
    pub fn new(hp: f64, class_state: Box<dyn ClassState>) -> Self {
        Self {
            hp,
            max_hp: hp,
            class_state,
            last_resource_update: 0,
            current_time: 0,
            skill_cooldowns: HashMap::new(),
            skill_charges: HashMap::new(),
            charge_recovery_times: HashMap::new(),
            skill_use_counts: HashMap::new(),
            combo_phases: HashMap::new(),
            combo_timeouts: HashMap::new(),
            chain_phases: HashMap::new(),
            chain_timeouts: HashMap::new(),
            buff_manager: BuffManager::new(),
            target_buff_manager: BuffManager::new(),
            trigger_cooldowns: HashMap::new(),
            full_resource_triggers: HashSet::new(),
        }
    }

    /// 자원 현재값 조회 — 엔진/APL 공용
    pub fn get_resource(&self, name: &str) -> f64 {
        match name {
            "Hp"    => self.hp,
            "MaxHp" => self.max_hp,
            _ => self.class_state.get_resource(name),
        }
    }

    /// 자원 현재값 설정
    pub fn set_resource(&mut self, name: &str, value: f64) {
        match name {
            "Hp" => self.hp = value.clamp(0.0, self.max_hp),
            _ => self.class_state.set_resource(name, value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::state::ArcanaState;
    use crate::profile::{BuffSpec, StackType};
    use hashbrown::HashMap;

    #[test]
    fn target_buffs_are_tracked_separately_from_actor_buffs() {
        let mut state = ActorState::new(1000.0, Box::new(ArcanaState::new(100.0, 100.0, Vec::new())));
        let spec = BuffSpec {
            id: "arcana_ruin_stack".to_string(),
            effects: HashMap::new(),
            max_stacks: 4,
            stack_type: StackType::Refresh,
            duration_ms: 10_000,
        };

        state.target_buff_manager.apply(spec, 10_000, 1);

        assert_eq!(state.target_buff_manager.stacks("arcana_ruin_stack", 1), 1);
        assert_eq!(state.buff_manager.stacks("arcana_ruin_stack", 1), 0);
    }
}
