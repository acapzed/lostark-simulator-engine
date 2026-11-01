//! Explicit prepared research snapshots through the common gear/APL pipeline.
//! Ordinary prepare/build_raid deliberately continues rejecting Slayer.
use crate::profile::{CharacterProfiles, GlobalTrigger, SkillTag, StatField};
use crate::profile::actor::BuffMetadata;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    schema: u32,
    model: String,
    character_level: u32,
    phase: String,
    specialization: f64,
    automatic_fury_bound: bool,
    static_stats: StaticStats,
    extra_triggers: Vec<GlobalTrigger>,
    cast_buff_damage_overlays: Vec<CastOverlay>,
    live_buff_overlays: Vec<LiveOverlay>,
    scaled_resource_overlays: Vec<ResourceOverlay>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StaticStats {
    damage_multiplier: f64,
    crit_rate_percent: f64,
    crit_damage_percent: f64,
    attack_speed_bonus: f64,
    move_speed_bonus: f64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CastOverlay { game_skill_id:u32, buff_id:String, bonus:f64 }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LiveOverlay { game_skill_id:u32, buff_id:String, damage:f64, crit_damage:f64 }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceOverlay { game_skill_id:u32, resource:String, cap:f64, bonus:f64, buff_id:Option<String> }

fn valid_number(value:f64)->Result<(),String>{
    if value.is_finite() {Ok(())} else {Err("슬레이어 고정 상태: 유한한 수 필요".into())}
}
fn references(value:&Value, count:usize)->Result<(),String>{
    match value {
        Value::Object(fields)=>for (key,v) in fields {
            if key=="SkillCastId" || key=="skill_id" {
                if !v.as_u64().is_some_and(|i|i<count as u64){return Err("슬레이어 사건의 실행 스킬ID 범위 밖".into());}
            } else if key=="skill_ids" {
                let ids=v.as_array().ok_or("슬레이어 사건의 스킬ID 배열 필요")?;
                if ids.iter().any(|i|!i.as_u64().is_some_and(|i|i<count as u64)){return Err("슬레이어 사건의 실행 스킬ID 범위 밖".into());}
            }
            references(v,count)?;
        },
        Value::Array(items)=>for v in items {references(v,count)?;},
        _=>{}
    }
    Ok(())
}

pub fn build(json:&str)->Result<CharacterProfiles,String>{
    let mut input:Value=serde_json::from_str(json).map_err(|e|format!("슬레이어 고정 상태 입력: {e}"))?;
    if input["character"]["className"]!="슬레이어" {return Err("슬레이어 고정 상태 전용 입력입니다".into());}
    let raw=input.get("slayerSnapshot").ok_or("명시적 슬레이어 고정 상태 계약 필요")?;
    let s:Snapshot=serde_json::from_value(raw.clone()).map_err(|e|format!("슬레이어 고정 상태 계약: {e}"))?;
    if s.schema!=1 || s.model!="fixed_state" || s.character_level!=70 || s.automatic_fury_bound
        || !["normal","burst","exhaustion"].contains(&s.phase.as_str()) {
        return Err("슬레이어 자동 분노/상태 전환은 미지원입니다. fixed_state v1만 허용합니다".into());
    }
    valid_number(s.specialization)?;
    if s.specialization<0. {return Err("슬레이어 특화는0이상이어야 합니다".into());}
    let stats=&s.static_stats;
    for n in [stats.damage_multiplier,stats.crit_rate_percent,stats.crit_damage_percent,stats.attack_speed_bonus,stats.move_speed_bonus] {valid_number(n)?;}
    let burst=s.phase=="burst";
    if stats.damage_multiplier<=0. || stats.crit_rate_percent!=if burst {30.} else {0.}
        || stats.attack_speed_bonus!=if burst {0.2} else {0.}
        || stats.move_speed_bonus!=if burst {0.2} else {0.}
        || (!burst && stats.crit_damage_percent!=0.) {
        return Err("슬레이어 고정 상태의 피해/폭주 보정 계약 불일치".into());
    }
    for path in ["/character/arkPassive/classNodes","/character/arkGrids/classCores","/cardData"] {
        if input.pointer(path).and_then(Value::as_array).is_some_and(|v|!v.is_empty()) {
            return Err("슬레이어 고정 상태의 중복 클래스 효과/카드 입력은 허용하지 않습니다".into());
        }
    }
    let count=input["character"]["skills"].as_array().ok_or("슬레이어 스킬 목록 필요")?.len();
    if count==0 || count>13 {return Err("슬레이어 고정 상태 스킬 수 범위 밖".into());}
    let data=input["skillData"].as_array().ok_or("슬레이어 가공 스킬 데이터 필요")?;
    if data.len()!=count {return Err("슬레이어 선택/가공 스킬 수 불일치".into());}
    let ordinary=[45050,45060,45070,45080,45100,45110,45190,45210,45220,45300,45600,45610,45620,45700,45710,45720,45730,45740];
    let mut seen=std::collections::HashSet::new();let mut slots=[0;6];
    for (chosen,row) in input["character"]["skills"].as_array().unwrap().iter().zip(data) {
        let id=row["gameSkillId"].as_u64().ok_or("슬레이어 원본 스킬ID 필요")?;
        let level=chosen["level"].as_u64().ok_or("슬레이어 스킬 레벨 필요")?;
        if !seen.insert(id) || chosen["name"]!=row["name"] {return Err("슬레이어 중복/스킬 이름 불일치".into());}
        let slot=if ordinary.contains(&id) {0} else {match id {
            45750|45760=>1,45800|45810=>2,45820|45830=>3,45004=>4,45003=>5,
            _=>return Err("슬레이어 범위 밖 스킬ID".into()),
        }};
        if level==0 || level>if slot==0 {20} else {1} || (slot>=4&&!burst) {return Err("슬레이어 스킬 레벨/고정 상태 불일치".into());}
        slots[slot]+=1;
    }
    if slots[0]>8 || slots[1..].iter().any(|n|*n>1) {return Err("슬레이어 슬롯별 스킬 수 초과".into());}
    references(&raw["extraTriggers"],count)?;
    for (key,v) in [("assumedTargetSeedOrLower",false),("assumedTargetAbnormalMoveImmune",true),
                    ("assumedTargetNerveVulnerable",false),("assumedTargetSlowVulnerable",false)] {
        input[key]=Value::Bool(v);
    }
    let mut profiles=super::build_inner(&input.to_string(),true)?;
    if (profiles.stat.specialization-s.specialization).abs()>1e-9 {
        return Err(format!("슬레이어 특화 기준 불일치: 가공 {}, 공용 스탯 {}",s.specialization,profiles.stat.specialization));
    }
    // A common cast-speed rune changes the owner clock. Keep independently
    // scheduled projectile/installed delays fixed, as in the reviewed routes.
    for skill in &mut profiles.skills {
        let row=input["skillData"].as_array().unwrap().iter().find(|r|r["name"]==skill.name).ok_or("슬레이어 원본 동작 시계 없음")?;
        let source=row["castTimesMs"][0].as_f64().filter(|n|*n>0.).ok_or("슬레이어 원본 동작 시간은0보다 커야 합니다")?;
        let scale=skill.cast_times_ms[0] as f64/source;
        if scale!=1. {for hit in &mut skill.hits {hit.action_delay_ms=(hit.action_delay_ms as f64*scale).floor()as u32;}}
    }
    let find=|id|profiles.skills.iter().position(|skill|skill.tags.contains(&SkillTag::SkillId(id)));
    let mut cast_indices=Vec::new();let mut live_indices=Vec::new();let mut resource_indices=Vec::new();
    for o in &s.cast_buff_damage_overlays {valid_number(o.bonus)?;cast_indices.push(find(o.game_skill_id).ok_or("시전 강화 대상 스킬 없음")?);}
    for o in &s.live_buff_overlays {valid_number(o.damage)?;valid_number(o.crit_damage)?;live_indices.push(find(o.game_skill_id).ok_or("적중 강화 대상 스킬 없음")?);}
    for o in &s.scaled_resource_overlays {
        valid_number(o.cap)?;valid_number(o.bonus)?;
        if o.resource!="Fury" || o.cap<=0. {return Err("슬레이어 외부 분노 비례 계약 오류".into());}
        resource_indices.push(find(o.game_skill_id).ok_or("분노 비례 대상 스킬 없음")?);
    }
    for (o,i) in s.cast_buff_damage_overlays.into_iter().zip(cast_indices) {profiles.skills[i].cast_buff_damage_bonuses.push((o.buff_id,o.bonus));}
    for (o,i) in s.live_buff_overlays.into_iter().zip(live_indices) {
        if o.damage!=0. {profiles.skills[i].buff_damage_bonuses.push((o.buff_id.clone(),o.damage));}
        if o.crit_damage!=0. {profiles.skills[i].buff_crit_damage_bonuses.push((o.buff_id,o.crit_damage));}
    }
    for (o,i) in s.scaled_resource_overlays.into_iter().zip(resource_indices) {
        profiles.skills[i].cast_resource_scaled_damage_bonuses.push(crate::profile::skill::ResourceScaledDamage {
            resource:o.resource,cap:o.cap,bonus:o.bonus,buff_id:o.buff_id,
        });
    }
    for (field,key,value) in [(StatField::DamageIncrease,"slayer_snapshot:damage",stats.damage_multiplier-1.),
        (StatField::CritRate,"slayer_snapshot:burst",stats.crit_rate_percent),
        (StatField::CritDmg,"slayer_snapshot:crit_damage",stats.crit_damage_percent),
        (StatField::AttackSpeed,"slayer_snapshot:burst",stats.attack_speed_bonus),
        (StatField::MovementSpeed,"slayer_snapshot:burst",stats.move_speed_bonus)] {profiles.stat.insert(field,key,value);}
    profiles.global_triggers.extend(s.extra_triggers);
    // This remains the already validated MP/Fury/UltimatePoint research shell.
    // Preserve explicit source/model information instead of claiming auto Slayer.
    profiles.class_name="버서커".into();
    profiles.buff_metadata.insert("slayer_snapshot:model".into(),BuffMetadata {
        name:format!("슬레이어 고정 상태: {}",s.phase),icon:String::new(),
        source:serde_json::json!({"sourceClass":"슬레이어","schema":1,"model":"fixed_state","phase":s.phase,"characterLevel":70,
            "automaticFuryBound":false,"wholeCombatStateMachineBound":false,"ordinaryClassSupported":false,
            "runtimeShell":"버서커 MP/Fury/UltimatePoint research shell"}).to_string(),
    });
    Ok(profiles)
}
