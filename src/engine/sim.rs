use hashbrown::HashMap;

use crate::constants::{
    BuffDebuffTraceDetail, CardUseEvent, CastEvent, DamageTraceEvent, HeldCardTrace,
    ResourceGainTrace, SimResult, SkillStat, TraceData,
};
use crate::engine::{
    events::{priority, Event, EventData, EventQueue},
    mechanics::{cooldown, damage},
    rng::Rng,
    snapshot::{self, CastSnapshot},
    state::{ActorState, ArcanaState, BerserkerState, ClassState},
};
use crate::profile::skill::{
    HitCondition, HitTriggerOn, SkillTag, TriggerCondition, TriggerEffect, TriggerFilter,
};
use crate::profile::SkillSlot;
use crate::profile::{AplAction, AplCondition, CharacterProfiles, SkillProfile, StatField};

// ---------------------------------------------------------------------------
// 진입점
// ---------------------------------------------------------------------------

/// N회 Iteration을 실행해 집계된 SimResult 반환.
/// seed는 seed_offset..seed_offset+n 순번을 그대로 사용해 재현성을 보장한다.
/// 워커별로 seed_offset을 달리하면 시드 중복 없이 병렬 실행 가능.
///
/// progress_cb: 진행도 보고용 JS 콜백. None이면 보고 안 함.
/// 매 PROGRESS_INTERVAL iter마다 (current, total) 인자로 호출.
pub fn simulate(
    profiles: &CharacterProfiles,
    n: u32,
    duration_ms: u32,
    seed_offset: u32,
    progress_cb: Option<&js_sys::Function>,
) -> SimResult {
    use wasm_bindgen::JsValue;

    // 콜백 빈도 — 너무 잦으면 JS↔WASM 트립 비용이 누적된다
    const PROGRESS_INTERVAL: u64 = 10;

    let mut total_damage = 0.0f64;
    let mut total_dps = 0.0f64;
    let mut cast_count = 0u32;
    let mut crit_count = 0u32;
    let mut skill_stats: HashMap<u32, SkillStat> = HashMap::new();
    let mut distribution = Vec::with_capacity(n as usize);

    for i in 0..n as u64 {
        let seed = seed_offset as u64 + i;

        // 시뮬레이션 실행점
        let r = simulate_one(profiles, seed, duration_ms);

        // 시뮬레이션 결과 합산
        total_damage += r.total_damage;
        total_dps += r.dps;
        cast_count += r.cast_count;
        crit_count += r.crit_count;
        distribution.push((r.dps, seed));

        // 스킬별 통계
        for (id, stat) in r.skill_stats {
            let e = skill_stats.entry(id).or_default();
            e.count += stat.count;
            e.total_damage += stat.total_damage;
            e.hit_count += stat.hit_count;
            e.crit_count += stat.crit_count;
        }

        // 진행도 보고 — 마지막 iter는 항상 보고
        if let Some(cb) = progress_cb {
            if (i + 1) % PROGRESS_INTERVAL == 0 || (i + 1) as u32 == n {
                let _ = cb.call2(
                    &JsValue::NULL,
                    &JsValue::from((i + 1) as u32),
                    &JsValue::from(n),
                );
            }
        }
    }

    for stat in skill_stats.values_mut() {
        if stat.count > 0 {
            stat.dpc = stat.total_damage / stat.count as f64;
        }
    }

    SimResult {
        total_damage,
        dps: if n > 0 { total_dps / n as f64 } else { 0.0 },
        cast_count,
        crit_count,
        skill_stats,
        distribution,
    }
}

/// 단일 Iteration 실행 후 SimResult 반환. trace 미수집.
pub fn simulate_one(profiles: &CharacterProfiles, seed: u64, duration_ms: u32) -> SimResult {
    let mut trace = None;
    simulate_one_inner(profiles, seed, duration_ms, &mut trace)
}

/// 단일 Iteration을 trace 모드로 실행 — cast 시퀀스 함께 반환.
pub fn trace_one(
    profiles: &CharacterProfiles,
    seed: u64,
    duration_ms: u32,
) -> (SimResult, TraceData) {
    let mut trace = Some(TraceData::default());
    let result = simulate_one_inner(profiles, seed, duration_ms, &mut trace);
    (result, trace.unwrap_or_default())
}

fn simulate_one_inner(
    profiles: &CharacterProfiles,
    seed: u64,
    duration_ms: u32,
    trace: &mut Option<TraceData>,
) -> SimResult {
    let mut rng = Rng::new(seed);
    let mut state = create_actor_state(profiles);
    for _ in 0..profiles.initial_card_draws {
        state.class_state.draw_card(&mut rng);
    }
    let mut queue = EventQueue::new();
    let max_time_ms = duration_ms as u64;

    // skill_id → skills 배열 인덱스
    let skill_index: HashMap<u32, usize> = profiles
        .skills
        .iter()
        .enumerate()
        .map(|(i, s)| (s.skill_id, i))
        .collect();

    let mut result: SimResult = SimResult::default();

    // 초기 ActionCheck
    queue.schedule_action_check(0);

    while let Some(event) = queue.pop() {
        let Event { time_ms, data, .. } = event;
        if time_ms > max_time_ms {
            break;
        }
        state.current_time = time_ms;

        use_matching_cards(
            profiles,
            &skill_index,
            &mut state,
            &mut queue,
            &mut rng,
            time_ms,
            max_time_ms,
            &mut result,
            trace,
        );

        match data {
            EventData::ActionCheck => {
                let cast_skill_id = on_action_check(
                    profiles,
                    &skill_index,
                    &mut state,
                    &mut queue,
                    &mut rng,
                    time_ms,
                    max_time_ms,
                    &mut result,
                    trace.is_some(),
                );
                // trace 모드일 때만 cast event 기록
                if let (Some(t), Some(id)) = (trace.as_mut(), cast_skill_id) {
                    if let Some(&idx) = skill_index.get(&id) {
                        let skill = &profiles.skills[idx];
                        t.casts.push(CastEvent {
                            time: time_ms as f64 / 1000.0,
                            skill_id: id,
                            skill_name: skill.name.clone(),
                            is_awakening: matches!(
                                skill.slot,
                                SkillSlot::Awakening
                                    | SkillSlot::HyperAwakeningTechniques
                                    | SkillSlot::SuperAwakening
                            ),
                            active_buffs: state.buff_manager.active_ids(time_ms),
                            target_debuffs: state.target_buff_manager.active_ids(time_ms),
                            active_buff_details: buff_trace_details(
                                state.buff_manager.active_buffs(time_ms),
                                "actor",
                                &profiles.buff_metadata,
                            ),
                            target_debuff_details: buff_trace_details(
                                state.target_buff_manager.active_buffs(time_ms),
                                "target",
                                &profiles.buff_metadata,
                            ),
                            active_buff_details_after_hit: Vec::new(),
                            target_debuff_details_after_hit: Vec::new(),
                            resource_gains: traced_skill_resource_gains(skill),
                            held_cards: held_card_details(&state, profiles),
                        });
                    }
                }
            }
            EventData::SkillHit {
                skill_id,
                hit_index,
                snap,
            } => {
                if let Some(&idx) = skill_index.get(&skill_id) {
                    on_skill_hit(
                        profiles,
                        &profiles.skills[idx],
                        hit_index,
                        &snap,
                        &mut state,
                        &mut queue,
                        &mut rng,
                        time_ms,
                        &mut result,
                        trace,
                    );
                }
            }
            EventData::BuffExpire { buff_id, expire_at } => {
                state.buff_manager.expire_if(&buff_id, expire_at);
            }
            EventData::TargetBuffExpire { buff_id, expire_at } => {
                state.target_buff_manager.expire_if(&buff_id, expire_at);
            }
            EventData::DotTick {
                dot_id,
                skill_id,
                hit,
                remaining_ticks,
                tick_interval_ms,
            } => {
                if let Some(&idx) = skill_index.get(&skill_id) {
                    let skill = &profiles.skills[idx];
                    let snap = snapshot::capture(profiles, skill, 0, &state, trace.is_some());
                    let hit_result = if trace.is_some() {
                        let (hit_result, formula) =
                            damage::calculate_runtime_damage_with_trace(&hit, &snap, &mut rng);
                        push_damage_trace(
                            trace,
                            DamageTraceEvent {
                                time: time_ms as f64 / 1000.0,
                                skill_id,
                                skill_name: skill.name.clone(),
                                hit_index: 0,
                                hit_id: hit.hit_id.clone(),
                                hit_name: hit.name.clone(),
                                damage_source: format!("dot:{dot_id}"),
                                result_damage: hit_result.damage,
                                is_crit: hit_result.is_crit,
                                resource_gains: Vec::new(),
                                held_cards_after_hit: Vec::new(),
                                formula,
                            },
                        );
                        hit_result
                    } else {
                        damage::calculate_runtime_damage(&hit, &snap, &mut rng)
                    };
                    record_hit_result(&mut result, skill_id, hit_result);
                }

                if remaining_ticks > 1 {
                    queue.push(Event {
                        time_ms: time_ms + tick_interval_ms as u64,
                        priority: priority::DOT_TICK,
                        data: EventData::DotTick {
                            dot_id,
                            skill_id,
                            hit,
                            remaining_ticks: remaining_ticks - 1,
                            tick_interval_ms,
                        },
                    });
                }
            }
            EventData::ChargeRecovery { skill_id } => {
                if state.charge_recovery_times.get(&skill_id).copied() != Some(time_ms) {
                    continue;
                }
                if let Some(&idx) = skill_index.get(&skill_id) {
                    let skill = &profiles.skills[idx];
                    let charges = state.skill_charges.entry(skill_id).or_insert(0);
                    if *charges < skill.max_stacks {
                        *charges += 1;
                        // 아직 최대가 아니면 다음 충전 회복 예약
                        if *charges < skill.max_stacks && skill.charge_recovery_ms > 0 {
                            let next = time_ms + skill.charge_recovery_ms as u64;
                            state.charge_recovery_times.insert(skill_id, next);
                            queue.push(Event {
                                time_ms: next,
                                priority: priority::CHARGE_RECOVERY,
                                data: EventData::ChargeRecovery { skill_id },
                            });
                        } else {
                            state.charge_recovery_times.remove(&skill_id);
                        }
                    }
                }
            }
            EventData::ScheduledTrigger { skill_id, effect } => {
                apply_trigger_effect(
                    &effect,
                    skill_id,
                    &mut state,
                    &mut queue,
                    &mut rng,
                    time_ms,
                );
            }
        }

        use_matching_cards(
            profiles,
            &skill_index,
            &mut state,
            &mut queue,
            &mut rng,
            time_ms,
            max_time_ms,
            &mut result,
            trace,
        );
    }

    // DPS 계산
    let duration_sec = max_time_ms as f64 / 1000.0;
    result.dps = if duration_sec > 0.0 {
        result.total_damage / duration_sec
    } else {
        0.0
    };

    for stat in result.skill_stats.values_mut() {
        if stat.count > 0 {
            stat.dpc = stat.total_damage / stat.count as f64;
        }
    }

    result
}

