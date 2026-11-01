use crate::builder::dto::BuildRequest;
use crate::builder::stat::StatBuilder;
use crate::profile::StatField;

fn effective_advanced_honing(equip: &crate::builder::dto::EquipmentInput) -> u32 {
    static RULES: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let rules = RULES.get_or_init(|| serde_json::from_str(include_str!("../../../skill-data/common/backend/equipment-honing.json")).expect("valid equipment honing catalog"));
    if equip.item_slot == "완갑" { return 0; }
    let known = rules["items"][&equip.set_name][&equip.item_slot].as_u64();
    let cap = known.unwrap_or(rules["legacyMaxStage"].as_u64().unwrap()) as u32;
    let stage = equip.advanced_refinement_level.min(cap);
    if known.is_some() && equip.item_level.saturating_sub(stage) < rules["minBaseItemLevel"].as_u64().unwrap() as u32 { 0 } else { stage }
}

pub fn register(stat: &mut StatBuilder, req: &BuildRequest) {
    for equip in &req.character.equipments {
        if equip.set_name.trim().is_empty() { continue; }
        let quality = equip.quality.clamp(0, 100) as u32;
        let advanced = effective_advanced_honing(equip);
        let basic_effect_mul = 1.0
            + req
                .advanced_honing_tables
                .get(&equip.item_slot)
                .map(|rows| {
                    rows.iter()
                        .filter(|row| row.stage <= advanced)
                        .map(|row| row.stage_bonus_stat_rate)
                        .sum::<f64>()
                        / 10_000.0
                })
                .unwrap_or(0.0);
        let row = req
            .equipment_tables
            .get(&equip.set_name)
            .and_then(|slots| slots.get(&equip.item_slot))
            .and_then(|rows| {
                rows.iter().find(|row| {
                    if equip.item_slot == "완갑" {
                        row.refinement_level == equip.refinement_level
                    } else {
                        row.item_level == equip.item_level
                    }
                })
            });

        if equip.item_slot == "무기" {
            let ap = row
                .map(|r| (r.weapon_attack * basic_effect_mul).floor())
                .unwrap_or(0.0);
            let quality_bonus = weapon_quality_bonus(equip.quality);

            stat.add_stat(
                StatField::WeaponAttackPower,
                format!("equipment:{}:무기", equip.set_name),
                ap,
            );
            if quality_bonus > 0.0 {
                stat.add_stat(
                    StatField::AdditionalDamage,
                    format!("equipment:{}:품질추피", equip.set_name),
                    quality_bonus / 100.0,
                );
            }
        } else {
            let weapon_attack = row
                .map(|r| (r.weapon_attack * basic_effect_mul).floor())
                .unwrap_or(0.0);
            let main = row
                .map(|r| (r.main_stat * basic_effect_mul).floor())
                .unwrap_or(0.0);
            let vit = row
                .map(|r| (r.vitality * basic_effect_mul).floor())
                .unwrap_or(0.0)
                + if equip.item_slot == "완갑" {
                    0.0
                } else {
                    armor_quality_vitality(quality)
                };
            let base_ap_bonus = row
                .map(|r| r.base_attack_power_percent)
                .unwrap_or(0.0)
                / 100.0;

            if weapon_attack > 0.0 {
                stat.add_stat(
                    StatField::WeaponAttackPower,
                    format!("equipment:{}:완갑무기공격력", equip.set_name),
                    weapon_attack,
                );
            }
            if base_ap_bonus > 0.0 {
                stat.add_additive(
                    format!("equipment:{}:완갑기본공격력", equip.set_name),
                    move |s| s.base_ap_mul += base_ap_bonus,
                );
            }

            stat.add_flat(
                format!("equipment:{}:{}", equip.set_name, equip.item_slot),
                move |s| {
                    s.base_main_stat += main;
                    s.base_vitality += vit;
                },
            );
        }
    }
}

/// 무기 품질(0~100)에 따른 추가 피해 보너스 (%).
/// 실제 품질 0 → 10%, 품질 100 → 30%. 음수는 품질 미상.
/// 공식: 10 + 0.002 * quality²
fn weapon_quality_bonus(quality: i32) -> f64 {
    if quality < 0 {
        return 0.0;
    }
    let q = quality.min(100) as f64;
    10.0 + 0.002 * q * q
}

/// 방어구 품질(0~100)에 따른 생명 활성력 보너스 (flat 체력).
/// 공식: 140 * 0.001 * quality²
fn armor_quality_vitality(quality: u32) -> f64 {
    140.0 * 0.001 * (quality as f64).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn honing_uses_raw_item_caps_and_minimum_base_level_with_legacy_fallback() {
        for (name,level,advanced,expected) in [
            ("세피로트 넬라",1755,40,40),("세피로트",1640,40,20),
            ("세피로트 도체",1830,40,0),("운명의 전율 마법 덱",1800,40,0),
            ("운명의 업화 마법 덱",1659,40,0),("운명의 업화 마법 덱",1660,40,40),
            ("legacy test weapon",1000,40,40),
        ] {
            let e=serde_json::from_value(serde_json::json!({"type":"무기","setName":name,"quality":100,"itemLevel":level,"refinementLevel":0,"advancedRefinementLevel":advanced})).unwrap();
            assert_eq!(effective_advanced_honing(&e),expected,"{name}");
        }
    }
    #[test]
    fn weapon_quality_zero_is_ten_percent_and_unknown_is_not_zero() {
        for (quality, expected) in [(-1,0.),(0,10.),(1,10.002),(50,15.),(100,30.),(101,30.)] {
            assert!((weapon_quality_bonus(quality)-expected).abs()<1e-12);
        }
    }
}
