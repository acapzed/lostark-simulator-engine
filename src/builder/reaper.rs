use crate::builder::dto::BuildRequest;
use crate::engine::state::class::reaper::{DISTORTION, FATAL_STEP, PERSONA, RAGE_SPEAR, SHADOW_DOT, SILENT_SMASHER, is_rush};
use crate::profile::reaper::{ReaperReference, ReaperScenario};
use crate::profile::{BuffSpec, StackType, HitTrigger, HitTriggerOn, SkillProfile, SkillSlot, SkillTag, StatField, StatProfile, TriggerEffect};

pub fn validate(req: &BuildRequest) -> Result<(), String> {
    if req.reaper_hp_baseline.is_some_and(|hp| !hp.is_finite() || hp<=0. || req.character.class_name != "리퍼" || req.reaper_build.is_none()) {
        return Err("리퍼 HP 임시 검사: 일반 리퍼 설정과 코어 제외 양의 최대 HP가 필요합니다".into());
    }
    if !req.reaper_incoming_damage.is_empty() && (req.character.class_name != "리퍼" || req.reaper_build.is_none()
        || req.reaper_hp_baseline.unwrap_or(req.character.stats.max_hp) <= 0. || req.reaper_incoming_damage.len()>1024
        || req.reaper_incoming_damage.iter().any(|hit| !hit.damage.is_finite() || hit.damage<0.)) {
        return Err("리퍼 피격 검사: 일반 리퍼 설정·양의 최대 HP·최대1024개·0 이상 유한한 피해량이 필요합니다".into());
    }
    if req.character.class_name != "리퍼" {
        if req.reaper_reference.is_some() || req.reaper_build.is_some() || req.reaper_identity_model.is_some() { return Err("리퍼 임시 설정은 리퍼 요청에만 사용 가능합니다".into()); }
        return Ok(());
    }
    if let Some(scenario) = req.reaper_build {
        if req.reaper_reference.is_some() { return Err("리퍼: reaperBuild와 reaperReference를 동시에 지정할 수 없습니다".into()); }
        return validate_build(req, scenario);
    }
    let reference = req.reaper_reference.as_ref().ok_or("리퍼: reaperReference 임시 시나리오가 필요합니다")?;
    if req.reaper_identity_model.is_some() { return Err("리퍼: 순환 검사 입력은 일반 설정에서만 사용 가능합니다".into()); }
    let character = &req.character;
    if character.skills.is_empty() || !character.equipments.is_empty() || !character.accessories.is_empty()
        || character.bracelet.is_some() || character.stone.is_some() || !character.gems.is_empty()
        || !character.engravings.is_empty() || character.ark_passive.is_some() || character.ark_grids.is_some()
        || character.ark_passive_karma.is_some() || !req.card_data.is_empty() || req.enable_awakening_potion
        || req.enable_feast || req.enable_pet_buff || req.collection_stats != 0.0
        || req.avatar_main_stat_percent != 0.0 || req.card_damage_bonus != 0.0 || req.collection_type_damage != 0.0
        || [req.collection_combat_stats.crit, req.collection_combat_stats.specialization,
            req.collection_combat_stats.swiftness, req.collection_combat_stats.domination,
            req.collection_combat_stats.endurance, req.collection_combat_stats.expertise].iter().any(|&value| value != 0)
    {
        return Err("리퍼 임시 실행: 고정 스킬·직업 효과만 지원하며 장비/각인/패시브/그리드/카드 선택은 지원하지 않습니다".into());
    }
    for chosen in &character.skills {
        if chosen.level != 1 || chosen.rune.is_some() || !chosen.tripods.is_empty() {
            return Err("리퍼 임시 실행: 1레벨·룬 없음·트라이포드 없음만 지원합니다".into());
        }
        let data = req.skill_data.iter().find(|data| data.name == chosen.name)
            .ok_or_else(|| format!("스킬 데이터 없음: {}", chosen.name))?;
        let expected = match data.game_skill_id {
            SHADOW_DOT => "쉐도우 닷",
            PERSONA if reference.scenario == ReaperScenario::DarkShadowDotLegacy => "페르소나",
            RAGE_SPEAR => "레이지 스피어",
            DISTORTION if reference.scenario == ReaperScenario::ThirstChaosSustain => "디스토션",
            FATAL_STEP if reference.scenario == ReaperScenario::ThirstChaosSustain => "페이탈 스텝",
            _ => return Err(format!("리퍼 임시 실행: 지원 범위 밖 스킬 {}", chosen.name)),
        };
        if data.name != expected || !data.cooldown.is_finite() || data.cooldown < 0.0
            || data.identity_gain.is_some() || data.hits.iter().any(|hit| hit.identity_gain.is_some())
        {
            return Err(format!("리퍼 임시 실행: 스킬 이름/ID·쿨다운·중복 수급 입력 오류: {}", chosen.name));
        }
    }
    Ok(())
}

fn validate_build(req: &BuildRequest, scenario: ReaperScenario) -> Result<(), String> {
    let character = &req.character;
    if character.skills.is_empty() { return Err("리퍼: 지원 스킬을 선택해주세요".into()); }
    if req.enable_awakening_potion || !req.card_data.is_empty() {
        return Err("리퍼 임시 지원: 각성 물약·아이덴티티 카드는 지원하지 않습니다".into());
    }
    passive_parameters(req, scenario)?;
    super::reaper_grid::resolve(req)?;
    if let Some(model) = &req.reaper_identity_model {
        if ![model.initial_dark, model.initial_heat, model.dark_per_second, model.persona_dark_loss_per_second, model.heat_loss_per_second]
            .iter().all(|value| value.is_finite() && (0.0..=1.0).contains(value))
            || !model.initial_chaos_seconds.is_finite() || !(0.0..=15.0).contains(&model.initial_chaos_seconds)
            || model.skill_gains.iter().any(|(id, gain)| ![gain.dark, gain.heat].iter().all(|value| value.is_finite() && (0.0..=1.0).contains(value))
                || !character.skills.iter().any(|chosen| req.skill_data.iter().any(|data| data.name == chosen.name && data.game_skill_id == *id))) {
            return Err("리퍼 순환 검사: 초기값/수급/초당 변화는0~1, 혼돈 초기 시간은0~15초, 수급 대상은 선택 스킬만 허용합니다".into());
        }
    }
    let tripod_catalog = tripod_catalog();
    for chosen in &character.skills {
        let data = req.skill_data.iter().find(|data| data.name == chosen.name)
            .ok_or_else(|| format!("리퍼 스킬 데이터 없음: {}", chosen.name))?;
        let expected = match data.game_skill_id {
            26050 => "스피닝 대거", 26060 => "스피릿 캐치", 26080 => "이블리스토",
            26120 => "데스 사이드", 26140 => "팬텀 댄서", 26800 => "라스트 그래피티",
            26110 => "샤벨 스팅거", 26560 => "블랙 미스트", 26600 => "쉐도우 스톰",
            26830 => "댄싱 오브 퓨리",
            26550 => "콜 오브 나이프", 26570 => "쉐도우 트랩",
            26900 => "월식 : 카덴차", 26910 => "일식 : 레퀴엠",
            26940 => "카덴차 델 루나", 26950 => "레퀴엠 델 솔", 26970 => "피니쉬 스텝",
            26520 => "블링크", 26960 => "쉐도우 나이프",
            26090 => "나이트메어", 26100 => "나이트메어 (재입력)",
            SHADOW_DOT => "쉐도우 닷", DISTORTION => "디스토션",
            RAGE_SPEAR => "레이지 스피어", SILENT_SMASHER => "사일런트 스매셔",
            PERSONA if scenario == ReaperScenario::DarkShadowDotLegacy => "페르소나",
            FATAL_STEP if scenario == ReaperScenario::ThirstChaosSustain => "페이탈 스텝",
            _ => return Err(format!("리퍼 임시 지원 범위 밖 스킬: {}", chosen.name)),
        };
        let max_level = if matches!(data.game_skill_id, PERSONA | FATAL_STEP) { 1 } else { 14 };
        if chosen.name != expected || chosen.level == 0 || chosen.level > max_level
            || !data.cooldown.is_finite() || data.cooldown < 0.0
            || data.identity_gain.is_some() || data.hits.iter().any(|hit| hit.identity_gain.is_some()) {
            return Err(format!("리퍼 임시 지원: {}의 기본 경로·1~{}레벨만 지원합니다", chosen.name, max_level));
        }
        let mut tiers = std::collections::BTreeSet::new();
        for choice in &chosen.tripods {
            let tripod = tripod_catalog.iter().find(|item| item["skillName"] == chosen.name && item["name"] == choice.name)
                .filter(|item| item["simulationSupported"] == true)
                .ok_or_else(|| format!("리퍼 미지원 트라이포드: {} · {}", chosen.name, choice.name))?;
            if choice.level.unwrap_or(1) != 1 { return Err(format!("리퍼 트라이포드 {}: 원본 1레벨만 지원합니다", choice.name)); }
            if tripod["excludedChoiceIds"].as_array().into_iter().flatten().any(|id| chosen.tripods.iter().any(|other| tripod_catalog.iter().any(|item| item["skillName"]==chosen.name && item["name"]==other.name && item["choiceId"]==*id))) {
                return Err(format!("리퍼 {}: {}의 원본 배타 선택을 함께 사용할 수 없습니다",chosen.name,choice.name));
            }

            let tier = tripod["tier"].as_u64().unwrap();
            if !tiers.insert(tier) { return Err(format!("리퍼 트라이포드: {} {}단계 중복 선택", chosen.name, tier + 1)); }
            if chosen.level < [4, 7, 10][tier as usize] { return Err(format!("리퍼 트라이포드 {}: 스킬 {}레벨 필요", choice.name, [4, 7, 10][tier as usize])); }
        }
        if chosen.rune.as_ref().is_some_and(|rune| rune.name != "질풍"
            || rune.effects.iter().any(|effect| effect.kind != "cast_speed" || !effect.value.is_finite() || !(0.0..=100.0).contains(&effect.value))) {
            return Err(format!("리퍼 임시 지원: {}에는 질풍 룬만 선택할 수 있습니다", chosen.name));
        }
    }
    for gem in &character.gems {
        if !(1..=10).contains(&gem.level) || !matches!(gem.gem_type.as_str(), "겁화" | "작열" | "멸화" | "홍염")
            || !super::register::gem::registered_target(req,gem) {
            return Err(format!("리퍼 임시 지원: 보석 대상/종류/레벨을 확인하세요: {}", gem.skill_name));
        }
    }
    if character.skills.iter().filter(|chosen| !matches!(chosen.name.as_str(), "페르소나" | "페이탈 스텝"
        | "월식 : 카덴차" | "일식 : 레퀴엠" | "카덴차 델 루나" | "레퀴엠 델 솔" | "피니쉬 스텝"
        | "쉐도우 나이프" | "나이트메어" | "나이트메어 (재입력)")).count() > 8 {
        return Err("리퍼: 일반 스킬은 최대 8개까지 선택할 수 있습니다".into());
    }
    if character.skills.iter().filter(|skill| matches!(skill.name.as_str(), "피니쉬 스텝" | "쉐도우 나이프")).count() > 1 {
        return Err("리퍼: T스킬은 1개만 선택할 수 있습니다".into());
    }
    for names in [["월식 : 카덴차", "일식 : 레퀴엠"], ["카덴차 델 루나", "레퀴엠 델 솔"]] {
        if character.skills.iter().filter(|skill| names.contains(&skill.name.as_str())).count() > 1 {
            return Err("리퍼: 각성기와 초각성기는 각각 1개만 선택할 수 있습니다".into());
        }
    }
    for equip in &character.equipments {
        if equip.set_name.is_empty() { continue; }
        if !req.equipment_tables.get(&equip.set_name).and_then(|slots| slots.get(&equip.item_slot))
            .is_some_and(|rows| rows.iter().any(|row| if equip.item_slot == "완갑" {
                row.refinement_level == equip.refinement_level
            } else { row.item_level == equip.item_level })) {
            return Err(format!("리퍼 장비 데이터 없음: {} {} Lv.{}", equip.set_name, equip.item_slot, equip.item_level));
        }
    }
    Ok(())
}

