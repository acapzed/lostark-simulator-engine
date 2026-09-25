use hashbrown::HashMap;

use crate::profile::skill::SkillTag;

// ---------------------------------------------------------------------------
// CompiledModifiers — build 과정의 중간 산물
// ModifierManager::compile() 결과물. CharacterProfiles에는 포함되지 않는다.
// ---------------------------------------------------------------------------

/// Tag → StatName → 최종 수치
#[derive(Debug, Clone, Default)]
pub struct CompiledModifiers {
    pub stats: HashMap<SkillTag, HashMap<String, f64>>,
}

impl CompiledModifiers {
    /// 곱연산 스탯 조회 (기본값 1.0)
    pub fn get_multiplier(&self, tag: &SkillTag, stat_name: &str) -> f64 {
        self.stats
            .get(tag)
            .and_then(|m| m.get(stat_name))
            .copied()
            .unwrap_or(1.0)
    }

    /// 합연산 스탯 조회 (기본값 0.0)
    pub fn get_additive(&self, tag: &SkillTag, stat_name: &str) -> f64 {
        self.stats
            .get(tag)
            .and_then(|m| m.get(stat_name))
            .copied()
            .unwrap_or(0.0)
    }
}

// ---------------------------------------------------------------------------
// Modifier 연산 종류
// ---------------------------------------------------------------------------

/// 로스트아크 ModifierManager 연산 순서
///
/// 동일 (태그, 스탯) 그룹 내 처리 순서:
///   1. OpAdd        → 합산  (flat_sum)
///   2. OpAddPercent → 합산 후 (1 + sum) 인수로 변환  (add_percent_factor)
///   3. OpMultiply   → 누적 곱 (1+a)×(1+b)×…  (multiply_product)
///   4. OpOverride   → 있으면 1~3 결과를 덮어쓰기
///
/// 최종값 = flat_sum + add_percent_factor × multiply_product
#[derive(Debug, Clone, PartialEq)]
pub enum OpType {
    /// 절댓값 합산 (기본값에 더해지는 delta)
    Add,
    /// 퍼센트 합산. 여러 개가 있으면 sum 후 (1 + sum) 인수로 변환
    AddPercent,
    /// 독립 곱연산. (1 + value) 씩 누적 곱
    Multiply,
    /// 1~3 단계를 무시하고 지정 값으로 덮어쓰기
    Override,
}

// ---------------------------------------------------------------------------
// Modifier (단일 수식표 항목)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Modifier {
    /// 적용 대상 스킬 태그
    pub target_tag: SkillTag,
    /// 수정할 스탯 이름 ("DamageMultiplier", "CritChanceBonus", …)
    pub stat_name: String,
    pub value: f64,
    pub op_type: OpType,
    /// 디버깅·추적용 소스 이름 ("ArkPassive_황후의속삭임", "ArkGrid_A", …)
    pub source: String,
}

// ---------------------------------------------------------------------------
// ModifierManager (장바구니 + compile)
// ---------------------------------------------------------------------------

/// 아크패시브 / 아크그리드 / 장비 효과를 Modifier로 수집하고
/// `compile()` 으로 일괄 계산하여 `CompiledModifiers` 를 생성한다.
///
/// # 사용 순서
/// ```text
/// let mut mm = ModifierManager::new();
/// mm.add_modifier(Modifier { … });  // Phase 4 진입 전에 모두 추가
/// let compiled = mm.compile();       // compile 이후 add_modifier 불가
/// ```
pub struct ModifierManager {
    modifiers: Vec<Modifier>,
}

impl ModifierManager {
    pub fn new() -> Self {
        Self {
            modifiers: Vec::new(),
        }
    }

    /// Modifier를 장바구니에 추가한다.
    pub fn add_modifier(&mut self, modifier: Modifier) {
        self.modifiers.push(modifier);
    }