fn use_matching_cards(
    profiles: &CharacterProfiles,
    skill_index: &HashMap<u32, usize>,
    state: &mut ActorState,
    queue: &mut EventQueue,
    rng: &mut Rng,
    time_ms: u64,
    max_time_ms: u64,
    result: &mut SimResult,
    trace: &mut Option<TraceData>,
) {
    if profiles.card_apl_actions.is_empty() || !state.class_state.has_cards() {
        return;
    }

    loop {
        let Some(action) = profiles.card_apl_actions.iter().find(|action| {
            !profiles
                .arcana_card_blocking_buffs
                .get(&action.card_id)
                .is_some_and(|buff_id| state.buff_manager.is_active(buff_id, time_ms))
            && action.conditions.iter().all(|condition| {
                match condition {
                    AplCondition::NextSkill(skill_id) => {
                        choose_apl_action(profiles, skill_index, state, time_ms, max_time_ms)
                            .is_some_and(|(next_skill_id, _)| next_skill_id == *skill_id)
                    }
                    _ => apl_condition_matches(
                        condition,
                        0,
                        skill_index,
                        &profiles.skills,
                        state,
                        time_ms,
                        max_time_ms,
                    ),
                }
            }) && state.class_state.use_card(action.card_id)
        }).cloned() else {
            break;
        };

        if let Some(trace) = trace.as_mut() {
            trace.card_uses.push(CardUseEvent {
                time: time_ms as f64 / 1000.0,
                card_id: action.card_id,
                card_name: action.card_name.clone(),
            });
        }
        if let Some(card_effect) = profiles.arcana_card_effects.get(&action.card_id) {
            apply_card_effect(
                profiles,
                card_effect,
                action.card_id,
                &action.card_name,
                state,
                queue,
                rng,
                time_ms,
                result,
                trace,
            );
        }
        fire_global_triggers(
            profiles,
            TriggerContext::CardUse { card_id: action.card_id },
            state,
            queue,
            rng,
            time_ms,
        );
        if !profiles
            .arcana_card_effects
            .get(&action.card_id)
            .is_some_and(copies_last_used_card)
        {
            state.class_state.remember_card_use(action.card_id);
        }
    }
}

fn apply_card_effect(
    profiles: &CharacterProfiles,
    effect: &TriggerEffect,
    card_id: u32,
    card_name: &str,
    state: &mut ActorState,
    queue: &mut EventQueue,
    rng: &mut Rng,
    time_ms: u64,
    result: &mut SimResult,
    trace: &mut Option<TraceData>,
) {
    match effect {
        TriggerEffect::Sequence(effects) => {
            for effect in effects {
                apply_card_effect(
                    profiles, effect, card_id, card_name, state, queue, rng, time_ms, result,
                    trace,
                );
            }
        }
        TriggerEffect::RandomChoice(effects) => {
            if !effects.is_empty() {
                let index = ((rng.next_f64() * effects.len() as f64) as usize)
                    .min(effects.len() - 1);
                apply_card_effect(
                    profiles, &effects[index], card_id, card_name, state, queue, rng, time_ms,
                    result, trace,
                );
            }
        }
        TriggerEffect::WeightedRandomChoice(effects) => {
            let total: f64 = effects.iter().map(|(weight, _)| weight.max(0.0)).sum();
            if total > 0.0 {
                let mut roll = rng.next_f64() * total;
                for (weight, effect) in effects {
                    roll -= weight.max(0.0);
                    if roll <= 0.0 {
                        apply_card_effect(
                            profiles, effect, card_id, card_name, state, queue, rng, time_ms,
                            result, trace,
                        );
                        break;
                    }
                }
            }
        }
        TriggerEffect::DealRuntimeDamage(spec) => {
            let Some(template) = profiles.skills.first() else {
                return;
            };
            let mut card = template.clone();
            card.skill_id = card_id;
            card.name = card_name.to_string();
            card.cast_buff_damage_bonuses.clear();
            card.cast_buff_crit_rate_bonuses.clear();
            let snap = snapshot::capture(profiles, &card, 0, state, trace.is_some());
            let hit_result = if trace.is_some() {
                let (hit_result, formula) =
                    damage::calculate_runtime_damage_with_trace(spec, &snap, rng);
                push_damage_trace(
                    trace,
                    DamageTraceEvent {
                        time: time_ms as f64 / 1000.0,
                        skill_id: card_id,
                        skill_name: card_name.to_string(),
                        hit_index: 0,
                        hit_id: spec.hit_id.clone(),
                        hit_name: spec.name.clone(),
                        damage_source: format!("card:{card_id}"),
                        result_damage: hit_result.damage,
                        is_crit: hit_result.is_crit,
                        resource_gains: Vec::new(),
                        held_cards_after_hit: Vec::new(),
                        formula,
                    },
                );
                hit_result
            } else {
                damage::calculate_runtime_damage(spec, &snap, rng)
            };
            record_hit_result(result, card_id, hit_result);
        }
        _ => apply_trigger_effect(effect, card_id, state, queue, rng, time_ms),
    }
}