fn tripod_catalog() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!("../../skill-data/reaper/backend/tripod-catalog.json")).unwrap()
}

pub fn resolve_routes(req: &mut BuildRequest) -> Result<(), String> {
    if req.reaper_build.is_none() { return Ok(()); }
    let catalog = tripod_catalog();
    for chosen in &req.character.skills {
        let data=req.skill_data.iter_mut().find(|data|data.name==chosen.name).unwrap();
        let normal_override=chosen.tripods.iter().any(|choice|catalog.iter().any(|item|item["skillName"]==chosen.name && item["name"]==choice.name && item["controlTypeOverride"]=="일반"));
        // shortcut: full-cast routes only; add separate inputs when early release or combo/chain execution is modelled.
        let holding_override=chosen.tripods.iter().any(|choice|catalog.iter().any(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["controlTypeOverride"]=="홀딩"));
        let distortion=chosen.tripods.iter().any(|choice|catalog.iter().any(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["distortionModel"].is_object()));
        data.skill_control_type=if holding_override || (matches!(data.game_skill_id,26560|26600) && !normal_override) {"Holding"} else {"Normal"}.into();
        for choice in &chosen.tripods {
            let item = catalog.iter().find(|item| item["skillName"] == chosen.name && item["name"] == choice.name).unwrap();
            if distortion && item["gustModel"].is_object() { continue; }
            for effect in item["runtimeEffects"].as_array().unwrap().iter().filter(|effect| effect["kind"] == "replace_route") {
                let data = req.skill_data.iter_mut().find(|data| data.name == chosen.name).unwrap();
                // Trusted catalog replaces the route before all rune/gem/stat pipelines run.
                data.cast_times_ms = serde_json::from_value(effect["route"]["castTimesMs"].clone()).map_err(|e| e.to_string())?;
                let hits=effect["route"]["hits"].as_array().unwrap().iter().filter(|hit|hit["active_skill_levels"].as_array().is_none_or(|levels|levels.iter().any(|level|level.as_u64()==Some(chosen.level as u64)))).cloned().collect();
                data.hits = serde_json::from_value(serde_json::Value::Array(hits)).map_err(|e| e.to_string())?;
            }
        }
    }
    // Conditional dagger choices replace the complete layer before any poison ID swaps.
    for chosen in &req.character.skills {
        let selected:Vec<_>=chosen.tripods.iter().map(|c|catalog.iter().find(|t|t["skillName"]==chosen.name&&t["name"]==c.name).unwrap()).collect();
        for effect in selected.iter().flat_map(|t|t["runtimeEffects"].as_array().unwrap()).filter(|e|matches!(e["kind"].as_str(),Some("spinning_rotation_route"|"iblis_breath_route"))) {
            let poison=selected.iter().any(|t|matches!(t["choiceId"].as_u64(),Some(1|2|3)));
            let nerve=selected.iter().any(|t|t["choiceId"]==3);let shadow=selected.iter().any(|t|t["choiceId"]==7);
            let swift=selected.iter().any(|t|t["choiceId"]==8);
            let variant=effect["variants"].as_array().unwrap().iter().find(|v|v["poisonSelected"]==poison && if effect["kind"]=="spinning_rotation_route" {v["nerveSelected"]==nerve&&v["shadowSelected"]==shadow} else {v["swiftSelected"]==swift}).unwrap();
            let data=req.skill_data.iter_mut().find(|d|d.name==chosen.name).unwrap();
            data.cast_times_ms=serde_json::from_value(variant["route"]["castTimesMs"].clone()).map_err(|e|e.to_string())?;
            data.hits=serde_json::from_value(variant["route"]["hits"].clone()).map_err(|e|e.to_string())?;
        }
    }
    // Compose Notify choices after layer routes, independent of input choice order.
    for chosen in &req.character.skills {
        let data = req.skill_data.iter_mut().find(|data| data.name == chosen.name).unwrap();
        for choice in &chosen.tripods {
            let item = catalog.iter().find(|item| item["skillName"] == chosen.name && item["name"] == choice.name).unwrap();
            for effect in item["runtimeEffects"].as_array().unwrap() {
                if effect["kind"] == "effect_swap" {
                    for hit in &mut data.hits {
                        let slot_matches = effect["hitIdPrefix"].as_str().is_none_or(|prefix| hit.hit_id == prefix || hit.hit_id.strip_prefix(prefix).is_some_and(|suffix| suffix.starts_with(":repeat:")));
                        if Some(hit.effect_id as u64) == effect["effectId"].as_u64() && slot_matches {
                            hit.effect_id = effect["replacementEffectId"].as_u64().unwrap() as u32;
                        }
                    }
                } else if effect["kind"] == "append_hits" {
                    let hits:Vec<crate::builder::dto::HitDataInput> = serde_json::from_value(effect["hits"].clone()).map_err(|e| e.to_string())?;
                    data.hits.extend(hits);
                    data.hits.sort_by_key(|hit| hit.action_delay_ms);
                } else if effect["kind"] == "call_wide_route" {
                    let condition=chosen.tripods.iter().filter_map(|c|catalog.iter().find(|t|t["skillName"]==chosen.name&&t["name"]==c.name)).filter_map(|t|t["choiceId"].as_u64()).find(|id|matches!(id,4|6)).unwrap_or(0);
                    let variant=effect["variants"].as_array().unwrap().iter().find(|v|v["conditionChoiceId"].as_u64()==Some(condition)).unwrap();
                    data.cast_times_ms=serde_json::from_value(variant["route"]["castTimesMs"].clone()).map_err(|e|e.to_string())?;
                    data.hits=serde_json::from_value(variant["route"]["hits"].clone()).map_err(|e|e.to_string())?;
                } else if effect["kind"] == "rage_reinput_route" {
                    let variant=effect["variants"].as_array().unwrap().iter().find(|variant|data.hits.iter().any(|hit|variant["damageEffectIds"].as_array().unwrap().iter().any(|id|id.as_u64()==Some(hit.effect_id as u64)))).ok_or("리퍼 은밀한 암살: 지원 재입력 경로 없음")?;
                    data.cast_times_ms=serde_json::from_value(variant["route"]["castTimesMs"].clone()).map_err(|e|e.to_string())?;
                    data.hits=serde_json::from_value(variant["route"]["hits"].clone()).map_err(|e|e.to_string())?;
                }
            }
        }
    }
    // Audited callbacks append after direct-hit swaps; they keep their own raw IDs and coefficients.
    // Select one region after all range swaps; paired raw calls must not duplicate one target.
    for chosen in &req.character.skills {
        let data=req.skill_data.iter_mut().find(|d|d.name==chosen.name).unwrap();
        for effect in chosen.tripods.iter().filter_map(|c|catalog.iter().find(|t|t["skillName"]==chosen.name&&t["name"]==c.name)).flat_map(|t|t["runtimeEffects"].as_array().unwrap()).filter(|e|e["kind"]=="mist_eye_swap") {
            for hit in &mut data.hits {
                if effect["effectId"].as_u64()==Some(hit.effect_id as u64) {hit.effect_id=effect[if req.assumed_target_mist_centre {"centreEffectId"} else {"outerEffectId"}].as_u64().unwrap() as u32;}
            }
        }
    }
    for chosen in &req.character.skills {
        let selected:Vec<_>=chosen.tripods.iter().map(|c|catalog.iter().find(|t|t["skillName"]==chosen.name&&t["name"]==c.name).unwrap()).collect();
        for effect in selected.iter().flat_map(|t|t["runtimeEffects"].as_array().unwrap()).filter(|e|e["kind"]=="phantom_shadow_hits") {
            let condition=selected.iter().filter_map(|t|t["choiceId"].as_u64()).find(|id|matches!(id,5|6)).unwrap_or(0);
            let poison=selected.iter().filter_map(|t|t["choiceId"].as_u64()).find(|id|matches!(id,1|2|3)).unwrap_or(1);
            let variant=effect["variants"].as_array().unwrap().iter().find(|v|v["conditionChoiceId"].as_u64()==Some(condition)&&v["poisonChoiceId"].as_u64()==Some(poison)).unwrap();
            let data=req.skill_data.iter_mut().find(|d|d.name==chosen.name).unwrap();
            let hits:Vec<crate::builder::dto::HitDataInput>=serde_json::from_value(variant["hits"].clone()).map_err(|e|e.to_string())?;data.hits.extend(hits);
            data.hits.sort_by_key(|h|h.action_delay_ms+h.fixed_delay_ms);
        }
    }
    for chosen in &req.character.skills {
        let data = req.skill_data.iter().find(|data| data.name == chosen.name).unwrap();
        for choice in &chosen.tripods {
            let item = catalog.iter().find(|item| item["skillName"] == chosen.name && item["name"] == choice.name).unwrap();
            if item["inactiveChainEffects"].as_array().into_iter().flatten().any(|chain| data.hits.iter().any(|hit| chain["effectId"].as_u64() == Some(hit.effect_id as u64)) && !(chain["executionSupported"]==true && chosen.tripods.iter().any(|choice| catalog.iter().any(|item| item["skillName"]==chosen.name && item["name"]==choice.name && item["runtimeEffects"].as_array().unwrap().iter().any(|effect|effect["kind"]=="bleeding" && effect["effectId"]==chain["effectId"]))))) {
                return Err(format!("리퍼 {} · {}: 출혈 전용 타격의 추가 효과는 아직 지원하지 않습니다",chosen.name,choice.name));
            }
            if item["runtimeEffects"].as_array().unwrap().iter().any(|e| matches!(e["kind"].as_str(),Some("corrosion"|"bleeding"))) && !item["runtimeEffects"].as_array().unwrap().iter().filter(|e| matches!(e["kind"].as_str(),Some("corrosion"|"bleeding"))).any(|e| data.hits.iter().any(|hit| e["effectId"].as_u64()==Some(hit.effect_id as u64))) {
                return Err(format!("리퍼 {}: 독 발동 타격 없는 경로는 지원하지 않습니다",chosen.name));
            }
        }
    }
    Ok(())
}

