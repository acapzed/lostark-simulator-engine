use std::collections::BinaryHeap;
use crate::engine::snapshot::CastSnapshot;
use crate::profile::{RuntimeDamageSpec, TriggerEffect};

/// 시뮬레이션 이벤트 payload
#[derive(Debug)]
pub enum EventData {
    ReaperIncomingDamage { damage: f64 },
    SkillEnd { skill_id: u32, cast_id: u32 },
    ReaperExpire,
    /// APL이 다음 행동을 결정하는 시점
    ActionCheck,
    /// 스킬 Hit — 캐스트 시점 snapshot을 inline(내부)에 보유함.
    SkillHit {
        skill_id: u32,
        cast_id: u32,
        hit_index: usize,
        snap: CastSnapshot,
    },
    /// 버프 만료.
    /// expire_at: 이 이벤트를 스케줄한 시점의 만료 시각.
    /// 버프가 갱신(Refresh)되면 expire_time이 바뀌므로 낡은 이벤트는 무시된다.
    BuffExpire {
        buff_id: String,
        expire_at: u64,
    },
    BuffExpireEffect {
        buff_id: String,
        expire_at: u64,
        skill_id: u32,
        effect: TriggerEffect,
    },
    /// 대상에게 걸린 버프/디버프 만료.
    TargetBuffExpire {
        buff_id: String,
        expire_at: u64,
    },
    /// DoT 틱
    DotTick {
        dot_id: String,
        target_stack_buff_id: Option<String>,
        independent: bool,
        cast_snapshot: Option<Box<CastSnapshot>>,
        skill_id: u32,
        cast_id: Option<u32>,
        hit: RuntimeDamageSpec,
        remaining_ticks: u32,
        tick_interval_ms: u32,
    },
    /// 충전형 스킬 충전 회복
    ChargeRecovery {
        skill_id: u32,
    },
    ScheduledTrigger {
        cast_snapshot: Option<Box<CastSnapshot>>,
        skill_id: u32,
        effect: TriggerEffect,
    },
}

/// 이벤트 우선순위 상수 — 동시각 이벤트 처리 순서 (값이 낮을수록 먼저)
pub mod priority {
    pub const SCHEDULED_TRIGGER: u8 = 0;
    pub const SKILL_HIT: u8 = 1;
    pub const TIMED_SKILL_EFFECT: u8 = 2; // +1ms 동작 Notify가 정수 시각에서 겹쳐도 타격 후 실행.
    pub const DOT_TICK: u8 = 2;
    pub const BUFF_EXPIRE: u8 = 3;
    pub const CHARGE_RECOVERY: u8 = 4;
    pub const SKILL_END: u8 = 5; // 마지막 타격 후, 다음 행동 판단 전.
    pub const ACTION_CHECK: u8 = 10;
}

pub struct Event {
    pub time_ms: u64,
    pub priority: u8,
    pub data: EventData,
}

struct QueuedEvent {
    event: Event,
    sequence: u64,
}

impl PartialEq for QueuedEvent {
    fn eq(&self, other: &Self) -> bool {
        self.event.time_ms == other.event.time_ms
            && self.event.priority == other.event.priority
            && self.sequence == other.sequence
    }
}

impl Eq for QueuedEvent {}

impl PartialOrd for QueuedEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueuedEvent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap은 max-heap → 역순 비교로 min-heap (시간 오름차순)
        other.event.time_ms.cmp(&self.event.time_ms)
            .then(other.event.priority.cmp(&self.event.priority))
            .then(other.sequence.cmp(&self.sequence))
    }
}

/// 우선순위 이벤트 큐 (min-heap: 시간 오름차순)
pub struct EventQueue(BinaryHeap<QueuedEvent>, u64, u64, bool);

impl EventQueue {
    pub fn new() -> Self {
        Self(BinaryHeap::new(), 0, 0, false)
    }

    pub fn push(&mut self, event: Event) {
        self.0.push(QueuedEvent { event, sequence: self.1 });
        self.1 += 1;
    }

    pub fn schedule_action_check(&mut self, time_ms: u64) {
        // ponytail: 이벤트 큐가 작을 때의 O(n) 병합. 큐 크기가 병목이면 토큰 방식으로 교체한다.
        let mut earliest = self
            .0
            .iter()
            .filter_map(|queued| {
                matches!(queued.event.data, EventData::ActionCheck).then_some(queued.event.time_ms)
            })
            .min()
            .map_or(time_ms, |queued| queued.min(time_ms));
        if self.3 {earliest=earliest.max(self.2);}
        self.0
            .retain(|queued| !matches!(queued.event.data, EventData::ActionCheck));
        self.push(Event {
            time_ms: earliest,
            priority: priority::ACTION_CHECK,
            data: EventData::ActionCheck,
        });
    }

    pub fn pop(&mut self) -> Option<Event> {
        self.0.pop().map(|queued| queued.event)
    }

    pub fn set_actor_action_ready(&mut self,time_ms:u64){self.2=time_ms;}
    pub fn actor_action_ready(&self)->u64{self.2}
    pub fn respect_action_clock(&mut self){self.3=true;}

    pub fn schedule_reaper_expire(&mut self, time_ms: u64) {
        if !self.0.iter().any(|queued| queued.event.time_ms == time_ms && matches!(queued.event.data, EventData::ReaperExpire)) {
            self.push(Event { time_ms, priority: priority::BUFF_EXPIRE, data: EventData::ReaperExpire });
        }
    }

    pub fn peek_time(&self) -> Option<u64> {
        self.0.peek().map(|queued| queued.event.time_ms)
    }

    pub fn remove_dot(&mut self, dot_id: &str) {
        self.0.retain(|queued| !matches!(
            &queued.event.data,
            EventData::DotTick { dot_id: queued, .. } if queued == dot_id
        ));
    }

    // pub fn is_empty(&self) -> bool {
    //     self.0.is_empty()
    // }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_earliest_action_check() {
        let mut queue = EventQueue::new();
        queue.schedule_action_check(100);
        queue.schedule_action_check(200);
        queue.schedule_action_check(50);

        assert_eq!(queue.pop().unwrap().time_ms, 50);
        assert!(queue.pop().is_none());
    }

    #[test]
    fn opted_in_action_clock_rechecks_future_wakeups_after_intervening_casts(){
        let mut queue=EventQueue::new();queue.respect_action_clock();queue.set_actor_action_ready(1000);queue.schedule_action_check(1000);
        assert_eq!(queue.pop().unwrap().time_ms,1000);queue.schedule_action_check(1500);
        queue.set_actor_action_ready(2000);queue.schedule_action_check(2000);assert_eq!(queue.pop().unwrap().time_ms,2000);
        queue.schedule_action_check(2500);assert_eq!(queue.pop().unwrap().time_ms,2500);
    }

    #[test]
    fn keeps_insertion_order_for_equal_time_and_priority() {
        let mut queue = EventQueue::new();
        for skill_id in [1, 2, 3] {
            queue.push(Event {
                time_ms: 100,
                priority: priority::CHARGE_RECOVERY,
                data: EventData::ChargeRecovery { skill_id },
            });
        }
        let ids: Vec<_> = (0..3).map(|_| match queue.pop().unwrap().data {
            EventData::ChargeRecovery { skill_id } => skill_id,
            _ => unreachable!(),
        }).collect();
        assert_eq!(ids, [1, 2, 3]);
    }
}
