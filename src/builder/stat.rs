use crate::profile::{StatField, StatProfile};

// ---------------------------------------------------------------------------
// StatBuilder — 클로저 큐 기반 스탯 빌더
// ---------------------------------------------------------------------------

type StatOp = Box<dyn FnOnce(&mut StatProfile)>;

/// 각 소스(장비/악세/팔찌 등)가 외부에서 클로저를 등록한다.
/// 모든 register 완료 후 build()로 StatProfile을 확정한다.
///
/// ## Phase 순서
/// - **Phase 0 flat**:       합산 — flat 고정값 (기본값 0.0 필드, `+=`)
/// - **Phase 1 additive**:   합산 — 비율 배율 (기본값 1.0 `_mul` 필드, `+=`)
/// - **Phase 2 multiply**:   곱산 — 피해 증가 계열 (기본값 1.0 필드, `*=`)
///
/// 같은 필드에 합산(Phase 1)과 곱산(Phase 2)이 섞이면 순서가 결과에 영향을 미치므로
/// 반드시 단계를 구분해야 한다.
pub struct StatBuilder {
    flat_ops: Vec<(String, StatOp)>,
    additive_ops: Vec<(String, StatOp)>,
    multiply_ops: Vec<(String, StatOp)>,
}

impl StatBuilder {
    pub fn new() -> Self {
        Self {
            flat_ops: Vec::new(),
            additive_ops: Vec::new(),
            multiply_ops: Vec::new(),
        }
    }

    /// Phase 0: flat 고정값 등록 (장비/악세/스톤/팔찌 기본 스탯, 전투 특성 등)
    pub fn add_flat(
        &mut self,
        source: impl Into<String>,
        f: impl FnOnce(&mut StatProfile) + 'static,
    ) {
        self.flat_ops.push((source.into(), Box::new(f)));
    }

    /// Phase 1: additive multiplier 등록 (`_mul` 필드, `+=` 합산)
    /// 예: `s.main_stat_mul += 0.08` (8% 보너스)
    pub fn add_additive(
        &mut self,
        source: impl Into<String>,
        f: impl FnOnce(&mut StatProfile) + 'static,
    ) {
        self.additive_ops.push((source.into(), Box::new(f)));
    }

    /// 소스 기반 버킷에 delta를 삽입한다.
    /// Phase 0(flat)으로 등록 — HashMap insert이므로 위상 순서 무관.
    /// 같은 source 키로 두 번 등록하면 마지막 값으로 덮어쓴다.
    pub fn add_stat(&mut self, field: StatField, source: impl Into<String>, delta: f64) {
        let source = source.into();
        self.flat_ops.push((
            source.clone(),
            Box::new(move |s: &mut StatProfile| s.insert(field, source.clone(), delta)),
        ));
    }

    /// Phase 2: multiplicative 등록 (`*=` 독립 곱)
    // pub fn add_multiply(
    //     &mut self,
    //     source: impl Into<String>,
    //     f: impl FnOnce(&mut StatProfile) + 'static,
    // ) {
    //     self.multiply_ops.push((source.into(), Box::new(f)));
    // }

    /// 등록된 클로저를 phase 순서대로 실행해 StatProfile을 확정한다.
    pub fn build(self) -> Result<StatProfile, String> {
        let mut profile = StatProfile::default();

        for (_, op) in self.flat_ops {
            op(&mut profile);
        }
        for (_, op) in self.additive_ops {
            op(&mut profile);
        }
        for (_, op) in self.multiply_ops {
            op(&mut profile);
        }

        profile.bake();
        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn equipment(stat: &mut StatBuilder) {
        stat.add_flat("equipment", |s| s.base_main_stat += 1000.0);
        stat.add_stat(StatField::WeaponAttackPower, "equipment", 500.0);
    }

    fn accessory(stat: &mut StatBuilder) {
        stat.add_additive("accessory", |s| s.weapon_ap_mul += 0.03);
        stat.add_stat(StatField::AttackPowerMul, "accessory", 0.02);
    }

    fn bracelet(stat: &mut StatBuilder) {
        stat.add_stat(StatField::DamageIncrease, "bracelet", 0.04);
    }

    fn gem(stat: &mut StatBuilder) {
        stat.add_additive("gem", |s| s.base_ap_mul += 0.012);
    }

    fn engraving(stat: &mut StatBuilder) {
        stat.add_stat(StatField::TargetDamageIncrease, "engraving", 0.20);
    }

    #[test]
    fn static_source_registration_order_does_not_change_stats() {
        let mut forward = StatBuilder::new();
        equipment(&mut forward);
        accessory(&mut forward);
        bracelet(&mut forward);
        gem(&mut forward);
        engraving(&mut forward);

        let mut reverse = StatBuilder::new();
        engraving(&mut reverse);
        gem(&mut reverse);
        bracelet(&mut reverse);
        accessory(&mut reverse);
        equipment(&mut reverse);

        let forward = forward.build().unwrap();
        let reverse = reverse.build().unwrap();
        assert_eq!(forward.base_main_stat, reverse.base_main_stat);
        assert_eq!(forward.weapon_ap_mul, reverse.weapon_ap_mul);
        assert_eq!(forward.base_ap_mul, reverse.base_ap_mul);
        for field in [
            StatField::WeaponAttackPower,
            StatField::AttackPowerMul,
            StatField::DamageIncrease,
            StatField::TargetDamageIncrease,
        ] {
            assert_eq!(forward.sum(field), reverse.sum(field));
        }
    }
}