pub fn register_tripods(pipelines: &mut crate::builder::skill::SkillPipelineMap, req: &BuildRequest) {
    if req.reaper_build.is_none() { return; }
    let catalog = tripod_catalog();
    let target_assumptions = (req.assumed_target_seed_or_lower, req.assumed_target_abnormal_move_immune);
    for chosen in &req.character.skills {
        for choice in &chosen.tripods {
            let effects = catalog.iter().find(|item| item["skillName"] == chosen.name && item["name"] == choice.name)
                .unwrap()["runtimeEffects"].as_array().unwrap().clone();
            pipelines.get_mut(&chosen.name).unwrap().add_stats(move |skill| {
                // Flat base cooldown changes precede percentage reductions from gems/modifiers.
                for effect in effects {
                    if let Some(condition) = effect["targetAssumption"].as_str() {
                        let enabled = match condition {
                            "seed_or_lower" => target_assumptions.0,
                            "abnormal_move_immune" => target_assumptions.1,
                            // shortcut: fixed PvE NPC only; PC targets need an audited target/input contract.
                            "pc" => false,
                            _ => unreachable!(),
                        };
                        if !enabled { continue; }
                    }
                    let value = effect["value"].as_f64().unwrap();
                    if effect["kind"]=="raw_stagger_ratio" {continue;}
                    if matches!(effect["kind"].as_str(),Some("attack_stage_speed" | "cast_movement_buff" | "protection_windows" | "destruction_level" | "active_cooldown_ms" | "skill_evasion" | "dagger_cooldown_buff" | "mist_eye_swap" | "spinning_rotation_route" | "iblis_breath_route" | "phantom_shadow_hits" | "replace_route" | "effect_swap" | "append_hits" | "corrosion" | "bleeding" | "nerve" | "target_slow" | "silent_shadow_target" | "chaos_damage" | "identity_gain" | "movement_buff" | "rage_reinput_route" | "end_cooldown_ms" | "call_wide_route")) { continue; } // Route before pipelines; cast/state effects after skill IDs.
                    if effect["kind"]=="chain_poison" {continue;} // Selected poison attaches the audited extra packet below.
                    if effect["kind"] == "element" {
                        skill.tags.retain(|tag| !matches!(tag, SkillTag::ElementFire | SkillTag::ElementWater | SkillTag::ElementElectricity | SkillTag::ElementWind | SkillTag::ElementDark | SkillTag::ElementHoly));
                        skill.tags.push(match value as u32 { 3 => SkillTag::ElementElectricity, 6 => SkillTag::ElementDark, _ => unreachable!() });
                        continue;
                    }
                    if effect["kind"] == "cooldown_ms" {
                        skill.cooldown_ms = (skill.cooldown_ms as f64 + value).max(0.0) as u32;
                        continue;
                    }
                    if effect["kind"] == "max_stacks" { skill.max_stacks = value as u32; continue; }
                    if effect["kind"] == "charge_recovery_ms" { skill.charge_recovery_ms = value as u32; continue; }
                    if effect["kind"] == "preparation_speed" {
                        let duration = effect["durationMs"].as_u64().unwrap() as u32;
                        let saved = duration - (duration as f64 / (1.0 + value)).round() as u32;
                        skill.cast_times_ms[0] = skill.cast_times_ms[0].checked_sub(saved).ok_or("리퍼 준비 단계가 시전 시간보다 깁니다")?;
                        for hit in &mut skill.hits { hit.action_delay_ms = hit.action_delay_ms.checked_sub(saved).ok_or("리퍼 준비 단계 이전의 타격은 이 선택으로 처리할 수 없습니다")?; }
                        continue;
                    }
                    let id = effect["effectId"].as_u64().unwrap() as u32;
                    if effect["kind"] == "back_damage" && !skill.tags.contains(&SkillTag::AttackBack) { continue; }
                    for hit in &mut skill.hits {
                        if id != 0 && hit.effect_id != id { continue; }
                        match effect["kind"].as_str().unwrap() {
                            "damage_increase" | "back_damage" => hit.damage_increase = (1.0 + hit.damage_increase) * (1.0 + value) - 1.0,
                            "crit_rate" => hit.crit_chance_bonus += value,
                            "crit_damage" => hit.crit_damage_bonus += value,
                            "defense_ignore" => { hit.defense_ignore = value; hit.defense_ignore_chance = 1.0; }
                            _ => unreachable!(),
                        }
                    }
                }
                Ok(())
            });
        }
    }
}

fn passive_parameters(req: &BuildRequest, scenario: ReaperScenario) -> Result<Option<serde_json::Value>, String> {
    let Some(input) = req.character.ark_passive.as_ref().filter(|input| !input.class_nodes.is_empty()) else { return Ok(None); };
    let catalog: Vec<serde_json::Value> = serde_json::from_str(include_str!("../../skill-data/reaper/backend/passive-catalog.json")).unwrap();
    let mut selected = std::collections::BTreeMap::new();
    for chosen in &input.class_nodes {
        let node = catalog.iter().find(|node| node["name"] == chosen.name).ok_or_else(|| format!("리퍼 미지원 직업 패시브: {}", chosen.name))?;
        if chosen.level == 0 || chosen.level > node["maxLevel"].as_u64().unwrap() as u32 || selected.insert(chosen.name.as_str(), chosen.level).is_some() {
            return Err(format!("리퍼 패시브 레벨/중복 오류: {}", chosen.name));
        }
    }
    let root = if scenario == ReaperScenario::DarkShadowDotLegacy { "달의 소리" } else { "피냄새" };
    if !selected.contains_key(root) || (selected.contains_key("달의 소리") && selected.contains_key("피냄새")) {
        return Err("리퍼: 직업 경로의 시작 노드를 선택하고 달의 소리/피냄새 배타 조건을 확인하세요".into());
    }
    let mut parameters = serde_json::Map::new();
    for node in &catalog {
        if let Some(&level) = selected.get(node["name"].as_str().unwrap()) {
            if node["exclusiveId"].as_u64().unwrap() > 0 && catalog.iter().any(|other| other["name"] != node["name"] && other["exclusiveId"] == node["exclusiveId"] && selected.contains_key(other["name"].as_str().unwrap())) {
                return Err(format!("리퍼 패시브 배타 선택 오류: {}", node["name"]));
            }
            let preceding_points: u64 = catalog.iter().filter(|other| other["type"] == node["type"] && other["tier"].as_u64() < node["tier"].as_u64())
                .map(|other| other["activatePoint"].as_u64().unwrap() * selected.get(other["name"].as_str().unwrap()).copied().unwrap_or(0) as u64).sum();
            if preceding_points < node["precedingTierPoints"].as_u64().unwrap() { return Err(format!("리퍼 패시브 이전 티어 포인트 미충족: {}", node["name"])); }
            if let Some(prerequisite) = catalog.iter().find(|parent| parent["id"] == node["prerequisiteId"]) {
                if selected.get(prerequisite["name"].as_str().unwrap()).copied().unwrap_or(0) < node["prerequisiteLevel"].as_u64().unwrap() as u32 {
                    return Err(format!("리퍼 패시브 선행 노드 미충족: {}", node["name"]));
                }
            }
            // Derive from the audited catalog, never from the client's supplied values.
            parameters.insert(node["runtimeKey"].as_str().unwrap().into(), node["effects"][(level - 1) as usize][0].clone());
            if node["runtimeKey"] == "handwork_damage" {
                parameters.insert("handwork_hit_ids".into(), node["hitSkillIds"].clone());
                parameters.insert("handwork_damage_ids".into(), node["damageSkillIds"].clone());
            }
            if node["runtimeKey"] == "finish_chaos_damage" { parameters.insert("finish_chaos_crit".into(), serde_json::json!([10, 15, 20][(level - 1) as usize])); }
            if node["runtimeKey"] == "finish_crit" { parameters.insert("finish_persona_crit_damage".into(), serde_json::json!([5, 10, 15][(level - 1) as usize])); }
        }
    }
    parameters.insert("selected_levels".into(), serde_json::to_value(selected).unwrap());
    Ok(Some(serde_json::Value::Object(parameters)))
}

