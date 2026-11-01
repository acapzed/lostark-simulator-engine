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
            e.raw_stagger_points += stat.raw_stagger_points;
            for (level, count) in stat.destruction_hits {
                *e.destruction_hits.entry(level).or_default() += count;
            }
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
    let mut state = create_actor_state(profiles);
    simulate_one_from_state(profiles, seed, duration_ms, trace, &mut state)
}

/// Explicit research state entry. Production callers retain create_actor_state
/// and its class guard; this does not register a new production class.
pub fn research_one(
    profiles: &CharacterProfiles,
    seed: u64,
    duration_ms: u32,
    state: &mut ActorState,
    collect_trace: bool,
) -> (SimResult, TraceData) {
    let mut trace = collect_trace.then(TraceData::default);
    let result = simulate_one_from_state(profiles, seed, duration_ms, &mut trace, state);
    (result, trace.unwrap_or_default())
}

fn simulate_one_from_state(
    profiles: &CharacterProfiles,
    seed: u64,
    duration_ms: u32,
    trace: &mut Option<TraceData>,
    mut state: &mut ActorState,
) -> SimResult {
    let mut rng = Rng::new(seed);
    for _ in 0..profiles.initial_card_draws {
        state.class_state.draw_card(&mut rng);
    }
    let mut queue = EventQueue::new();
    let max_time_ms = duration_ms as u64;
    let reactive_reduction=profiles.reaper_reference.as_ref().and_then(|reference|reference.metadata["reactive_damage_reduction"].as_f64()).unwrap_or(0.);

    // skill_id → skills 배열 인덱스
    let skill_index: HashMap<u32, usize> = profiles
        .skills
        .iter()
        .enumerate()
        .map(|(i, s)| (s.skill_id, i))
        .collect();

    let mut result: SimResult = SimResult::default();
    sync_reaper(&mut state, &mut queue, trace, "initial", 0, "", 0);
    sync_valkyrie(&mut state);
    record_valkyrie_state(&state, trace, "initial", 0, 0);

    for hit in &profiles.reaper_incoming_damage {
        queue.push(Event { time_ms:hit.time_ms as u64, priority:priority::SCHEDULED_TRIGGER,
            data:EventData::ReaperIncomingDamage {damage:hit.damage} });
    }
    // 초기 ActionCheck
    queue.schedule_action_check(0);

    while let Some(event) = queue.pop() {
        let Event { time_ms, data, .. } = event;
        if time_ms > max_time_ms {
            break;
        }
        state.current_time = time_ms;
        if state.class_state.as_reaper_mut().is_some_and(|reaper| reaper.advance(time_ms)) {
            sync_reaper(&mut state, &mut queue, trace, "expire", 0, "", time_ms);
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

        match data {
            EventData::ReaperIncomingDamage {damage} => {
                let before=state.hp;
                state.hp=(state.hp-damage*(1.-reactive_reduction)).max(0.);
                if let Some(t)=trace.as_mut() { t.incoming_damage_events.push(crate::constants::IncomingDamageTrace {
                    time:time_ms as f64/1000.,input_damage:damage,applied_damage:before-state.hp,hp_before:before,hp_after:state.hp}); }
            },
            EventData::ReaperExpire => {},
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
                            cast_id: result.cast_count,
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
                            target_movement_speed_reduction: state.target_buff_manager.target_movement_speed_reduction(time_ms),
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
                if let Some(id) = cast_skill_id {
                    record_reaper_state(&state, trace, "cast", reaper_game_id(&profiles.skills[skill_index[&id]]), "", time_ms);
                    record_valkyrie_state(&state, trace, "cast", reaper_game_id(&profiles.skills[skill_index[&id]]), time_ms);
                }
            }
            EventData::SkillHit {
                skill_id,
                cast_id,
                hit_index,
                snap,
            } => {
                if let Some(&idx) = skill_index.get(&skill_id) {
                    on_skill_hit(
                        profiles,
                        &profiles.skills[idx],
                        cast_id,
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
            EventData::BuffExpireEffect { buff_id, expire_at, skill_id, effect } => {
                // This event owns expiration; no ordinary BuffExpire is queued
                // for ApplyBuffWithExpiry. At the preceding integer millisecond
                // only the current token has exactly one millisecond remaining.
                let current = expire_at > 0 && state.buff_manager.remaining_ms(&buff_id, expire_at - 1) == 1;
                if current {
                    state.buff_manager.expire_if(&buff_id, expire_at);
                    apply_trigger_effect(&effect, skill_id, state, &mut queue, &mut rng, time_ms);
                }
            }
            EventData::TargetBuffExpire { buff_id, expire_at } => {
                state.target_buff_manager.expire_if(&buff_id, expire_at);
            }
            EventData::DotTick {
                dot_id,
                target_stack_buff_id,
                independent,
                cast_snapshot,
                skill_id,
                cast_id,
                hit,
                remaining_ticks,
                tick_interval_ms,
            } => {
                if let Some(&idx) = skill_index.get(&skill_id) {
                    let skill = &profiles.skills[idx];
                    let stacks = target_stack_buff_id.as_ref().map_or(1,|id| state.target_buff_manager.stacks(id,time_ms));
                    if stacks == 0 { continue; }
                    let independent_skill = (independent || target_stack_buff_id.is_some()).then(|| {
                        let mut independent = skill.clone();
                        independent.tags.clear();
                        independent.cast_buff_damage_bonuses.clear();
                        independent.cast_buff_stack_damage_bonuses.clear();
                        independent.cast_resource_scaled_damage_bonuses.clear();
                        independent.buff_damage_bonuses.clear();
                        independent.buff_crit_damage_bonuses.clear();
                        independent.source_damage_multiplier = 1.0;
                        independent.cast_resource_damage_bonuses.clear();
                        independent.cast_buff_crit_rate_bonuses.clear();
                        independent.cast_resource_crit_rate_bonuses.clear();
                        independent.cast_buff_back_attack_id = None;
                        independent.back_attack_variant = None;
                        independent.evolution_damage_bonuses.clear();
                        independent
                    });
                    let scaled = target_stack_buff_id.as_ref().map(|_| crate::profile::RuntimeDamageSpec {
                        base_damage:hit.base_damage*stacks as f64,skill_modifier:hit.skill_modifier*stacks as f64,..hit.clone()
                    });
                    let dot_hit = scaled.as_ref().unwrap_or(&hit);
                    let mut snap = snapshot::capture(profiles, independent_skill.as_ref().unwrap_or(skill), 0, &state, trace.is_some());
                    // Freeze only source-skill cast bonuses; actor/target buffs
                    // remain live. Independent items and Reaper poison are excluded.
                    if !independent && state.class_state.as_reaper().is_none() {
                        if let Some(cast) = cast_snapshot.as_deref() {
                            snap.replace_cast_damage_bonus(cast);
                            snap.replace_cast_crit_bonus(cast);
                        }
                    }
                    let hit_result = if trace.is_some() {
                        let (hit_result, formula) =
                            damage::calculate_runtime_damage_with_trace(dot_hit, &snap, &mut rng);
                        push_damage_trace(
                            trace,
                            DamageTraceEvent {
                                time: time_ms as f64 / 1000.0,
                                cast_id,
                                skill_id,
                                skill_name: skill.name.clone(),
                                hit_index: 0,
                                hit_id: hit.hit_id.clone(),
                                hit_name: hit.name.clone(),
                                damage_source: format!("dot:{dot_id}"),
                                destruction_level: dot_hit.destruction,
                                raw_stagger_points: 0.0,
                                result_damage: hit_result.damage,
                                is_crit: hit_result.is_crit,
                                resource_gains: Vec::new(),
                                held_cards_after_hit: Vec::new(),
                                formula,
                            },
                        );
                        hit_result
                    } else {
                        damage::calculate_runtime_damage(dot_hit, &snap, &mut rng)
                    };
                    record_hit_result(&mut result, skill_id, hit_result, dot_hit.destruction);
                }

                if remaining_ticks > 1 {
                    queue.push(Event {
                        time_ms: time_ms + tick_interval_ms as u64,
                        priority: priority::DOT_TICK,
                        data: EventData::DotTick {
                            dot_id,
                            target_stack_buff_id,
                            independent,
                            cast_snapshot,
                            skill_id,
                            cast_id,
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
                            let interval = snapshot::capture(profiles, skill, 0, &state, false).actual_charge_recovery_ms;
                            let next = time_ms + interval as u64;
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
            EventData::SkillEnd {skill_id,cast_id} => {
                let landed=state.cast_landings.remove(&cast_id).unwrap_or(false);
                if landed {
                    if let Some(&idx)=skill_index.get(&skill_id) {
                        for effect in &profiles.skills[idx].on_end_if_hit {
                            apply_trigger_effect(effect,skill_id,&mut state,&mut queue,&mut rng,time_ms);
                        }
                    }
                }
            }
            EventData::ScheduledTrigger { skill_id, effect, cast_snapshot } => {
                apply_trigger_effect_with_cast(
                    &effect,
                    skill_id,
                    &mut state,
                    &mut queue,
                    &mut rng,
                    time_ms,
                    cast_snapshot.as_deref(),
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
        // 카드 사용 효과가 상태를 바꾸므로 다음 탐색에서는 다시 계산한다.
        let mut next_action = None;
        let Some(action) = profiles.card_apl_actions.iter().find(|action| {
            if !state.class_state.has_card(action.card_id) {
                return false;
            }
            !profiles
                .arcana_card_blocking_buffs
                .get(&action.card_id)
                .is_some_and(|buff_id| state.buff_manager.is_active(buff_id, time_ms))
            && action.conditions.iter().all(|condition| {
                match condition {
                    AplCondition::NextSkill(skill_id) => {
                        next_action.get_or_insert_with(|| {
                            choose_apl_action(profiles, skill_index, state, time_ms, max_time_ms)
                        })
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
        TriggerEffect::Chance { chance, effect } => {
            if rng.next_f64() < *chance {
                apply_card_effect(
                    profiles, effect, card_id, card_name, state, queue, rng, time_ms,
                    result, trace,
                );
            }
        }
        TriggerEffect::ActivateCardEffect(source_card_id) => {
            if let Some(effect) = profiles.arcana_card_effects.get(source_card_id) {
                let name = profiles.arcana_card_names.get(source_card_id)
                    .map(String::as_str).unwrap_or(card_name);
                apply_card_effect(
                    profiles, effect, *source_card_id, name, state, queue, rng, time_ms,
                    result, trace,
                );
            }
        }
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
            card.tags.clear();
            card.tags.push(SkillTag::SkillId(card_id));
            card.source_damage_multiplier = 1.0;
            card.evolution_damage_bonuses.clear();
            card.cast_buff_damage_bonuses.clear();
            card.cast_buff_stack_damage_bonuses.clear();
            card.cast_resource_scaled_damage_bonuses.clear();
            card.buff_damage_bonuses.clear();
            card.buff_crit_damage_bonuses.clear();
            card.cast_resource_damage_bonuses.clear();
            card.cast_buff_crit_rate_bonuses.clear();
            card.cast_resource_crit_rate_bonuses.clear();
            card.cast_buff_back_attack_id = None;
            card.back_attack_variant = None;
            let snap = snapshot::capture(profiles, &card, 0, state, trace.is_some());
            let hit_result = if trace.is_some() {
                let (hit_result, formula) =
                    damage::calculate_runtime_damage_with_trace(spec, &snap, rng);
                push_damage_trace(
                    trace,
                    DamageTraceEvent {
                        time: time_ms as f64 / 1000.0,
                        cast_id: None,
                        skill_id: card_id,
                        skill_name: card_name.to_string(),
                        hit_index: 0,
                        hit_id: spec.hit_id.clone(),
                        hit_name: spec.name.clone(),
                        damage_source: format!("card:{card_id}"),
                        destruction_level: spec.destruction,
                        raw_stagger_points: 0.0,
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
            record_hit_result(result, card_id, hit_result, spec.destruction);
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
    if state.class_state.as_reaper().is_some_and(|reaper|
        reaper.scenario == crate::profile::reaper::ReaperScenario::ThirstChaosSustain && reaper.chaos_until == 0 && !reaper.has_identity_model())
    {
        return None; // End sustain actions, but let already reserved hits finish.
    }
    let chosen = choose_apl_action(profiles, skill_index, state, time_ms, max_time_ms);

    if let Some((skill_id, idx)) = chosen {
        let skill = &profiles.skills[idx];
        let current_phase = cooldown::current_phase(skill, state, time_ms);
        let game_id = reaper_game_id(skill);
        // RA09: Fatal's own first hit receives the new ten-second buff.
        if game_id == crate::engine::state::class::reaper::FATAL_STEP {
            if let Some(reaper) = state.class_state.as_reaper_mut() { reaper.cast(game_id); }
        }
        if skill.slot != SkillSlot::BasicAttack {
            fire_global_triggers(
                profiles,
                TriggerContext::BeforeSkillCast { skill_id },
                state,
                queue,
                rng,
                time_ms,
            );
        }
        let back_variant = skill.cast_buff_back_attack_id.as_ref()
            .filter(|id| state.buff_manager.is_active(id, time_ms))
            .and(skill.back_attack_variant.as_deref());
        let skill = back_variant.unwrap_or(skill);
        let mut snap = snapshot::capture(profiles, skill, current_phase, state, trace_enabled);
        snap.forced_back_attack = back_variant.is_some();
        if game_id == crate::engine::state::class::reaper::NIGHTMARE && state.class_state.as_reaper().is_some_and(|reaper| reaper.chaos_until > time_ms) {
            snap.actual_cooldown_ms = (snap.actual_cooldown_ms as f64 * 0.60).round() as u32;
        }
        let cast_time = snap.actual_cast_time_ms as u64;
        if let Some(reaper) = state.class_state.as_reaper_mut() {
            if game_id != crate::engine::state::class::reaper::FATAL_STEP { reaper.cast(game_id); }
        }
        if game_id == crate::engine::state::class::reaper::PERSONA {
            for other in profiles.skills.iter().filter(|other| reaper_game_id(other) == crate::engine::state::class::reaper::NIGHTMARE) {
                reduce_skill_cooldown(state, queue, other.skill_id, 0.30, time_ms);
            }
        }
        if let Some(valkyrie) = state.class_state.as_valkyrie_mut() {
            assert!(valkyrie.cast(game_id), "selected an unavailable Valkyrie identity action");
            if game_id == crate::engine::state::class::valkyrie::KNIGHT {
                for mobility in profiles.skills.iter().filter(|s| reaper_game_id(s) == crate::engine::state::class::valkyrie::MOBILITY) {
                    // Reset readiness without waking another action inside the current cast.
                    // The source mobility skill has no charge stack; normal cast-end scheduling resumes it.
                    state.skill_cooldowns.insert(mobility.skill_id, time_ms);
                }
            }
            sync_valkyrie(state);
        }

        // State trace is recorded by the caller after this successful use.
        sync_reaper(state, queue, &mut None, "", game_id, "", time_ms);

        // 자원 소모
        snap.paid_mp = pay_resource(skill, state, rng, current_phase);

        // 스킬 단위 자원 획득
        for (res, &amount) in &skill.resource_gains {
            gain_resource(state, rng, res, amount);
        }

        // 쿨타임/충전/체인 상태 갱신
        let charge_recovery_ms = if cooldown::consume(skill, state, time_ms, snap.actual_cooldown_ms) > 0 {
            snap.actual_charge_recovery_ms
        } else { 0 };
        // Valkyrie's regular and super awakening share source CooltimeGroup48520.
        if state.class_state.as_valkyrie().is_some() && matches!(game_id,48520|48540) {
            for other in profiles.skills.iter().filter(|s|matches!(reaper_game_id(s),48520|48540)) {
                state.skill_cooldowns.insert(other.skill_id,time_ms+snap.actual_cooldown_ms as u64);
            }
        }
        if let Some(v)=state.class_state.as_valkyrie_mut() {
            let amount=v.on_core_cast(game_id);
            if amount>0 {
                for other in profiles.skills.iter().filter(|s|reaper_game_id(s)==48150) {
                    if let Some(end)=state.skill_cooldowns.get_mut(&other.skill_id) {*end=end.saturating_sub(amount as u64).max(time_ms);}
                }
            }
            sync_valkyrie(state);
        }

        // stance가 바뀌어도 동일한 기본 공격 콤보의 다음 단계를 이어 간다.
        if skill.tags.iter().any(|tag| matches!(tag, SkillTag::SkillId(19000 | 19270))) {
            for counterpart in profiles.skills.iter().filter(|other| {
                other.skill_id != skill_id && other.tags.iter().any(|tag|
                    matches!(tag, SkillTag::SkillId(19000 | 19270)))
            }) {
                if let Some(&phase) = state.combo_phases.get(&skill_id) {
                    state.combo_phases.insert(counterpart.skill_id, phase);
                    state.combo_timeouts.insert(counterpart.skill_id, state.combo_timeouts[&skill_id]);
                } else {
                    state.combo_phases.remove(&counterpart.skill_id);
                    state.combo_timeouts.remove(&counterpart.skill_id);
                }
            }
        }

        // 충전 회복 예약 (충전형 스킬)
        if skill.max_stacks > 1 && charge_recovery_ms > 0 {
            let charges = state.skill_charges.get(&skill_id).copied().unwrap_or(0);
            if charges < skill.max_stacks && !state.charge_recovery_times.contains_key(&skill_id) {
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
        let cast_id = result.cast_count;
        result.skill_stats.entry(skill_id).or_default().count += 1;
        if !skill.on_end_if_hit.is_empty() {
            state.cast_landings.insert(cast_id,false);
            queue.push(Event {time_ms:time_ms+cast_time,priority:priority::SKILL_END,data:EventData::SkillEnd {skill_id,cast_id}});
        }

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
                    cast_id,
                    hit_index,
                    snap: snap.clone(),
                },
            });
        }

        // 피해 없는 동작 Notify도 동일한 시전 스냅샷으로 예약한다.
        for effect in skill.timed_effects.iter().filter(|effect| effect.phase==current_phase) {
            let base_cast_ms=skill.cast_times_ms.get(current_phase as usize).or_else(||skill.cast_times_ms.first()).copied().unwrap_or(0);
            let effect_time=time_ms+scale_action_delay(effect.action_delay_ms,base_cast_ms,snap.actual_cast_time_ms);
            if effect_time<=max_time_ms {
                let mut scheduled=effect.effect.clone();
                if effect.until_cast_end {
                    if let TriggerEffect::ApplyBuff(buff)=&mut scheduled {
                        let end=effect.action_end_ms.map(|end|time_ms+scale_action_delay(end,base_cast_ms,snap.actual_cast_time_ms)).unwrap_or(time_ms+cast_time).min(time_ms+cast_time);
                        buff.duration_ms=end.saturating_sub(effect_time) as u32;
                    }
                    if effect_time==time_ms {
                        apply_trigger_effect_with_cast(&scheduled,skill_id,state,queue,rng,time_ms,Some(&snap));
                        continue;
                    }
                }
                queue.push(Event {time_ms:effect_time,priority:if effect.action_end_ms.is_some() {priority::SCHEDULED_TRIGGER} else {priority::TIMED_SKILL_EFFECT},
                    data:EventData::ScheduledTrigger {skill_id,effect:scheduled,cast_snapshot:Some(Box::new(snap.clone()))}});
            }
        }

        // 캐스트 완료 후 다음 ActionCheck
        let next_action = if let Some(v)=state.class_state.as_valkyrie_mut() {
            let base=skill.cast_times_ms.get(current_phase as usize).or_else(||skill.cast_times_ms.first()).copied().unwrap_or(0);
            v.begin_action(game_id,time_ms,base,snap.actual_cast_time_ms)
        }else{time_ms + cast_time.max(1)};
        queue.set_actor_action_ready(next_action);
        if next_action <= max_time_ms {
            queue.schedule_action_check(next_action);
        }

        // GlobalTrigger — SkillCast
        if skill.slot != SkillSlot::BasicAttack {
            fire_global_triggers(
                profiles,
                TriggerContext::SkillCast { skill_id },
                state,
                queue,
                rng,
                time_ms,
            );
        }

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
    let next_time_condition = next_timed_condition_time(profiles, state, time_ms, max_time_ms);

    let next_window=state.class_state.as_valkyrie().and_then(|v|v.next_window(time_ms)).unwrap_or(u64::MAX);
    let next_state_change = next_event.min(next_time_condition).min(next_window);
    let next = if next_action_ready != u64::MAX {
        next_action_ready.min(next_state_change)
    } else if next_cooldown <= time_ms + 1 {
        next_state_change.min(time_ms + IDLE_POLL_INTERVAL_MS)
    } else {
        next_cooldown.min(next_state_change)
    };

    if next > time_ms + 1 || (next == next_time_condition && next > time_ms) {
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
            let (_, idx) = arcana_stance_selection(profiles, action.skill_id, *skill_index.get(&action.skill_id)?, state, time_ms);
            let skill = &profiles.skills[idx];
            if !can_pay_resource(skill, state, time_ms) {
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
                            skill.skill_id,
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

fn next_timed_condition_time(
    profiles: &CharacterProfiles,
    state: &ActorState,
    time_ms: u64,
    max_time_ms: u64,
) -> u64 {
    profiles
        .apl_actions
        .iter()
        .flat_map(|action| action.conditions.iter())
        .chain(profiles.card_apl_actions.iter().flat_map(|action| action.conditions.iter()))
        .filter_map(|condition| {
            match condition {
                AplCondition::Resource { resource, operator, value } if resource == "ReaperChaosRemaining" => {
                    let reaper = state.class_state.as_reaper()?;
                    if *value < 0.0 { return None; }
                    let threshold = reaper.chaos_until as f64 - value * 1000.0;
                    if threshold < 0.0 { return None; }
                    let wake = match operator {
                        crate::profile::CompareOperator::Lte => threshold.ceil() as u64,
                        crate::profile::CompareOperator::Lt if *value > 0.0 => threshold.floor() as u64 + 1,
                        crate::profile::CompareOperator::Eq if threshold.fract() == 0.0 => threshold as u64,
                        crate::profile::CompareOperator::Ne if state.get_resource(resource) == *value => time_ms + 1,
                        _ => return None,
                    };
                    (wake > time_ms && wake <= reaper.chaos_until).then_some(wake)
                }
                AplCondition::TimeRemaining { operator, value_ms } =>
                    countdown_boundary(max_time_ms.saturating_sub(time_ms), *operator, *value_ms, time_ms),
                AplCondition::BuffRemaining { buff_id, operator, value_ms } =>
                    countdown_boundary(state.buff_manager.remaining_ms(buff_id, time_ms), *operator, *value_ms, time_ms),
                AplCondition::SkillCooldownRemaining { skill_id, operator, value_ms } => {
                    let skill = profiles.skills.iter().find(|skill| skill.skill_id == *skill_id)?;
                    let remaining = if cooldown::is_available(skill, state, time_ms) { 0 } else {
                        let timers = if skill.max_stacks > 1 { &state.charge_recovery_times } else { &state.skill_cooldowns };
                        timers.get(skill_id).copied().unwrap_or(time_ms).saturating_sub(time_ms)
                    };
                    countdown_boundary(remaining, *operator, *value_ms, time_ms)
                },
                _ => None,
            }
        })
        .min()
        .unwrap_or(u64::MAX)
}

// A countdown only changes truth at equality and the following millisecond.
// Reserve those transitions instead of polling every simulation tick.
fn countdown_boundary(remaining: u64, operator: crate::profile::CompareOperator, value: u64, now: u64) -> Option<u64> {
    use crate::profile::CompareOperator::{Lt, Lte, Eq, Ne, Gt, Gte};
    if remaining == 0 { return None; }
    let delta = match operator {
        Lte | Eq | Ne | Gt if remaining > value => remaining - value,
        Lt | Gte if remaining >= value && value > 0 => remaining - value + 1,
        Eq | Ne if remaining == value && value > 0 => 1,
        _ => return None,
    };
    Some(now.saturating_add(delta))
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
    cast_id: u32,
    hit_index: usize,
    snap: &CastSnapshot,
    state: &mut ActorState,
    queue: &mut EventQueue,
    rng: &mut Rng,
    time_ms: u64,
    result: &mut SimResult,
    trace: &mut Option<TraceData>,
) {
    let skill = if snap.forced_back_attack { skill.back_attack_variant.as_deref().unwrap_or(skill) } else { skill };
    let Some(hit) = skill.hits.get(hit_index) else {
        return;
    };
    let game_id = reaper_game_id(skill);
    if profiles.reaper_reference.as_ref().is_some_and(|reference| reference.missed_hit_ids.contains(&hit.hit_id)) {
        sync_reaper(state, queue, trace, "miss", game_id, &hit.hit_id, time_ms);
        update_cast_trace_state(trace, profiles, cast_id, state, time_ms);
        return;
    }
    if let Some(landed)=state.cast_landings.get_mut(&cast_id) { *landed=true; }
    let mut live_snap;
    let snap = if hit.use_snapshot {
        snap
    } else {
        live_snap = snapshot::capture(profiles, skill, hit.phase, state, trace.is_some());
        live_snap.paid_mp = snap.paid_mp;
        live_snap.replace_cast_damage_bonus(snap);
        live_snap.replace_cast_crit_bonus(snap);
        live_snap.replace_reaper_boost(snap.reaper_boost);
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
                cast_id: Some(cast_id),
                skill_id: skill.skill_id,
                skill_name: skill.name.clone(),
                hit_index,
                hit_id: hit.hit_id.clone(),
                hit_name: hit.name.clone(),
                damage_source: "skill_hit".to_string(),
                destruction_level: hit.destruction,
                raw_stagger_points: hit.stagger,
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
    // shortcut: raw level counts only; target parts, windows and break-point calibration are not modelled.
    if hit.destruction > 0 { *entry.destruction_hits.entry(hit.destruction).or_default() += 1; }
    // shortcut: source AI points only; target stagger units, windows and completion are not calibrated.
    entry.raw_stagger_points += hit.stagger;
    if hit_result.is_crit {
        entry.crit_count += 1;
    }

    if let Some(reaper) = state.class_state.as_reaper_mut() {
        let before = (reaper.dark, reaper.heat);
        let gain = profiles.reaper_reference.as_ref().and_then(|reference| reference.gauge_test.as_ref())
            .and_then(|test| test.hit_gains.get(&hit.hit_id)).copied();
        reaper.hit_with_gain(game_id, true, gain);
        for (resource, gained) in [("ReaperDark", reaper.dark - before.0), ("ReaperHeat", reaper.heat - before.1)] {
            if gained <= 0.0 { continue; }
            if let Some(event) = trace.as_mut().and_then(|trace| trace.damage_events.last_mut()) {
                event.resource_gains.push(ResourceGainTrace { resource: resource.into(), amount: gained });
            }
        }
        sync_reaper(state, queue, trace, "hit", game_id, &hit.hit_id, time_ms);
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
            cast_id,
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
        apply_trigger_effect_with_cast(&trigger.effect, skill.skill_id, state, queue, rng, time_ms, Some(snap));
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

    update_cast_trace_state(trace, profiles, cast_id, state, time_ms);
    update_hit_trace_cards(trace, profiles, cast_id, hit_index, state);
}

fn apply_trigger_damage_effect(
    effect: &TriggerEffect,
    skill: &SkillProfile,
    cast_id: u32,
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
        TriggerEffect::ReduceOwnCooldownOnHit { amount_ms, max_per_cast_ms } => {
            let used = state.cast_cooldown_reductions.entry((skill_id, cast_id)).or_default();
            let amount = (*amount_ms).min(max_per_cast_ms.saturating_sub(*used));
            *used += amount;
            change_remaining_cooldown(state, queue, skill_id, time_ms, |remaining| remaining.saturating_sub(amount));
            true
        }
        TriggerEffect::DealDamageByTargetBuffStacks {
            buff_id,
            hits_by_stack,
            forced_stacks_while_buff,
            card_gauge_by_stack,
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
            let card_gain = card_gauge_by_stack.get(runtime_hit_index).copied().unwrap_or(0.0);
            if card_gain > 0.0 {
                gain_resource(state, rng, "CardGauge", card_gain);
            }
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
                        cast_id: Some(cast_id),
                        skill_id,
                        skill_name: skill.name.clone(),
                        hit_index,
                        hit_id: runtime_hit.hit_id.clone(),
                        hit_name: runtime_hit.name.clone(),
                        damage_source: format!("target_buff_stacks:{buff_id}:{stacks}"),
                        destruction_level: runtime_hit.destruction,
                        raw_stagger_points: 0.0,
                        result_damage: hit_result.damage,
                        is_crit: hit_result.is_crit,
                        resource_gains: if card_gain > 0.0 { vec![ResourceGainTrace { resource: "CardGauge".into(), amount: card_gain }] } else { Vec::new() },
                        held_cards_after_hit: Vec::new(),
                        formula,
                    },
                );
                hit_result
            } else {
                damage::calculate_runtime_damage(&runtime_hit, snap, rng)
            };
            record_hit_result(result, skill_id, hit_result, runtime_hit.destruction);
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
                        cast_id: Some(cast_id),
                        skill_id,
                        skill_name: skill.name.clone(),
                        hit_index,
                        hit_id: hit.hit_id.clone(),
                        hit_name: hit.name.clone(),
                        damage_source: format!("additional_effect:{effect_id}"),
                        destruction_level: hit.destruction,
                        raw_stagger_points: 0.0,
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
            record_hit_result(result, skill_id, hit_result, hit.destruction);
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
            independent,
        } => {
            schedule_dot(queue, dot_id, skill_id, Some(cast_id), hit, *first_tick_ms, *tick_interval_ms, *tick_count, *refresh, *independent, None, time_ms, Some(snap));
            true
        }
        TriggerEffect::ScheduleTargetStackDot { dot_id,hit,stack_buff_id,first_tick_ms,tick_interval_ms,tick_count } => {
            schedule_dot(queue,dot_id,skill_id,Some(cast_id),hit,*first_tick_ms,*tick_interval_ms,*tick_count,true,false,Some(stack_buff_id.clone()),time_ms,None);
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
    cast_id: u32,
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
        .find(|cast| cast.cast_id == cast_id)
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
    cast_id: u32,
    hit_index: usize,
    state: &ActorState,
) {
    let Some(event) = trace.as_mut().and_then(|trace| trace.damage_events.iter_mut().rev().find(
        |event| event.cast_id == Some(cast_id)
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
    } else if spec.effects.contains_key(&StatField::MovementSpeed) || spec.effects.contains_key(&StatField::MovementSpeedMax) {
        "이동속도 증가"
    } else if let Some(rate)=spec.effects.get(&StatField::DodgeRate) {
        return format!("회피율 증가 ({:.0}%)",rate*100.);
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

fn record_hit_result(result: &mut SimResult, skill_id: u32, hit_result: damage::HitResult, destruction: u32) {
    result.total_damage += hit_result.damage;
    if hit_result.is_crit {
        result.crit_count += 1;
    }
    let entry = result.skill_stats.entry(skill_id).or_default();
    entry.total_damage += hit_result.damage;
    entry.hit_count += 1;
    if destruction > 0 { *entry.destruction_hits.entry(destruction).or_default() += 1; }
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
                TriggerEffect::DamageBonusPerTargetBuffStack { buff_id, per_stack } => {
                    Some(state.target_buff_manager.stacks(buff_id, time_ms) as f64 * per_stack)
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

fn phase_base_cost(skill: &SkillProfile, resource: &str, cost: f64, phase: u32) -> f64 {
    if resource == "Mp" && skill.free_mana_phases.contains(&phase) { 0.0 } else { cost }
}

fn can_pay_resource(skill: &SkillProfile, state: &ActorState, time_ms: u64) -> bool {
    let phase = if skill.free_mana_phases.is_empty() { 0 } else { cooldown::current_phase(skill, state, time_ms) };
    skill
        .resource_costs
        .iter()
        .all(|(res, &cost)| {
            let cost = phase_base_cost(skill, res, cost, phase);
            let cost = if res == "Mp" {
                cost + state.class_state.get_resource_max("Mp") * skill.extra_mp_cost_ratio
            } else { cost };
            (res == "Mp" && state.buff_manager.is_active("190941", state.current_time))
                || state.get_resource(res) >= cost
        })
}

fn pay_resource(skill: &SkillProfile, state: &mut ActorState, rng: &mut Rng, phase: u32) -> f64 {
    let waive_mp = state.buff_manager.is_active("190941", state.current_time)
        || (skill.mp_cost_waiver_chance > 0.0
            && rng.next_f64() < skill.mp_cost_waiver_chance);
    let mut paid_mp = 0.0;
    for (res, &cost) in &skill.resource_costs {
        if waive_mp && res == "Mp" {
            continue;
        }
        let cost = phase_base_cost(skill, res, cost, phase);
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
            let (skill_id, idx) =
                arcana_stance_selection(profiles, action.skill_id, idx, state, time_ms);
            let skill = &profiles.skills[idx];
            if is_castable(skill, state, time_ms)
                && apl_action_conditions_match(
                    action,
                    skill_id,
                    skill_index,
                    &profiles.skills,
                    state,
                    time_ms,
                    max_time_ms,
                )
            {
                Some((skill_id, idx))
            } else {
                None
            }
        });
    }

    profiles.apl_order.iter().find_map(|&skill_id| {
        let idx = *skill_index.get(&skill_id)?;
        let (skill_id, idx) = arcana_stance_selection(profiles, skill_id, idx, state, time_ms);
        let skill = &profiles.skills[idx];
        is_castable(skill, state, time_ms).then_some((skill_id, idx))
    })
}

fn arcana_stance_selection(
    profiles: &CharacterProfiles,
    skill_id: u32,
    idx: usize,
    state: &ActorState,
    time_ms: u64,
) -> (u32, usize) {
    // 삼두사 Buff→stance=1은 툴팁과 19000의 변경 조건을 결합한 추론이다.
    if profiles.skills[idx].tags.contains(&SkillTag::SkillId(19000))
        && state.buff_manager.is_active("190970", time_ms)
    {
        if let Some((idx, skill)) = profiles
            .skills
            .iter()
            .enumerate()
            .find(|(_, skill)| skill.tags.contains(&SkillTag::SkillId(19270)))
        {
            return (skill.skill_id, idx);
        }
    }
    (skill_id, idx)
}

fn is_castable(skill: &SkillProfile, state: &ActorState, time_ms: u64) -> bool {
    cooldown::is_available(skill, state, time_ms) && can_pay_resource(skill, state, time_ms)
        && state.class_state.as_reaper().is_none_or(|reaper| reaper.can_cast(reaper_game_id(skill)))
        && state.class_state.as_valkyrie().is_none_or(|valkyrie| valkyrie.can_cast(reaper_game_id(skill)) && valkyrie.action_allows(reaper_game_id(skill),time_ms))
}

fn apl_action_conditions_match(
    action: &AplAction,
    selected_skill_id: u32,
    skill_index: &HashMap<u32, usize>,
    skills: &[SkillProfile],
    state: &ActorState,
    time_ms: u64,
    max_time_ms: u64,
) -> bool {
    action.conditions.iter().all(|condition| {
        apl_condition_matches(
            condition,
            selected_skill_id,
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
        AplCondition::SkillTargetStackGain {
            debuff_id,
            operator,
            value,
        } => skill_by_id(current_skill_id, skill_index, skills).is_some_and(|skill| {
            let gain = skill
                .hits
                .iter()
                .flat_map(|hit| &hit.triggers)
                .filter_map(|trigger| match &trigger.effect {
                    TriggerEffect::ApplyTargetBuffStacks { spec, stacks }
                        if spec.id == *debuff_id
                            && matches!(trigger.on, HitTriggerOn::OnHit)
                            && trigger.chance == 1.0 =>
                    {
                        Some(*stacks)
                    }
                    _ => None,
                })
                .max()
                .unwrap_or(0);
            operator.compare(gain as f64, *value)
        }),
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
        "소서리스" => {
            let model:serde_json::Value=serde_json::from_str(&profiles.buff_metadata.get("sorceress_snapshot:model").expect("소서리스 연구 계약 없음").source).expect("소서리스 연구 계약 오류");
            let mut s=crate::engine::state::class::sorceress::SorceressState::new(max_mp,max_mp,model["initialMagic"].as_f64().unwrap(),model["initialUltimatePoint"].as_f64().unwrap()).unwrap();
            if model["phase"]!="normal"{s.activate().unwrap();}Box::new(s)
        },
        "발키리" => Box::new(crate::engine::state::class::valkyrie::ValkyrieState::new(
            max_mp, profiles.valkyrie_identity.clone().expect("발키리 아이덴티티 연구 계약 없음"),
        )),
        "리퍼" => Box::new(crate::engine::state::class::reaper::ReaperState::from_reference(
            max_mp, profiles.reaper_reference.as_ref().expect("리퍼 임시 설정 없음"),
        )),
        other => panic!("지원하지 않는 직업 상태: {other}"),
    };
    ActorState::new(profiles.max_hp, class_state)
}

fn reaper_game_id(skill: &SkillProfile) -> u32 {
    skill.tags.iter().find_map(|tag| match tag { SkillTag::SkillId(id) => Some(*id), _ => None }).unwrap_or(0)
}

fn sync_valkyrie(state: &mut ActorState) {
    use crate::engine::state::class::valkyrie::{SWORD_BUFF, LOST_BUFF, LAST_LIGHT_BUFF,CORE_FINALE_BUFF,CORE_ORIGIN_BUFF,CORE_WHISPER_BUFF};
    let Some(valkyrie) = state.class_state.as_valkyrie() else { return; };
    let values=[(SWORD_BUFF,valkyrie.swords),(LOST_BUFF,u32::from(valkyrie.lost_sword)),(LAST_LIGHT_BUFF,u32::from(valkyrie.last_light)),(CORE_FINALE_BUFF,u32::from(valkyrie.core_finale)),(CORE_ORIGIN_BUFF,valkyrie.core_origin),(CORE_WHISPER_BUFF,u32::from(valkyrie.core_whisper))];
    for (id,count) in values {
        state.buff_manager.expire(id);
        if count>0 {
            state.buff_manager.apply(crate::profile::BuffSpec {id:id.into(),effects:HashMap::new(),max_stacks:count,
                stack_type:crate::profile::StackType::Refresh,duration_ms:u32::MAX},u64::MAX,count);
        }
    }
}

fn record_valkyrie_state(state: &ActorState, trace: &mut Option<TraceData>, event: &str, game_id: u32, time_ms: u64) {
    if let (Some(v),Some(trace))=(state.class_state.as_valkyrie(),trace.as_mut()) {
        trace.valkyrie_events.push(crate::constants::ValkyrieStateTrace {time:time_ms as f64/1000.,event:event.into(),game_skill_id:game_id,
            light:v.light,light_swords:v.swords,lost_light_sword:v.lost_sword,finale_ready:v.can_cast(crate::engine::state::class::valkyrie::FINALE),last_light:v.tracks_last_light().then_some(v.last_light),cores:v.tracks_cores().then_some(crate::constants::ValkyrieCoreStateTrace {destiny_count:v.destiny_count,origin_stacks:v.core_origin,finale_boost:v.core_finale,whisper_boost:v.core_whisper,mp:state.get_resource("Mp")})});
    }
}

fn record_reaper_state(state: &ActorState, trace: &mut Option<TraceData>, event: &str, game_id: u32, hit_id: &str, time_ms: u64) {
    if let (Some(reaper), Some(trace)) = (state.class_state.as_reaper(), trace.as_mut()) {
        trace.class_events.push(crate::constants::ReaperStateTrace {
            time: time_ms as f64 / 1000.0, event: event.into(), game_skill_id: game_id, hit_id: hit_id.into(),
            dark: reaper.dark, heat: reaper.heat, persona: reaper.persona,
            chaos_remaining: reaper.chaos_until.saturating_sub(time_ms) as f64 / 1000.0,
            fatal_remaining: reaper.fatal_until.saturating_sub(time_ms) as f64 / 1000.0,
            nightmare_remaining: (reaper.nightmare_until > time_ms).then_some(reaper.nightmare_until.saturating_sub(time_ms) as f64 / 1000.0),
            weakness: reaper.weakness, blade: reaper.blade,
        });
    }
}

fn sync_reaper(state: &mut ActorState, queue: &mut EventQueue, trace: &mut Option<TraceData>, event: &str, game_id: u32, hit_id: &str, time_ms: u64) {
    let Some(reaper) = state.class_state.as_reaper() else { return; };
    let expiration = reaper.next_expiration();
    let buffs = [
        ("reaper:persona", if reaper.persona { 1 } else { 0 }, u64::MAX, 1),
        ("reaper:chaos", if reaper.chaos_until > time_ms { 1 } else { 0 }, reaper.chaos_until, 1),
        ("reaper:fatal", if reaper.fatal_until > time_ms { 1 } else { 0 }, reaper.fatal_until, 1),
        ("reaper:weakness", reaper.weakness, u64::MAX, 3),
        ("reaper:blade", if reaper.blade { 1 } else { 0 }, u64::MAX, 1),
    ];
    for (id, stacks, until, max_stacks) in buffs {
        state.buff_manager.expire(id);
        if stacks > 0 {
            // Display mirror only; the class boost is captured separately.
            state.buff_manager.apply(crate::profile::BuffSpec {
                id: id.into(), effects: HashMap::new(), max_stacks,
                stack_type: crate::profile::StackType::Refresh, duration_ms: u32::MAX,
            }, until, stacks);
        }
    }
    if let Some(until) = expiration { queue.schedule_reaper_expire(until); }
    if !event.is_empty() { record_reaper_state(state, trace, event, game_id, hit_id, time_ms); }
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
            (TriggerFilter::SkillCastTag(tag), TriggerContext::SkillCast { skill_id }) => {
                profiles.skills.iter().find(|s| s.skill_id == *skill_id)
                    .is_some_and(|s| s.tags.contains(tag))
            }
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
            TriggerCondition::BuffAtMaxStacks(id) => {
                state.buff_manager.is_at_max_stacks(id, time_ms)
            }
            TriggerCondition::BuffStacksAtLeast { buff_id, stacks } => {
                state.buff_manager.stacks(buff_id, time_ms) >= *stacks
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
    apply_trigger_effect_with_cast(effect,skill_id,state,queue,rng,time_ms,None);
}

fn apply_trigger_effect_with_cast(
    effect: &TriggerEffect,
    skill_id: u32,
    state: &mut ActorState,
    queue: &mut EventQueue,
    rng: &mut Rng,
    time_ms: u64,
    cast_snapshot: Option<&CastSnapshot>,
) {
    match effect {
        TriggerEffect::RespectActionClock => {queue.respect_action_clock();queue.schedule_action_check(time_ms.max(queue.actor_action_ready()));}
        TriggerEffect::Sequence(effects) => {
            for effect in effects {
                apply_trigger_effect_with_cast(effect, skill_id, state, queue, rng, time_ms, cast_snapshot);
            }
        }
        TriggerEffect::Chance { chance, effect } => {
            if rng.next_f64() < *chance {
                apply_trigger_effect_with_cast(effect, skill_id, state, queue, rng, time_ms, cast_snapshot);
            }
        }
        TriggerEffect::RandomChoice(effects) => {
            if !effects.is_empty() {
                let index = (rng.next_f64() * effects.len() as f64) as usize;
                apply_trigger_effect_with_cast(&effects[index], skill_id, state, queue, rng, time_ms, cast_snapshot);
            }
        }
        TriggerEffect::WeightedRandomChoice(effects) => {
            let total: f64 = effects.iter().map(|(weight, _)| weight.max(0.0)).sum();
            if total > 0.0 {
                let mut roll = rng.next_f64() * total;
                for (weight, effect) in effects {
                    roll -= weight.max(0.0);
                    if roll <= 0.0 {
                        apply_trigger_effect_with_cast(effect, skill_id, state, queue, rng, time_ms, cast_snapshot);
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
                    cast_snapshot:cast_snapshot.map(|s|Box::new(s.clone())),
                },
            });
        }
        TriggerEffect::ExtendBuffDuration { buff_id, amount_ms } => {
            if let Some(expire_at) = state.buff_manager.extend_active(buff_id, time_ms, *amount_ms) {
                queue.push(Event {
                    time_ms: expire_at,
                    priority: priority::BUFF_EXPIRE,
                    data: EventData::BuffExpire { buff_id: buff_id.clone(), expire_at },
                });
            }
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
        TriggerEffect::ApplyBuffWithExpiry { spec, on_expire } => {
            let expire_at = state.buff_manager.apply(spec.clone(), time_ms + spec.duration_ms as u64, 1);
            queue.push(Event { time_ms: expire_at, priority: priority::BUFF_EXPIRE,
                data: EventData::BuffExpireEffect { buff_id: spec.id.clone(), expire_at, skill_id, effect: (**on_expire).clone() } });
        }
        TriggerEffect::AfterCurrentAction { effect } => {
            queue.push(Event { time_ms: time_ms.max(queue.actor_action_ready()), priority: priority::SCHEDULED_TRIGGER,
                data: EventData::ScheduledTrigger { skill_id, effect: (**effect).clone(), cast_snapshot:cast_snapshot.map(|s|Box::new(s.clone())) } });
        }
        TriggerEffect::IfBuffActive { buff_id, effect } => {
            if state.buff_manager.is_active(buff_id,time_ms) {apply_trigger_effect_with_cast(effect,skill_id,state,queue,rng,time_ms,cast_snapshot);}
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
        TriggerEffect::ReduceOwnCooldownOnHit { .. } => {}, // Requires an actual hit and cast_id; handled above.
        TriggerEffect::ReduceCooldownsFlat { skill_ids, amount_ms } => {
            for skill_id in skill_ids {
                change_remaining_cooldown(state,queue,*skill_id,time_ms,|remaining| remaining.saturating_sub(*amount_ms));
            }
        }
        TriggerEffect::ReduceCooldownsFlatAfterAction { skill_ids, amount_ms } => {
            queue.respect_action_clock();
            for skill_id in skill_ids {
                change_remaining_cooldown_inner(state,queue,*skill_id,time_ms,|remaining|remaining.saturating_sub(*amount_ms),true);
            }
        }
        TriggerEffect::RefreshReaperChaos => {
            if let Some(reaper) = state.class_state.as_reaper_mut() {
                reaper.refresh_chaos_after_dagger_cast();
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
            // Hits precede expiry events at the same time; an expired stack must not refresh.
            state.target_buff_manager.expire_if(&buff_id, time_ms);
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
            state.target_buff_manager.expire_if(&buff_id, time_ms);
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
            let max_val = state.get_resource_max(resource);
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
        TriggerEffect::ResetSkillState { skill_id:target,max_charges } => {
            queue.respect_action_clock();
            state.skill_cooldowns.remove(target);
            state.charge_recovery_times.remove(target);
            if *max_charges>1 {state.skill_charges.insert(*target,*max_charges);}
            queue.schedule_action_check(time_ms.max(queue.actor_action_ready()));
        }
        TriggerEffect::ResetConsumedSkillCooldown {
            buff_id,
            max_charges,
        } => {
            let reset = if *max_charges > 1 {
                let charges = state.skill_charges.entry(skill_id).or_insert(*max_charges);
                *charges = (*charges + 1).min(*max_charges);
                state.skill_cooldowns.remove(&skill_id);
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
        TriggerEffect::DamageBonus(_) | TriggerEffect::DamageBonusPerBuffStack { .. }
            | TriggerEffect::DamageBonusPerTargetBuffStack { .. } => {
            // runtime_damage_bonus_for_hit에서 히트 계산 전에 처리한다.
        }
        TriggerEffect::DealDamageByTargetBuffStacks { .. } => {
            // on_skill_hit에서 snapshot/result가 있는 상태로 처리한다.
        }
        TriggerEffect::DealRuntimeDamage(_) | TriggerEffect::ActivateCardEffect(_) => {
            // 카드 사용 경로에서 snapshot/result와 함께 처리한다.
        }
        TriggerEffect::DealDamageAndSetTargetBuffStacks { .. } => {
            // on_skill_hit에서 snapshot/result/대상 Buff를 함께 처리한다.
        }
        TriggerEffect::ScheduleDot { dot_id, hit, first_tick_ms, tick_interval_ms, tick_count, refresh, independent } => {
            schedule_dot(queue, dot_id, skill_id, None, hit, *first_tick_ms, *tick_interval_ms, *tick_count, *refresh, *independent, None, time_ms, cast_snapshot);
        }
        TriggerEffect::ScheduleTargetStackDot { dot_id,hit,stack_buff_id,first_tick_ms,tick_interval_ms,tick_count } => {
            schedule_dot(queue,dot_id,skill_id,None,hit,*first_tick_ms,*tick_interval_ms,*tick_count,true,false,Some(stack_buff_id.clone()),time_ms,None);
        }
    }
}

fn gain_resource(state: &mut ActorState, rng: &mut Rng, resource: &str, amount: f64) {
    let current = state.get_resource(resource);
    let max = state.get_resource_max(resource);
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
    cast_id: Option<u32>,
    hit: &crate::profile::RuntimeDamageSpec,
    first_tick_ms: u32,
    tick_interval_ms: u32,
    tick_count: u32,
    refresh: bool,
    independent: bool,
    target_stack_buff_id: Option<String>,
    time_ms: u64,
    cast_snapshot: Option<&CastSnapshot>,
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
            target_stack_buff_id,
            independent,
            cast_snapshot:if independent {None}else{cast_snapshot.map(|s|Box::new(s.clone()))},
            skill_id,
            cast_id,
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
    change_remaining_cooldown(state,queue,skill_id,time_ms,|remaining| (remaining as f64 * multiplier) as u64);
}

fn change_remaining_cooldown(
    state: &mut ActorState,
    queue: &mut EventQueue,
    skill_id: u32,
    time_ms: u64,
    remaining: impl Fn(u64) -> u64,
) {
    change_remaining_cooldown_inner(state,queue,skill_id,time_ms,remaining,false);
}

fn change_remaining_cooldown_inner(
    state:&mut ActorState,queue:&mut EventQueue,skill_id:u32,time_ms:u64,
    remaining:impl Fn(u64)->u64,wait_for_action:bool,
) {
    let mut next_action_check = None;
    if let Some(end) = state.skill_cooldowns.get_mut(&skill_id) {
        if *end > time_ms {
            *end = time_ms + remaining(*end - time_ms);
            next_action_check = Some(*end);
        }
    }
    if let Some(end) = state.charge_recovery_times.get_mut(&skill_id) {
        if *end > time_ms {
            *end = time_ms + remaining(*end - time_ms);
            next_action_check = Some(next_action_check.map_or(*end, |next| next.min(*end)));
            queue.push(Event {
                time_ms: *end,
                priority: priority::CHARGE_RECOVERY,
                data: EventData::ChargeRecovery { skill_id },
            });
        }
    }
    if let Some(next) = next_action_check {
        queue.schedule_action_check(if wait_for_action {next.max(queue.actor_action_ready())}else{next});
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cast_group_shares_one_cooldown_and_does_not_match_hits_or_other_casts() {
        let mut p=crate::builder::build(include_str!("../../fixtures/reaper-thirst-reference-request.json")).unwrap();
        for s in &mut p.skills {s.tags.retain(|t|!matches!(t,crate::profile::SkillTag::SkillGroup(99)));}
        p.skills[0].tags.push(crate::profile::SkillTag::SkillGroup(99));p.skills[1].tags.push(crate::profile::SkillTag::SkillGroup(99));
        p.global_triggers=vec![crate::profile::GlobalTrigger {filter:TriggerFilter::SkillCastTag(crate::profile::SkillTag::SkillGroup(99)),condition:TriggerCondition::Always,
            effect:TriggerEffect::ApplyBuff(BuffSpec {id:"cast_group".into(),effects:Default::default(),max_stacks:10,stack_type:StackType::Refresh,duration_ms:10000}),cooldown_ms:Some(1000)}];
        let mut state=ActorState::new(100.,Box::new(BerserkerState::new(100.,200.)));let mut queue=EventQueue::new();let mut rng=Rng::new(42);
        for (id,time) in [(0,0),(1,999),(2,1000),(1,1000)] {fire_global_triggers(&p,TriggerContext::SkillCast {skill_id:p.skills[id].skill_id},&mut state,&mut queue,&mut rng,time);}
        assert_eq!(state.buff_manager.stacks("cast_group",1000),2);
        fire_global_triggers(&p,TriggerContext::HitEvent {skill_id:p.skills[0].skill_id,tags:&p.skills[0].tags,is_crit:false},&mut state,&mut queue,&mut rng,2000);
        assert_eq!(state.buff_manager.stacks("cast_group",2000),2);
    }
    #[test]
    fn source_duration_extension_preserves_stacks_and_ignores_stale_expiry_without_resurrection() {
        use super::*;
        use crate::engine::state::class::berserker::BerserkerState;
        let mut state=ActorState::new(100.0,Box::new(BerserkerState::new(100.0,10000.0)));
        let mut queue=EventQueue::new();let mut rng=Rng::new(42);
        let extend=TriggerEffect::ExtendBuffDuration{buff_id:"source_rage".into(),amount_ms:1500};
        apply_trigger_effect(&extend,0,&mut state,&mut queue,&mut rng,0);
        assert!(queue.pop().is_none());
        let spec=BuffSpec{id:"source_rage".into(),effects:HashMap::from([(StatField::AttackPowerMul,0.06)]),max_stacks:1,stack_type:StackType::Refresh,duration_ms:1000};
        apply_trigger_effect(&TriggerEffect::ApplyBuff(spec.clone()),0,&mut state,&mut queue,&mut rng,0);
        apply_trigger_effect(&extend,0,&mut state,&mut queue,&mut rng,500);
        assert_eq!(state.buff_manager.stacks("source_rage",500),1);
        assert_eq!(state.buff_manager.remaining_ms("source_rage",500),2000);
        let old=queue.pop().unwrap();assert_eq!(old.time_ms,1000);
        if let EventData::BuffExpire{buff_id,expire_at}=old.data {state.buff_manager.expire_if(&buff_id,expire_at);}else{panic!("expected old expiry");}
        assert!(state.buff_manager.is_active("source_rage",1000));
        apply_trigger_effect(&extend,0,&mut state,&mut queue,&mut rng,1100);
        let old=queue.pop().unwrap();assert_eq!(old.time_ms,2500);
        if let EventData::BuffExpire{buff_id,expire_at}=old.data {state.buff_manager.expire_if(&buff_id,expire_at);}else{panic!("expected superseded expiry");}
        assert_eq!(state.buff_manager.remaining_ms("source_rage",2500),1500);
        let last=queue.pop().unwrap();assert_eq!(last.time_ms,4000);
        if let EventData::BuffExpire{buff_id,expire_at}=last.data {state.buff_manager.expire_if(&buff_id,expire_at);}else{panic!("expected final expiry");}
        apply_trigger_effect(&extend,0,&mut state,&mut queue,&mut rng,4000);
        assert!(!state.buff_manager.is_active("source_rage",4000));assert!(queue.pop().is_none());
    }
    use super::*;
    use crate::profile::{BuffSpec, StackType};

    #[test]
    fn arcana_mana_payment_matrix_keeps_free_casts_and_refunds_separate() {
        let profiles = crate::builder::build(include_str!("../../fixtures/arcana-sample-request.json")).unwrap();
        for base_cost in [0.0, 100.0, 67.2] {
            for extra_ratio in [0.0, 0.02] {
                for star in [false, true] {
                    for waiver in [0.0, 1.0] {
                        let mut skill = profiles.skills[0].clone();
                        skill.resource_costs = HashMap::from([("Mp".into(), base_cost)]);
                        skill.extra_mp_cost_ratio = extra_ratio;
                        skill.mp_cost_waiver_chance = waiver;
                        let mut state = ActorState::new(100.0, Box::new(crate::engine::state::class::arcana::ArcanaState::new(1000.0,10000.0,Vec::new())));
                        if star { state.buff_manager.apply(BuffSpec { id:"190941".into(),effects:HashMap::new(),max_stacks:1,stack_type:StackType::Refresh,duration_ms:3000 },3000,1); }
                        assert!(can_pay_resource(&skill,&state,0));
                        let mut rng=Rng::new(42);
                        let expected = if star || waiver == 1.0 { 0.0 } else { base_cost + 1000.0 * extra_ratio };
                        let paid=pay_resource(&skill,&mut state,&mut rng,0);
                        assert!((paid-expected).abs()<1e-9);
                        assert!((state.get_resource("Mp")-(1000.0-expected)).abs()<1e-9);
                        let mut snap=snapshot::capture(&profiles,&skill,0,&state,false);snap.paid_mp=paid;
                        apply_trigger_damage_effect(&TriggerEffect::RefundCastMp {percent:0.5},&skill,1,0,skill.skill_id,&snap,&mut state,&mut EventQueue::new(),&mut rng,0,&mut SimResult::default(),&mut None);
                        assert!((state.get_resource("Mp")-(1000.0-expected*0.5)).abs()<1e-9);
                    }
                }
            }
        }
    }

    #[test]
    fn arcana_star_expiry_and_probabilistic_waiver_do_not_refund_unpaid_mana() {
        let profiles=crate::builder::build(include_str!("../../fixtures/arcana-sample-request.json")).unwrap();
        let mut skill=profiles.skills[0].clone();skill.resource_costs=HashMap::from([("Mp".into(),100.0)]);skill.extra_mp_cost_ratio=0.02;
        for (time,expected) in [(2999,0.0),(3000,120.0)] {
            let mut state=ActorState::new(100.0,Box::new(crate::engine::state::class::arcana::ArcanaState::new(1000.0,10000.0,Vec::new())));
            state.buff_manager.apply(BuffSpec{id:"190941".into(),effects:HashMap::new(),max_stacks:1,stack_type:StackType::Refresh,duration_ms:3000},3000,1);
            state.current_time=time;
            assert_eq!(pay_resource(&skill,&mut state,&mut Rng::new(42),0),expected);
        }
        skill.mp_cost_waiver_chance=0.67;
        let mut free=0;
        for seed in 0..1000 {
            let mut state=ActorState::new(100.0,Box::new(crate::engine::state::class::arcana::ArcanaState::new(1000.0,10000.0,Vec::new())));
            let paid=pay_resource(&skill,&mut state,&mut Rng::new(seed),0);
            assert!(paid==0.0 || paid==120.0);
            if paid==0.0 {free+=1;}
        }
        assert!((600..740).contains(&free),"free casts: {free}");
    }

    #[test]
    fn scoped_flat_cooldown_keeps_exact_remaining_time_but_waits_for_action_end(){
        let mut state=ActorState::new(100.,Box::new(BerserkerState::new(100.,200.)));state.skill_cooldowns.insert(1,1200);
        let mut queue=EventQueue::new();queue.set_actor_action_ready(2000);
        apply_trigger_effect(&TriggerEffect::ReduceCooldownsFlatAfterAction{skill_ids:vec![1],amount_ms:500},0,&mut state,&mut queue,&mut Rng::new(42),1000);
        assert_eq!(state.skill_cooldowns[&1],1000);assert_eq!(queue.pop().unwrap().time_ms,2000);
        state.skill_cooldowns.insert(1,3200);apply_trigger_effect(&TriggerEffect::ReduceCooldownsFlatAfterAction{skill_ids:vec![1],amount_ms:500},0,&mut state,&mut queue,&mut Rng::new(42),3000);
        assert_eq!(state.skill_cooldowns[&1],3000);assert_eq!(queue.pop().unwrap().time_ms,3000);
    }

    #[test]
    fn selected_skill_reset_restores_all_charges_preserving_other_state(){
        let mut state=ActorState::new(100.,Box::new(BerserkerState::new(100.,200.)));
        state.skill_cooldowns.extend([(1,5000),(2,6000)]);state.skill_charges.extend([(1,0),(2,1)]);
        state.charge_recovery_times.extend([(1,3000),(2,4000)]);state.skill_use_counts.insert(1,7);state.combo_phases.insert(1,2);
        let mut queue=EventQueue::new();queue.set_actor_action_ready(2000);let mut rng=Rng::new(42);
        apply_trigger_effect(&TriggerEffect::ResetSkillState{skill_id:1,max_charges:3},0,&mut state,&mut queue,&mut rng,1000);
        assert!(!state.skill_cooldowns.contains_key(&1));assert!(!state.charge_recovery_times.contains_key(&1));assert_eq!(state.skill_charges[&1],3);
        assert_eq!(state.skill_cooldowns[&2],6000);assert_eq!(state.charge_recovery_times[&2],4000);assert_eq!(state.skill_charges[&2],1);
        assert_eq!(state.skill_use_counts[&1],7);assert_eq!(state.combo_phases[&1],2);
        assert_eq!(queue.pop().unwrap().time_ms,2000);
        apply_trigger_effect(&TriggerEffect::ResetSkillState{skill_id:1,max_charges:3},0,&mut state,&mut queue,&mut rng,1000);assert_eq!(state.skill_charges[&1],3);
    }

    #[test]
    fn apl_countdown_boundaries_reselect_at_exact_millisecond() {
        use crate::profile::CompareOperator::{Lt, Lte, Eq, Ne, Gt, Gte};
        for (op, expected) in [(Lte, 5000), (Lt, 5001), (Eq, 5000), (Ne, 5000), (Gt, 5000), (Gte, 5001)] {
            assert_eq!(countdown_boundary(10000, op, 5000, 0), Some(expected));
        }
        assert_eq!(countdown_boundary(5000, Eq, 5000, 5000), Some(5001));
        assert_eq!(countdown_boundary(0, Lt, 0, 0), None);
        assert_eq!(countdown_boundary(10000, Lte, 20000, 0), None);

        for (operator, expected) in [(Lte, 5.0), (Lt, 5.001), (Eq, 5.0)] {
            let mut profiles = crate::builder::build(include_str!("../../fixtures/arcana-representative-request.json")).unwrap();
            profiles.global_triggers.clear();
            for skill in &mut profiles.skills {
                skill.resource_costs.clear();
                skill.cast_times_ms = vec![100];
                skill.cooldown_ms = 10000;
            }
            let first = profiles.skills[0].skill_id;
            let second = profiles.skills[1].skill_id;
            profiles.apl_actions = vec![
                AplAction { skill_id: first, conditions: vec![AplCondition::TimeRemaining { operator: Eq, value_ms: 20000 }] },
                AplAction { skill_id: second, conditions: vec![
                    AplCondition::SkillCooldownRemaining { skill_id: first, operator, value_ms: 5000 },
                    AplCondition::SkillCooldownRemaining { skill_id: first, operator: Gt, value_ms: 0 },
                ] },
            ];
            let trace = crate::engine::trace_one(&profiles, 42, 20000).1;
            assert_eq!(trace.casts.iter().find(|cast| cast.skill_id == second).unwrap().time, expected);
        }
    }

    #[test]
    fn apl_variant_uses_selected_skill_for_implicit_conditions_and_rechecks_state() {
        let mut profiles = crate::builder::build(include_str!("../../fixtures/arcana-representative-request.json")).unwrap();
        let mut base = profiles.skills[0].clone();
        base.skill_id = 0;
        base.tags = vec![SkillTag::SkillId(19000)];
        base.resource_costs.clear();
        let mut variant = base.clone();
        variant.skill_id = 1;
        variant.tags = vec![SkillTag::SkillId(19270)];
        for hit in &mut base.hits { hit.triggers.clear(); }
        variant.hits.clear();
        base.hits[0].triggers.push(crate::profile::HitTrigger {
            on: HitTriggerOn::OnHit, chance: 1.0,
            effect: TriggerEffect::ApplyTargetBuffStacks {
                spec: BuffSpec { id: "arcana_ruin_stack".into(), effects: HashMap::new(), max_stacks: 4, stack_type: StackType::Refresh, duration_ms: 10000 }, stacks: 2,
            },
        });
        profiles.skills = vec![base, variant];
        profiles.apl_actions = vec![AplAction { skill_id: 0, conditions: vec![
            AplCondition::CooldownReady,
            AplCondition::SkillTargetStackGain { debuff_id: "arcana_ruin_stack".into(), operator: crate::profile::CompareOperator::Eq, value: 0.0 },
        ] }];
        let index = [(0, 0), (1, 1)].into_iter().collect();
        let mut state = ActorState::new(100., Box::new(ArcanaState::new(100., 10000., vec![])));
        state.skill_cooldowns.insert(0, 10000);
        state.buff_manager.apply(BuffSpec { id: "190970".into(), effects: HashMap::new(), max_stacks: 1, stack_type: StackType::Refresh, duration_ms: 1000 }, 1000, 1);
        assert_eq!(choose_apl_action(&profiles, &index, &state, 0, 20000), Some((1, 1)));
        state.skill_cooldowns.clear();
        state.buff_manager.expire("190970");
        assert_eq!(choose_apl_action(&profiles, &index, &state, 0, 20000), None);
        state.skill_cooldowns.insert(0, 10000);
        assert_eq!(choose_apl_action(&profiles, &index, &state, 1000, 20000), None);
    }

    #[test]
    fn apl_buff_and_card_countdowns_reserve_boundaries_after_refresh() {
        let mut profiles = crate::builder::build(include_str!("../../fixtures/arcana-representative-request.json")).unwrap();
        profiles.apl_actions.clear();
        profiles.card_apl_actions = vec![crate::profile::CardAplAction { card_id: 1, card_name: "test".into(), conditions: vec![
            AplCondition::BuffRemaining { buff_id: "test".into(), operator: crate::profile::CompareOperator::Lt, value_ms: 5000 },
        ] }];
        let mut state = ActorState::new(100., Box::new(ArcanaState::new(100., 10000., vec![])));
        let spec = BuffSpec { id: "test".into(), effects: HashMap::new(), max_stacks: 1, stack_type: StackType::Refresh, duration_ms: 10000 };
        state.buff_manager.apply(spec.clone(), 10000, 1);
        assert_eq!(next_timed_condition_time(&profiles, &state, 0, 30000), 5001);
        state.buff_manager.apply(spec, 15000, 1);
        assert_eq!(next_timed_condition_time(&profiles, &state, 5000, 30000), 10001);
    }

    #[test]
    fn hp_gains_use_actor_maximum_and_clamp_without_changing_class_resources() {
        let mut state=ActorState::new(10000.0,Box::new(ArcanaState::new(100.0,100.0,vec![])));
        let mut queue=EventQueue::new();let mut rng=Rng::new(42);state.hp=1000.0;
        apply_trigger_effect(&TriggerEffect::GainResource{resource:"Hp".into(),amount:2000.0},0,&mut state,&mut queue,&mut rng,0);
        assert_eq!(state.hp,3000.0);
        apply_trigger_effect(&TriggerEffect::GainResourcePercent{resource:"Hp".into(),percent:0.5},0,&mut state,&mut queue,&mut rng,0);
        assert_eq!(state.hp,8000.0);
        gain_resource(&mut state,&mut rng,"Hp",4000.0);assert_eq!(state.hp,10000.0);
        gain_resource(&mut state,&mut rng,"Hp",2000.0);assert_eq!(state.hp,10000.0);
        state.set_resource("Mp",10.0);gain_resource(&mut state,&mut rng,"Mp",5.0);assert_eq!(state.get_resource("Mp"),15.0);
    }

    #[test]
    fn absorption_heal_shares_hit_cooldown_including_overheal_and_resets_per_actor() {
        let mut profiles=crate::builder::build(include_str!("../../fixtures/reaper-thirst-reference-request.json")).unwrap();
        profiles.global_triggers=vec![crate::profile::GlobalTrigger{filter:TriggerFilter::AnyHit,condition:TriggerCondition::Always,effect:TriggerEffect::GainResource{resource:"Hp".into(),amount:2000.0},cooldown_ms:Some(20000)}];
        let mut state=ActorState::new(10000.0,Box::new(crate::engine::state::class::reaper::ReaperState::new(100.0,crate::profile::reaper::ReaperScenario::ThirstChaosSustain)));
        let mut queue=EventQueue::new();let mut rng=Rng::new(42);state.hp=1000.0;
        fire_global_triggers(&profiles,TriggerContext::SkillCast{skill_id:1},&mut state,&mut queue,&mut rng,0);assert_eq!(state.hp,1000.0);
        for (time,expected) in [(0,3000.0),(0,3000.0),(1,3000.0),(19999,3000.0),(20000,5000.0)] {
            fire_global_triggers(&profiles,TriggerContext::HitEvent{skill_id:1,tags:&[],is_crit:false},&mut state,&mut queue,&mut rng,time);assert_eq!(state.hp,expected);
        }
        state.hp=10000.0;
        fire_global_triggers(&profiles,TriggerContext::HitEvent{skill_id:2,tags:&[],is_crit:true},&mut state,&mut queue,&mut rng,40000);assert_eq!(state.hp,10000.0);
        state.hp=1000.0;
        fire_global_triggers(&profiles,TriggerContext::HitEvent{skill_id:2,tags:&[],is_crit:true},&mut state,&mut queue,&mut rng,59999);assert_eq!(state.hp,1000.0);
        fire_global_triggers(&profiles,TriggerContext::HitEvent{skill_id:2,tags:&[],is_crit:true},&mut state,&mut queue,&mut rng,60000);assert_eq!(state.hp,3000.0);
        let mut fresh=ActorState::new(10000.0,Box::new(crate::engine::state::class::reaper::ReaperState::new(100.0,crate::profile::reaper::ReaperScenario::ThirstChaosSustain)));fresh.hp=1000.0;
        fire_global_triggers(&profiles,TriggerContext::HitEvent{skill_id:1,tags:&[],is_crit:false},&mut fresh,&mut queue,&mut rng,0);assert_eq!(fresh.hp,3000.0);
    }

    #[test]
    fn reaper_chaos_countdown_condition_wakes_before_expiry_without_other_actions() {
        let mut profiles=crate::builder::build(include_str!("../../fixtures/reaper-thirst-reference-request.json")).unwrap();
        let distortion=profiles.skills.iter_mut().find(|s|reaper_game_id(s)==crate::engine::state::class::reaper::DISTORTION).unwrap();
        distortion.cooldown_ms=10000;let id=distortion.skill_id;
        let fatal=profiles.skills.iter().find(|s|reaper_game_id(s)==crate::engine::state::class::reaper::FATAL_STEP).unwrap().skill_id;
        profiles.apl_actions=vec![AplAction{skill_id:fatal,conditions:vec![]},AplAction{skill_id:id,conditions:vec![AplCondition::Resource {resource:"ReaperChaosRemaining".into(),operator:crate::profile::CompareOperator::Lte,value:5.}]}];
        let trace=crate::engine::trace_one(&profiles,42,40000).1;
        assert_eq!(trace.casts.iter().filter(|c|c.skill_id==id).take(3).map(|c|c.time).collect::<Vec<_>>(),vec![10.,20.,30.]);
    }

    #[test]
    fn reaper_chaos_countdown_condition_keeps_strict_fractional_and_zero_boundaries() {
        use crate::profile::CompareOperator::{Lt,Lte,Eq,Ne,Gt,Gte};
        let mut profiles=crate::builder::build(include_str!("../../fixtures/reaper-thirst-reference-request.json")).unwrap();
        let state=ActorState::new(100.,Box::new(crate::engine::state::class::reaper::ReaperState::new(100.,crate::profile::reaper::ReaperScenario::ThirstChaosSustain)));
        for (operator,value,expected) in [(Lte,5.,10000),(Lt,5.,10001),(Eq,5.,10000),(Lte,5.0001,10000),(Lt,5.0001,10000),(Eq,5.0001,u64::MAX),(Lte,0.,15000),(Lt,0.,u64::MAX),(Lte,-1.,u64::MAX),(Lte,20.,u64::MAX),(Gt,5.,u64::MAX),(Gte,5.,u64::MAX),(Ne,15.,1)] {
            profiles.apl_actions=vec![AplAction{skill_id:0,conditions:vec![AplCondition::Resource{resource:"ReaperChaosRemaining".into(),operator,value}]}];
            assert_eq!(next_timed_condition_time(&profiles,&state,0,40000),expected,"{operator:?} {value}");
        }
        profiles.apl_actions=vec![AplAction{skill_id:0,conditions:vec![AplCondition::Resource{resource:"ReaperDark".into(),operator:Lte,value:5.}]}];
        assert_eq!(next_timed_condition_time(&profiles,&state,0,40000),u64::MAX);
        let mut reaper=crate::engine::state::class::reaper::ReaperState::new(100.,crate::profile::reaper::ReaperScenario::ThirstChaosSustain);reaper.advance(10000);let mut at_boundary=ActorState::new(100.,Box::new(reaper));at_boundary.current_time=10000;
        profiles.apl_actions=vec![AplAction{skill_id:0,conditions:vec![AplCondition::Resource{resource:"ReaperChaosRemaining".into(),operator:Lt,value:5.}]}];
        let index=profiles.skills.iter().enumerate().map(|(i,s)|(s.skill_id,i)).collect();let queue=EventQueue::new();
        assert_eq!(next_action_check_time(&profiles,&index,&at_boundary,&queue,10000,40000),10001);
        let mut arcana=ActorState::new(100.,Box::new(ArcanaState::new(100.,10000.,vec![])));arcana.current_time=10000;
        assert_eq!(next_timed_condition_time(&profiles,&arcana,10000,40000),u64::MAX);
    }

    #[test]
    fn flat_cooldown_reduction_clamps_running_timers_and_preserves_ready_skills() {
        let mut state=ActorState::new(100.0,Box::new(crate::engine::state::class::reaper::ReaperState::new(100.0,crate::profile::reaper::ReaperScenario::ThirstChaosSustain)));
        state.skill_cooldowns.insert(1,1500);
        state.charge_recovery_times.insert(1,2200);
        state.skill_cooldowns.insert(2,900);
        state.skill_cooldowns.insert(3,9000);
        let mut queue=EventQueue::new();
        apply_trigger_effect(&TriggerEffect::ReduceCooldownsFlat {skill_ids:vec![1,2,4],amount_ms:1900},0,&mut state,&mut queue,&mut Rng::new(42),1000);
        assert_eq!(state.skill_cooldowns[&1],1000);
        assert_eq!(state.charge_recovery_times[&1],1000);
        assert_eq!(state.skill_cooldowns[&2],900);
        assert_eq!(state.skill_cooldowns[&3],9000);
        assert!(!state.skill_cooldowns.contains_key(&4));
        assert!(!state.charge_recovery_times.contains_key(&4));
    }

    #[test]
    fn card_effect_rechecks_next_skill_before_using_the_remaining_card() {
        use crate::profile::apl::CardAplAction;

        let mut profiles = crate::builder::build(include_str!(
            "../../fixtures/arcana-representative-request.json"
        )).unwrap();
        for skill in &mut profiles.skills {
            skill.resource_costs.clear();
        }
        let first = profiles.skills[0].skill_id;
        let fallback = profiles.skills[1].skill_id;
        let skill_index = profiles.skills.iter().enumerate()
            .map(|(i, skill)| (skill.skill_id, i)).collect();
        profiles.apl_actions = vec![
            AplAction { skill_id: first, conditions: vec![AplCondition::BuffActive("after_card".into())] },
            AplAction { skill_id: fallback, conditions: Vec::new() },
        ];
        profiles.card_apl_actions = vec![
            CardAplAction { card_id: 999, card_name: "미보유".into(), conditions: vec![AplCondition::NextSkill(first)] },
            CardAplAction { card_id: 19098, card_name: "두 번째".into(), conditions: vec![AplCondition::NextSkill(first)] },
            CardAplAction { card_id: 19097, card_name: "첫 번째".into(), conditions: vec![AplCondition::NextSkill(fallback)] },
        ];
        profiles.global_triggers.clear();
        profiles.arcana_card_blocking_buffs.clear();
        profiles.arcana_card_effects.clear();
        profiles.arcana_card_effects.insert(19097, TriggerEffect::ApplyBuff(BuffSpec {
            id: "after_card".into(), effects: HashMap::new(), max_stacks: 1,
            stack_type: StackType::Refresh, duration_ms: 1000,
        }));
        let mut arcana = ArcanaState::new(100.0, 10000.0, Vec::new());
        arcana.cards = vec![19098, 19097];
        let mut state = ActorState::new(100.0, Box::new(arcana));
        let mut queue = EventQueue::new();
        let mut rng = Rng::new(42);
        let mut result = SimResult::default();
        let mut trace = Some(TraceData::default());

        use_matching_cards(&profiles, &skill_index, &mut state, &mut queue,
            &mut rng, 0, 180000, &mut result, &mut trace);

        assert_eq!(trace.unwrap().card_uses.iter().map(|card| card.card_id)
            .collect::<Vec<_>>(), vec![19097, 19098]);
        assert!(!state.class_state.has_cards());
    }

    #[test]
    fn redraw_card_effect_is_not_recorded_as_its_own_copy_target() {
        assert!(copies_last_used_card(&TriggerEffect::Sequence(vec![
            TriggerEffect::DrawLastUsedCard,
        ])));
        assert!(!copies_last_used_card(&TriggerEffect::DrawCard));
    }

    #[test]
    fn wheel_restores_one_consumed_charge() {
        for initial_charges in [0, 1] {
        let mut state = ActorState::new(
            100.0,
            Box::new(ArcanaState::new(100.0, 100.0, Vec::new())),
        );
        state.skill_charges.insert(7, initial_charges);
        state.skill_cooldowns.insert(7, 500);
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

        assert_eq!(state.skill_charges[&7], initial_charges + 1);
        assert!(!state.skill_cooldowns.contains_key(&7));
        assert_eq!(state.charge_recovery_times.contains_key(&7), initial_charges == 0);
        if initial_charges == 0 { assert_eq!(state.charge_recovery_times[&7], 1000); }
        assert!(!state.buff_manager.is_active("190950", 0));
        }
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

#[cfg(test)]
mod sorceress_expiry_tests {
    use super::*;
    use crate::profile::BuffSpec;
    fn mp_at(duration:u32,kind:&str)->f64 {
        let mut p=crate::builder::build(include_str!("../../fixtures/reaper-thirst-reference-request.json")).unwrap();
        p.stat=crate::profile::StatProfile{base_attack_power:10000.,..Default::default()};
        if kind=="fast_action" {p.stat.insert(crate::profile::StatField::AttackSpeed,"test",0.3);}
        p.stat.bake();
        p.skills.truncate(1);p.apl_order=vec![p.skills[0].skill_id];p.apl_actions.clear();
        if kind=="clock_action" {p.skills[0].cast_buff_clock_multipliers.push(("clock".into(),0.7));}
        let s=&mut p.skills[0];s.resource_costs.clear();s.resource_gains.clear();s.max_uses=1;s.cast_times_ms=vec![2000];s.timed_effects.clear();
        for h in &mut s.hits {h.triggers.clear();}
        let gain=TriggerEffect::GainResource{resource:"Mp".into(),amount:10.};
        let expiry=TriggerEffect::ApplyBuffWithExpiry{spec:BuffSpec{id:"source".into(),effects:Default::default(),max_stacks:1,stack_type:crate::profile::StackType::Refresh,duration_ms:1000},on_expire:Box::new(gain.clone())};
        let mut sequence=vec![expiry.clone()];
        if kind=="consumed" {sequence.push(TriggerEffect::Schedule{delay_ms:500,effect:Box::new(TriggerEffect::ConsumeBuffStacks{buff_id:"source".into(),stacks:1})});}
        if kind=="refreshed" {sequence.push(TriggerEffect::Schedule{delay_ms:500,effect:Box::new(expiry)});}
        if kind.ends_with("action") {sequence=vec![TriggerEffect::AfterCurrentAction{effect:Box::new(gain)}];}
        p.global_triggers=vec![crate::profile::GlobalTrigger{filter:TriggerFilter::SkillCastId(p.skills[0].skill_id),condition:TriggerCondition::Always,effect:TriggerEffect::Sequence(sequence),cooldown_ms:Some(u32::MAX)}];
        let class=crate::engine::state::class::sorceress::SorceressState::new(100.,50.,0.,0.).unwrap();let mut state=ActorState::new(100.,Box::new(class));
        if kind=="clock_action" {state.buff_manager.apply(BuffSpec{id:"clock".into(),effects:Default::default(),max_stacks:1,stack_type:crate::profile::StackType::Refresh,duration_ms:3000},3000,1);}
        research_one(&p,42,duration,&mut state,false);state.get_resource("Mp")
    }
    #[test]fn expiry_chain_runs_once_only_for_current_unconsumed_token(){
        assert_eq!(mp_at(999,"natural"),50.);assert_eq!(mp_at(1000,"natural"),60.);assert_eq!(mp_at(2000,"natural"),60.);
        assert_eq!(mp_at(2000,"consumed"),50.);assert_eq!(mp_at(1000,"refreshed"),50.);assert_eq!(mp_at(1499,"refreshed"),50.);assert_eq!(mp_at(1500,"refreshed"),60.);
    }
    #[test]fn after_action_uses_the_actual_ready_deadline(){
        assert_eq!(mp_at(1999,"action"),50.);assert_eq!(mp_at(2000,"action"),60.);
        assert_eq!(mp_at(1537,"fast_action"),50.);assert_eq!(mp_at(1538,"fast_action"),60.);
        assert_eq!(mp_at(1399,"clock_action"),50.);assert_eq!(mp_at(1400,"clock_action"),60.);
    }
}
#[cfg(test)]
mod cast_dot_tests {
    use super::*;
    use crate::profile::{BuffSpec, RuntimeDamageSpec, StackType};
    fn run(bonus: bool, timed: bool, independent: bool) -> TraceData {
        let mut p=crate::builder::build(include_str!("../../fixtures/reaper-thirst-reference-request.json")).unwrap();
        p.stat=crate::profile::StatProfile{base_attack_power:10000.,..Default::default()};p.stat.bake();
        p.skills.truncate(1);p.apl_order=vec![p.skills[0].skill_id];p.apl_actions.clear();p.global_triggers.clear();
        let s=&mut p.skills[0];s.max_uses=1;s.resource_costs.clear();s.resource_gains.clear();s.timed_effects.clear();s.cast_times_ms=vec![500];
        s.hits.truncate(1);s.hits[0].triggers.clear();s.hits[0].phase=0;s.hits[0].action_delay_ms=0;s.hits[0].fixed_delay_ms=100;s.hits[0].use_snapshot=false;s.hits[0].crit_chance_bonus=0.;
        s.cast_buff_damage_bonuses=if bonus {vec![("ready".into(),0.3)]}else{vec![]};
        s.cast_buff_crit_rate_bonuses=if bonus {vec![("ready".into(),0.25)]}else{vec![]};
        let dot=TriggerEffect::ScheduleDot{dot_id:"cast_probe".into(),hit:RuntimeDamageSpec{destruction:0,hit_id:"cast_probe".into(),name:"probe".into(),base_damage:10.,skill_modifier:0.2,damage_increase:0.,crit_chance_bonus:0.,crit_damage_bonus:0.,random_crit_damage_chance:0.,random_crit_damage_bonus:0.,defense_ignore:0.,defense_ignore_chance:0.},first_tick_ms:500,tick_interval_ms:1000,tick_count:3,refresh:true,independent};
        if timed {s.timed_effects.push(crate::profile::skill::TimedSkillEffect{phase:0,action_delay_ms:100,until_cast_end:false,action_end_ms:None,effect:TriggerEffect::Schedule{delay_ms:50,effect:Box::new(dot)}});}
        else {s.hits[0].triggers.push(crate::profile::HitTrigger{on:crate::profile::HitTriggerOn::OnHit,chance:1.,effect:dot});}
        p.global_triggers.push(crate::profile::GlobalTrigger{filter:TriggerFilter::SkillCastId(s.skill_id),condition:TriggerCondition::Always,effect:TriggerEffect::ConsumeBuffStacks{buff_id:"ready".into(),stacks:1},cooldown_ms:None});
        let class=crate::engine::state::class::sorceress::SorceressState::new(100.,50.,0.,0.).unwrap();let mut state=ActorState::new(100.,Box::new(class));
        state.buff_manager.apply(BuffSpec{id:"ready".into(),effects:Default::default(),max_stacks:1,stack_type:StackType::Refresh,duration_ms:1000},1000,1);
        research_one(&p,42,4000,&mut state,true).1
    }
    #[test]
    fn consumed_cast_enhancement_reaches_source_dots_but_not_independent_dots(){
        for timed in [false,true] {for independent in [false,true] {
            let base=run(false,timed,independent);let boosted=run(true,timed,independent);
            let dots:Vec<_>=boosted.damage_events.iter().filter(|h|h.damage_source=="dot:cast_probe").collect();assert_eq!(dots.len(),3);
            let direct=boosted.damage_events.iter().find(|h|h.damage_source=="skill_hit").unwrap();let plain=base.damage_events.iter().find(|h|h.damage_source=="skill_hit").unwrap();
            assert!((direct.formula.damage_increase_multiplier-plain.formula.damage_increase_multiplier-0.3).abs()<1e-12);
            for h in dots {let b=base.damage_events.iter().find(|b|b.time==h.time&&b.damage_source==h.damage_source).unwrap();assert!((h.formula.crit_rate-b.formula.crit_rate-if independent {0.}else{0.25}).abs()<1e-12);let delta=h.formula.damage_increase_multiplier-b.formula.damage_increase_multiplier;assert!((delta-if independent {0.}else{0.3}).abs()<1e-12,"timed={timed} independent={independent} delta={delta}");}
        }}
    }
}
