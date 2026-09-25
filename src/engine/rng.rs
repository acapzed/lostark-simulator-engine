/// Xorshift64 의사 난수 생성기 (WASM 호환, 결정적, 외부 의존성 없음)
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // 연속된 iteration seed도 첫 난수부터 충분히 달라지도록 상태를 한 번 섞는다.
        let mut state = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        state = (state ^ (state >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        state = (state ^ (state >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Self { state: state ^ (state >> 31) }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// [0.0, 1.0) 범위의 부동소수점
    pub fn next_f64(&mut self) -> f64 {
        // 상위 53비트를 사용해 f64 정밀도 확보
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// 치명타 판정 (rate: 0.0..1.0)
    pub fn is_crit(&mut self, rate: f64) -> bool {
        self.next_f64() < rate
    }
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn seeds_are_reproducible_and_adjacent_seeds_are_diffused() {
        let first = Rng::new(1).next_f64();
        assert_eq!(first, Rng::new(1).next_f64());
        assert!((first - Rng::new(2).next_f64()).abs() > 0.01);
    }
}
