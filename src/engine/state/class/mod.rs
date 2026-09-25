pub mod arcana;
pub mod berserker;

use crate::engine::rng::Rng;

/// 직업별 런타임 상태 인터페이스
///
/// - engine/APL은 get_resource/set_resource만 사용 (직업을 모름)
/// - 직업별 코드는 concrete 타입으로 downcast해서 필드 직접 접근
pub trait ClassState: std::fmt::Debug {
    fn clone_box(&self) -> Box<dyn ClassState>;

    /// 자원 현재값 조회 ("Mp", "Battery", "Fury", "CardGauge" 등)
    /// 해당 자원이 없으면 0.0 반환
    fn get_resource(&self, name: &str) -> f64;

    /// 자원 현재값 설정
    fn set_resource(&mut self, name: &str, value: f64);

    /// 자원 최대값 조회 (회복 계산, APL 퍼센트 조건용)
    fn get_resource_max(&self, name: &str) -> f64;

    /// 카드 뽑기. 기본 no-op — 아르카나만 오버라이드.
    fn draw_card(&mut self, _rng: &mut Rng) {}

    fn has_cards(&self) -> bool { false }

    fn has_card(&self, _card_id: u32) -> bool { false }

    fn card_count(&self, _card_id: u32) -> usize { 0 }

    fn held_cards(&self) -> Vec<u32> { Vec::new() }

    /// 지정 카드 사용. 손패에 없거나 비아르카나면 false.
    fn use_card(&mut self, _card_id: u32) -> bool { false }

    fn remember_card_use(&mut self, _card_id: u32) {}

    fn draw_last_used_card(&mut self) {}
}

impl Clone for Box<dyn ClassState> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}