fn copies_last_used_card(effect: &TriggerEffect) -> bool {
    match effect {
        TriggerEffect::DrawLastUsedCard => true,
        TriggerEffect::Sequence(effects) => effects.iter().any(copies_last_used_card),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// 이벤트 핸들러
// ---------------------------------------------------------------------------

/// 반환값: 캐스트된 skill_id (있으면). 없으면 None.
/// trace 모드에서 caller가 cast event 기록에 사용.
fn on_action_check(
    profiles: &CharacterProfiles,
    skill_index: &HashMap<u32, usize>,
    state: &mut ActorState,
    queue: &mut EventQueue,
    rng: &mut Rng,
    time_ms: u64,
    max_time_ms: u64,
    result: &mut SimResult,
    trace_enabled: bool,
) -> Option<u32> {
    let chosen = choose_apl_action(profiles, skill_index, state, time_ms, max_time_ms);

    if let Some((skill_id, idx)) = chosen {
        let skill = &profiles.skills[idx];
        let current_phase = cooldown::current_phase(skill, state, time_ms);
        fire_global_triggers(
            profiles,
            TriggerContext::BeforeSkillCast { skill_id },
            state,
            queue,
            rng,
            time_ms,
        );
        let mut snap = snapshot::capture(profiles, skill, current_phase, state, trace_enabled);
        let cast_time = snap.actual_cast_time_ms as u64;

        // 자원 소모
        snap.paid_mp = pay_resource(skill, state, rng);

        // 스킬 단위 자원 획득
        for (res, &amount) in &skill.resource_gains {
            gain_resource(state, rng, res, amount);
        }

        // 쿨타임/충전/체인 상태 갱신
        let charge_recovery_ms = cooldown::consume(skill, state, time_ms, snap.actual_cooldown_ms);

        // 충전 회복 예약 (충전형 스킬)
        if skill.max_stacks > 1 && charge_recovery_ms > 0 {
            let charges = state.skill_charges.get(&skill_id).copied().unwrap_or(0);
            if charges < skill.max_stacks {
                let next = time_ms + charge_recovery_ms as u64;
                state.charge_recovery_times.insert(skill_id, next);
                queue.push(Event {
                    time_ms: next,
                    priority: priority::CHARGE_RECOVERY,
                    data: EventData::ChargeRecovery { skill_id },
                });
            }
        }

        // 캐스트 카운트
        *state.skill_use_counts.entry(skill_id).or_default() += 1;
        result.cast_count += 1;
        result.skill_stats.entry(skill_id).or_default().count += 1;

        // 현재 페이즈에 해당하는 히트 이벤트 스케줄
        for (hit_index, hit) in skill.hits.iter().enumerate() {
            if hit.phase != current_phase {
                continue;
            }
            let base_cast_ms = skill
                .cast_times_ms
                .get(current_phase as usize)
                .or_else(|| skill.cast_times_ms.first())
                .copied()
                .unwrap_or(0);
            let action_delay_ms =
                scale_action_delay(hit.action_delay_ms, base_cast_ms, snap.actual_cast_time_ms);
            let hit_time = time_ms + action_delay_ms + hit.fixed_delay_ms as u64;
            if hit_time > max_time_ms {
                continue;
            }
            queue.push(Event {
                time_ms: hit_time,
                priority: priority::SKILL_HIT,
                data: EventData::SkillHit {
                    skill_id,
                    hit_index,
                    snap: snap.clone(),
                },
            });
        }

        // 캐스트 완료 후 다음 ActionCheck
        let next_action = time_ms + cast_time.max(1);
        if next_action <= max_time_ms {
            queue.schedule_action_check(next_action);
        }

        // GlobalTrigger — SkillCast
        fire_global_triggers(
            profiles,
            TriggerContext::SkillCast { skill_id },
            state,
            queue,
            rng,
            time_ms,
        );

        Some(skill_id)
    } else {
        // 사용 가능한 APL action 없음 → 다음 상태 변화까지 대기.
        // 조건 APL에서는 스킬 쿨이 이미 돌아왔지만 조건이 불만족인 상태가 흔하다.
        // 이때 단순 next_available_time()은 current+1ms polling으로 이어져 장시간 시뮬레이션을 크게 늦춘다.
        let wait =
            next_action_check_time(profiles, skill_index, state, queue, time_ms, max_time_ms);
        if wait <= max_time_ms {
            queue.schedule_action_check(wait);
        }
        None
    }
}

fn scale_action_delay(delay_ms: u32, base_cast_ms: u32, actual_cast_ms: u32) -> u64 {
    if base_cast_ms == 0 {
        delay_ms as u64
    } else {
        delay_ms as u64 * actual_cast_ms as u64 / base_cast_ms as u64
    }
}

fn next_action_check_time(
    profiles: &CharacterProfiles,
    skill_index: &HashMap<u32, usize>,
    state: &ActorState,
    queue: &EventQueue,
    time_ms: u64,
    max_time_ms: u64,
) -> u64 {
    const IDLE_POLL_INTERVAL_MS: u64 = 50;

    let next_action_ready =
        next_condition_satisfied_action_time(profiles, skill_index, state, time_ms, max_time_ms);
    let next_cooldown = cooldown::next_available_time(&profiles.skills, state, time_ms);
    let next_event = queue.peek_time().unwrap_or(u64::MAX);
    let next_time_condition = next_time_remaining_condition_time(profiles, time_ms, max_time_ms);

    let next_state_change = next_event.min(next_time_condition);
    let next = if next_action_ready != u64::MAX {
        next_action_ready.min(next_state_change)
    } else if next_cooldown <= time_ms + 1 {
        next_state_change.min(time_ms + IDLE_POLL_INTERVAL_MS)
    } else {
        next_cooldown.min(next_state_change)
    };

    if next > time_ms + 1 {
        next
    } else {
        time_ms + IDLE_POLL_INTERVAL_MS
    }
}

fn next_condition_satisfied_action_time(
    profiles: &CharacterProfiles,
    skill_index: &HashMap<u32, usize>,
    state: &ActorState,
    time_ms: u64,
    max_time_ms: u64,
) -> u64 {
    profiles
        .apl_actions
        .iter()
        .filter_map(|action| {
            let skill = skill_by_id(action.skill_id, skill_index, &profiles.skills)?;
            if !can_pay_resource(skill, state) {
                return None;
            }

            let mut ready_at = cooldown::available_time(skill, state, time_ms);
            for condition in &action.conditions {
                match condition {
                    AplCondition::CooldownReady => {
                        ready_at = ready_at.max(cooldown::available_time(skill, state, time_ms));
                    }
                    AplCondition::SkillCooldownReady(skill_id) => {
                        let checked = skill_by_id(*skill_id, skill_index, &profiles.skills)?;
                        ready_at = ready_at.max(cooldown::available_time(checked, state, time_ms));
                    }
                    _ => {
                        if !apl_condition_matches(
                            condition,
                            action.skill_id,
                            skill_index,
                            &profiles.skills,
                            state,
                            time_ms,
                            max_time_ms,
                        ) {
                            return None;
                        }
                    }
                }
            }

            (ready_at > time_ms + 1).then_some(ready_at)
        })
        .min()
        .unwrap_or(u64::MAX)
}

fn next_time_remaining_condition_time(
    profiles: &CharacterProfiles,
    time_ms: u64,
    max_time_ms: u64,
) -> u64 {
    profiles
        .apl_actions
        .iter()
        .flat_map(|action| action.conditions.iter())
        .filter_map(|condition| {
            let AplCondition::TimeRemaining { operator, value_ms } = condition else {
                return None;
            };
            match operator {
                // remaining <= value becomes true when current time reaches duration - value.
                crate::profile::CompareOperator::Lt | crate::profile::CompareOperator::Lte => {
                    let threshold = max_time_ms.saturating_sub(*value_ms);
                    (threshold > time_ms).then_some(threshold)
                }
                _ => None,
            }
        })
        .min()
        .unwrap_or(u64::MAX)
}

fn traced_skill_resource_gains(skill: &SkillProfile) -> Vec<ResourceGainTrace> {
    let mut gains = skill.resource_gains.iter()
        .filter(|(resource, amount)| resource.as_str() != "Mp" && **amount != 0.0)
        .map(|(resource, amount)| ResourceGainTrace {
            resource: resource.clone(),
            amount: *amount,
        })
        .collect::<Vec<_>>();
    gains.sort_by(|a, b| a.resource.cmp(&b.resource));
    gains
}

fn traced_hit_resource_gains(hit: &crate::profile::skill::SkillHit) -> Vec<ResourceGainTrace> {
    hit.triggers.iter()
        .filter(|trigger| trigger.chance >= 1.0 && matches!(&trigger.on, HitTriggerOn::OnHit))
        .filter_map(|trigger| match &trigger.effect {
            TriggerEffect::GainResource { resource, amount }
                if resource != "Mp" && *amount != 0.0 => Some(ResourceGainTrace {
                    resource: resource.clone(),
                    amount: *amount,
                }),
            _ => None,
        })
        .collect()
}

fn on_skill_hit(
    profiles: &CharacterProfiles,
    skill: &SkillProfile,
    hit_index: usize,
    snap: &CastSnapshot,
    state: &mut ActorState,
    queue: &mut EventQueue,
    rng: &mut Rng,
    time_ms: u64,
    result: &mut SimResult,
    trace: &mut Option<TraceData>,
) {
    let Some(hit) = skill.hits.get(hit_index) else {
        return;
    };
    let mut live_snap;
    let snap = if hit.use_snapshot {
        snap
    } else {
        live_snap = snapshot::capture(profiles, skill, hit.phase, state, trace.is_some());
        live_snap.paid_mp = snap.paid_mp;
        live_snap.damage_increase += snap.cast_buff_damage_bonus - live_snap.cast_buff_damage_bonus;
        live_snap.cast_buff_damage_bonus = snap.cast_buff_damage_bonus;
        &live_snap
    };

    let runtime_damage_bonus = runtime_damage_bonus_for_hit(hit, state, time_ms);
    let hit_result = if trace.is_some() {
        let (hit_result, formula) =
            damage::calculate_hit_with_trace(hit, snap, rng, runtime_damage_bonus);
        push_damage_trace(
            trace,
            DamageTraceEvent {
                time: time_ms as f64 / 1000.0,
                skill_id: skill.skill_id,
                skill_name: skill.name.clone(),
                hit_index,
                hit_id: hit.hit_id.clone(),
                hit_name: hit.name.clone(),
                damage_source: "skill_hit".to_string(),
                result_damage: hit_result.damage,
                is_crit: hit_result.is_crit,
                resource_gains: traced_hit_resource_gains(hit),
                held_cards_after_hit: Vec::new(),
                formula,
            },
        );
        hit_result
    } else {
        damage::calculate_hit_with_runtime_damage_bonus(hit, snap, rng, runtime_damage_bonus)
    };

    result.total_damage += hit_result.damage;
    if hit_result.is_crit {
        result.crit_count += 1;
    }
    let entry = result.skill_stats.entry(skill.skill_id).or_default();
    entry.total_damage += hit_result.damage;
    entry.hit_count += 1;
    if hit_result.is_crit {
        entry.crit_count += 1;
    }

    // HitTrigger 처리
    for trigger in &hit.triggers {
        let fire = match &trigger.on {
            HitTriggerOn::OnHit => true,
            HitTriggerOn::OnCrit => hit_result.is_crit,
            HitTriggerOn::OnHitIf(condition) => {
                hit_condition_matches(condition, state, time_ms, Some(hit_result.is_crit))
            }
        };
        if !fire {
            continue;
        }
        if trigger.chance < 1.0 && rng.next_f64() >= trigger.chance {
            continue;
        }
        if apply_trigger_damage_effect(
            &trigger.effect,
            skill,
            hit_index,
            skill.skill_id,
            snap,
            state,
            queue,
            rng,
            time_ms,
            result,
            trace,
        ) {
            continue;
        }
        apply_trigger_effect(&trigger.effect, skill.skill_id, state, queue, rng, time_ms);
    }

    // GlobalTrigger — AnyHit / AnyCrit / SkillTag
    fire_global_triggers(
        profiles,
        TriggerContext::HitEvent {
            skill_id: skill.skill_id,
            tags: &skill.tags,
            is_crit: hit_result.is_crit,
        },
        state,
        queue,
        rng,
        time_ms,
    );

    update_cast_trace_state(trace, profiles, skill.skill_id, state, time_ms);
    update_hit_trace_cards(trace, profiles, skill.skill_id, hit_index, state);
}

fn apply_trigger_damage_effect(
    effect: &TriggerEffect,
    skill: &SkillProfile,
    hit_index: usize,
    skill_id: u32,
    snap: &CastSnapshot,
    state: &mut ActorState,
    queue: &mut EventQueue,
    rng: &mut Rng,
    time_ms: u64,
    result: &mut SimResult,
    trace: &mut Option<TraceData>,
) -> bool {
    match effect {
        TriggerEffect::RefundCastMp { percent } => {
            let cur = state.get_resource("Mp");
            let max_mp = state.class_state.get_resource_max("Mp");
            state.set_resource("Mp", (cur + snap.paid_mp * percent).min(max_mp));
            true
        }
        TriggerEffect::DealDamageByTargetBuffStacks {
            buff_id,
            hits_by_stack,
            forced_stacks_while_buff,
        } => {
            let stacks = forced_stacks_while_buff
                .as_ref()
                .filter(|(id, _)| state.buff_manager.is_active(id, time_ms))
                .map(|(_, stacks)| *stacks)
                .unwrap_or_else(|| state.target_buff_manager.stacks(buff_id, time_ms));
            if stacks == 0 {
                return true;
            }
            let runtime_hit_index = stacks.saturating_sub(1) as usize;
            let Some(runtime_hit) = hits_by_stack.get(runtime_hit_index) else {
                return true;
            };
            let mut runtime_hit = runtime_hit.clone();
            if runtime_hit.random_crit_damage_chance > 0.0
                && rng.next_f64() < runtime_hit.random_crit_damage_chance
            {
                runtime_hit.crit_damage_bonus += runtime_hit.random_crit_damage_bonus;
            }
            let hit_result = if trace.is_some() {
                let (hit_result, formula) =
                    damage::calculate_runtime_damage_with_trace(&runtime_hit, snap, rng);
                push_damage_trace(
                    trace,
                    DamageTraceEvent {
                        time: time_ms as f64 / 1000.0,
                        skill_id,
                        skill_name: skill.name.clone(),
                        hit_index,
                        hit_id: runtime_hit.hit_id.clone(),
                        hit_name: runtime_hit.name.clone(),
                        damage_source: format!("target_buff_stacks:{buff_id}:{stacks}"),
                        result_damage: hit_result.damage,
                        is_crit: hit_result.is_crit,
                        resource_gains: Vec::new(),
                        held_cards_after_hit: Vec::new(),
                        formula,
                    },
                );
                hit_result
            } else {
                damage::calculate_runtime_damage(&runtime_hit, snap, rng)
            };
            record_hit_result(result, skill_id, hit_result);
            true
        }
        TriggerEffect::DealDamageAndSetTargetBuffStacks {
            effect_id,
            hit,
            spec,
            stacks,
        } => {
            let hit_result = if trace.is_some() {
                let (hit_result, formula) =
                    damage::calculate_runtime_damage_with_trace(hit, snap, rng);
                push_damage_trace(
                    trace,
                    DamageTraceEvent {
                        time: time_ms as f64 / 1000.0,
                        skill_id,
                        skill_name: skill.name.clone(),
                        hit_index,
                        hit_id: hit.hit_id.clone(),
                        hit_name: hit.name.clone(),
                        damage_source: format!("additional_effect:{effect_id}"),
                        result_damage: hit_result.damage,
                        is_crit: hit_result.is_crit,
                        resource_gains: Vec::new(),
                        held_cards_after_hit: Vec::new(),
                        formula,
                    },
                );
                hit_result
            } else {
                damage::calculate_runtime_damage(hit, snap, rng)
            };
            record_hit_result(result, skill_id, hit_result);
            apply_trigger_effect(
                &TriggerEffect::ApplyTargetBuffStacks {
                    spec: spec.clone(),
                    stacks: *stacks,
                },
                skill_id,
                state,
                queue,
                rng,
                time_ms,
            );
            true
        }
        TriggerEffect::ScheduleDot {
            dot_id,
            hit,
            first_tick_ms,
            tick_interval_ms,
            tick_count,
            refresh,
        } => {
            schedule_dot(queue, dot_id, skill_id, hit, *first_tick_ms, *tick_interval_ms, *tick_count, *refresh, time_ms);
            true
        }
        _ => false,
    }
}

fn push_damage_trace(trace: &mut Option<TraceData>, event: DamageTraceEvent) {
    if let Some(trace) = trace {
        trace.damage_events.push(event);
    }
}

fn update_cast_trace_state(
    trace: &mut Option<TraceData>,
    profiles: &CharacterProfiles,
    skill_id: u32,
    state: &ActorState,
    time_ms: u64,
) {
    let Some(trace) = trace else {
        return;
    };
    let Some(cast) = trace
        .casts
        .iter_mut()
        .rev()
        .find(|cast| cast.skill_id == skill_id)
    else {
        return;
    };

    cast.active_buff_details_after_hit =
        buff_trace_details(state.buff_manager.active_buffs(time_ms), "actor", &profiles.buff_metadata);
    cast.target_debuff_details_after_hit =
        buff_trace_details(state.target_buff_manager.active_buffs(time_ms), "target", &profiles.buff_metadata);
}

fn update_hit_trace_cards(
    trace: &mut Option<TraceData>,
    profiles: &CharacterProfiles,
    skill_id: u32,
    hit_index: usize,
    state: &ActorState,
) {
    let Some(event) = trace.as_mut().and_then(|trace| trace.damage_events.iter_mut().rev().find(
        |event| event.skill_id == skill_id
            && event.hit_index == hit_index
            && event.damage_source == "skill_hit"
    )) else {
        return;
    };
    event.held_cards_after_hit = held_card_details(state, profiles);
}

fn held_card_details(state: &ActorState, profiles: &CharacterProfiles) -> Vec<HeldCardTrace> {
    state.class_state.held_cards().into_iter().map(|card_id| HeldCardTrace {
        card_id,
        card_name: profiles.arcana_card_names.get(&card_id)
            .cloned()
            .unwrap_or_else(|| card_id.to_string()),
    }).collect()
}

fn buff_trace_details<'a>(
    buffs: impl Iterator<Item = (&'a crate::profile::BuffSpec, u32)>,
    source: &str,
    metadata: &HashMap<String, crate::profile::actor::BuffMetadata>,
) -> Vec<BuffDebuffTraceDetail> {
    buffs
        .map(|(spec, stacks)| {
            let meta = metadata.get(&spec.id).or_else(|| {
                spec.id
                    .strip_prefix("engraving:")
                    .and_then(|rest| rest.split_once(':'))
                    .and_then(|(name, _)| metadata.get(&format!("engraving:{name}:")))
            });
            BuffDebuffTraceDetail {
                id: spec.id.clone(),
                name: meta
                    .map(|value| value.name.clone())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| display_buff_name(spec, source)),
                icon: meta.map(|value| value.icon.clone()).unwrap_or_default(),
                stacks,
                source: meta
                    .map(|value| value.source.clone())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| source.to_string()),
            }
        })
        .collect()
}