    /// 로아 공식 순서로 모든 Modifier를 일괄 계산하여 `CompiledModifiers`를 생성한다.
    ///
    /// ## 연산 순서 (docs/damage-formula.md)
    ///
    /// 동일 `(target_tag, stat_name)` 그룹에서:
    ///
    /// ```text
    /// flat_sum           = Σ OpAdd.value
    /// add_percent_factor = 1 + Σ OpAddPercent.value
    /// multiply_product   = Π (1 + OpMultiply.value)
    ///
    /// result = flat_sum + add_percent_factor × multiply_product
    /// → OpOverride 있으면 override.value 로 덮어씀
    /// ```
    pub fn compile(self) -> CompiledModifiers {
        // (SkillTag, stat_name) 별로 Modifier 분류
        // SkillTag가 Clone + PartialEq + Eq + Hash 이므로 HashMap 키로 사용 가능
        let mut groups: HashMap<(SkillTag, String), Vec<Modifier>> = HashMap::new();

        for m in self.modifiers {
            groups
                .entry((m.target_tag.clone(), m.stat_name.clone()))
                .or_default()
                .push(m);
        }

        let mut stats: HashMap<SkillTag, HashMap<String, f64>> = HashMap::new();

        for ((tag, stat_name), mods) in groups {
            // 1. OpAdd → 합산
            let flat_sum: f64 = mods
                .iter()
                .filter(|m| m.op_type == OpType::Add)
                .map(|m| m.value)
                .sum();

            // 2. OpAddPercent → 합산 후 (1 + sum) 인수로 변환
            let add_percent_factor: f64 = 1.0
                + mods
                    .iter()
                    .filter(|m| m.op_type == OpType::AddPercent)
                    .map(|m| m.value)
                    .sum::<f64>();

            // 3. OpMultiply → (1+a)×(1+b)×… 누적 곱
            let multiply_product: f64 = mods
                .iter()
                .filter(|m| m.op_type == OpType::Multiply)
                .map(|m| 1.0 + m.value)
                .product();

            // 최종값 = flat_sum + add_percent_factor × multiply_product
            let mut result = flat_sum + add_percent_factor * multiply_product;

            // 4. OpOverride → 있으면 위 결과를 덮어쓰기 (마지막 값 우선)
            if let Some(ov) = mods.iter().rev().find(|m| m.op_type == OpType::Override) {
                result = ov.value;
            }

            stats
                .entry(tag)
                .or_default()
                .insert(stat_name, result);
        }

        CompiledModifiers { stats }
    }
}

