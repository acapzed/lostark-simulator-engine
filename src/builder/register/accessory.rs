use crate::builder::dto::BuildRequest;
use crate::builder::stat::StatBuilder;
use crate::profile::StatField;

pub fn register(stat: &mut StatBuilder, req: &BuildRequest) {
    // 인덱스를 key에 포함해 같은 효과를 가진 여러 악세서리가 서로 덮어쓰지 않도록 함
    for (i, acc) in req.character.accessories.iter().enumerate() {
        let main = acc.base_stats.main_stat;
        let vit = acc.base_stats.vitality;
        stat.add_flat(format!("accessory[{i}]:기본스탯"), move |s| {
            s.base_main_stat += main;
            s.base_vitality += vit;
        });

        for effect in &acc.polishing_effects {
            let val = effect.value;
            match effect.effect_type.as_str() {
                "공격력" => {
                    if effect.is_percentage {
                        stat.add_stat(StatField::AttackPowerMul, format!("accessory[{i}]:공격력%"), val / 100.0);
                    } else {
                        stat.add_flat(format!("accessory[{i}]:공격력"), move |s| s.base_attack_power += val);
                    }
                }
                "무기 공격력" => {
                    if effect.is_percentage {
                        stat.add_additive(format!("accessory[{i}]:무기공격력%"), move |s| {
                            s.weapon_ap_mul += val / 100.0
                        });
                    } else {
                        stat.add_stat(StatField::WeaponAttackPower, format!("accessory[{i}]:무기공격력"), val);
                    }
                }
                "추가 피해" => stat.add_stat(StatField::AdditionalDamage,     format!("accessory[{i}]:추가피해"), val / 100.0),
                "치명타 적중률" => stat.add_stat(StatField::CritRate,         format!("accessory[{i}]:치적률"),   val),
                "치명타 피해"   => stat.add_stat(StatField::CritDmg,          format!("accessory[{i}]:치적피해"), val),
                "적에게 주는 피해" => stat.add_stat(StatField::TargetDamageIncrease, format!("accessory[{i}]:적주피"), val / 100.0),

                "최대 마나" => {
                    stat.add_flat(format!("accessory[{i}]:최대마나"), move |s| s.max_mana += val)
                }

                "최대 생명력" => stat.add_flat(format!("accessory[{i}]:최대HP"), move |s| s.max_hp += val),

                "상태이상 공격 지속시간"
                | "전투 중 생명력 회복량"
                | "세레나데, 신앙, 조화 게이지 획득량"
                | "낙인력"
                | "파티원 회복 효과"
                | "파티원 보호막 효과"
                | "아군 공격력 강화 효과"
                | "아군 피해량 강화 효과" => {}

                _ => {
                    #[cfg(debug_assertions)]
                    eprintln!("[accessory] 알 수 없는 연마 효과: {}", effect.effect_type);
                }
            }
        }
    }
}