fn display_buff_name(spec: &crate::profile::BuffSpec, source: &str) -> String {
    if spec.id == "arcana_ruin_stack" {
        return "스택트".to_string();
    }
    let name = if spec.effects.contains_key(&StatField::AttackSpeed) {
        "공격속도 증가"
    } else if spec.effects.contains_key(&StatField::MovementSpeed) {
        "이동속도 증가"
    } else if spec.effects.contains_key(&StatField::CritRate) {
        "치명타 적중률 증가"
    } else if spec.effects.contains_key(&StatField::CritDmg) {
        "치명타 피해 증가"
    } else if spec.effects.contains_key(&StatField::CooldownReduction) {
        "재사용 대기시간 감소"
    } else if spec.effects.contains_key(&StatField::TargetDefenseReduction) {
        "방어력 감소"
    } else if spec.effects.contains_key(&StatField::AttackPowerMul) {
        "공격력 증가"
    } else if spec.effects.contains_key(&StatField::DamageIncrease)
        || spec.effects.contains_key(&StatField::TargetDamageIncrease)
    {
        "피해 증가"
    } else if source == "target" {
        "대상 상태 효과"
    } else {
        "상태 효과"
    };
    name.to_string()
}

fn record_hit_result(result: &mut SimResult, skill_id: u32, hit_result: damage::HitResult) {
    result.total_damage += hit_result.damage;
    if hit_result.is_crit {
        result.crit_count += 1;
    }
    let entry = result.skill_stats.entry(skill_id).or_default();
    entry.total_damage += hit_result.damage;
    entry.hit_count += 1;
    if hit_result.is_crit {
        entry.crit_count += 1;
    }
}

