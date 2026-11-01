//! Common gear/APL path for an explicit fixed-state Sorceress research model.
use crate::profile::{CharacterProfiles,GlobalTrigger,SkillTag,TriggerEffect};
use crate::profile::actor::BuffMetadata;
use serde::Deserialize;
use serde_json::Value;
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Snapshot {schema:u32,model:String,character_level:u32,phase:String,specialization:f64,initial_magic:f64,initial_ultimate_point:f64,automatic_magic_bound:bool,
 unresolved_core_events:Vec<Value>,extra_triggers:Vec<GlobalTrigger>,hit_resource_effects:Vec<HitOverlay>,cast_buff_damage_overlays:Vec<CastOverlay>,cast_buff_clock_overlays:Vec<CastOverlay>,live_buff_overlays:Vec<LiveOverlay>}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct HitOverlay {hit_id:String,effect:TriggerEffect}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct CastOverlay {game_skill_id:u32,buff_id:String,bonus:f64}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct LiveOverlay {game_skill_id:u32,buff_id:String,damage:f64,crit_damage:f64}
fn references(v:&Value,count:usize)->Result<(),String>{
 match v {Value::Object(fields)=>for(k,v)in fields{
  if ["SkillCastId","BeforeSkillCastId","skill_id"].contains(&k.as_str())&&!v.as_u64().is_some_and(|n|n<count as u64){return Err("소서리스 사건 스킬 참조 범위 밖".into());}
  if k=="skill_ids"&&!v.as_array().is_some_and(|a|a.iter().all(|v|v.as_u64().is_some_and(|n|n<count as u64))){return Err("소서리스 사건 스킬 배열 범위 밖".into());}
  references(v,count)?;
 },Value::Array(a)=>for v in a{references(v,count)?;},_=>{}};Ok(())
}
pub fn build(json:&str)->Result<CharacterProfiles,String>{
 let mut input:Value=serde_json::from_str(json).map_err(|e|e.to_string())?;
 if input["character"]["className"]!="소서리스"||input["character"].get("level").is_some_and(|v|v!=70){return Err("소서리스 전투70 연구 전용 입력".into());}
 if input.get("berserkerSnapshot").is_some()||input.get("slayerSnapshot").is_some(){return Err("연구 실행 계약 중복".into());}
 let raw=input.get("sorceressSnapshot").ok_or("소서리스 명시 연구 계약 필요")?.clone();
 let s:Snapshot=serde_json::from_value(raw.clone()).map_err(|e|e.to_string())?;
 if s.schema!=1||s.model!="fixed_state"||s.character_level!=70||s.automatic_magic_bound||!["normal","enhanced","released"].contains(&s.phase.as_str())||!s.specialization.is_finite()||s.specialization<0.{return Err("소서리스 fixed_state v1 계약 오류".into());}
 if !s.initial_magic.is_finite()||!(0. ..=10000.).contains(&s.initial_magic)||!s.initial_ultimate_point.is_finite()||!(0. ..=10000.).contains(&s.initial_ultimate_point)||s.phase=="released"&&s.initial_magic!=10000.||s.phase=="enhanced"&&!(3000. ..10000.).contains(&s.initial_magic){return Err("초기 마력/초각성 상태 불일치".into());}
 for path in ["/character/arkPassive/classNodes","/character/arkGrids/classCores","/cardData"]{if input.pointer(path).and_then(Value::as_array).is_some_and(|a|!a.is_empty()){return Err("중복 클래스/코어/카드 효과".into());}}
 let chosen=input["character"]["skills"].as_array().ok_or("스킬 목록 필요")?;let data=input["skillData"].as_array().ok_or("스킬 원본 필요")?;
 if chosen.is_empty()||chosen.len()!=data.len(){return Err("선택 스킬/원본 개수 불일치".into());}
 let mut ids=std::collections::HashSet::new();let mut slots=[0;5];
 for(c,r)in chosen.iter().zip(data){let id=r["gameSkillId"].as_u64().ok_or("원본ID 필요")?;let lv=c["level"].as_u64().ok_or("스킬 레벨 필요")?;
  let slot=match id {37200..=37350 if id%10==0=>0,37400|37410=>1,37360|37370=>2,37380|37390=>3,37110=>4,_=>return Err("지원 범위 밖 소서리스 스킬".into())};
  if !ids.insert(id)||c["name"]!=r["name"]||lv==0||lv>if slot==0{if id==37290{10}else{20}}else{1}||slot==4&&s.phase!="normal"{return Err("스킬 이름/레벨/상태 불일치".into());}slots[slot]+=1;
 }
 if slots[0]>8||slots[1..].iter().any(|n|*n>1){return Err("소서리스 슬롯별 스킬 수 초과".into());}
 references(&raw["extraTriggers"],chosen.len())?;references(&raw["hitResourceEffects"],chosen.len())?;
 for(k,v)in [("assumedTargetSeedOrLower",false),("assumedTargetAbnormalMoveImmune",true),("assumedTargetNerveVulnerable",false),("assumedTargetSlowVulnerable",false)]{input[k]=Value::Bool(v);}
 let mut p=super::build_inner(&input.to_string(),true)?;
 if (p.stat.specialization-s.specialization).abs()>1e-9{return Err(format!("가공 특화{}와 공통 스탯{} 불일치",s.specialization,p.stat.specialization));}
 for skill in &mut p.skills {
  let row=input["skillData"].as_array().unwrap().iter().find(|r|r["name"]==skill.name).ok_or("동작 시계 없음")?;
  let base=row["castTimesMs"][0].as_f64().filter(|n|*n>0.).ok_or("동작 시계 오류")?;let scale=skill.cast_times_ms[0]as f64/base;
  if scale!=1.{for h in &mut skill.hits{h.action_delay_ms=(h.action_delay_ms as f64*scale).floor()as u32;}}
 }
 for o in s.hit_resource_effects {let mut found=false;for skill in &mut p.skills {for hit in &mut skill.hits{if hit.hit_id==o.hit_id{found=true;hit.triggers.push(crate::profile::HitTrigger{on:crate::profile::HitTriggerOn::OnHit,effect:o.effect.clone(),chance:1.});}}}if !found{return Err("수급 사건의 원본 타격 없음".into());}}
 for o in s.cast_buff_damage_overlays {if !o.bonus.is_finite(){return Err("시전 피해 배율 오류".into());}let skill=p.skills.iter_mut().find(|skill|skill.tags.contains(&SkillTag::SkillId(o.game_skill_id))).ok_or("시전 피해 대상 없음")?;skill.cast_buff_damage_bonuses.push((o.buff_id,o.bonus));}
 for o in s.cast_buff_clock_overlays {if !o.bonus.is_finite()||o.bonus<=0.||o.bonus>1.{return Err("시전 시계 배율 오류".into());}let skill=p.skills.iter_mut().find(|skill|skill.tags.contains(&SkillTag::SkillId(o.game_skill_id))).ok_or("시계 대상 없음")?;skill.cast_buff_clock_multipliers.push((o.buff_id,o.bonus));}
 for o in s.live_buff_overlays {if !o.damage.is_finite()||!o.crit_damage.is_finite(){return Err("적중 배율 오류".into());}let skill=p.skills.iter_mut().find(|skill|skill.tags.contains(&SkillTag::SkillId(o.game_skill_id))).ok_or("적중 효과 대상 없음")?;if o.damage!=0.{skill.buff_damage_bonuses.push((o.buff_id.clone(),o.damage));}if o.crit_damage!=0.{skill.buff_crit_damage_bonuses.push((o.buff_id,o.crit_damage));}}
 p.global_triggers.extend(s.extra_triggers);p.class_name="소서리스".into();
 p.buff_metadata.insert("sorceress_snapshot:model".into(),BuffMetadata{name:format!("소서리스 고정 상태: {}",s.phase),icon:String::new(),source:serde_json::json!({"sourceClass":"소서리스","schema":1,"model":"fixed_state","phase":s.phase,"characterLevel":70,"initialMagic":s.initial_magic,"initialUltimatePoint":s.initial_ultimate_point,"automaticMagicBound":false,"absoluteMagicGain":null,"naturalMagicDrain":null,"ordinaryClassSupported":false,"unresolvedCoreEvents":s.unresolved_core_events}).to_string()});
 Ok(p)
}
