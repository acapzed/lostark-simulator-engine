use crate::builder::dto::BuildRequest;
use crate::builder::stat::StatBuilder;

pub fn register(stat: &mut StatBuilder, req: &BuildRequest) {
    let Some(stone) = &req.character.stone else {
        return;
    };

    let vitality = stone.base_stats.vitality + stone.bonus_stats.vitality;
    if vitality > 0.0 {
        stat.add_flat("stone:체력", move |s| s.base_vitality += vitality);
    }

    // ── 97돌 이상 기본 공격력 +1.50% ──────────────────────────────────────
    // 각 증가 각인의 단계 합이 5 이상이면 D. 기본 공격력 +1.50%
    // (보석과 동일하게 base_ap_mul 버킷에 누적)
    let stage_sum: u32 = req
        .character
        .engravings
        .iter()
        .map(|e| e.ability_level)
        .sum();
    if stage_sum >= 5 {
        stat.add_additive("stone:97돌기본공격력".to_string(), |s| {
            s.base_ap_mul += 0.015
        });
    }
}
