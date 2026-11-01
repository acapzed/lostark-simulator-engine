pub mod modifier;
pub mod register;

pub(crate) mod apl;
pub(crate) mod valkyrie;
mod valkyrie_build;
pub(crate) mod slayer_snapshot;
pub(crate) mod berserker_snapshot;
pub(crate) mod sorceress_snapshot;
mod class;
mod dto;
pub mod skill;
mod stat;
mod reaper;
mod reaper_grid;

pub use dto::BuildRequest;

use crate::profile::{CharacterProfiles, GlobalTrigger};
use hashbrown::HashMap;
use modifier::ModifierManager;
use skill::SkillPipelineMap;
pub use stat::StatBuilder;

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
pub fn build_raid(json: &str) -> Result<CharacterProfiles, String> {
    // Public executions always use the fixed raid target. Ignore legacy overrides.
    let mut input: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("입력 파싱 실패: {e}"))?;
    let object = input.as_object_mut().ok_or("입력은 JSON 객체여야 합니다.")?;
    for (key, value) in [
        ("assumedTargetSeedOrLower", false),
        ("assumedTargetAbnormalMoveImmune", true),
        ("assumedTargetNerveVulnerable", false),
        ("assumedTargetSlowVulnerable", false),
    ] {
        object.insert(key.into(), serde_json::Value::Bool(value));
    }
    build(&input.to_string())
}

// Configurable target predicates remain internal for source/effect unit tests.
pub fn build(json: &str) -> Result<CharacterProfiles, String> {
    build_inner(json, false)
}