fn runtime_damage_bonus_for_hit(
    hit: &crate::profile::SkillHit,
    state: &ActorState,
    time_ms: u64,
) -> f64 {
    hit.triggers
        .iter()
        .filter_map(|trigger| {
            match &trigger.effect {
                TriggerEffect::DamageBonus(value) => {
                    let fire = match &trigger.on {
                        HitTriggerOn::OnHit => true,
                        HitTriggerOn::OnCrit => false,
                        HitTriggerOn::OnHitIf(condition) => {
                            hit_condition_matches(condition, state, time_ms, None)
                        }
                    };
                    fire.then_some(*value)
                }
                TriggerEffect::DamageBonusPerBuffStack { buff_id, per_stack } => {
                    Some(state.buff_manager.stacks(buff_id, time_ms) as f64 * per_stack)
                }
                _ => None,
            }
        })
        .sum()
}

fn hit_condition_matches(
    condition: &HitCondition,
    state: &ActorState,
    time_ms: u64,
    is_crit: Option<bool>,
) -> bool {
    match condition {
        HitCondition::Always => true,
        HitCondition::IsCrit => is_crit.unwrap_or(false),
        HitCondition::TargetHpBelow(_) => false,
        HitCondition::BuffActive(id) => state.buff_manager.is_active(id, time_ms),
        HitCondition::TargetBuffActive(id) => state.target_buff_manager.is_active(id, time_ms),
        HitCondition::TargetBuffAtMaxStacks(id) => {
            state.target_buff_manager.is_at_max_stacks(id, time_ms)
        }
    }
}

// ---------------------------------------------------------------------------
// 헬퍼
// ---------------------------------------------------------------------------

fn can_pay_resource(skill: &SkillProfile, state: &ActorState) -> bool {
    skill
        .resource_costs
        .iter()
        .all(|(res, &cost)| {
            let cost = if res == "Mp" {
                cost + state.class_state.get_resource_max("Mp") * skill.extra_mp_cost_ratio
            } else { cost };
            (res == "Mp" && state.buff_manager.is_active("190941", state.current_time))
                || state.get_resource(res) >= cost
        })
}

fn pay_resource(skill: &SkillProfile, state: &mut ActorState, rng: &mut Rng) -> f64 {
    let waive_mp = state.buff_manager.is_active("190941", state.current_time)
        || (skill.mp_cost_waiver_chance > 0.0
            && rng.next_f64() < skill.mp_cost_waiver_chance);
    let mut paid_mp = 0.0;
    for (res, &cost) in &skill.resource_costs {
        if waive_mp && res == "Mp" {
            continue;
        }
        let cost = if res == "Mp" {
            cost + state.class_state.get_resource_max("Mp") * skill.extra_mp_cost_ratio
        } else { cost };
        let cur = state.get_resource(res);
        state.set_resource(res, (cur - cost).max(0.0));
        if res == "Mp" {
            paid_mp += cost.min(cur);
        }
    }
    paid_mp
}

