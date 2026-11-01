use crate::builder::dto::BuildRequest;
use crate::builder::stat::StatBuilder;
use crate::profile::StatField;

pub fn register(stat: &mut StatBuilder, req: &BuildRequest) {
    let val = req.collection_stats;
    stat.add_flat("collection", move |s| s.base_main_stat += val);
    let combat = req.collection_combat_stats;
    stat.add_flat("collection:전투특성", move |s| {
        s.crit += combat.crit as f64;
        s.specialization += combat.specialization as f64;
        s.swiftness += combat.swiftness as f64;
        s.domination += combat.domination as f64;
        s.endurance += combat.endurance as f64;
        s.expertise += combat.expertise as f64;
    });

    if req.collection_type_damage > 0.0 {
        stat.add_stat(StatField::TypeDamage, "collection:카드도감계열피해", req.collection_type_damage / 100.0);
    }

    if req.card_damage_bonus > 0.0 {
        stat.add_stat(
            StatField::ElementalDamage,
            "card:set_damage",
            req.card_damage_bonus / 100.0,
        );
    }
}
