pub mod modifier;
pub mod register;

mod apl;
mod class;
mod dto;
mod skill;
mod stat;

pub use dto::BuildRequest;

use crate::profile::{CharacterProfiles, GlobalTrigger};
use hashbrown::HashMap;
use modifier::ModifierManager;
use skill::SkillPipelineMap;
use stat::StatBuilder;

/// CharacterProfiles 빌드 진입점.
///
/// 전체 파이프라인:
///
/// ```text
/// [큐 생성]
///   StatBuilder      — stat 변경점 큐
///   SkillPipelineMap — 스킬별 변경점 큐
///   ModifierManager  — 태그 기반 modifier 큐
///
/// [register — 각 소스가 필요한 큐에 자유롭게 등록]
///   equipment   → stat
///   accessory   → stat
///   stone       → stat
///   bracelet    → stat
///   avatar      → stat
///   collection  → stat
///   pet         → stat
///   feast       → stat
///   karma       → stat
///   tripod      → skill
///   gem         → skill
///   ark_grid    → stat
///   ark_passive → stat + modifier (태그 기반 modifier)
///
/// [build — 등록 완료 후 일괄 실행]
///   stat_builder.build()        → StatProfile
///   skill_pipelines.build_all() → Vec<SkillProfile>
///   modifier_manager.compile()  → CompiledModifiers
///
/// [의존 단계 — 태그 확정 후]
///   skill::apply_modifiers() → 태그 기반 modifier 수치 적용
///   skill::apply_triggers()  → 런타임 조건부 GlobalTrigger 수집
/// ```
pub fn build(json: &str) -> Result<CharacterProfiles, String> {
    let req: BuildRequest =
        serde_json::from_str(json).map_err(|e| format!("입력 파싱 실패: {e}"))?;
    if req.character.class_name != "아르카나" {
        return Err(format!("지원하지 않는 직업: {}", req.character.class_name));
    }

    // [큐 생성]
    let mut stat_builder = StatBuilder::new();
    let mut skill_pipelines =
        SkillPipelineMap::new(req.character.skills.iter().map(|s| s.name.clone()));
    let mut modifier_manager = ModifierManager::new();
    let mut global_triggers: Vec<GlobalTrigger> = Vec::new();

    // [register]
    register::equipment::register(&mut stat_builder, &req);
    register::accessory::register(&mut stat_builder, &req);
    register::stone::register(&mut stat_builder, &req);
    register::bracelet::register(
        &mut stat_builder,
        &mut modifier_manager,
        &mut global_triggers,
        &req,
    );
    register::avatar::register(&mut stat_builder, &req);
    register::collection::register(&mut stat_builder, &req);
    register::pet::register(&mut stat_builder, &req);
    register::consumable::register(&mut stat_builder, &req);
    register::karma::register(&mut stat_builder, &mut modifier_manager, &req);
    register::engraving::register(
        &mut stat_builder,
        &mut modifier_manager,
        &mut global_triggers,
        &mut skill_pipelines,
        &req,
    )?;
    register::rune::register(&mut skill_pipelines, &mut global_triggers, &req);
    register::gem::register(&mut stat_builder, &mut skill_pipelines, &req);
    register::ark_grid::register(
        &mut stat_builder,
        &mut modifier_manager,
        &mut global_triggers,
        &mut skill_pipelines,
        &req,
    )?;
    register::ark_passive::register(
        &mut stat_builder,
        &mut modifier_manager,
        &mut global_triggers,
        &mut skill_pipelines,
        &req,
    )?;

    // [build]
    let stat = stat_builder.build()?;
    let mut skills = skill_pipelines.build_all(&req.skill_data, &req.character.skills)?;
    if skills.iter().any(|skill| skill.tags.contains(&crate::profile::SkillTag::SkillId(19000)))
        && req.card_data.iter().any(|card| card.runtime_effects.iter().any(|effect| effect.kind == "basic_attack_card_buff"))
        && !skills.iter().any(|skill| skill.tags.contains(&crate::profile::SkillTag::SkillId(19270)))
    {
        return Err("삼두사 기본 공격: 변경 스킬 19270 데이터 없음".to_string());
    }
    class::arcana::apply_specialization(&mut skills, stat.specialization);
    let compiled = modifier_manager.compile();

    // [의존 단계]
    skill::apply_modifiers(&mut skills, &compiled);
    global_triggers.extend(skill::apply_triggers(&req.skill_data, &req.character.skills));

    let apl_profile = apl::build(&req.apl_config, &skills, &req.card_data, &req.buff_data)?;
    let max_mp = stat.max_mana;
    let arcana_card_pool = class::arcana::build_card_pool(&req);
    let (arcana_card_effects, arcana_card_blocking_buffs) =
        class::arcana::card::build_effects(&req, &mut skills, &mut global_triggers)?;
    let mut buff_metadata: HashMap<String, crate::profile::actor::BuffMetadata> = req
        .buff_data
        .iter()
        .map(|buff| (
            buff.id.clone(),
            crate::profile::actor::BuffMetadata {
                name: buff.name.clone(),
                icon: buff.icon.clone(),
                source: buff.source.clone(),
            },
        ))
        .collect();
    for engraving in &req.character.engravings {
        buff_metadata.insert(
            format!("engraving:{}:", engraving.name),
            crate::profile::actor::BuffMetadata {
                name: String::new(),
                icon: engraving.icon.clone(),
                source: format!("각인 · {}", engraving.name),
            },
        );
    }

    Ok(CharacterProfiles {
        name: req.character.name.clone(),
        class_name: req.character.class_name.clone(),
        stat,
        skills,
        max_hp: req.character.stats.max_hp,
        max_mp,
        target_defense: req.target_defense.max(0.0),
        global_triggers,
        apl_order: apl_profile.order,
        apl_actions: apl_profile.actions,
        card_apl_actions: apl_profile.card_actions,
        arcana_card_pool,
        arcana_card_names: req.card_data.iter()
            .map(|card| (card.skill_id, card.name.clone()))
            .collect(),
        initial_card_draws: register::consumable::initial_card_draws(&req),
        arcana_card_effects,
        arcana_card_blocking_buffs,
        buff_metadata,
    })
}
