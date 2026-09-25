use crate::builder::dto::BuildRequest;
use crate::builder::stat::StatBuilder;
use crate::profile::StatField;

// ── 만찬 수치 (최상급 기준) ──────────────────────────────────────────────────
// TownChef 120001의 일반 공이속 선택지 -> SkillBuff 100410424.
const FEAST_WEAPON_POWER: f64 = 1800.0;
const FEAST_ATTACK_MOVE_SPEED: f64 = 0.05;

// ── 아드로핀 물약 수치 ───────────────────────────────────────────────────────
// damage-formula-data.md E. 최종 공격력 — 공격력(%) 소스
// TODO: 레벨별 수치 확인 필요
// const ADROPHINE_ATTACK_POWER_MUL: f64 = 0.06; // 6% 예시

pub fn register(stat: &mut StatBuilder, req: &BuildRequest) {
    // ── 만찬 ─────────────────────────────────────────────────────────────────
    if req.enable_feast {
        stat.add_stat(
            StatField::WeaponAttackPower,
            "consumable:만찬:무기공격력",
            FEAST_WEAPON_POWER,
        );
        stat.add_stat(
            StatField::AttackSpeed,
            "consumable:만찬:공격속도",
            FEAST_ATTACK_MOVE_SPEED,
        );
        stat.add_stat(
            StatField::MovementSpeed,
            "consumable:만찬:이동속도",
            FEAST_ATTACK_MOVE_SPEED,
        );
    }

    // ── 아드로핀 물약 ────────────────────────────────────────────────────────
    // TODO: enable_adrophine 플래그 및 수치 확정 후 활성화
    // if req.enable_adrophine {
    //     stat.add_stat(StatField::AttackPowerMul, "consumable:아드로핀", ADROPHINE_ATTACK_POWER_MUL);
    // }
}

pub fn initial_card_draws(req: &BuildRequest) -> u32 {
    if req.enable_awakening_potion && req.character.class_name == "아르카나" {
        2
    } else {
        0
    }
}