fn choose_apl_action(
    profiles: &CharacterProfiles,
    skill_index: &HashMap<u32, usize>,
    state: &ActorState,
    time_ms: u64,
    max_time_ms: u64,
) -> Option<(u32, usize)> {
    if !profiles.apl_actions.is_empty() {
        return profiles.apl_actions.iter().find_map(|action| {
            let idx = *skill_index.get(&action.skill_id)?;
            let skill = &profiles.skills[idx];
            if is_castable(skill, state, time_ms)
                && apl_action_conditions_match(
                    action,
                    skill_index,
                    &profiles.skills,
                    state,
                    time_ms,
                    max_time_ms,
                )
            {
                Some((action.skill_id, idx))
            } else {
                None
            }
        });
    }

    profiles.apl_order.iter().find_map(|&skill_id| {
        let idx = *skill_index.get(&skill_id)?;
        let skill = &profiles.skills[idx];
        is_castable(skill, state, time_ms).then_some((skill_id, idx))
    })
}

fn is_castable(skill: &SkillProfile, state: &ActorState, time_ms: u64) -> bool {
    cooldown::is_available(skill, state, time_ms) && can_pay_resource(skill, state)
}

fn apl_action_conditions_match(
    action: &AplAction,
    skill_index: &HashMap<u32, usize>,
    skills: &[SkillProfile],
    state: &ActorState,
    time_ms: u64,
    max_time_ms: u64,
) -> bool {
    action.conditions.iter().all(|condition| {
        apl_condition_matches(
            condition,
            action.skill_id,
            skill_index,
            skills,
            state,
            time_ms,
            max_time_ms,
        )
    })
}

fn apl_condition_matches(
    condition: &AplCondition,
    current_skill_id: u32,
    skill_index: &HashMap<u32, usize>,
    skills: &[SkillProfile],
    state: &ActorState,
    time_ms: u64,
    max_time_ms: u64,
) -> bool {
    match condition {
        AplCondition::CooldownReady => skill_by_id(current_skill_id, skill_index, skills)
            .map(|skill| cooldown::is_available(skill, state, time_ms))
            .unwrap_or(false),
        AplCondition::SkillCooldownReady(skill_id) => skill_by_id(*skill_id, skill_index, skills)
            .map(|skill| cooldown::is_available(skill, state, time_ms))
            .unwrap_or(false),
        AplCondition::SkillCooldownRemaining {
            skill_id,
            operator,
            value_ms,
        } => {
            let remaining =
                skill_cooldown_remaining_ms(*skill_id, skill_index, skills, state, time_ms);
            operator.compare(remaining as f64, *value_ms as f64)
        }
        AplCondition::Resource {
            resource,
            operator,
            value,
        } => operator.compare(state.get_resource(resource), *value),
        AplCondition::CardHeld(card_id) => state.class_state.has_card(*card_id),
        AplCondition::CardCount {
            card_id,
            operator,
            value,
        } => operator.compare(state.class_state.card_count(*card_id) as f64, *value),
        AplCondition::NextSkill(_) => false,
        AplCondition::BuffActive(buff_id) => state.buff_manager.is_active(buff_id, time_ms),
        AplCondition::BuffInactive(buff_id) => !state.buff_manager.is_active(buff_id, time_ms),
        AplCondition::BuffRemaining {
            buff_id,
            operator,
            value_ms,
        } => {
            let remaining = state.buff_manager.remaining_ms(buff_id, time_ms);
            operator.compare(remaining as f64, *value_ms as f64)
        }
        AplCondition::TargetDebuffStacks {
            debuff_id,
            operator,
            value,
        } => {
            let stacks = state.target_buff_manager.stacks(debuff_id, time_ms) as f64;
            operator.compare(stacks, *value)
        }
        AplCondition::ComboPhase {
            skill_id,
            operator,
            value,
        } => {
            let phase = skill_by_id(*skill_id, skill_index, skills)
                .map(|skill| cooldown::current_phase(skill, state, time_ms))
                .unwrap_or(0) as f64;
            operator.compare(phase, *value)
        }
        AplCondition::ChainPhase {
            skill_id,
            operator,
            value,
        } => {
            let phase = skill_by_id(*skill_id, skill_index, skills)
                .map(|skill| cooldown::current_phase(skill, state, time_ms))
                .unwrap_or(0) as f64;
            operator.compare(phase, *value)
        }
        AplCondition::TimeRemaining { operator, value_ms } => {
            operator.compare(max_time_ms.saturating_sub(time_ms) as f64, *value_ms as f64)
        }
    }
}

fn skill_by_id<'a>(
    skill_id: u32,
    skill_index: &HashMap<u32, usize>,
    skills: &'a [SkillProfile],
) -> Option<&'a SkillProfile> {
    skill_index.get(&skill_id).and_then(|idx| skills.get(*idx))
}

fn skill_cooldown_remaining_ms(
    skill_id: u32,
    skill_index: &HashMap<u32, usize>,
    skills: &[SkillProfile],
    state: &ActorState,
    time_ms: u64,
) -> u64 {
    let Some(skill) = skill_by_id(skill_id, skill_index, skills) else {
        return 0;
    };

    if cooldown::is_available(skill, state, time_ms) {
        return 0;
    }

    let end = if skill.max_stacks > 1 {
        state
            .charge_recovery_times
            .get(&skill_id)
            .copied()
            .unwrap_or(time_ms)
    } else {
        state
            .skill_cooldowns
            .get(&skill_id)
            .copied()
            .unwrap_or(time_ms)
    };
    end.saturating_sub(time_ms)
}

fn create_actor_state(profiles: &CharacterProfiles) -> ActorState {
    let max_mp = if profiles.max_mp > 0.0 { profiles.max_mp } else { 100.0 };
    let class_state: Box<dyn ClassState> = match profiles.class_name.as_str() {
        "아르카나" => Box::new(ArcanaState::new(
            max_mp,
            crate::engine::state::class::arcana::CARD_GAUGE_MAX,
            profiles.arcana_card_pool.clone(),
        )),
        "버서커" => Box::new(BerserkerState::new(max_mp, 200.0)),
        _ => Box::new(ArcanaState::new(max_mp, 0.0, Vec::new())),
    };
    ActorState::new(profiles.max_hp, class_state)
}

// ---------------------------------------------------------------------------
// GlobalTrigger 처리
// ---------------------------------------------------------------------------

/// fire_global_triggers 호출 컨텍스트
enum TriggerContext<'a> {
    BeforeSkillCast { skill_id: u32 },
    /// 스킬 캐스트 시점 (아드레날린 등 SkillCast filter)
    SkillCast { skill_id: u32 },
    /// 스킬 히트 시점 (AnyHit / AnyCrit / SkillTag filter)
    HitEvent {
        skill_id: u32,
        tags: &'a [SkillTag],
        is_crit: bool,
    },
    CardUse { card_id: u32 },
}

