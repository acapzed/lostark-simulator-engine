pub mod ark_grid;
pub mod ark_passive;

use crate::builder::dto::{ArkGridClassCoreInput, ArkPassiveClassNodeInput, BuildRequest};
use crate::builder::modifier::ModifierManager;
use crate::builder::skill::SkillPipelineMap;
use crate::builder::stat::StatBuilder;
use crate::profile::GlobalTrigger;

pub(super) const ARCANA_RUIN_STACK_BUFF_ID: &str = "arcana_ruin_stack";

/// 아크 패시브 클래스 전용 노드를 적용한다.
pub fn apply_ark_passive(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    class_nodes: &[ArkPassiveClassNodeInput],
    req: &BuildRequest,
) {
    ark_passive::apply(stat, modifier, global_triggers, skill_pipelines, class_nodes, req);
}

/// 아크 그리드 질서 코어를 적용한다.
pub fn apply_ark_grid(
    stat: &mut StatBuilder,
    modifier: &mut ModifierManager,
    global_triggers: &mut Vec<GlobalTrigger>,
    skill_pipelines: &mut SkillPipelineMap,
    class_cores: &[ArkGridClassCoreInput],
    req: &BuildRequest,
) {
    ark_grid::apply(
        stat,
        modifier,
        global_triggers,
        skill_pipelines,
        class_cores,
        req,
    );
}
