use std::collections::BinaryHeap;
use crate::engine::snapshot::CastSnapshot;
use crate::profile::{RuntimeDamageSpec, TriggerEffect};

/// 시뮬레이션 이벤트 payload
#[derive(Debug)]
pub enum EventData {
    /// APL이 다음 행동을 결정하는 시점
    ActionCheck,
    /// 스킬 Hit — 캐스트 시점 snapshot을 inline(내부)에 보유함.
    SkillHit {
        skill_id: u32,
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
    /// 대상에게 걸린 버프/디버프 만료.
    TargetBuffExpire {
        buff_id: String,
        expire_at: u64,
    },
    /// DoT 틱
    DotTick {
        dot_id: String,
        skill_id: u32,
        hit: RuntimeDamageSpec,
        remaining_ticks: u32,
        tick_interval_ms: u32,
    },
    /// 충전형 스킬 충전 회복
    ChargeRecovery {
        skill_id: u32,
    },
    ScheduledTrigger {
        skill_id: u32,
        effect: TriggerEffect,
    },
}

/// 이벤트 우선순위 상수 — 동시각 이벤트 처리 순서 (값이 낮을수록 먼저)
pub mod priority {
    pub const SCHEDULED_TRIGGER: u8 = 0;
    pub const SKILL_HIT: u8 = 1;
    pub const DOT_TICK: u8 = 2;
    pub const BUFF_EXPIRE: u8 = 3;
    pub const CHARGE_RECOVERY: u8 = 4;
    pub const ACTION_CHECK: u8 = 10;
}

pub struct Event {
    pub time_ms: u64,
    pub priority: u8,
    pub data: EventData,
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.time_ms == other.time_ms && self.priority == other.priority
    }
}

impl Eq for Event {}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Event {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap은 max-heap → 역순 비교로 min-heap (시간 오름차순)
        other.time_ms.cmp(&self.time_ms)
            .then(other.priority.cmp(&self.priority))
    }
}

/// 우선순위 이벤트 큐 (min-heap: 시간 오름차순)
pub struct EventQueue(BinaryHeap<Event>);

impl EventQueue {
    pub fn new() -> Self {
        Self(BinaryHeap::new())
    }

    pub fn push(&mut self, event: Event) {
        self.0.push(event);
    }

    pub fn schedule_action_check(&mut self, time_ms: u64) {
        // ponytail: 이벤트 큐가 작을 때의 O(n) 병합. 큐 크기가 병목이면 토큰 방식으로 교체한다.
        let earliest = self
            .0
            .iter()
            .filter_map(|event| {
                matches!(event.data, EventData::ActionCheck).then_some(event.time_ms)
            })
            .min()
            .map_or(time_ms, |queued| queued.min(time_ms));
        self.0
            .retain(|event| !matches!(event.data, EventData::ActionCheck));
        self.0.push(Event {
            time_ms: earliest,
            priority: priority::ACTION_CHECK,
            data: EventData::ActionCheck,
        });
    }

    pub fn pop(&mut self) -> Option<Event> {
        self.0.pop()
    }

    pub fn peek_time(&self) -> Option<u64> {
        self.0.peek().map(|event| event.time_ms)
    }

    pub fn remove_dot(&mut self, dot_id: &str) {
        self.0.retain(|event| !matches!(
            &event.data,
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
}