/// profiles.global_triggers를 순회하며 컨텍스트에 맞는 트리거를 발동한다.
fn fire_global_triggers(
    profiles: &CharacterProfiles,
    ctx: TriggerContext<'_>,
    state: &mut ActorState,
    queue: &mut EventQueue,
    rng: &mut Rng,
    time_ms: u64,
) {
    for (idx, trigger) in profiles.global_triggers.iter().enumerate() {
        // 필터 매칭
        let filter_matches = match (&trigger.filter, &ctx) {
            (TriggerFilter::SkillCast, TriggerContext::SkillCast { .. }) => true,
            (TriggerFilter::AnyHit, TriggerContext::HitEvent { .. }) => true,
            (TriggerFilter::AnyCrit, TriggerContext::HitEvent { is_crit, .. }) => *is_crit,
            (TriggerFilter::SkillTag(tag), TriggerContext::HitEvent { tags, .. }) => {
                tags.contains(tag)
            }
            (TriggerFilter::SkillId(id), TriggerContext::SkillCast { skill_id }) => {
                *id == *skill_id
            }
            (TriggerFilter::SkillId(id), TriggerContext::HitEvent { skill_id, .. }) => {
                *id == *skill_id
            }
            (TriggerFilter::SkillCastId(id), TriggerContext::SkillCast { skill_id }) => {
                *id == *skill_id
            }
            (TriggerFilter::BeforeSkillCastId(id), TriggerContext::BeforeSkillCast { skill_id }) => {
                *id == *skill_id
            }
            (TriggerFilter::CardUse, TriggerContext::CardUse { .. }) => true,
            _ => false,
        };
        if !filter_matches {
            continue;
        }

        // 트리거 쿨타임 체크
        if let Some(cd_ms) = trigger.cooldown_ms {
            if state
                .trigger_cooldowns
                .get(&idx)
                .is_some_and(|last| time_ms < *last + cd_ms as u64)
            {
                continue;
            }
        }

        // 런타임 조건 체크
        let cond_ok = match &trigger.condition {
            TriggerCondition::Always => true,
            TriggerCondition::Chance(chance) => rng.next_f64() < *chance,
            TriggerCondition::BuffActive(id) => state.buff_manager.is_active(id, time_ms),
            TriggerCondition::ResourceAbove {
                resource,
                threshold,
            } => state.get_resource(resource) >= *threshold,
            TriggerCondition::AfterSkill(_) => false, // TODO
            TriggerCondition::BuffAtMaxStacks(id) => {
                state.buff_manager.is_at_max_stacks(id, time_ms)
            }
        };
        if !cond_ok {
            continue;
        }

        // 쿨타임 갱신
        if trigger.cooldown_ms.is_some() {
            state.trigger_cooldowns.insert(idx, time_ms);
        }

        let skill_id = match &ctx {
            TriggerContext::BeforeSkillCast { skill_id } => *skill_id,
            TriggerContext::SkillCast { skill_id } => *skill_id,
            TriggerContext::HitEvent { skill_id, .. } => *skill_id,
            TriggerContext::CardUse { card_id } => *card_id,
        };

        apply_trigger_effect(&trigger.effect, skill_id, state, queue, rng, time_ms);
    }
}

/// 트리거 효과를 State/Queue에 적용한다.
/// HitTrigger와 GlobalTrigger 양쪽에서 공통으로 사용한다.
fn apply_trigger_effect(
    effect: &TriggerEffect,
    skill_id: u32,
    state: &mut ActorState,
    queue: &mut EventQueue,
    rng: &mut Rng,
    time_ms: u64,
) {
    match effect {
        TriggerEffect::Sequence(effects) => {
            for effect in effects {
                apply_trigger_effect(effect, skill_id, state, queue, rng, time_ms);
            }
        }
        TriggerEffect::Chance { chance, effect } => {
            if rng.next_f64() < *chance {
                apply_trigger_effect(effect, skill_id, state, queue, rng, time_ms);
            }
        }
        TriggerEffect::RandomChoice(effects) => {
            if !effects.is_empty() {
                let index = (rng.next_f64() * effects.len() as f64) as usize;
                apply_trigger_effect(&effects[index], skill_id, state, queue, rng, time_ms);
            }
        }
        TriggerEffect::WeightedRandomChoice(effects) => {
            let total: f64 = effects.iter().map(|(weight, _)| weight.max(0.0)).sum();
            if total > 0.0 {
                let mut roll = rng.next_f64() * total;
                for (weight, effect) in effects {
                    roll -= weight.max(0.0);
                    if roll <= 0.0 {
                        apply_trigger_effect(effect, skill_id, state, queue, rng, time_ms);
                        break;
                    }
                }
            }
        }
        TriggerEffect::Schedule { delay_ms, effect } => {
            queue.push(Event {
                time_ms: time_ms + *delay_ms as u64,
                priority: priority::SCHEDULED_TRIGGER,
                data: EventData::ScheduledTrigger {
                    skill_id,
                    effect: (**effect).clone(),
                },
            });
        }
        TriggerEffect::ApplyBuff(spec) => {
            let expire = time_ms + spec.duration_ms as u64;
            let buff_id = spec.id.clone();
            let actual_expire = state.buff_manager.apply(spec.clone(), expire, 1);
            queue.push(Event {
                time_ms: actual_expire,
                priority: priority::BUFF_EXPIRE,
                data: EventData::BuffExpire {
                    buff_id,
                    expire_at: actual_expire,
                },
            });
        }
        TriggerEffect::ApplyBuffStacks { spec, stacks } => {
            let expire = time_ms + spec.duration_ms as u64;
            let buff_id = spec.id.clone();
            let actual_expire = state.buff_manager.apply(spec.clone(), expire, *stacks);
            queue.push(Event {
                time_ms: actual_expire,
                priority: priority::BUFF_EXPIRE,
                data: EventData::BuffExpire {
                    buff_id,
                    expire_at: actual_expire,
                },
            });
        }
        TriggerEffect::ConsumeBuffStacks { buff_id, stacks } => {
            state.buff_manager.consume_stacks(buff_id, *stacks, time_ms);
        }
        TriggerEffect::ReduceSkillCooldown { percent } => {
            reduce_skill_cooldown(state, queue, skill_id, *percent, time_ms);
        }
        TriggerEffect::ReduceCooldowns { skill_ids, percent } => {
            for skill_id in skill_ids {
                reduce_skill_cooldown(state, queue, *skill_id, *percent, time_ms);
            }
        }
        TriggerEffect::ReduceCooldownsOnResourceFull { resource, skill_ids, percent } => {
            let maximum = state.class_state.get_resource_max(resource);
            if maximum > 0.0
                && state.get_resource(resource) >= maximum
                && state.full_resource_triggers.insert(resource.clone())
            {
                for skill_id in skill_ids {
                    reduce_skill_cooldown(state, queue, *skill_id, *percent, time_ms);
                }
            }
        }
        TriggerEffect::ApplyTargetBuff(spec) => {
            let expire = time_ms + spec.duration_ms as u64;
            let buff_id = spec.id.clone();
            let actual_expire = state.target_buff_manager.apply(spec.clone(), expire, 1);
            queue.push(Event {
                time_ms: actual_expire,
                priority: priority::BUFF_EXPIRE,
                data: EventData::TargetBuffExpire {
                    buff_id,
                    expire_at: actual_expire,
                },
            });
        }
        TriggerEffect::ApplyTargetBuffStacks { spec, stacks } => {
            let expire = time_ms + spec.duration_ms as u64;
            let buff_id = spec.id.clone();
            let actual_expire = state
                .target_buff_manager
                .apply(spec.clone(), expire, *stacks);
            queue.push(Event {
                time_ms: actual_expire,
                priority: priority::BUFF_EXPIRE,
                data: EventData::TargetBuffExpire {
                    buff_id,
                    expire_at: actual_expire,
                },
            });
        }
        TriggerEffect::SetTargetBuffStacks { spec, stacks } => {
            let expire = time_ms + spec.duration_ms as u64;
            let buff_id = spec.id.clone();
            state.target_buff_manager.expire(&buff_id);
            let actual_expire = state
                .target_buff_manager
                .apply(spec.clone(), expire, *stacks);
            queue.push(Event {
                time_ms: actual_expire,
                priority: priority::BUFF_EXPIRE,
                data: EventData::TargetBuffExpire {
                    buff_id,
                    expire_at: actual_expire,
                },
            });
        }
        TriggerEffect::ConsumeTargetBuff {
            buff_id,
            preserve_chance,
            preserve_required_stacks,
            preserve_resource_gain,
        } => {
            let stacks = state.target_buff_manager.stacks(buff_id, time_ms);
            let can_preserve = *preserve_required_stacks == 0
                || stacks == *preserve_required_stacks;
            if can_preserve && *preserve_chance > 0.0 && rng.next_f64() < *preserve_chance {
                if let Some((resource, amount)) = preserve_resource_gain {
                    gain_resource(state, rng, resource, *amount);
                }
                return;
            }
            state.target_buff_manager.expire(buff_id);
        }
        TriggerEffect::GainResource { resource, amount } => {
            gain_resource(state, rng, resource, *amount);
        }
        TriggerEffect::GainResourcePercent { resource, percent } => {
            let max_val = state.class_state.get_resource_max(resource);
            gain_resource(state, rng, resource, max_val * percent);
        }
        TriggerEffect::RefundCastMp { .. } => {
            // 적중 스냅샷이 있는 apply_trigger_damage_effect에서 처리한다.
        }
        TriggerEffect::DrawCard => {
            state.class_state.draw_card(rng);
        }
        TriggerEffect::DrawLastUsedCard => {
            state.class_state.draw_last_used_card();
        }
        TriggerEffect::ResetSkillCooldown => {
            state.skill_cooldowns.remove(&skill_id);
        }
        TriggerEffect::ResetConsumedSkillCooldown {
            buff_id,
            max_charges,
        } => {
            let reset = if *max_charges > 1 {
                let charges = state.skill_charges.entry(skill_id).or_insert(*max_charges);
                *charges = (*charges + 1).min(*max_charges);
                if *charges == *max_charges {
                    state.charge_recovery_times.remove(&skill_id);
                }
                true
            } else {
                state.skill_cooldowns.remove(&skill_id).is_some()
            };
            if reset {
                state.buff_manager.expire(buff_id);
            }
        }
        TriggerEffect::DamageBonus(_) | TriggerEffect::DamageBonusPerBuffStack { .. } => {
            // runtime_damage_bonus_for_hit에서 히트 계산 전에 처리한다.
        }
        TriggerEffect::DealDamageByTargetBuffStacks { .. } => {
            // on_skill_hit에서 snapshot/result가 있는 상태로 처리한다.
        }
        TriggerEffect::DealRuntimeDamage(_) => {
            // 카드 사용 경로에서 snapshot/result와 함께 처리한다.
        }
        TriggerEffect::DealDamageAndSetTargetBuffStacks { .. } => {
            // on_skill_hit에서 snapshot/result/대상 Buff를 함께 처리한다.
        }
        TriggerEffect::ScheduleDot { dot_id, hit, first_tick_ms, tick_interval_ms, tick_count, refresh } => {
            schedule_dot(queue, dot_id, skill_id, hit, *first_tick_ms, *tick_interval_ms, *tick_count, *refresh, time_ms);
        }
    }
}

