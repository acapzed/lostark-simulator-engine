use crate::builder::{class, dto::BuildRequest, modifier::ModifierManager, skill::SkillPipelineMap, stat::StatBuilder};
use crate::builder::register::sim_effects;
use crate::profile::skill::GlobalTrigger;

pub fn register(
    stat: &mut StatBuilder,
    modifier_manager: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    req: &BuildRequest,
) {
    let Some(grids) = &req.character.ark_grids else { return };

    // ── 젬 (공통) — 데이터 드리븐, 레벨 기반 선택 ──────────────────────
    for gem in &grids.gems {
        let level_idx = gem.level.saturating_sub(1) as usize;
        let Some(effects) = gem.effects_by_level.get(level_idx) else { continue };
        if effects.is_empty() { continue; }
        let source = format!("ark_grid:gem:{}:L{}", gem.slot, gem.level);
        sim_effects::apply_effect_list(stat, modifier_manager, global_triggers, req, effects, &source);
    }

    // ── 혼돈 코어 (공통) — 데이터 드리븐, 포인트 필터링 ────────────────
    let mut identity_gain_percent = 0.0;
    let mut cooldown_reduction_percent = 0.0;
    for core in &grids.common_cores {
        for opt in &core.options {
            if core.points < opt.point_requirement { continue; }
            if opt.sim_effects.is_empty() { continue; }
            for effect in &opt.sim_effects {
                match effect.kind.as_str() {
                    "identity_gain_multiplier" => identity_gain_percent += effect.value,
                    "normal_skill_cooldown_reduction" => cooldown_reduction_percent += effect.value,
                    _ => {}
                }
            }
            let source = format!("ark_grid:common:{}:p{}", core.name, opt.point_requirement);
            sim_effects::apply_effect_list(stat, modifier_manager, global_triggers, req, &opt.sim_effects, &source);
        }
    }
    // ── 질서 코어 (클래스 전용) — 클래스 코드 위임 ───────────────────────
    class::register_ark_grid(
        stat,
        modifier_manager,
        global_triggers,
        skill_pipelines,
        &grids.class_cores,
        req,
    );
    if identity_gain_percent > 0.0 {
        sim_effects::apply_card_gauge_multiplier(skill_pipelines, req, identity_gain_percent);
    }
    if cooldown_reduction_percent > 0.0 {
        sim_effects::apply_normal_skill_cooldown_reduction(
            skill_pipelines,
            req,
            cooldown_reduction_percent,
        );
    }
}
