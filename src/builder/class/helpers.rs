use crate::builder::skill::SkillPipeline;

/// 모든 히트의 damage_increase를 bonus만큼 누적한다.
pub fn damage_increase(pipeline: &mut SkillPipeline, bonus: f64) {
    pipeline.add_stats(move |skill| {
        for hit in &mut skill.hits {
            hit.damage_increase += bonus;
        }
        Ok(())
    });
}

/// cooldown_ms를 rate 비율만큼 감소시킨다.
pub fn cooldown_reduction(pipeline: &mut SkillPipeline, rate: f64) {
    pipeline.add_stats(move |skill| {
        let reduce_ms = (skill.cooldown_ms as f64 * rate) as u32;
        skill.cooldown_ms = skill.cooldown_ms.saturating_sub(reduce_ms);
        Ok(())
    });
}

/// 모든 cast_times_ms를 rate 비율만큼 감소시킨다.
pub fn cast_time_reduction(pipeline: &mut SkillPipeline, rate: f64) {
    pipeline.add_stats(move |skill| {
        for ct in &mut skill.cast_times_ms {
            *ct = (*ct as f64 * (1.0 - rate)) as u32;
        }
        Ok(())
    });
}

/// Mp 비용을 rate 비율만큼 감소시킨다.
pub fn mana_reduction(pipeline: &mut SkillPipeline, rate: f64) {
    pipeline.add_stats(move |skill| {
        if let Some(cost) = skill.resource_costs.get_mut("Mp") {
            *cost *= 1.0 - rate;
        }
        Ok(())
    });
}

/// 모든 히트의 defense_ignore를 value만큼 누적한다.
pub fn defense_ignore(pipeline: &mut SkillPipeline, value: f64) {
    pipeline.add_stats(move |skill| {
        for hit in &mut skill.hits {
            hit.defense_ignore += value;
        }
        Ok(())
    });
}

/// 특정 phase의 히트에 crit_chance_bonus를 누적한다.
pub fn crit_chance_bonus(pipeline: &mut SkillPipeline, bonus: f64, phase: Option<u32>) {
    pipeline.add_stats(move |skill| {
        for hit in &mut skill.hits {
            if phase.map_or(true, |p| hit.phase == p) {
                hit.crit_chance_bonus += bonus;
            }
        }
        Ok(())
    });
}

/// 특정 phase의 히트에 crit_damage_bonus를 누적한다.
pub fn crit_damage_bonus(pipeline: &mut SkillPipeline, bonus: f64, phase: Option<u32>) {
    pipeline.add_stats(move |skill| {
        for hit in &mut skill.hits {
            if phase.map_or(true, |p| hit.phase == p) {
                hit.crit_damage_bonus += bonus;
            }
        }
        Ok(())
    });
}