fn gain_resource(state: &mut ActorState, rng: &mut Rng, resource: &str, amount: f64) {
    let current = state.get_resource(resource);
    let max = state.class_state.get_resource_max(resource);
    if resource == "CardGauge" && max > 0.0 {
        let mut next = current + amount;
        while next >= max {
            next -= max;
            state.class_state.draw_card(rng);
        }
        state.set_resource(resource, next);
        return;
    }
    state.set_resource(resource, (current + amount).min(max));
}

fn schedule_dot(
    queue: &mut EventQueue,
    dot_id: &str,
    skill_id: u32,
    hit: &crate::profile::RuntimeDamageSpec,
    first_tick_ms: u32,
    tick_interval_ms: u32,
    tick_count: u32,
    refresh: bool,
    time_ms: u64,
) {
    if tick_count == 0 {
        return;
    }
    if refresh {
        queue.remove_dot(dot_id);
    }
    queue.push(Event {
        time_ms: time_ms + first_tick_ms as u64,
        priority: priority::DOT_TICK,
        data: EventData::DotTick {
            dot_id: dot_id.to_string(),
            skill_id,
            hit: hit.clone(),
            remaining_ticks: tick_count,
            tick_interval_ms,
        },
    });
}

fn reduce_skill_cooldown(
    state: &mut ActorState,
    queue: &mut EventQueue,
    skill_id: u32,
    percent: f64,
    time_ms: u64,
) {
    let multiplier = (1.0 - percent).max(0.0);
    let mut next_action_check = None;
    if let Some(end) = state.skill_cooldowns.get_mut(&skill_id) {
        if *end > time_ms {
            *end = time_ms + ((*end - time_ms) as f64 * multiplier) as u64;
            next_action_check = Some(*end);
        }
    }
    if let Some(end) = state.charge_recovery_times.get_mut(&skill_id) {
        if *end > time_ms {
            *end = time_ms + ((*end - time_ms) as f64 * multiplier) as u64;
            next_action_check = Some(next_action_check.map_or(*end, |next| next.min(*end)));
            queue.push(Event {
                time_ms: *end,
                priority: priority::CHARGE_RECOVERY,
                data: EventData::ChargeRecovery { skill_id },
            });
        }
    }
    if let Some(next) = next_action_check {
        queue.schedule_action_check(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{BuffSpec, StackType};

    #[test]
    fn redraw_card_effect_is_not_recorded_as_its_own_copy_target() {
        assert!(copies_last_used_card(&TriggerEffect::Sequence(vec![
            TriggerEffect::DrawLastUsedCard,
        ])));
        assert!(!copies_last_used_card(&TriggerEffect::DrawCard));
    }

    #[test]
    fn wheel_restores_one_consumed_charge() {
        let mut state = ActorState::new(
            100.0,
            Box::new(ArcanaState::new(100.0, 100.0, Vec::new())),
        );
        state.skill_charges.insert(7, 1);
        state.charge_recovery_times.insert(7, 1000);
        state.buff_manager.apply(
            BuffSpec {
                id: "190950".to_string(),
                effects: HashMap::new(),
                max_stacks: 1,
                stack_type: StackType::Refresh,
                duration_ms: u32::MAX,
            },
            u32::MAX as u64,
            1,
        );
        let mut queue = EventQueue::new();
        let mut rng = Rng::new(1);

        apply_trigger_effect(
            &TriggerEffect::ResetConsumedSkillCooldown {
                buff_id: "190950".to_string(),
                max_charges: 2,
            },
            7,
            &mut state,
            &mut queue,
            &mut rng,
            0,
        );

        assert_eq!(state.skill_charges[&7], 2);
        assert!(!state.charge_recovery_times.contains_key(&7));
        assert!(!state.buff_manager.is_active("190950", 0));
    }

    #[test]
    fn card_gauge_draws_a_card_and_keeps_the_overflow() {
        let mut state = ActorState::new(
            100.0,
            Box::new(ArcanaState::new(
                100.0,
                10_000.0,
                vec![crate::profile::actor::ArcanaCardPoolEntry {
                    skill_id: 19097,
                    draw_weight: 1.0,
                }],
            )),
        );
        let mut rng = Rng::new(1);

        gain_resource(&mut state, &mut rng, "CardGauge", 11_000.0);

        assert_eq!(state.get_resource("CardGauge"), 1_000.0);
        assert!(state.class_state.use_card(19097));

        gain_resource(&mut state, &mut rng, "CardGauge", 30_000.0);

        assert_eq!(state.get_resource("CardGauge"), 1_000.0);
        assert!(state.class_state.use_card(19097));
        assert!(!state.class_state.use_card(19097));
    }

    #[test]
    fn preserved_ruin_stacks_and_bonus_gauge_share_one_proc() {
        let mut state = ActorState::new(
            100.0,
            Box::new(ArcanaState::new(
                100.0,
                10_000.0,
                vec![crate::profile::actor::ArcanaCardPoolEntry {
                    skill_id: 19097,
                    draw_weight: 1.0,
                }],
            )),
        );
        state.target_buff_manager.apply(
            BuffSpec {
                id: "arcana_ruin_stack".to_string(),
                effects: HashMap::new(),
                max_stacks: 4,
                stack_type: StackType::Refresh,
                duration_ms: 10_000,
            },
            10_000,
            4,
        );
        let mut queue = EventQueue::new();
        let mut rng = Rng::new(1);

        apply_trigger_effect(
            &TriggerEffect::ConsumeTargetBuff {
                buff_id: "arcana_ruin_stack".to_string(),
                preserve_chance: 1.0,
                preserve_required_stacks: 4,
                preserve_resource_gain: Some(("CardGauge".to_string(), 1_900.0)),
            },
            0,
            &mut state,
            &mut queue,
            &mut rng,
            0,
        );

        assert_eq!(state.target_buff_manager.stacks("arcana_ruin_stack", 0), 4);
        assert_eq!(state.get_resource("CardGauge"), 1_900.0);
    }
}