fn build_inner(json: &str, slayer_snapshot: bool) -> Result<CharacterProfiles, String> {
    let mut canonical: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("입력 파싱 실패: {e}"))?;
    let mut valkyrie = valkyrie_build::resolve(&mut canonical)?;
    let mut req: BuildRequest =
        serde_json::from_value(canonical).map_err(|e| format!("입력 파싱 실패: {e}"))?;
    if !req.character.stats.max_hp.is_finite() || req.character.stats.max_hp < 0.0 {
        return Err("최대 생명력: 0 이상의 유한한 수를 입력해주세요.".into());
    }
    if !req.collection_type_damage.is_finite() || req.collection_type_damage < 0.0 {
        return Err("카드 도감 계열 피해: 0 이상의 유한한 수를 입력해주세요.".into());
    }
    if req.character.class_name != "아르카나" && req.character.class_name != "리퍼" && valkyrie.is_none()
        && !(slayer_snapshot && matches!(req.character.class_name.as_str(),"슬레이어"|"버서커"|"소서리스")) {
        return Err(format!("지원하지 않는 직업: {}", req.character.class_name));
    }
    register::gem::validate(&req)?;
    reaper::validate(&req)?;
    reaper::resolve_routes(&mut req)?;

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
    class::arcana::register_identity_reference(&mut skill_pipelines, &req);
    register::rune::register(&mut skill_pipelines, &mut global_triggers, &req);
    reaper::register_tripods(&mut skill_pipelines, &req);
    let reaper_grid_effects = if req.character.class_name == "리퍼" {
        reaper_grid::register(&mut stat_builder, &mut modifier_manager, &mut global_triggers, &mut skill_pipelines, &req)?
    } else { Vec::new() };
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
    let mut stat = stat_builder.build()?;
    let mut skills = skill_pipelines.build_all(&req.skill_data, &req.character.skills)?;
    if skills.iter().any(|skill| skill.tags.contains(&crate::profile::SkillTag::SkillId(19000)))
        && req.card_data.iter().any(|card| card.runtime_effects.iter().any(|effect| effect.kind == "basic_attack_card_buff"))
        && !skills.iter().any(|skill| skill.tags.contains(&crate::profile::SkillTag::SkillId(19270)))
    {
        return Err("삼두사 기본 공격: 변경 스킬 19270 데이터 없음".to_string());
    }
    if req.character.class_name == "아르카나" {
        class::arcana::apply_specialization(&mut skills, stat.specialization);
    }
    let mut reaper_reference = reaper::configure(&req, &mut stat, &mut skills, &mut global_triggers)?;
    let mut max_hp=req.character.stats.max_hp;
    let hp_effects:Vec<_>=reaper_grid_effects.iter().filter(|effect|effect["kind"]=="raw_hp_delta").collect();
    if req.reaper_hp_baseline.is_some() {
        max_hp=req.reaper_hp_baseline.unwrap();
        let con:f64=hp_effects.iter().map(|effect|effect["conValue"].as_f64().unwrap()).sum();
        let flat:f64=hp_effects.iter().map(|effect|effect["flatHpValue"].as_f64().unwrap()).sum();
        // shortcut: raw Con*2+flat delta probe only; replace after gear multipliers/rounding and imported HP basis are calibrated.
        max_hp+=hp_effects.iter().map(|effect|effect["conValue"].as_f64().unwrap()*effect["conToHp"].as_f64().unwrap()+effect["flatHpValue"].as_f64().unwrap()).sum::<f64>();
        stat.base_vitality+=con;stat.max_hp+=flat;
        reaper_reference.as_mut().unwrap().metadata["raw_hp_model"]=serde_json::json!({"baseMaxHp":req.reaper_hp_baseline.unwrap(),"additionalCon":con,"additionalFlatHp":flat,"conToHp":2.,"maxHp":max_hp,"policy":"생명: 코어 제외 HP + 추가 체력×PC 원본 계수2 + 고정 HP의 명시적 임시 검사. 장비/체력/최대 HP 배율과 게임 반올림 미교정; 가져온 최종 HP를 기준으로 쓰지 않음. Hp/MaxHp·회복 상한은 같은 계산값 사용"});
    }
    if let Some(reference)=reaper_reference.as_mut().filter(|_|!req.reaper_incoming_damage.is_empty()) {
        reference.metadata["incoming_damage_policy"]=serde_json::json!("입력 피해는 방어 계산 후/코어 감소 전 값. 같은 시각은 입력 순서·타격/회복 전 피격; HP0 하한·회복은 기존 상한. 실제 적 공격/방어·회피/면역·사망/중단·HP 조건부 각인 교정은 제외한 임시 HP 소비 검사");
    }
    if let Some(rate)=reaper_grid_effects.iter().filter(|effect|effect["kind"]=="reactive_damage_reduction")
        .map(|effect|effect["value"].as_f64().unwrap()).max_by(f64::total_cmp) {
        let reference=reaper_reference.as_mut().unwrap();
        reference.metadata["reactive_damage_reduction"]=serde_json::json!(rate);
        reference.metadata["reactive_damage_policy"]=serde_json::json!("안정적인 공격: 최강 단계0.5/1.5/2.5% 하나만 입력 피격에 적용. 방어 후 입력값×(1-감소율), HP0 하한·기존회복상한; 다른 방어/감소·게임사망/중단/처리순서는 미교정. 추가 피해는 별도 기존 버킷");
    }
    if let Some(reference)=reaper_reference.as_mut().filter(|_|req.character.ark_grids.as_ref().is_some_and(|grid|grid.reaper_slots.iter().any(|slot|slot.name=="강철의 흔적"))) {
        reference.metadata["steel_trace_grid_policy"]=serde_json::json!("강철의 흔적: 단일 적대 대상의 적중 후 6초 방어 감소0.2/0.6/1.0% 중 최강 단계 하나로 교체·갱신. 자기 물리/마법 방어 증가는 보류하며 입력 피격은 이미 방어 계산 후 피해다. 파티 중첩·게임 적용 순서 미교정인 레이드 피해 임시 모델");
    }
    let raw_stagger_ratio:f64=reaper_grid_effects.iter().filter(|effect|effect["kind"]=="raw_stagger_ratio")
        .map(|effect|effect["value"].as_f64().unwrap()).sum();
    if let Some(reference)=reaper_reference.as_mut().filter(|_|raw_stagger_ratio>0.) {
        for skill in &mut skills {for hit in &mut skill.hits {hit.stagger*=1.+raw_stagger_ratio;}}
        reference.metadata["raw_stagger_grid_ratio"]=serde_json::json!(raw_stagger_ratio);
        reference.metadata["critical_trace_grid_policy"]=serde_json::json!("치명적인 흔적: 최강 Ability 단계의 단일6초 대상 디버프0.3/0.9/1.5%p·하위 단계 합산 제외. Stat54 원본 인수/10000의 자기 증가를 합산해 구현된 적중 원본 점수에 한 번 곱하는 임시 모델. 트라이포드와 별도 곱; 독/독립 추가 공격·실제 무력화 게이지/완료·게임 반올림/파티 교정 제외");
    }
    let identity_gain: f64 = reaper_grid_effects.iter().filter(|e| e["kind"]=="identity_gain_multiplier")
        .map(|e| e["value"].as_f64().unwrap()).sum();
    if let Some(reference) = reaper_reference.as_mut().filter(|_| identity_gain>0.0) {
        reference.metadata["identity_grid_gain"] = serde_json::json!(identity_gain);
        reference.metadata["identity_grid_policy"] = serde_json::json!("신념 영웅: 보정 전 입력 적중 수급×(1+코어 배율). 어둠/혼돈·트포/굶주림과 별도 곱; 기본 참고 수급에도 적용. 자연 증가/감소·시작량은 그대로인 임시 모델. 실제 수급/적용 범위는 게임 미교정; 상위 아군 강화 미지원");
    }
    if let Some(reference) = reaper_reference.as_mut().filter(|_| reaper_grid_effects.iter().any(|effect| effect["kind"] == "on_hit_heal")) {
        reference.metadata["heal_grid_policy"] = serde_json::json!("흡수의 일격: 직접 적중 시 자기 고정 회복·최대 생명력 제한·20초 공통 간격·상위 단계 교체. 만충도 간격 소비/독립 틱 발동 제외는 임시 정책. 기본 만충·피격 입력 없음; 유효 회복량/생존율·게임 처리 순서는 미검증");
    }
    reaper_grid::apply_stage_speed(&mut skills, &reaper_grid_effects);
    let compiled = modifier_manager.compile();

    // [의존 단계]
    skill::apply_modifiers(&mut skills, &compiled);
    skill::apply_skill_dot_modifiers(&mut skills, &compiled, &req);
    if let Some(reference) = &mut valkyrie {
        reference.attach(&mut skills, stat.specialization)?;
    }
    reaper_grid::apply_runtime_damage(&mut skills, &mut global_triggers, &reaper_grid_effects);
    // Rebuild only the non-back variant through the same tripods, class and modifiers.
    // The recursive request already has AttackBack, so it cannot create another variant.
    for skill in skills.iter_mut().filter(|skill| skill.cast_buff_back_attack_id.is_some()
        && !skill.tags.contains(&crate::profile::SkillTag::AttackBack)) {
        let mut back_request: serde_json::Value = serde_json::from_str(json).unwrap();
        let data = back_request["skillData"].as_array_mut().unwrap().iter_mut()
            .find(|data| data["name"] == skill.name).unwrap();
        data["properties"]["attackType"] = serde_json::json!("백 어택");
        let back_profiles = build(&back_request.to_string())?;
        skill.back_attack_variant = Some(Box::new(back_profiles.skills.into_iter()
            .find(|back| back.skill_id == skill.skill_id).unwrap()));
    }
    global_triggers.extend(skill::apply_triggers(&req.skill_data, &req.character.skills));

    let apl_profile = apl::build(&req.apl_config, &skills, &req.card_data, &req.buff_data)?;
    let max_mp = stat.max_mana;
    let arcana_card_pool = class::arcana::build_card_pool(&req);
    let (mut arcana_card_effects, arcana_card_blocking_buffs) =
        class::arcana::card::build_effects(&req, &mut skills, &mut global_triggers)?;
    class::arcana::card::apply_damage_modifiers(&mut arcana_card_effects, &compiled);
    register::gem::apply_conditional_hit_damage(&mut skills, &req);
    skill::apply_independent_dot_modifiers(&mut skills, &mut global_triggers, &compiled, &req);
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
        reaper_incoming_damage: req.reaper_incoming_damage.clone(),
        name: req.character.name.clone(),
        class_name: req.character.class_name.clone(),
        reaper_reference,
        valkyrie_identity: valkyrie.map(|v| v.identity),
        stat,
        skills,
        max_hp,
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
