mod helpers;
pub mod arcana;

use crate::builder::dto::{ArkGridClassCoreInput, ArkPassiveClassNodeInput, BuildRequest};
use crate::builder::modifier::ModifierManager;
use crate::builder::skill::SkillPipelineMap;
use crate::builder::stat::StatBuilder;
use crate::profile::skill::GlobalTrigger;

/// 아크 패시브 클래스 전용 노드를 등록한다.
/// sim_effects로 표현 불가한 복잡한 메커니즘 (카드 관련, 조건부 버프 등)을 처리한다.
pub fn register_ark_passive(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    class_nodes: &[ArkPassiveClassNodeInput],
    req: &BuildRequest,
) {
    match req.character.class_name.as_str() {
        "아르카나" => arcana::apply_ark_passive(stat, modifier, global_triggers, skill_pipelines, class_nodes, req),
        _ => {}
    }
}

/// 아크 그리드 질서 코어 (클래스 전용)를 등록한다.
pub fn register_ark_grid(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    class_cores: &[ArkGridClassCoreInput],
    req: &BuildRequest,
) {
    match req.character.class_name.as_str() {
        "아르카나" => arcana::apply_ark_grid(
            stat,
            modifier,
            global_triggers,
            skill_pipelines,
            class_cores,
            req,
        ),
        _ => {}
    }
}