pub fn configure(req: &BuildRequest, stat: &mut StatProfile, skills: &mut [SkillProfile], global_triggers: &mut Vec<crate::profile::GlobalTrigger>) -> Result<Option<ReaperReference>, String> {
    let mut tripod_stage_speeds=vec![];
    let Some(mut reference) = req.reaper_reference.clone().or_else(|| req.reaper_build.map(|scenario| ReaperReference {
        scenario, missed_hit_ids: vec![], gauge_test: None, metadata: serde_json::Value::Null,
    })) else { return Ok(None); };
    if let Some(test) = &reference.gauge_test {
        if reference.scenario != ReaperScenario::DarkShadowDotLegacy || !test.initial_dark.is_finite()
            || !(0.0..=1.0).contains(&test.initial_dark) || test.hit_gains.is_empty()
            || test.hit_gains.iter().any(|(id, gain)| {
                ![gain.dark, gain.heat].iter().all(|value| value.is_finite() && (0.0..=1.0).contains(value))
                    || !skills.iter().filter(|skill| skill.tags.contains(&SkillTag::SkillId(SHADOW_DOT)))
                        .flat_map(|skill| &skill.hits).any(|hit| &hit.hit_id == id)
            })
        {
            return Err("리퍼 합성 게이지 검사: 달의 소리·0~1 초기값/수급·쉐도우 닷 타격 ID만 허용합니다".into());
        }
    }
    let mut hit_ids = std::collections::HashSet::new();
    if skills.iter().flat_map(|skill| &skill.hits).any(|hit| !hit_ids.insert(&hit.hit_id)) {
        return Err("리퍼 임시 실행: 타격 ID가 중복되었습니다".into());
    }
    for missed in &reference.missed_hit_ids {
        if !skills.iter().flat_map(|skill| &skill.hits).any(|hit| &hit.hit_id == missed) {
            return Err(format!("리퍼 임시 실행: 없는 미스 타격 ID: {missed}"));
        }
    }
    let contract: serde_json::Value = serde_json::from_str(include_str!("../../skill-data/reaper/backend/research/community-identity.json"))
        .map_err(|error| format!("리퍼 임시 계약 파싱 실패: {error}"))?;
    let scenario_name = serde_json::to_value(reference.scenario).unwrap();
    let scenario = contract["reference_scenarios"].as_array().unwrap().iter()
        .find(|scenario| scenario["id"] == scenario_name).unwrap();
    reference.metadata = serde_json::json!({
        "status": "provisional", "reference_scenario_id": scenario_name,
        "adopted_assumption_ids": scenario["adopted_assumption_ids"],
        "initial_state": scenario, "excluded_scope": scenario["excluded"],
        "source_ids": ["RC01", "RC02", "RC03", "RC04", "RC05", "ROF01"],
        "fixed_passives": if reference.scenario == ReaperScenario::DarkShadowDotLegacy {
            "달의 소리/유령 무희/그림자 밟기/급소 확보 Lv3"
        } else { "피냄새/굶주림/갈증/살인귀 Lv3·혼돈 강화 Lv5" },
        "damage_scope": "원본 1레벨 기본 경로 후보; 특화 피해/독/분신/선택 경로 제외",
        "runtime_scope": "opt-in 고정 참고 입력·직접 타격·CLI/WASM/참고 웹; 정식 캐릭터/DB 시드 제외",
        "damage_assumptions": ["raw 계수 (A+B)/2 + AP*F/10000 후보식", "캐릭터70레벨 페이탈 Variable 후보", "Stage0 모션 잠금 후보", "스킬 백어택 속성대로 모든 직접 타격 적격 판정"],
        "reference_attack_speed_percent": 140,
        "reference_base_attack_power": 10000,
        "synthetic_gauge_test": reference.gauge_test,
        "gauge_test_scope": "RP04/05 경계 사건 검사; 실제 수급량·혼돈 진입/자연량 검증 제외",
    });
    if req.reaper_build.is_some() {
        if !stat.base_main_stat.is_finite() || stat.base_main_stat <= 0.0
            || !stat.sum(StatField::WeaponAttackPower).is_finite() || stat.sum(StatField::WeaponAttackPower) <= 0.0 {
            return Err("리퍼: 무기와 주스탯 장비를 설정해 공격력을 구성해주세요".into());
        }
        if !stat.specialization.is_finite() || stat.specialization < 0.0 {
            return Err("리퍼: 특화는 0 이상의 유한한 값이어야 합니다".into());
        }
        let spec_damage = 1.0 + stat.specialization / 699.0 * 0.31;
        for skill in skills.iter_mut().filter(|skill| skill.tags.iter().any(|tag| matches!(tag, SkillTag::SkillId(id) if is_rush(*id)))) {
            for hit in &mut skill.hits {
                hit.damage_increase = (1.0 + hit.damage_increase) * spec_damage - 1.0;
            }
        }
        let rush_ids: Vec<_> = skills.iter().filter(|skill| skill.tags.iter().any(|tag| matches!(tag, SkillTag::SkillId(id) if is_rush(*id)))).map(|skill| skill.skill_id).collect();
        for skill in skills.iter_mut() {
            if skill.slot == SkillSlot::Awakening {
                for hit in &mut skill.hits { hit.damage_increase = (1.0 + hit.damage_increase) * (1.0 + stat.specialization / 699.0 * 0.1528) - 1.0; }
            }
            if skill.tags.contains(&SkillTag::SkillId(26910)) {
                for hit in &mut skill.hits {
                    hit.triggers.push(HitTrigger { on: HitTriggerOn::OnHit, effect: TriggerEffect::ReduceCooldowns { skill_ids: rush_ids.clone(), percent: 0.10 }, chance: 1.0 });
                }
            }
        }
        reference.metadata["runtime_scope"] = serde_json::json!("리퍼 전투 스킬 목록27개·기본 실행27개; 공통 장비/효과·선택/게이지 부분 지원");
        reference.metadata["clone_nightmare_policy"] = serde_json::json!("블링크6타·쉐도우 나이프2타; 단일 대상·콜백 생성 즉시 적중·소유자 피해 상속·분신 독립 타격 ID; 나이트메어 표식7초·재입력1회·페르소나 남은 쿨30%/혼돈 기본 쿨40% 감소");
        reference.metadata["awakening_route_policy"] = serde_json::json!("카덴차12/레퀴엠3/델루나8/델솔1/피니쉬6타; 레퀴엠 적중마다 급습 남은 쿨10% 감소; 초각성 시작 만충10000·1회·계수1, 은신/이동 단순화");
        reference.metadata["initial_ultimate_point"] = serde_json::json!(10000);
        reference.metadata["ultimate_coefficient"] = serde_json::json!(1.0);
        reference.metadata["projectile_route_policy"] = serde_json::json!("콜 오브 나이프5타/쉐도우 트랩1타; 단일 대상·생성 시 Timer 시작·공속 독립·콜백/이동 단순화");
        reference.metadata["repeat_route_policy"] = serde_json::json!("샤벨4/미스트5/스톰12/퓨리5타; 완전 홀딩·애니메이션 구간 분배, 퓨리 반복 모션4회; MultiHitCount 중복 제외·공중/중도 취소 제외");
        reference.metadata["direct_route_policy"] = serde_json::json!("추가 스킬은 전체 기본 타격/재입력을 각 단계 애니메이션 종료 후 연결; 조기 입력/취소 제외; 쉐도우 닷 외 게이지 수급 미산정");
        let tripods = tripod_catalog();
        let destruction:serde_json::Value=serde_json::from_str(include_str!("../../skill-data/reaper/backend/destruction-rules.json")).unwrap();
        for skill in skills.iter_mut() {
            for hit in &mut skill.hits { hit.destruction=destruction["baseLevels"][hit.effect_id.to_string()].as_u64().unwrap_or(0) as u32; }
        }
        reference.metadata["destruction_policy"]=destruction;
        let stagger:serde_json::Value=serde_json::from_str(include_str!("../../skill-data/reaper/backend/stagger-rules.json")).unwrap();
        for skill in skills.iter_mut() {
            for hit in &mut skill.hits { hit.stagger=stagger["basePoints"][hit.effect_id.to_string()].as_f64().unwrap_or(0.0); }
        }
        reference.metadata["raw_stagger_policy"]=stagger;

        reference.metadata["damage_scope"] = serde_json::json!(format!("선택 레벨 원본 기본 피해·특화 표시식 후보; 트라이포드{}개 Lv1·분신 기본 경로, 독/선택 경로 교정 제외",tripods.iter().filter(|t|t["simulationSupported"]==true).count()));
        reference.metadata["tripod_policy"] = serde_json::json!("원본 수치/속성/방어 무시/스택/준비 속도/사용 쿨감/혼돈 조건/유언 경로/지면 강타/백어택 조건/추진력 경로/절제된 동작/그림자 충전/처형/독 미선택 단검 보정/넓은 공격 타격 교체/퍼지는 안개/부식/출혈 독·미스트 거짓 안개/안개 폭발·정적 대상 조건4선택·검은 폭풍/넓은 안개·콜 지속/치명·덫 강화/올가미·이동 버프/재입력/종료 쿨감·명시적 신경 독 상태를 포함한 지원 선택의1레벨만 계산. 단계 배타·스킬4/7/10레벨 해금; 기본 쿨/충전 변경 후 보석 쿨감; effectId별 피해/치명 보정. 성장/나머지 경로 변경/독·제어의 실제 게임 교정 미지원, 클라이언트 values 미신뢰");
        reference.metadata["corrosion_policy"] = serde_json::json!("원본 ChainType1은 SkillEffect 연결:260410→SkillBuff280000, CombatEffect280010→SkillBuff280010. 7스킬 기본 마지막 타격에 부식 Lv1; 중독8초/공유3중첩·방어력12% 감소12초. 첫 틱500ms/1초 주기; 재적용 때 전체 중첩/타이머 갱신·DoT 재시작·최종 발동 스킬 귀속은 게임 관측 없는 임시 정책. 독은 원 스킬 트포/보석·방향/속성/자원 수급을 상속하지 않으며 공통 능력치/대상 방어 감소는 틱마다 계산");
        reference.metadata["bleeding_policy"] = serde_json::json!("스피닝/스피릿/쉐도우 닷/이블리스토/데스/팬텀/샤벨 출혈 Lv1(샤벨 기본4타 중 마지막 기본출혈1회·두개의 그림자 적중25%/날카로운 단검 실제치명50% 추가1회):SkillEffect260420→출혈Buff280020·12초/공유10중첩,500ms 첫 틱/1초 주기. max stack→260421→과다출혈Buff280030,표시Effect260422와 같은 고정1925.5/계수41.769의 즉시1타. 최대 중첩 전체 소비/틱 재시작/최종 시전 귀속은 게임 관측 없는 임시 정책; 부식과 별도 상태. 원 스킬 트포/보석·방향/속성/자원 미상속, 기습 백어택+3/치명적인 단검 치명타+2 출혈은 원본 행마다 처리; 스피릿 치명/백어택+1·이블리스토 백어택+2; 스피릿/이블리스토의 넘어짐·밀림·정지 반응과 시전 영향은 미계산. 데스의 밀림 거리/시전 취소는 미계산·슬롯 이름 대응은 모델 계약; 나머지 경로/성장·실제 게임 교정 미지원");
        if req.character.skills.iter().any(|skill| skill.name=="블랙 미스트" && skill.tripods.iter().any(|choice| matches!(choice.name.as_str(),"거짓 안개"|"안개 폭발"))) {
            reference.metadata["mist_projectile_policy"]=serde_json::json!("거짓 안개500ms 시전/생성329ms 뒤 고정250/650/1050/1450/1850ms의5타; 안개 폭발은 기본 완전 홀딩5타 뒤 생성3520ms/고정0/500/1000ms의3타에만+58%. 생성만 공속 영향, Timer는 고정인 임시 단일 대상 모델. MultiHitCount를5회에 다시 곱하지 않으며 충돌/이동/적 속도 감소/조기 취소/실게임 시간과 게이지 교정은 미계산");
        }
        if req.character.skills.iter().any(|skill|skill.name=="쉐도우 트랩" && skill.tripods.iter().any(|choice|choice.name=="그림자 늪")) {
            reference.metadata["trap_swamp_policy"]=tripods.iter().find(|item|item["gameSkillId"]==26570 && item["choiceId"]==5).unwrap()["trapSwampModel"].clone();
        }
        if req.character.skills.iter().any(|skill|skill.name=="쉐도우 트랩" && skill.tripods.iter().any(|choice|choice.name=="그림자 올가미")) {
            reference.metadata["trap_snare_policy"]=serde_json::json!("생성 후200ms 폭발/+125%·400ms 기본 타격/피해 유지·덫 강화/스택/사용 쿨감 조합. ExGroup은 Notify 배타이며36슬롯의 범위20% 교체 자식 적용·중복 없음. 고정 대상 임시 모델·기본 게이지 미확정/입력 타격 수급·끌어당김/정지·내부 조건 상속/콜백 word9/성장/실게임 교정 미지원");
        }
        if req.character.skills.iter().any(|skill|skill.name=="라스트 그래피티" && skill.tripods.iter().any(|choice|choice.name=="그림자 올가미")) {
            reference.metadata["graffiti_snare_policy"]=serde_json::json!("준비181ms 분신1타/+100%·933ms 본타·1700ms 시전; 범위 증가/급습 강화/혼돈 강화 조합. 범위18슬롯 자식268082 교체·20레벨 원값 유지·비활성Stage2/유언 중복 없음. 고정 대상/소유자 피해/급습 시전 상속은 임시 모델; 마우스 위치/끌어당김/반응·조건부 슬롯/성장/게임 교정 미지원");
        }
        if req.character.skills.iter().any(|skill|skill.name=="사일런트 스매셔" && skill.tripods.iter().any(|choice|choice.name=="가벼운 동작")) {
            reference.metadata["silent_light_policy"]=tripod_catalog().into_iter().find(|item|item["gameSkillId"]==SILENT_SMASHER && item["choiceId"]==2).unwrap()["silentLightModel"].clone();
        }
        if req.character.skills.iter().any(|skill|skill.name=="쉐도우 트랩" && skill.tripods.iter().any(|choice|choice.name=="덫 강화")) {
            reference.metadata["trap_range_policy"]=serde_json::json!("덫 강화 범위/높이150→180 원본 타격265700→265730; 피해/수급/연계·기본1타/시전/고유ID·생성 후400ms 유지·연속 공격/그림자 활성/그림자 올가미/그림자 늪 조합·ExGroup5/6은 Notify 출처로 보존. 고정 대상 임시 모델·다중 대상/반응/조건부 상속/성장/게임 교정 미지원");
        }
        if req.character.skills.iter().any(|skill|skill.name=="콜 오브 나이프" && skill.tripods.iter().any(|choice| matches!(choice.name.as_str(),"지속되는 부름"|"치명적인 그림자"))) {
            reference.metadata["call_projectile_policy"]=serde_json::json!("콜 지속되는 부름7타/치명적인 그림자5타·치명률30%p·입력 타격 수급30%; 시전1267ms/생성339ms·이후350ms 고정 간격·공포의 부름 또는 절망의 부름 조합. 고정 대상 임시 모델·실제 조건부 슬롯 상속·반응/성장/게임 교정 미지원·기본 수급 미확정/자연량 미보정");
        }
        if req.character.skills.iter().any(|skill|skill.name=="블랙 미스트" && skill.tripods.iter().any(|choice|choice.name=="넓은 안개")) {
            reference.metadata["wide_mist_policy"]=serde_json::json!("기본/거짓 안개/안개 폭발의 범위20% 타격 교체; 피해/수급/고유ID/Timer 간격 유지·퍼지는 안개 추가1타 유지·폭발+58%를 교체된3타에도1회 적용. 고정 대상 임시 모델·다중 대상/실제 범위·안개의 눈/미지원 층/성장/게임 교정 미지원·ParamValue3=4 내부 조건 상속 미확정");
        }
        if req.character.skills.iter().any(|skill| skill.name=="쉐도우 스톰" && skill.tripods.iter().any(|choice|choice.name=="검은 폭풍")) {
            reference.metadata["black_storm_policy"]=serde_json::json!("기본12타/시전 시간 유지·반복10타266001→266070·직접 피해+95%·20레벨 피해/게이지 키 동일. 범위/끌어당김/넘어짐 반응 보존만·고정 대상 임시 모델·성장/다른 미지원 층/실게임 시간 교정 미지원");
        }
        reference.metadata["target_condition_assumptions"] = serde_json::json!({
            "seed_or_lower":req.assumed_target_seed_or_lower,
            "abnormal_move_immune":req.assumed_target_abnormal_move_immune,
            "policy":"명시적 정적 가정·기본 false·선택 스킬 직접 타격만·독 미상속; 대상 등급/면역 변화 미모델링·Type0 활성화 연결 미확정·툴팁/명시적 CE 임시 모델"});
        reference.metadata["selected_tripods"] = serde_json::json!(req.character.skills.iter().flat_map(|skill| skill.tripods.iter().map(move |tripod| serde_json::json!({"skill":skill.name,"name":tripod.name,"level":tripod.level.unwrap_or(1)}))).collect::<Vec<_>>());

        if req.character.skills.iter().any(|chosen|chosen.name=="레이지 스피어" && chosen.tripods.iter().any(|choice|choice.name=="은밀한 암살")) {
            let skill=skills.iter_mut().find(|skill|skill.tags.contains(&SkillTag::SkillId(26810))).unwrap();
            let item=tripods.iter().find(|item|item["gameSkillId"]==26810 && item["choiceId"]==2).unwrap();
            let model=&item["rageReinputModel"];
            if let Some(route)=item["runtimeEffects"][0]["variants"].as_array().unwrap().iter().find(|route|skill.hits.iter().any(|hit|route["damageEffectIds"].as_array().unwrap().iter().any(|id|id.as_u64()==Some(hit.effect_id as u64)))) {
                let delay=route["actionDelayMs"].as_u64().unwrap() as u32;
                let buff=&model["movementBuff"];
                skill.timed_effects.push(crate::profile::skill::TimedSkillEffect {phase:0,action_delay_ms:delay,until_cast_end:false,action_end_ms:None,
                    effect:TriggerEffect::ApplyBuff(crate::profile::BuffSpec {id:buff["buffId"].as_str().unwrap().into(),effects:hashbrown::HashMap::from([(StatField::MovementSpeedMax,buff["value"].as_f64().unwrap())]),max_stacks:1,stack_type:crate::profile::StackType::Refresh,duration_ms:buff["durationMs"].as_u64().unwrap() as u32})});
                reference.metadata["rage_movement_notify_policy"]=serde_json::json!("은밀한 암살 선택 때만 기본/집중/처형의 마지막 타격 다음1ms 자동 재입력·원본02 입력 구간 안·후속933ms 동작/2초/+40%. 적중/빗나감 독립·시전 속도 보정·이후 시전 스냅샷 반영. 임시 입력 시점/Type57 인수/이동/취소·Duplex2 비교/게임 시각 미검증");
            }
        }
        let mut chaos_damage = std::collections::BTreeMap::new();
        let mut identity_gain = std::collections::BTreeMap::new();
        if let Some(call)=skills.iter_mut().find(|s|s.name=="콜 오브 나이프") {
            let control:serde_json::Value=serde_json::from_str(include_str!("../../skill-data/reaper/backend/call-control-rules.json")).unwrap();
            reference.metadata["call_control_policy"]=serde_json::json!({"slowEnabled":req.assumed_target_slow_vulnerable,"fearEnabled":req.assumed_target_nerve_vulnerable,"rules":control});
            if req.assumed_target_slow_vulnerable || req.assumed_target_nerve_vulnerable {
                let mark_id=control["markBuffId"].as_str().unwrap().to_string();
                let effects=if req.assumed_target_slow_vulnerable {hashbrown::HashMap::from([(StatField::TargetMovementSpeedReduction,control["slowPerStack"].as_f64().unwrap())])} else {hashbrown::HashMap::new()};
                let mark=BuffSpec {id:mark_id.clone(),effects,max_stacks:control["maxStacks"].as_u64().unwrap() as u32,stack_type:StackType::Refresh,duration_ms:control["durationMs"].as_u64().unwrap() as u32};
                let mut complete=Vec::new();
                if req.assumed_target_nerve_vulnerable {complete.push(TriggerEffect::ApplyTargetBuff(BuffSpec {id:control["fearBuffId"].as_str().unwrap().into(),effects:hashbrown::HashMap::new(),max_stacks:1,stack_type:StackType::Refresh,duration_ms:control["fearDurationMs"].as_u64().unwrap() as u32}));}
                complete.push(TriggerEffect::ConsumeTargetBuff {buff_id:mark_id.clone(),preserve_chance:0.,preserve_required_stacks:0,preserve_resource_gain:None});
                for hit in call.hits.iter_mut().filter(|h|control["effectIds"].as_array().unwrap().iter().any(|id|id.as_u64()==Some(h.effect_id as u64))) {
                    hit.triggers.push(HitTrigger {on:HitTriggerOn::OnHit,effect:TriggerEffect::ApplyTargetBuff(mark.clone()),chance:1.});
                    hit.triggers.push(HitTrigger {on:HitTriggerOn::OnHitIf(crate::profile::skill::HitCondition::TargetBuffAtMaxStacks(mark_id.clone())),effect:TriggerEffect::Sequence(complete.clone()),chance:1.});
                }
            }
        }
        for chosen in &req.character.skills {
            // State attachment order follows trusted choice IDs, independent of saved request order.
            let mut ordered_choices:Vec<_>=chosen.tripods.iter().collect();
            ordered_choices.sort_by_key(|c|tripods.iter().find(|t|t["skillName"]==chosen.name&&t["name"]==c.name).unwrap()["choiceId"].as_u64().unwrap());
            for choice in ordered_choices {
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["rawStaggerModel"].is_object()) {
                    let skill=skills.iter_mut().find(|s|s.name==chosen.name).unwrap();
                    for effect in item["runtimeEffects"].as_array().unwrap() {
                        for hit in &mut skill.hits {hit.stagger*=1.0+effect["value"].as_f64().unwrap();}
                    }
                    reference.metadata["raw_stagger_choice_policy"][&chosen.name]=item["rawStaggerModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["destructionModel"].is_object()) {
                    let skill=skills.iter_mut().find(|s|s.name==chosen.name).unwrap();
                    for effect in item["runtimeEffects"].as_array().unwrap() {
                        for hit in skill.hits.iter_mut().filter(|h|effect["effectId"].as_u64()==Some(h.effect_id as u64)) { hit.destruction=effect["value"].as_u64().unwrap() as u32; }
                    }
                }

                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["mistEyeModel"].is_object()) {
                    reference.metadata["mist_eye_policy"]=serde_json::json!({"centreEnabled":req.assumed_target_mist_centre,"model":item["mistEyeModel"]});
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["stormZoneModel"].is_object()) {
                    reference.metadata["storm_zone_policy"]=item["stormZoneModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["deathEyeModel"].is_object()) {
                    reference.metadata["death_eye_policy"]=item["deathEyeModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["shadowDotWideModel"].is_object()) {
                    reference.metadata["shadow_dot_wide_policy"]=item["shadowDotWideModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["phantomShadowModel"].is_object()) {
                    reference.metadata["phantom_shadow_policy"]=item["phantomShadowModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["iblisBreathModel"].is_object()) {
                    reference.metadata["iblis_breath_policy"]=item["iblisBreathModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["spinningRotationModel"].is_object()) {
                    reference.metadata["spinning_rotation_policy"]=item["spinningRotationModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["phantomLayerModel"].is_object()) {
                    reference.metadata["phantom_layer_policy"]=item["phantomLayerModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["deathCallModel"].is_object()) {
                    reference.metadata["death_call_policy"]=item["deathCallModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["iblisSwiftModel"].is_object()) {
                    reference.metadata["iblis_swift_policy"]=item["iblisSwiftModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["poisonLayerModel"].is_object()) {
                    reference.metadata["poison_layer_policy"][&chosen.name]=item["poisonLayerModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["deathTurnModel"].is_object()) {
                    reference.metadata["death_turn_policy"]=item["deathTurnModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["distortionModel"].is_object()) {
                    reference.metadata["distortion_policy"]=item["distortionModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["furyShadowModel"].is_object()) {
                    reference.metadata["fury_shadow_policy"]=item["furyShadowModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["blinkForwardModel"].is_object()) {
                    reference.metadata["blink_forward_policy"]=item["blinkForwardModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["blinkSwirlModel"].is_object()) {
                    reference.metadata["blink_swirl_policy"]=item["blinkSwirlModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["blinkCrushModel"].is_object()) {
                    reference.metadata["blink_crush_policy"]=item["blinkCrushModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["blinkDeathModel"].is_object()) {
                    reference.metadata["blink_death_policy"]=item["blinkDeathModel"].clone();
                }
                if let Some(item)=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name&&item["blinkStrengthModel"].is_object()) {
                    reference.metadata["blink_strength_policy"]=item["blinkStrengthModel"].clone();
                }
                for effect in tripods.iter().find(|item| item["skillName"] == chosen.name && item["name"] == choice.name).unwrap()["runtimeEffects"].as_array().unwrap() {
                    if effect["kind"]=="protection_windows" {
                        let skill=skills.iter_mut().find(|skill|skill.name==chosen.name).unwrap();
                        let variants=effect["variants"].as_array().unwrap();
                        let selected=|id:u64|chosen.tripods.iter().any(|choice|tripods.iter().any(|t|t["skillName"]==chosen.name&&t["name"]==choice.name&&t["choiceId"].as_u64()==Some(id)));
                        let condition=variants.iter().filter_map(|v|v["conditionChoiceId"].as_u64()).find(|&id|id!=0&&selected(id)).unwrap_or(0);
                        let variant=variants.iter().find(|v|v["conditionChoiceId"].as_u64()==Some(condition)).ok_or("리퍼 보호 구간에 대응하는 공격 경로가 없습니다")?;
                        // shortcut: records exact provisional windows; incoming attacks, interrupts and holding timing need combat input and game calibration.
                        for window in variant["windows"].as_array().unwrap() {
                            let buff=BuffSpec {id:effect["buffId"].as_str().unwrap().into(),effects:hashbrown::HashMap::new(),max_stacks:1,stack_type:StackType::Refresh,duration_ms:0};
                            skill.timed_effects.push(crate::profile::skill::TimedSkillEffect {phase:0,action_delay_ms:window["actionDelayMs"].as_u64().unwrap() as u32,until_cast_end:true,action_end_ms:Some(window["actionEndMs"].as_u64().unwrap() as u32),effect:TriggerEffect::ApplyBuff(buff)});
                        }
                        let mut policy=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name).unwrap()["protectionWindowModel"].clone();
                        policy["selectedVariant"]=variant.clone();reference.metadata["protection_window_policies"][chosen.name.as_str()]=policy;
                    }
                    if effect["kind"]=="attack_stage_speed" {
                        tripod_stage_speeds.push(effect.clone());
                        reference.metadata["spirit_leap_policy"]=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name).unwrap()["spiritLeapModel"].clone();
                    }
                    if effect["kind"]=="cast_movement_buff" {
                        let buff=BuffSpec {id:effect["buffId"].as_str().unwrap().into(),effects:hashbrown::HashMap::from([(StatField::MovementSpeed,effect["value"].as_f64().unwrap())]),max_stacks:1,stack_type:StackType::Refresh,duration_ms:0};
                        skills.iter_mut().find(|s|s.name==chosen.name).unwrap().timed_effects.push(crate::profile::skill::TimedSkillEffect {phase:0,action_delay_ms:0,until_cast_end:true,action_end_ms:None,effect:TriggerEffect::ApplyBuff(buff)});
                        reference.metadata["mist_mobility_policy"]=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name).unwrap()["mistMobilityModel"].clone();
                    }
                    if effect["kind"]=="skill_evasion" {
                        // shortcut: full normal/holding cast only; incoming attacks, dodge rolls and early cancellation are not modelled.
                        let buff=BuffSpec {id:effect["buffId"].as_str().unwrap().into(),effects:hashbrown::HashMap::from([(StatField::DodgeRate,effect["value"].as_f64().unwrap())]),max_stacks:1,stack_type:StackType::Refresh,duration_ms:0};
                        skills.iter_mut().find(|s|s.name==chosen.name).unwrap().timed_effects.push(crate::profile::skill::TimedSkillEffect {phase:0,action_delay_ms:0,until_cast_end:true,action_end_ms:None,effect:TriggerEffect::ApplyBuff(buff)});
                        let item=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name).unwrap();
                        reference.metadata["evasion_policy"]=if item["gustModel"].is_object() {item["gustModel"].clone()} else {item["evasionModel"].clone()};
                    }
                    if effect["kind"]=="dagger_cooldown_buff" {
                        let buff=BuffSpec {id:effect["buffId"].as_str().unwrap().into(),effects:hashbrown::HashMap::new(),max_stacks:1,stack_type:StackType::Refresh,duration_ms:effect["durationMs"].as_u64().unwrap() as u32};
                        for hit in &mut skills.iter_mut().find(|skill|skill.name==chosen.name).unwrap().hits {hit.triggers.push(HitTrigger {on:HitTriggerOn::OnHit,effect:TriggerEffect::ApplyBuff(buff.clone()),chance:1.});}
                        reference.metadata["shabel_mark_policy"]=effect.clone();
                        reference.metadata["shabel_mark_model"]=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name).unwrap()["shabelMarkModel"].clone();
                    }
                    if effect["kind"]=="silent_shadow_target" {
                        let id=effect["buffId"].as_str().unwrap().to_string();
                        let buff=BuffSpec {id:id.clone(),effects:hashbrown::HashMap::new(),max_stacks:effect["maxStacks"].as_u64().unwrap() as u32,stack_type:crate::profile::StackType::Refresh,duration_ms:effect["durationMs"].as_u64().unwrap() as u32};
                        for hit in &mut skills.iter_mut().find(|skill|skill.name==chosen.name).unwrap().hits {
                            for condition in [crate::profile::skill::HitCondition::TargetBuffActive(id.clone()),crate::profile::skill::HitCondition::TargetBuffAtMaxStacks(id.clone())] {
                                hit.triggers.push(HitTrigger {on:HitTriggerOn::OnHitIf(condition),effect:TriggerEffect::DamageBonus(effect["value"].as_f64().unwrap()),chance:1.});
                            }
                            if effect["shadowEffectIds"].as_array().unwrap().iter().any(|eid|eid.as_u64()==Some(hit.effect_id as u64)) {
                                hit.triggers.push(HitTrigger {on:HitTriggerOn::OnHit,effect:TriggerEffect::ApplyTargetBuff(buff.clone()),chance:1.});
                            }
                        }
                        reference.metadata["silent_shadow_policy"]=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name).unwrap()["silentShadowModel"].clone();
                    }
                    if effect["kind"]=="call_wide_route" {
                        let condition=chosen.tripods.iter().filter_map(|c|tripods.iter().find(|t|t["skillName"]==chosen.name&&t["name"]==c.name)).filter_map(|t|t["choiceId"].as_u64()).find(|id|matches!(id,4|6)).unwrap_or(0);
                        let variant=effect["variants"].as_array().unwrap().iter().find(|v|v["conditionChoiceId"].as_u64()==Some(condition)).unwrap();
                        reference.metadata["call_wide_policy"]=serde_json::json!({"rangeMultiplier":1.+effect["value"].as_f64().unwrap(),"wideWrapperId":variant["wideWrapperId"],"rawCallbackWord9":variant["callbackWord9Raw"],"policy":"기본08/지속48/치명68 명시적 고정 대상·피해/수급/5/7/5타/시각/제어 유지. 범위 배율 기록만·다중 대상 커버리지/이동·Param3/4 내부 조건·기본 막타word9=1 실제 의미·성장/게임 교정 미확정"});
                    }
                    if effect["kind"]=="target_slow" {
                        reference.metadata["target_slow_policy"]=serde_json::json!({"enabled":req.assumed_target_slow_vulnerable,"policy":"명시적 감속 가능 고정 대상·직접 적중/지원 콜백마다 갱신·각 singleton 타이머·활성 최대 감속. Duplex2 비교/콜백 상속/면역/실제 이동/AI/게임 교정 미확정; 자기 속도/피해 변화 없음"});
                        if !req.assumed_target_slow_vulnerable {continue;}
                        let buff=BuffSpec {id:effect["buffId"].as_str().unwrap().into(),effects:hashbrown::HashMap::from([(StatField::TargetMovementSpeedReduction,effect["value"].as_f64().unwrap())]),max_stacks:1,stack_type:StackType::Refresh,duration_ms:effect["durationMs"].as_u64().unwrap() as u32};
                        for hit in &mut skills.iter_mut().find(|s|s.name==chosen.name).unwrap().hits {
                            hit.triggers.push(HitTrigger {on:HitTriggerOn::OnHit,effect:TriggerEffect::ApplyTargetBuff(buff.clone()),chance:1.});
                        }
                    }
                    if effect["kind"]=="nerve" {
                        reference.metadata["nerve_policy"]=serde_json::json!({"enabled":req.assumed_target_nerve_vulnerable,"policy":"명시적 암흑/공포 적용 가능 가정·막타1공유중첩/4초갱신·3중첩 공포3초단일상태 뒤 암흑소비·원본 Overlap0 보존/실행1상태는 임시·피해/시야/이동/AI/실제면역/게임교정 미계산"});
                        if !req.assumed_target_nerve_vulnerable {continue;}
                        let darkness_id="reaper:nerve:darkness".to_string();
                        let darkness=BuffSpec {id:darkness_id.clone(),effects:hashbrown::HashMap::new(),max_stacks:3,stack_type:StackType::Refresh,duration_ms:4000};
                        let fear=BuffSpec {id:"reaper:nerve:fear".into(),effects:hashbrown::HashMap::new(),max_stacks:1,stack_type:StackType::Refresh,duration_ms:3000};
                        // ponytail: explicit vulnerable target and singleton fear; native immunity/Overlap0 need game calibration.
                        for hit in skills.iter_mut().find(|s|s.name==chosen.name).unwrap().hits.iter_mut().filter(|hit|Some(hit.effect_id as u64)==effect["effectId"].as_u64()) {
                            hit.triggers.push(HitTrigger {on:HitTriggerOn::OnHit,effect:TriggerEffect::ApplyTargetBuff(darkness.clone()),chance:1.});
                            hit.triggers.push(HitTrigger {on:HitTriggerOn::OnHitIf(crate::profile::skill::HitCondition::TargetBuffAtMaxStacks(darkness_id.clone())),effect:TriggerEffect::Sequence(vec![TriggerEffect::ApplyTargetBuff(fear.clone()),TriggerEffect::ConsumeTargetBuff {buff_id:darkness_id.clone(),preserve_chance:0.,preserve_required_stacks:0,preserve_resource_gain:None}]),chance:1.});
                        }
                    }
                    if effect["kind"]=="end_cooldown_ms" {
                        let skill=skills.iter_mut().find(|skill|skill.name==chosen.name).unwrap();
                        skill.on_end_if_hit.push(TriggerEffect::ReduceCooldownsFlat {skill_ids:vec![skill.skill_id],amount_ms:effect["value"].as_u64().unwrap()});
                        reference.metadata["storm_end_cooldown_policy"]=serde_json::json!("전체 시전 종료까지 고정 적 하나에1회 이상 적중하면 남은 쿨1.3초 감소·전타 빗나감0·같은 적 다타를1명 집계·시전별 초기화·마지막 타격 후/다음 행동 전 종료. 미리 종료/취소/중단·다중 적/12.5초상한·ParamValue10 내부 의미·게임 교정은 미지원인 임시 모델");
                    }
                    if effect["kind"] == "movement_buff" {
                        let buff=crate::profile::BuffSpec {id:effect["buffId"].as_str().unwrap().into(),
                            effects:hashbrown::HashMap::from([(StatField::MovementSpeedMax,effect["value"].as_f64().unwrap())]),
                            max_stacks:1,stack_type:crate::profile::StackType::Refresh,duration_ms:effect["durationMs"].as_u64().unwrap() as u32};
                        let skill=skills.iter_mut().find(|skill|skill.name==chosen.name).unwrap();
                        if let Some(delay)=effect["actionDelayMs"].as_u64() {
                            let delay=chosen.tripods.iter().filter_map(|choice|tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name)).find_map(|item|item["distortionModel"]["gustNotifyDelayMs"].as_u64()).unwrap_or(delay);
                            skill.timed_effects.push(crate::profile::skill::TimedSkillEffect {phase:0,action_delay_ms:delay as u32,until_cast_end:false,action_end_ms:None,effect:TriggerEffect::ApplyBuff(buff)});
                            reference.metadata["gust_policy"]=tripods.iter().find(|item|item["skillName"]==chosen.name&&item["name"]==choice.name).unwrap()["gustModel"].clone();
                        } else {
                            for hit in &mut skill.hits {hit.triggers.push(HitTrigger {on:HitTriggerOn::OnHit,effect:TriggerEffect::ApplyBuff(buff.clone()),chance:1.});}
                            reference.metadata["movement_buff_policy"]=serde_json::json!("콜3초/+19.2%·트랩4초/+24.6%; 적중 후 소유자 버프/각 시간 갱신·빗나감/독 미발동. 양의 비교형 이동 버프는 활성 최댓값·독립 만료의 임시 정책. Duplex2/decrease_compare의 게임 중복 판정·콜백 상속/성장은 미검증");
                        }
                    }
                    if matches!(effect["kind"].as_str(),Some("corrosion"|"bleeding")) {
                        use crate::profile::{BuffSpec,StackType,RuntimeDamageSpec};
                        let bleeding = effect["kind"]=="bleeding";
                        let poison_id = format!("reaper:{}",effect["kind"].as_str().unwrap());
                        let poison = BuffSpec { id:poison_id.clone(),effects:hashbrown::HashMap::new(),max_stacks:effect["maxStacks"].as_u64().unwrap() as u32,
                            stack_type:StackType::Refresh,duration_ms:effect["durationMs"].as_u64().unwrap() as u32 };
                        let damage = RuntimeDamageSpec {destruction:0,hit_id:format!("{poison_id}:tick"),name:if bleeding {"출혈 독"} else {"부식 독"}.into(),base_damage:effect["fixedDamage"].as_f64().unwrap(),
                                skill_modifier:effect["damageRatio"].as_f64().unwrap(),damage_increase:0.,crit_chance_bonus:0.,crit_damage_bonus:0.,
                                random_crit_damage_chance:0.,random_crit_damage_bonus:0.,defense_ignore:0.,defense_ignore_chance:0.};
                        let dot = TriggerEffect::ScheduleTargetStackDot { dot_id:poison_id.clone(),stack_buff_id:poison_id.clone(),hit:damage.clone(),
                            first_tick_ms:effect["firstTickMs"].as_u64().unwrap() as u32,tick_interval_ms:effect["intervalMs"].as_u64().unwrap() as u32,
                            tick_count:(effect["durationMs"].as_u64().unwrap()/effect["intervalMs"].as_u64().unwrap()) as u32 };
                        let mut on_hit=vec![TriggerEffect::ApplyTargetBuff(poison.clone())];
                        if !bleeding {
                            on_hit.push(TriggerEffect::ApplyTargetBuff(BuffSpec {id:"reaper:corrosion:defense".into(),effects:hashbrown::HashMap::from([(StatField::TargetDefenseReduction,effect["defenseReduction"].as_f64().unwrap())]),
                                max_stacks:1,stack_type:StackType::Refresh,duration_ms:effect["defenseDurationMs"].as_u64().unwrap() as u32 }));
                        }
                        on_hit.push(dot);
                        let skill=skills.iter_mut().find(|skill| skill.name==chosen.name).unwrap();
                        let back=skill.tags.contains(&SkillTag::AttackBack);
                        let mut chance_pattern=None;
                        for hit in &mut skill.hits {
                            if Some(hit.effect_id as u64)==effect["effectId"].as_u64() {
                                let trigger_start=hit.triggers.len();
                                for effect in on_hit.clone() {
                                    hit.triggers.push(HitTrigger {on:HitTriggerOn::OnHit,effect,chance:1.});
                                }
                                if bleeding {
                                    let burst_id="reaper:bleeding:burst".to_string();
                                    for effect in [
                                        TriggerEffect::ApplyTargetBuff(BuffSpec {id:burst_id.clone(),effects:hashbrown::HashMap::new(),max_stacks:1,stack_type:StackType::Refresh,duration_ms:effect["burstDurationMs"].as_u64().unwrap() as u32}),
                                        TriggerEffect::ScheduleTargetStackDot {dot_id:burst_id.clone(),stack_buff_id:burst_id,hit:RuntimeDamageSpec {hit_id:"reaper:bleeding:burst".into(),name:"과다출혈".into(),base_damage:effect["burstFixedDamage"].as_f64().unwrap(),skill_modifier:effect["burstDamageRatio"].as_f64().unwrap(),..damage.clone()},first_tick_ms:0,tick_interval_ms:1000,tick_count:1},
                                        TriggerEffect::ConsumeTargetBuff {buff_id:poison_id.clone(),preserve_chance:0.,preserve_required_stacks:0,preserve_resource_gain:None}
                                    ] {
                                        hit.triggers.push(HitTrigger {on:HitTriggerOn::OnHitIf(crate::profile::skill::HitCondition::TargetBuffAtMaxStacks(poison_id.clone())),effect,chance:1.});
                                    }
                                    let per_stack=hit.triggers[trigger_start..].to_vec();
                                    chance_pattern=Some(per_stack.clone());
                                    let mut chains:Vec<_>=chosen.tripods.iter().flat_map(|choice| tripods.iter().find(|item|item["skillName"]==chosen.name && item["name"]==choice.name).unwrap()["inactiveChainEffects"].as_array().into_iter().flatten()).collect();
                                    chains.sort_by_key(|chain|chain["sourceRow"].as_u64().unwrap());
                                    for chain in chains {
                                        if chain["executionSupported"]!=true || chain["effectId"]!=effect["effectId"] || (chain["procOn"]=="back" && !back) {continue;}
                                        // Each original CombatEffect row applies once; max-stack consumption runs between rows.
                                        for _ in 0..chain["extraStacks"].as_u64().unwrap() {
                                            for mut trigger in per_stack.clone() {
                                                if chain["procOn"]=="crit" && matches!(trigger.on,HitTriggerOn::OnHit) {trigger.on=HitTriggerOn::OnCrit;}
                                                hit.triggers.push(trigger);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(pattern)=chance_pattern {
                            for chance in chosen.tripods.iter().filter_map(|choice| tripods.iter().find(|item|item["skillName"]==chosen.name && item["name"]==choice.name).unwrap().get("bleedChance")) {
                                let chance=if let Some(variants)=chance["variants"].as_array() {
                                    let swift=chosen.tripods.iter().any(|c|tripods.iter().any(|t|t["skillName"]==chosen.name&&t["name"]==c.name&&t["choiceId"]==8));
                                    variants.iter().find(|v|v["swiftSelected"]==swift).unwrap()
                                } else {chance};
                                if chance.get("patternEffectId").is_some_and(|id|id!=&effect["effectId"]) {continue;}
                                // One roll for apply + timer; max-stack effects follow synchronously.
                                // Reachable target stacks never remain at max after a completed hit.
                                let packet=TriggerEffect::Sequence(pattern.iter().filter(|trigger| matches!(trigger.on,HitTriggerOn::OnHit)).map(|trigger|trigger.effect.clone()).collect());
                                for hit in &mut skill.hits {
                                    let swift=effect["sourceArtifact"]=="research/poison-layer-source-audit.json";
                                    if if let Some(targets)=chance["targetEffectIds"].as_array() { !targets.iter().any(|id|id.as_u64()==Some(hit.effect_id as u64)) } else if swift {Some(hit.effect_id as u64)!=effect["effectId"].as_u64()} else {!matches!(hit.effect_id,261120|261121|261122)} {continue;}
                                    for _ in 0..chance["extraStacks"].as_u64().unwrap() {
                                    hit.triggers.push(HitTrigger {on:if chance["procOn"]=="crit" {HitTriggerOn::OnCrit} else {HitTriggerOn::OnHit},effect:packet.clone(),chance:chance["probability"].as_f64().unwrap()});
                                    hit.triggers.extend(pattern.iter().filter(|trigger| matches!(trigger.on,HitTriggerOn::OnHitIf(_))).cloned());
                                    }
                                }
                                if chosen.name=="샤벨 스팅거" {reference.metadata["shabel_bleed_chance_binding"]=serde_json::json!("선택된 출혈의 기본4타/속전속결2타에 툴팁/유일 CE 대응으로 추가중첩 적용; raw Type0 활성화와 새 층 상속은 미확정인 명시적 임시 모델. 확률은 각 hit의 중첩/틱 예약에1회, 최대중첩 처리/소비는 같은 hit에서 순차 실행; 독 틱/과다출혈에는 추가효과 없음");}
                            }
                        }
                    }
                    if effect["kind"] == "chaos_damage" {
                        let id = req.skill_data.iter().find(|skill| skill.name == chosen.name).unwrap().game_skill_id;
                        chaos_damage.insert(id, effect["value"].as_f64().unwrap());
                    }
                    if effect["kind"] == "identity_gain" {
                        let id = req.skill_data.iter().find(|skill| skill.name == chosen.name).unwrap().game_skill_id;
                        identity_gain.insert(id, effect["value"].as_f64().unwrap());
                    }
                    if effect["kind"] != "active_cooldown_ms" { continue; }
                    let source = skills.iter().find(|skill| skill.name == chosen.name).unwrap().skill_id;
                    let target_ids: Vec<_> = skills.iter().filter(|skill| skill.tags.iter().any(|tag| matches!(tag, SkillTag::SkillId(id) if effect["targetSkillIds"].as_array().unwrap().iter().any(|target| target.as_u64() == Some(*id as u64))))).map(|skill| skill.skill_id).collect();
                    global_triggers.push(crate::profile::GlobalTrigger {
                        filter: crate::profile::skill::TriggerFilter::SkillCastId(source), condition: crate::profile::skill::TriggerCondition::Always,
                        effect: TriggerEffect::ReduceCooldownsFlat {skill_ids:target_ids,amount_ms:effect["value"].as_u64().unwrap()},cooldown_ms:None,
                    });
                }
            }
        }
        reference.metadata["chaos_tripod_damage"] = serde_json::json!(chaos_damage);
        reference.metadata["identity_tripod_gain"] = serde_json::json!(identity_gain);
        reference.metadata["ark_grid_policy"] = serde_json::json!("질서 별4종·혼돈 해 재빠른/현란한 공격·달 불타는 일격·별 공격/무기/속도 코어·4등급의 원본 효과를 임시 지원(화상은 기본 보스 이상 대상, 시드 이하 가정이면 코어 전체 효과 제외·매 적중 재시작6틱). 요구 깨달음·의지력 예산·젬4칸 검증, 합산 젬 효과40레벨까지 원본 조회. 유언 선택 조건 포함, 다른 코어/조건 미지원");
        reference.metadata["selected_ark_grids"] = serde_json::json!(req.character.ark_grids.iter().flat_map(|grid| grid.reaper_slots.iter().map(|slot| serde_json::json!({
            "name":slot.name,"grade":slot.grade,"points":slot.gems.iter().map(|gem| gem.gem_point).sum::<u32>(),
            "gems":slot.gems.iter().map(|gem| serde_json::json!({"name":gem.name,"point":gem.gem_point,"efficiency":gem.willpower_efficiency,
                "effects":gem.effects.iter().map(|effect| serde_json::json!({"name":effect.name,"level":effect.level})).collect::<Vec<_>>()})).collect::<Vec<_>>()}))).collect::<Vec<_>>());
        reference.metadata["input_mode"] = serde_json::json!("configured_build");
        if let Some(parameters) = passive_parameters(req, reference.scenario)? {
            let percent = |key: &str| parameters[key].as_f64().unwrap_or(0.0) / 100.0;
            for skill in skills.iter_mut() {
                let knife = skill.tags.contains(&SkillTag::SkillId(26960));
                let mut damage = 1.0;
                if skill.slot == SkillSlot::SuperAwakening { damage *= 1.0 + percent("super_damage"); }
                if skill.slot == SkillSlot::Awakening { skill.max_uses += parameters["awakening_uses"].as_u64().unwrap_or(0) as u32; }
                if skill.slot == SkillSlot::HyperAwakeningTechniques {
                    damage *= 1.0 + percent("technique_damage");
                    let speed = 1.0 + percent("technique_speed");
                    for duration in &mut skill.cast_times_ms { *duration = (*duration as f64 / speed) as u32; }
                    for hit in &mut skill.hits { hit.action_delay_ms = (hit.action_delay_ms as f64 / speed) as u32; }
                    skill.cooldown_ms = (skill.cooldown_ms as f64 * (1.0 - percent("technique_cooldown"))) as u32;
                }
                if knife {
                    damage *= 1.0 + percent("knife_damage");
                    skill.cooldown_ms = (skill.cooldown_ms as f64 * (1.0 - percent("knife_cooldown"))) as u32;
                }
                for hit in &mut skill.hits {
                    hit.damage_increase = (1.0 + hit.damage_increase) * damage - 1.0;
                    if knife && percent("knife_cooldown") > 0.0 { hit.crit_chance_bonus += 0.20; }
                }
            }
            if percent("charged_fury") > 0.0 {
                global_triggers.push(crate::profile::GlobalTrigger {
                    filter: crate::profile::skill::TriggerFilter::AnyHit, condition: crate::profile::skill::TriggerCondition::Always,
                    effect: TriggerEffect::ReduceCooldownsOnResourceFull { resource: "UltimatePoint".into(),
                        skill_ids: skills.iter().filter(|skill| skill.slot == SkillSlot::Awakening).map(|skill| skill.skill_id).collect(), percent: percent("charged_fury") }, cooldown_ms: None,
                });
            }
            reference.metadata["selected_passives"] = parameters;
            reference.metadata["fixed_passives"] = serde_json::Value::Null;
        }
        reference.metadata["passive_policy"] = serde_json::json!("깨달음12·도약10노드 전 레벨; 대체/급습/태세/방향 조건·손놀림5중첩6초; T 시전/Action 지연 보정, Timer 공속 독립; 충전된 분노는 기존 공통 첫 적중 만충 감지1회. 이동/무력화/독은 미계산. 빈 선택의 기존 저장 설정은 고정 묶음 유지");
        if let Some(model) = &req.reaper_identity_model {
            reference.metadata["identity_model"] = serde_json::to_value(model).unwrap();
            reference.metadata["identity_model_status"] = serde_json::json!("explicit_calibration_inputs_not_game_measurements");
            reference.metadata["identity_model_policy"] = serde_json::json!("1=만충; 선택 스킬의 각 적중에 입력량, 굶주림은 혼돈 수급에 별도 적용. 적중 전 어둠 분기/초과분 폐기·혼돈 만충15초 진입·종료 후 재수급; 자연 변화100ms 검사·페르소나 어둠0 종료. 입력값은 실측/커뮤니티 확정값이 아님");
        }
        reference.metadata["shadow_dot_gain_multiplier"] = serde_json::json!((1.0 + stat.specialization / 699.0 * 0.3) / (1.0 + 600.0 / 699.0 * 0.3));
        reference.metadata["specialization_damage_multiplier"] = serde_json::json!(spec_damage);
        reference.metadata["excluded_scope"] = serde_json::json!(["나머지 트라이포드·2~5레벨 성장", "풍요 등 질풍 외 룬", "독·무력화·이동 효과", "분신 AI/거리/선택 상속 교정", "자연 변화", "혼돈 진입/재진입", "초각성 수급·은신/이동", "아크그리드", "에스더 무기"]);
        if req.reaper_identity_model.is_some() {
            reference.metadata["excluded_scope"].as_array_mut().unwrap().retain(|scope| scope != "자연 변화" && scope != "혼돈 진입/재진입");
            reference.metadata["excluded_scope"].as_array_mut().unwrap().push(serde_json::json!("입력 수급/자연량의 실제 게임 정합성"));
            reference.metadata["initial_state"] = reference.metadata["identity_model"].clone();
        }
        reference.metadata.as_object_mut().unwrap().remove("reference_attack_speed_percent");
        reference.metadata.as_object_mut().unwrap().remove("reference_base_attack_power");
    } else {
        stat.specialization = 600.0;
        stat.base_attack_power = 10000.0;
        stat.insert(StatField::AttackSpeed, "reaper_reference:공속140%", 0.40);
    }
    // No Arcana specialization or card-gauge scaling is applied to this class.
    debug_assert!(skills.iter().all(|skill| skill.tags.iter().any(|tag| matches!(tag, SkillTag::SkillId(_)))));
    super::reaper_grid::apply_stage_speed(skills,&tripod_stage_speeds);
    Ok(Some(reference))
}
