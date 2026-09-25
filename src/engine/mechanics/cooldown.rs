use crate::engine::state::ActorState;
use crate::profile::skill::{SkillProfile, SkillType};

/// 스킬 사용 가능 여부 확인
pub fn is_available(skill: &SkillProfile, state: &ActorState, current_time: u64) -> bool {
    available_time(skill, state, current_time) <= current_time
}

pub fn current_phase(skill: &SkillProfile, state: &ActorState, current_time: u64) -> u32 {
    let skill_id = skill.skill_id;
    let (phases, timeouts) = match &skill.skill_type {
        SkillType::Combo { .. } => (&state.combo_phases, &state.combo_timeouts),
        SkillType::Chain { .. } => (&state.chain_phases, &state.chain_timeouts),
        _ => return 0,
    };
    let phase = phases.get(&skill_id).copied().unwrap_or(0);
    (phase > 0 && current_time <= timeouts.get(&skill_id).copied().unwrap_or(0))
        .then_some(phase)
        .unwrap_or(0)
}

pub fn available_time(skill: &SkillProfile, state: &ActorState, current_time: u64) -> u64 {
    let skill_id = skill.skill_id;

    if skill.max_uses > 0
        && state.skill_use_counts.get(&skill_id).copied().unwrap_or(0) >= skill.max_uses
    {
        return u64::MAX;
    }

    // 콤보 스킬: 콤보 진행 중이면 타임아웃 이내에 사용 가능
    if let SkillType::Combo { .. } = &skill.skill_type {
        if current_phase(skill, state, current_time) > 0 {
            return current_time;
        }
    }

    // 체인 스킬: 체인 진행 중이면 타임아웃 이내에 사용 가능
    if let SkillType::Chain { .. } = &skill.skill_type {
        if current_phase(skill, state, current_time) > 0 {
            return current_time;
        }
    }

    // 충전형 스킬: 충전이 남아있으면 사용 가능
    if skill.max_stacks > 1 {
        let charges = state.skill_charges.get(&skill_id).copied().unwrap_or(skill.max_stacks);
        if charges > 0 {
            return current_time;
        }
        return state
            .charge_recovery_times
            .get(&skill_id)
            .copied()
            .unwrap_or(current_time + 1);
    }

    // 일반 쿨타임
    let cd_end = state.skill_cooldowns.get(&skill_id).copied().unwrap_or(0);
    cd_end.max(current_time)
}

/// 스킬 사용 후 쿨타임 / 충전 / 체인 상태 갱신.
///
/// 반환값: 충전형 스킬의 경우 `charge_recovery_ms`, 그 외 0.
pub fn consume(
    skill: &SkillProfile,
    state: &mut ActorState,
    current_time: u64,
    actual_cooldown_ms: u32,
) -> u32 {
    let skill_id = skill.skill_id;

    // 콤보 스킬
    if let SkillType::Combo { last_phase, phase_timeout_ms } = &skill.skill_type {
        let current_phase = current_phase(skill, state, current_time);
        if current_phase < *last_phase {
            // 다음 콤보 페이즈로 전환 — 이 시전에는 쿨타임 없음
            state.combo_phases.insert(skill_id, current_phase + 1);
            state.combo_timeouts.insert(skill_id, current_time + *phase_timeout_ms as u64);
            return 0;
        } else {
            // 마지막 페이즈 → 콤보 리셋 후 쿨타임 적용
            state.combo_phases.remove(&skill_id);
            state.combo_timeouts.remove(&skill_id);
        }
    }

    // 체인 스킬
    if let SkillType::Chain { last_phase, phase_timeout_ms } = &skill.skill_type {
        let current_phase = current_phase(skill, state, current_time);
        if current_phase < *last_phase {
            // 다음 체인 페이즈로 전환 — 이 시전에는 쿨타임 없음
            state.chain_phases.insert(skill_id, current_phase + 1);
            state.chain_timeouts.insert(skill_id, current_time + *phase_timeout_ms as u64);
            return 0;
        } else {
            // 마지막 페이즈 → 체인 리셋 후 쿨타임 적용
            state.chain_phases.remove(&skill_id);
            state.chain_timeouts.remove(&skill_id);
        }
    }

    // 충전형 스킬
    if skill.max_stacks > 1 {
        let charges = state.skill_charges.entry(skill_id).or_insert(skill.max_stacks);
        if *charges > 0 {
            *charges -= 1;
        }
        return skill.charge_recovery_ms;
    }

    // 일반 쿨타임
    state.skill_cooldowns.insert(skill_id, current_time + actual_cooldown_ms as u64);
    0
}

/// 현재 사용 가능한 스킬이 없을 때, 다음 ActionCheck를 스케줄할 시각을 반환한다.
pub fn next_available_time(
    skills: &[SkillProfile],
    state: &ActorState,
    current_time: u64,
) -> u64 {
    let mut earliest = u64::MAX;

    for skill in skills {
        let skill_id = skill.skill_id;

        if available_time(skill, state, current_time) == u64::MAX {
            continue;
        }

        // 콤보/체인 진행 중이면 즉시 사용 가능 (이미 available 체크에서 걸렸어야 함)
        if let SkillType::Combo { .. } = &skill.skill_type {
            if current_phase(skill, state, current_time) > 0 {
                return current_time;
            }
        }

        if let SkillType::Chain { .. } = &skill.skill_type {
            if current_phase(skill, state, current_time) > 0 {
                return current_time;
            }
        }

        let cd_end = if skill.max_stacks > 1 {
            // 충전형: 다음 회복 시각
            state.charge_recovery_times.get(&skill_id).copied().unwrap_or(0)
        } else {
            state.skill_cooldowns.get(&skill_id).copied().unwrap_or(0)
        };

        if cd_end < earliest {
            earliest = cd_end;
        }
    }

    if earliest == u64::MAX { current_time + 1 } else { earliest.max(current_time + 1) }
}
