use crate::builder::dto::BuildRequest;
use crate::builder::stat::StatBuilder;
use crate::profile::StatField;

pub fn register(stat: &mut StatBuilder, req: &BuildRequest) {
    if !req.enable_pet_buff {
        return;
    }

    match req.pet_buff_type.as_str() {
        "치명" => stat.add_flat("pet:버프", |s| s.crit += 160.0),
        "특화" => stat.add_flat("pet:버프", |s| s.specialization += 160.0),
        "신속" => stat.add_flat("pet:버프", |s| s.swiftness += 160.0),
        "제압" => stat.add_flat("pet:버프", |s| s.domination += 160.0),
        "인내" => stat.add_flat("pet:버프", |s| s.endurance += 160.0),
        "숙련" => stat.add_flat("pet:버프", |s| s.expertise += 160.0),
        _ => {}
    }

    let add = req.pet_talents.additional_damage;
    if add > 0.0 {
        stat.add_stat(StatField::AdditionalDamage, "pet:추가피해", add / 100.0);
    }
    let type_dmg = req.pet_talents.type_damage;
    if type_dmg > 0.0 {
        stat.add_stat(StatField::TypeDamage, "pet:계열피해", type_dmg / 100.0);
    }

    let main_pct = req.pet_talents.main_stat_percent;
    if main_pct > 0.0 {
        stat.add_additive("pet:주스탯%", move |s| {
            s.main_stat_mul += main_pct / 100.0
        });
    }
}
