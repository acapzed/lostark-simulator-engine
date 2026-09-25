use crate::builder::dto::BuildRequest;
use crate::builder::modifier::{Modifier, ModifierManager, OpType};
use crate::builder::stat::StatBuilder;
use crate::profile::{SkillTag, StatField};

pub fn register(stat: &mut StatBuilder, modifier: &mut ModifierManager, req: &BuildRequest) {
    let Some(karma) = &req.character.ark_passive_karma else { return };

    let evo_dmg    = karma.evolution.rank  as f64 * 1.0;
    // karma.leap.rank        → 도약 포인트 (2P/랭크) — 게임 내 자원, DPS 무관
    // karma.enlightenment.rank → 깨달음 포인트 (1P/랭크) — 게임 내 자원, DPS 무관
    let hyper_dmg  = karma.leap.level      as f64 * 0.5;
    let weapon_pct = karma.enlightenment.level as f64 * 0.1;

    if evo_dmg > 0.0 {
        stat.add_stat(StatField::EvolutionDamage, "karma:진화형피해", evo_dmg / 100.0);
    }

    // ArkPassiveStigma LevelOption Stat 104 = ultimate_awakening_dam_rate.
    if hyper_dmg > 0.0 {
        modifier.add_modifier(Modifier {
            target_tag: SkillTag::CategorySuperAwakening,
            stat_name:  "DamageMultiplier".to_string(),
            value:      hyper_dmg / 100.0,
            op_type:    OpType::Multiply,
            source:     "karma:초각성피해".to_string(),
        });
    }

    if weapon_pct > 0.0 {
        stat.add_additive("karma:무기공%", move |s| s.weapon_ap_mul += weapon_pct / 100.0);
    }
}
