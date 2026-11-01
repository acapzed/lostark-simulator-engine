use crate::builder::dto::{BuildRequest,GemInput};
use serde_json::Value;

pub(super) struct Target {
    pub known: bool, pub allowed: bool, pub owner: Option<u32>,
    pub ids: Vec<u32>, pub key: String, pub legacy_name: Option<String>,
}
fn catalog()->&'static Value {
    static DATA:std::sync::OnceLock<Value>=std::sync::OnceLock::new();
    DATA.get_or_init(||serde_json::from_str(include_str!("../../../skill-data/common/backend/gem-target-eligibility.json")).unwrap())
}
pub(super) fn class_id(name:&str)->Option<u32>{catalog()["classes"][name].as_u64().map(|n|n as u32)}
fn known(id:u32,row:&Value,group:bool,field:&str)->Target {
    Target{known:true,allowed:row[field].as_bool().unwrap_or(false),owner:row["classId"].as_u64().map(|n|n as u32),
        ids:if group{row["members"].as_array().unwrap().iter().map(|n|n.as_u64().unwrap()as u32).collect()}else{vec![id]},
        key:format!("{}:{id}",if group{"group"}else{"skill"}),legacy_name:None}
}
fn legacy(id:Option<u32>,name:Option<&str>)->Target {
    Target{known:false,allowed:true,owner:None,ids:id.into_iter().collect(),
        key:name.filter(|s|!s.is_empty()).map(|s|format!("skill:{s}")).unwrap_or_else(||format!("external:{}",id.unwrap_or(0))),
        legacy_name:if id.is_none(){name.map(str::to_owned)}else{None}}
}
fn member(id:u32,name:Option<&str>,field:&str)->Target {
    let data=catalog();let row=&data["skills"][id.to_string()];
    if row.is_null(){return legacy(Some(id),name);}
    if row[field]==true{return known(id,row,false,field);}
    if let Some((key,g))=data["groups"].as_object().unwrap().iter().find(|(_,g)|g["classId"]==row["classId"]&&g[field]==true&&g["members"].as_array().unwrap().iter().any(|m|m.as_u64()==Some(id as u64))){return known(key.parse().unwrap(),g,true,field);}
    known(id,row,false,field)
}
pub(super) fn resolve(req:&BuildRequest,gem:&GemInput,damage:bool)->Target {
    let data=catalog();let field=if damage{"damage"}else{"cooldown"};
    if let Some(id)=gem.skill_id {
        let group=&data["groups"][id.to_string()];
        if !group.is_null(){return known(id,group,true,field);}
        return member(id,req.skill_data.iter().find(|s|s.game_skill_id==id).map(|s|s.name.as_str()),field);
    }
    if let Some(local)=req.skill_data.iter().find(|s|s.name==gem.skill_name){
        if local.game_skill_id!=0{return member(local.game_skill_id,Some(&local.name),field);}
        return legacy(None,Some(&local.name));
    }
    let owner=class_id(&req.character.class_name);
    let named_groups:Vec<_>=data["groups"].as_object().unwrap().iter().filter(|(_,g)|g["classId"].as_u64().map(|n|n as u32)==owner&&g["name"]==gem.skill_name).collect();
    let groups:Vec<_>=named_groups.iter().filter(|(_,g)|g[field]==true).collect();
    if groups.len()==1{return known(groups[0].0.parse().unwrap(),groups[0].1,true,field);}
    if let Some((id,g))=named_groups.first(){return Target{allowed:false,..known(id.parse().unwrap(),g,true,field)};}
    let candidates:Vec<_>=data["skills"].as_object().unwrap().iter().filter(|(_,s)|s["classId"].as_u64().map(|n|n as u32)==owner&&s["name"]==gem.skill_name).map(|(id,_)|member(id.parse().unwrap(),Some(&gem.skill_name),field)).collect();
    let mut allowed=std::collections::BTreeMap::new();
    for t in candidates.iter().filter(|t|t.allowed){allowed.entry(t.key.clone()).or_insert(t);}
    if allowed.len()==1 {let t=allowed.into_values().next().unwrap();return Target{known:t.known,allowed:t.allowed,owner:t.owner,ids:t.ids.clone(),key:t.key.clone(),legacy_name:t.legacy_name.clone()};}
    if let Some(t)=candidates.into_iter().next(){return Target{allowed:false,..t};}
    legacy(None,Some(&gem.skill_name))
}
