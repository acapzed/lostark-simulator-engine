use super::ClassState;
use crate::engine::rng::Rng;
use crate::profile::actor::ArcanaCardPoolEntry;

pub const CARD_GAUGE_MAX: f64 = 10_000.0;

/// 아르카나 런타임 상태
#[derive(Debug, Clone)]
pub struct ArcanaState {
    pub mp: f64,
    pub max_mp: f64,
    pub card_gauge: f64,
    pub max_card_gauge: f64,
    pub ultimate_point: f64,
    pub cards: Vec<u32>, // 현재 보유 카드 skill ID
    card_pool: Vec<ArcanaCardPoolEntry>,
    last_used_card: Option<u32>,
}

impl ArcanaState {
    pub fn new(max_mp: f64, max_card_gauge: f64, card_pool: Vec<ArcanaCardPoolEntry>) -> Self {
        Self {
            mp: max_mp,
            max_mp,
            card_gauge: 0.0,
            max_card_gauge,
            ultimate_point: 0.0,
            cards: Vec::new(),
            card_pool,
            last_used_card: None,
        }
    }
}

impl ClassState for ArcanaState {
    fn clone_box(&self) -> Box<dyn ClassState> {
        Box::new(self.clone())
    }

    fn get_resource(&self, name: &str) -> f64 {
        match name {
            "Mp" => self.mp,
            "CardGauge" => self.card_gauge,
            "UltimatePoint" => self.ultimate_point,
            _ => 0.0,
        }
    }

    fn set_resource(&mut self, name: &str, value: f64) {
        match name {
            "Mp" => self.mp = value.clamp(0.0, self.max_mp),
            "CardGauge" => self.card_gauge = value.clamp(0.0, self.max_card_gauge),
            "UltimatePoint" => self.ultimate_point = value.clamp(0.0, 10000.0),
            _ => {}
        }
    }

    fn get_resource_max(&self, name: &str) -> f64 {
        match name {
            "Mp" => self.max_mp,
            "CardGauge" => self.max_card_gauge,
            "UltimatePoint" => 10000.0,
            _ => 0.0,
        }
    }

    fn draw_card(&mut self, rng: &mut Rng) {
        if self.card_pool.is_empty() || self.cards.len() >= 2 {
            return;
        }
        let total_weight: f64 = self
            .card_pool
            .iter()
            .filter(|card| !self.cards.contains(&card.skill_id))
            .map(|card| card.draw_weight.max(0.0))
            .sum();
        if total_weight <= 0.0 {
            return;
        }
        let mut roll = rng.next_f64() * total_weight;
        for card in &self.card_pool {
            if self.cards.contains(&card.skill_id) {
                continue;
            }
            roll -= card.draw_weight.max(0.0);
            if roll < 0.0 {
                self.cards.push(card.skill_id);
                return;
            }
        }
    }

    fn has_cards(&self) -> bool {
        !self.cards.is_empty()
    }

    fn has_card(&self, card_id: u32) -> bool {
        self.cards.contains(&card_id)
    }

    fn card_count(&self, card_id: u32) -> usize {
        self.cards.iter().filter(|held| **held == card_id).count()
    }

    fn held_cards(&self) -> Vec<u32> {
        self.cards.clone()
    }

    fn use_card(&mut self, card_id: u32) -> bool {
        let Some(index) = self.cards.iter().position(|held| *held == card_id) else {
            return false;
        };
        self.cards.remove(index);
        true
    }

    fn remember_card_use(&mut self, card_id: u32) {
        self.last_used_card = Some(card_id);
    }

    fn draw_last_used_card(&mut self) {
        if self.cards.len() < 2 {
            if let Some(card_id) = self.last_used_card {
                self.cards.push(card_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_only_from_the_raw_card_pool() {
        let mut state = ArcanaState::new(100.0, 100.0, vec![
            ArcanaCardPoolEntry { skill_id: 19097, draw_weight: 1.0 },
            ArcanaCardPoolEntry { skill_id: 19280, draw_weight: 1.0 },
        ]);
        let mut rng = Rng::new(1);

        for _ in 0..20 {
            state.draw_card(&mut rng);
        }

        assert_eq!(state.cards.len(), 2);
        assert!(state.cards.iter().all(|id| [19097, 19280].contains(id)));
    }

    #[test]
    fn uses_card_draw_weights() {
        let mut state = ArcanaState::new(100.0, 100.0, vec![
            ArcanaCardPoolEntry { skill_id: 19097, draw_weight: 0.0 },
            ArcanaCardPoolEntry { skill_id: 19280, draw_weight: 1.0 },
        ]);
        let mut rng = Rng::new(1);

        state.draw_card(&mut rng);

        assert_eq!(state.cards, vec![19280]);
    }

    #[test]
    fn does_not_draw_a_card_already_in_hand() {
        let mut state = ArcanaState::new(100.0, 100.0, vec![
            ArcanaCardPoolEntry { skill_id: 19097, draw_weight: 100.0 },
            ArcanaCardPoolEntry { skill_id: 19280, draw_weight: 1.0 },
        ]);
        let mut rng = Rng::new(1);

        state.draw_card(&mut rng);
        state.draw_card(&mut rng);

        assert_eq!(state.cards.len(), 2);
        assert_ne!(state.cards[0], state.cards[1]);
    }

    #[test]
    fn reports_a_specific_card_in_hand() {
        let mut state = ArcanaState::new(100.0, 100.0, Vec::new());
        state.cards.push(19098);
        state.cards.push(19098);

        assert!(state.has_card(19098));
        assert!(!state.has_card(19097));
        assert_eq!(state.card_count(19098), 2);
    }

    #[test]
    fn redraws_the_last_remembered_card_without_exceeding_two_slots() {
        let mut state = ArcanaState::new(100.0, 100.0, Vec::new());
        state.remember_card_use(19098);
        state.draw_last_used_card();
        state.draw_last_used_card();
        state.draw_last_used_card();

        assert_eq!(state.cards, vec![19098, 19098]);
    }

    #[test]
    fn clamps_the_super_awakening_gauge_to_its_raw_cost() {
        let mut state = ArcanaState::new(100.0, 100.0, Vec::new());
        state.set_resource("UltimatePoint", 10_001.0);

        assert_eq!(state.get_resource("UltimatePoint"), 10_000.0);
        assert_eq!(state.get_resource_max("UltimatePoint"), 10_000.0);
    }

    #[test]
    fn clamps_card_gauge_to_the_raw_tutorial_threshold() {
        let mut state = ArcanaState::new(100.0, CARD_GAUGE_MAX, Vec::new());
        state.set_resource("CardGauge", CARD_GAUGE_MAX + 1.0);

        assert_eq!(state.get_resource("CardGauge"), CARD_GAUGE_MAX);
        assert_eq!(state.get_resource_max("CardGauge"), CARD_GAUGE_MAX);
    }
}
