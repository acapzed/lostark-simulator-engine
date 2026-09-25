use hashbrown::HashMap;

use crate::profile::{skill::StackType, BuffSpec};

/// 활성 버프 단일 항목
#[derive(Debug, Clone)]
pub struct Buff {
    pub id: String,
    /// 버프 만료 시각 (ms)
    pub expire_time: u64,
    pub stacks: u32,
    /// 버프 명세 — snapshot 시 effects를 참조한다
    pub spec: BuffSpec,
}

/// 활성 버프 전체 관리
#[derive(Debug, Clone, Default)]
pub struct BuffManager {
    buffs: HashMap<String, Buff>,
}

impl BuffManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 버프를 적용하고 실제로 설정된 만료 시각(expire_time 토큰)을 반환한다.
    /// BuffExpire 이벤트 스케줄에 이 토큰을 사용하면 갱신된 버프를 낡은 이벤트가
    /// 잘못 제거하는 문제를 방지할 수 있다.
    pub fn apply(&mut self, spec: BuffSpec, expire_time: u64, stacks_to_add: u32) -> u64 {
        let max_stacks = spec.max_stacks;
        let stack_type = spec.stack_type.clone();

        let entry = self.buffs.entry(spec.id.clone()).or_insert_with(|| Buff {
            id: spec.id.clone(),
            expire_time,
            stacks: 0,
            spec: spec.clone(),
        });

        match stack_type {
            StackType::Refresh => {
                // 스택 합산, 최대치 제한. 타이머는 새 만료 시각으로 갱신.
                entry.stacks = (entry.stacks + stacks_to_add).min(max_stacks);
                entry.expire_time = expire_time;
            }
            StackType::Extend => {
                entry.stacks = (entry.stacks + stacks_to_add).min(max_stacks);
                entry.expire_time = entry.expire_time.max(expire_time);
            }
            StackType::Independent => {
                entry.stacks = stacks_to_add;
                entry.expire_time = expire_time;
            }
        }
        entry.spec = spec;
        entry.expire_time
    }

    pub fn is_active(&self, id: &str, current_time: u64) -> bool {
        self.buffs
            .get(id)
            .map(|b| b.expire_time > current_time)
            .unwrap_or(false)
    }

    /// 지정 버프가 현재 최대 중첩에 도달해 있는지 확인한다.
    pub fn is_at_max_stacks(&self, id: &str, current_time: u64) -> bool {
        self.buffs
            .get(id)
            .filter(|b| b.expire_time > current_time)
            .map(|b| b.stacks >= b.spec.max_stacks)
            .unwrap_or(false)
    }

    pub fn remaining_ms(&self, id: &str, current_time: u64) -> u64 {
        self.buffs
            .get(id)
            .filter(|b| b.expire_time > current_time)
            .map(|b| b.expire_time - current_time)
            .unwrap_or(0)
    }

    pub fn stacks(&self, id: &str, current_time: u64) -> u32 {
        self.buffs
            .get(id)
            .filter(|b| b.expire_time > current_time)
            .map(|b| b.stacks)
            .unwrap_or(0)
    }

    /// 저장된 만료 시각이 토큰과 일치할 때만 버프를 제거한다.
    /// 버프가 갱신(Refresh)되어 expire_time이 바뀐 경우 낡은 이벤트는 무시된다.
    pub fn expire_if(&mut self, id: &str, expire_at: u64) {
        if let Some(b) = self.buffs.get(id) {
            if b.expire_time == expire_at {
                self.buffs.remove(id);
            }
        }
    }

    pub fn expire(&mut self, id: &str) {
        self.buffs.remove(id);
    }

    pub fn consume_stacks(&mut self, id: &str, stacks: u32, current_time: u64) {
        let Some(buff) = self.buffs.get_mut(id) else {
            return;
        };
        if buff.expire_time <= current_time || buff.stacks <= stacks {
            self.buffs.remove(id);
        } else {
            buff.stacks -= stacks;
        }
    }

    /// snapshot 시 활성 버프의 (spec, stacks) 이터레이터 반환
    pub fn active_buffs(&self, current_time: u64) -> impl Iterator<Item = (&BuffSpec, u32)> {
        self.buffs
            .values()
            .filter(move |b| b.expire_time > current_time)
            .map(|b| (&b.spec, b.stacks))
    }

    /// Trace 모드용 — 현재 활성 버프 목록 반환
    pub fn active_ids(&self, current_time: u64) -> Vec<String> {
        self.buffs
            .values()
            .filter(|b| b.expire_time > current_time)
            .map(|b| b.id.clone())
            .collect()
    }
}