impl Default for ModifierManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 단위 테스트 (로아 합/곱 연산 순서)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_modifier(tag: SkillTag, stat: &str, value: f64, op: OpType) -> Modifier {
        Modifier {
            target_tag: tag,
            stat_name: stat.to_string(),
            value,
            op_type: op,
            source: "test".to_string(),
        }
    }

    // 테스트 편의 헬퍼 — 특정 태그/스탯의 컴파일 결과 조회
    fn get(compiled: &CompiledModifiers, tag: &SkillTag, stat: &str) -> f64 {
        compiled
            .stats
            .get(tag)
            .and_then(|m| m.get(stat))
            .copied()
            .unwrap_or(f64::NAN)
    }

    /// docs/damage-formula.md 예시:
    /// ArkPassive OpMultiply(0.15) × ArkGrid OpMultiply(0.10)
    /// → 1.15 × 1.10 = 1.265
    #[test]
    fn multiply_two_sources() {
        let mut mm = ModifierManager::new();
        mm.add_modifier(make_modifier(
            SkillTag::CategoryRuin,
            "DamageMultiplier",
            0.15,
            OpType::Multiply,
        ));
        mm.add_modifier(make_modifier(
            SkillTag::CategoryRuin,
            "DamageMultiplier",
            0.10,
            OpType::Multiply,
        ));
        let compiled = mm.compile();

        let result = get(&compiled, &SkillTag::CategoryRuin, "DamageMultiplier");
        // flat_sum=0, add_percent_factor=1.0, multiply_product=1.265
        // result = 0 + 1.0 × 1.265 = 1.265
        assert!(
            (result - 1.265).abs() < 1e-9,
            "expected 1.265, got {result}"
        );

        let mut reversed = ModifierManager::new();
        reversed.add_modifier(make_modifier(
            SkillTag::CategoryRuin,
            "DamageMultiplier",
            0.10,
            OpType::Multiply,
        ));
        reversed.add_modifier(make_modifier(
            SkillTag::CategoryRuin,
            "DamageMultiplier",
            0.15,
            OpType::Multiply,
        ));
        assert_eq!(
            result,
            get(
                &reversed.compile(),
                &SkillTag::CategoryRuin,
                "DamageMultiplier"
            )
        );
    }

    /// OpMultiply 단일 항목 — (1 + 0.20) = 1.20
    #[test]
    fn single_multiply() {
        let mut mm = ModifierManager::new();
        mm.add_modifier(make_modifier(
            SkillTag::CategoryNormal,
            "DamageMultiplier",
            0.20,
            OpType::Multiply,
        ));
        let compiled = mm.compile();

        let result = get(&compiled, &SkillTag::CategoryNormal, "DamageMultiplier");
        // 0 + 1.0 × 1.20 = 1.20
        assert!((result - 1.20).abs() < 1e-9, "expected 1.20, got {result}");
    }

    /// OpAdd 두 항목 — delta 합산 후 base(1.0) 포함
    /// e.g. PowerBonus: +0.1 and +0.05 → 0.15 + 1.0×1.0 = 1.15
    #[test]
    fn add_two_flat() {
        let mut mm = ModifierManager::new();
        mm.add_modifier(make_modifier(
            SkillTag::CategoryNormal,
            "PowerBonus",
            0.10,
            OpType::Add,
        ));
        mm.add_modifier(make_modifier(
            SkillTag::CategoryNormal,
            "PowerBonus",
            0.05,
            OpType::Add,
        ));
        let compiled = mm.compile();

        let result = get(&compiled, &SkillTag::CategoryNormal, "PowerBonus");
        // flat_sum=0.15, add_percent_factor=1.0, multiply_product=1.0
        // result = 0.15 + 1.0×1.0 = 1.15
        assert!((result - 1.15).abs() < 1e-9, "expected 1.15, got {result}");
    }

    /// OpAddPercent 두 항목 — 합산 후 (1+sum) 인수
    /// 0.05 + 0.10 = 0.15 → add_percent_factor = 1.15
    /// result = 0 + 1.15×1.0 = 1.15
    #[test]
    fn add_percent_two_sources() {
        let mut mm = ModifierManager::new();
        mm.add_modifier(make_modifier(
            SkillTag::CategoryNormal,
            "CritBonus",
            0.05,
            OpType::AddPercent,
        ));
        mm.add_modifier(make_modifier(
            SkillTag::CategoryNormal,
            "CritBonus",
            0.10,
            OpType::AddPercent,
        ));
        let compiled = mm.compile();

        let result = get(&compiled, &SkillTag::CategoryNormal, "CritBonus");
        // flat_sum=0, add_percent_factor=1+0.15=1.15, multiply_product=1.0
        // result = 0 + 1.15×1.0 = 1.15
        assert!((result - 1.15).abs() < 1e-9, "expected 1.15, got {result}");
    }

    /// Add + Multiply 혼합
    /// OpAdd(0.1) + OpMultiply(0.2)
    /// flat_sum=0.1, factor=1.0, multiply=1.2
    /// result = 0.1 + 1.0×1.2 = 1.3
    #[test]
    fn add_and_multiply_mixed() {
        let mut mm = ModifierManager::new();
        mm.add_modifier(make_modifier(
            SkillTag::AttackBack,
            "Bonus",
            0.10,
            OpType::Add,
        ));
        mm.add_modifier(make_modifier(
            SkillTag::AttackBack,
            "Bonus",
            0.20,
            OpType::Multiply,
        ));
        let compiled = mm.compile();

        let result = get(&compiled, &SkillTag::AttackBack, "Bonus");
        assert!((result - 1.30).abs() < 1e-9, "expected 1.30, got {result}");
    }

    /// OpOverride — 다른 연산 결과를 완전히 덮어씀
    #[test]
    fn override_wins() {
        let mut mm = ModifierManager::new();
        mm.add_modifier(make_modifier(
            SkillTag::CategoryRuin,
            "DamageMultiplier",
            0.15,
            OpType::Multiply,
        ));
        mm.add_modifier(make_modifier(
            SkillTag::CategoryRuin,
            "DamageMultiplier",
            2.0,
            OpType::Override,
        ));
        let compiled = mm.compile();

        let result = get(&compiled, &SkillTag::CategoryRuin, "DamageMultiplier");
        assert!((result - 2.0).abs() < 1e-9, "expected 2.0 (override), got {result}");
    }

    /// 태그가 다른 Modifier는 독립적으로 컴파일
    #[test]
    fn different_tags_are_independent() {
        let mut mm = ModifierManager::new();
        mm.add_modifier(make_modifier(
            SkillTag::CategoryRuin,
            "DamageMultiplier",
            0.15,
            OpType::Multiply,
        ));
        mm.add_modifier(make_modifier(
            SkillTag::CategoryNormal,
            "DamageMultiplier",
            0.10,
            OpType::Multiply,
        ));
        let compiled = mm.compile();

        let ruin = get(&compiled, &SkillTag::CategoryRuin, "DamageMultiplier");
        let normal = get(&compiled, &SkillTag::CategoryNormal, "DamageMultiplier");

        // 두 태그가 서로 영향 없음
        assert!((ruin - 1.15).abs() < 1e-9, "ruin expected 1.15, got {ruin}");
        assert!((normal - 1.10).abs() < 1e-9, "normal expected 1.10, got {normal}");
    }

    /// 스탯이 없으면 CompiledModifiers에서 조회 시 get_multiplier → 1.0 반환
    #[test]
    fn missing_stat_returns_default() {
        let mm = ModifierManager::new();
        let compiled = mm.compile();

        assert_eq!(
            compiled.get_multiplier(&SkillTag::CategoryRuin, "DamageMultiplier"),
            1.0
        );
        assert_eq!(
            compiled.get_additive(&SkillTag::CategoryRuin, "CritBonus"),
            0.0
        );
    }

    /// AddPercent + Multiply 혼합 — (1+0.1)×(1+0.2) = 1.1×1.2 = 1.32
    #[test]
    fn add_percent_and_multiply() {
        let mut mm = ModifierManager::new();
        mm.add_modifier(make_modifier(
            SkillTag::CategoryStacked,
            "DmgMul",
            0.10,
            OpType::AddPercent,
        ));
        mm.add_modifier(make_modifier(
            SkillTag::CategoryStacked,
            "DmgMul",
            0.20,
            OpType::Multiply,
        ));
        let compiled = mm.compile();

        let result = get(&compiled, &SkillTag::CategoryStacked, "DmgMul");
        // flat_sum=0, add_percent_factor=1.1, multiply_product=1.2
        // result = 0 + 1.1×1.2 = 1.32
        assert!((result - 1.32).abs() < 1e-9, "expected 1.32, got {result}");
    }
}
