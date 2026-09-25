use crate::builder::dto::BuildRequest;
use crate::builder::stat::StatBuilder;
use crate::profile::StatField;

pub fn register(stat: &mut StatBuilder, req: &BuildRequest) {
    let val = req.collection_stats;
    stat.add_flat("collection", move |s| s.base_main_stat += val);

    if req.card_damage_bonus > 0.0 {
        stat.add_stat(
            StatField::ElementalDamage,
            "card:set_damage",
            req.card_damage_bonus / 100.0,
        );
    }
}
