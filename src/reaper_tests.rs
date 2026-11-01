use crate::{builder, engine};
use serde_json::{json, Value};
use crate::profile::StatField;

#[test]
fn reaper_damage_gems_multiply_runtime_tripod_bonuses_on_actual_hits() {
    for (id, name, choices) in [
        (26820, "사일런트 스매셔", vec!["가벼운 동작", "그림자 잔상", "지면 강타"]),
    ] {
        let mut input = corrosion_input(id, name, &[268200]);
        input["character"]["skills"][0]["tripods"] = json!(choices.iter().map(|n|json!({"name":n,"level":1})).collect::<Vec<_>>());
        input["character"]["gems"] = json!([]);
        input["aplConfig"] = json!(format!("actions:\n - skill: {name}\n"));
        let base = builder::build(&input.to_string()).unwrap();
        let reference = engine::trace_one(&base, 42, 5000).1;
        assert!(reference.damage_events.iter().any(|h|h.formula.factors.iter().any(|f|f.sources.iter().any(|s|s.source=="runtime_damage_bonus" && s.value>0.0))), "{name}: exercise a real runtime bonus");
        for (kind, level, expected) in [("겁화",8,1.36),("겁화",10,1.44),("멸화",8,1.24),("멸화",10,1.4)] {
            input["character"]["gems"] = json!([{"skillName":name,"skillId":id,"type":kind,"level":level,"skillEffect":"피해 증가"}]);
            let profile = builder::build(&input.to_string()).unwrap();
            let actual = engine::trace_one(&profile, 42, 5000).1;
            assert_eq!(actual.damage_events.len(),reference.damage_events.len());
            for (on,off) in actual.damage_events.iter().zip(&reference.damage_events) {
                assert_eq!((on.time,on.is_crit,&on.hit_id),(off.time,off.is_crit,&off.hit_id));
                let expected_damage=off.result_damage * (on.formula.base_term/off.formula.base_term) * expected;
                assert!((on.result_damage/expected_damage-1.0).abs()<1e-12,"{name}/{kind}{level}/{}: {} != {expected_damage}",on.hit_id,on.result_damage);
            }
        }
    }
}

#[test]
fn public_raid_entry_ignores_legacy_target_predicates_for_damage_and_control() {
    for (id, name, choice, ids) in [
        (26070, "쉐도우 닷", "죽음의 일격", vec![260701, 260702]),
        (26510, "디스토션", "약육강식", vec![265108]),
        (26080, "이블리스토", "독 : 신경", vec![260800, 260801, 260802, 260803]),
        (26060, "스피릿 캐치", "발목 절단", vec![260600, 260612]),
        (26550, "콜 오브 나이프", "지속되는 부름", vec![265420, 265421, 265421, 265421, 265422]),
    ] {
        let mut input = corrosion_input(id, name, &ids);
        input["character"]["skills"][0]["tripods"] = json!([{ "name": choice, "level": 1 }]);
        input["assumedTargetSeedOrLower"] = json!(false);
        input["assumedTargetAbnormalMoveImmune"] = json!(true);
        input["assumedTargetNerveVulnerable"] = json!(false);
        input["assumedTargetSlowVulnerable"] = json!(false);
        let expected = builder::build(&input.to_string()).unwrap();
        let expected_profile = serde_json::to_value(&expected).unwrap();
        let expected_trace = serde_json::to_value(engine::trace_one(&expected, 42, 10000)).unwrap();
        for mask in 0..16 {
            for (bit, key) in ["assumedTargetSeedOrLower", "assumedTargetAbnormalMoveImmune",
                "assumedTargetNerveVulnerable", "assumedTargetSlowVulnerable"].iter().enumerate() {
                input[*key] = json!(mask & (1 << bit) != 0);
            }
            let actual = builder::build_raid(&input.to_string()).unwrap();
            assert_eq!(serde_json::to_value(&actual).unwrap(), expected_profile, "{name}/{mask}");
            assert_eq!(serde_json::to_value(engine::trace_one(&actual, 42, 10000)).unwrap(), expected_trace);
        }
        for key in ["assumedTargetSeedOrLower", "assumedTargetAbnormalMoveImmune",
            "assumedTargetNerveVulnerable", "assumedTargetSlowVulnerable"] {
            input.as_object_mut().unwrap().remove(key);
        }
        assert_eq!(serde_json::to_value(builder::build_raid(&input.to_string()).unwrap()).unwrap(), expected_profile);
    }
}

#[test]
fn reaper_all_base_routes_use_raw_stagger_points_without_changing_damage() {
    let data:Value=serde_json::from_str(include_str!("../skill-data/reaper/backend/simulation-data.json")).unwrap();
    let rules:Value=serde_json::from_str(include_str!("../skill-data/reaper/backend/stagger-rules.json")).unwrap();
    for skill in data["skills"].as_array().unwrap() {
        let id=skill["gameSkillId"].as_u64().unwrap() as u32;
        let name=skill["name"].as_str().unwrap();
        if id==26040 {continue;}
        let mut input=corrosion_input(id,name,&[]);
        input["character"]["skills"][0]["tripods"]=json!([]);
        if matches!(id,26040|26980) {input["character"]["skills"][0]["level"]=json!(1);}
        input["aplConfig"]=json!(format!("actions:\n - skill: {}\n",serde_json::to_string(name).unwrap()));
        let mut profile=builder::build(&input.to_string()).unwrap();profile.skills[0].max_uses=1;
        for hit in &profile.skills[0].hits {assert_eq!(hit.stagger,rules["basePoints"][hit.effect_id.to_string()].as_f64().unwrap_or(0.));}
        let (result,trace)=engine::trace_one(&profile,42,10000);
        let points:f64=trace.damage_events.iter().map(|event|event.raw_stagger_points).sum();
        assert_eq!(result.skill_stats.get(&0).map(|s|s.raw_stagger_points).unwrap_or(0.),points);
        let mut plain=profile.clone();for hit in &mut plain.skills[0].hits {hit.stagger=0.;}
        let old=engine::trace_one(&plain,42,10000).0;
        assert_eq!((result.total_damage,result.crit_count,result.cast_count),(old.total_damage,old.crit_count,old.cast_count));
        let mut missed=profile.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=missed.skills[0].hits.iter().map(|hit|hit.hit_id.clone()).collect();
        assert!(engine::trace_one(&missed,42,10000).0.skill_stats.values().all(|s|s.raw_stagger_points==0.));
    }
}

#[test]
fn reaper_raw_stagger_counts_landed_source_points_without_changing_combat() {
    let expected=std::collections::HashMap::from([(268000,206.),(268010,206.),(268071,258.),(268073,258.),(268081,62.),(268082,62.),(268100,147.),(268171,5.),(268172,147.),(268180,147.)]);
    for (id,name,base,choices) in [(26800,"라스트 그래피티",268000,vec![vec![],vec!["유언"],vec!["그림자 올가미"],vec!["급습 강화","그림자 올가미"]]),(26810,"레이지 스피어",268100,vec![vec![],vec!["집중 공격"],vec!["처형"],vec!["집중 공격","순살"]])] {
        for choices in choices {for level in [10,14] {
            let mut input=corrosion_input(id,name,&[base]);input["character"]["skills"][0]["level"]=json!(level);
            input["character"]["skills"][0]["tripods"]=json!(choices.iter().map(|name|json!({"name":name,"level":1})).collect::<Vec<_>>());
            input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n"));
            let mut profile=builder::build(&input.to_string()).unwrap();profile.skills[0].max_uses=1;
            for hit in &profile.skills[0].hits {assert_eq!(hit.stagger,expected.get(&hit.effect_id).copied().unwrap_or(0.));}
            let (result,trace)=engine::trace_one(&profile,42,10000);
            let sum:f64=trace.damage_events.iter().map(|e|e.raw_stagger_points).sum();assert!(sum>0.);
            assert_eq!(result.skill_stats[&0].raw_stagger_points,sum);
            for event in &trace.damage_events {let hit=profile.skills[0].hits.iter().find(|h|h.hit_id==event.hit_id).unwrap();assert_eq!(event.raw_stagger_points,hit.stagger);}
            assert_eq!(engine::simulate(&profile,3,10000,42,None).skill_stats[&0].raw_stagger_points,sum*3.);
            for _ in 0..3 {assert_eq!(engine::trace_one(&profile,42,10000).0.skill_stats[&0].raw_stagger_points,sum);}
            let mut plain=profile.clone();for hit in &mut plain.skills[0].hits {hit.stagger=0.;}
            let old=engine::trace_one(&plain,42,10000);
            let strip=|v:Value| {let mut v=v;if let Some(stats)=v[0]["skill_stats"].as_object_mut() {for stat in stats.values_mut() {stat.as_object_mut().unwrap().remove("raw_stagger_points");}}for event in v[1]["damageEvents"].as_array_mut().unwrap() {event.as_object_mut().unwrap().remove("rawStaggerPoints");}v};
            assert_eq!(strip(serde_json::to_value((&result,&trace)).unwrap()),strip(serde_json::to_value(&old).unwrap()));
            let mut missed=profile.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=missed.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();
            let (result,trace)=engine::trace_one(&missed,42,10000);assert!(trace.damage_events.is_empty());assert!(result.skill_stats.values().all(|s|s.raw_stagger_points==0.));
            assert!(engine::trace_one(&profile,42,1).0.skill_stats.values().all(|s|s.raw_stagger_points==0.));
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
            let mut reverse=builder::build(&input.to_string()).unwrap();reverse.skills[0].max_uses=1;
            assert_eq!(serde_json::to_value(engine::trace_one(&reverse,42,10000)).unwrap(),serde_json::to_value(engine::trace_one(&profile,42,10000)).unwrap());
        }}
    }
}

#[test]
fn reaper_stagger_choices_apply_provisional_ratio_once_to_owner_routes() {
    for (id,name,base,choice,routes) in [(26800,"라스트 그래피티",268000,"무력화 강화",vec![vec![],vec!["유언"],vec!["그림자 올가미"],vec!["급습 강화","유언"],vec!["혼돈 강화","그림자 올가미"]]),(26810,"레이지 스피어",268100,"뇌진탕",vec![vec![],vec!["집중 공격"],vec!["처형"],vec!["기습"],vec!["집중 공격","치명적인 단검"],vec!["처형","순살"]])] {
        for routes in routes {for level in [10,14] {for fast in [false,true] {
            let mut input=corrosion_input(id,name,&[base]);input["character"]["skills"][0]["level"]=json!(level);
            input["character"]["skills"][0]["tripods"]=json!(routes.iter().map(|name|json!({"name":name,"level":1})).collect::<Vec<_>>());
            input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n"));
            let mut plain=builder::build(&input.to_string()).unwrap();plain.skills[0].max_uses=1;
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":choice,"level":1,"values":[999999]}));
            let mut profile=builder::build(&input.to_string()).unwrap();profile.skills[0].max_uses=1;
            if fast {for p in [&mut plain,&mut profile] {p.stat.attack_speed.insert("test".into(),50.);p.stat.bake();}}
            assert!(profile.reaper_reference.as_ref().unwrap().metadata["raw_stagger_choice_policy"][name].is_object());
            for (a,b) in profile.skills[0].hits.iter().zip(&plain.skills[0].hits) {assert_eq!(a.stagger,b.stagger*1.4);}
            let (new,t)=engine::trace_one(&profile,42,10000);let (old,other)=engine::trace_one(&plain,42,10000);
            assert_eq!((new.total_damage,new.crit_count,new.cast_count),(old.total_damage,old.crit_count,old.cast_count));
            for (a,b) in t.damage_events.iter().zip(&other.damage_events) {let mut a=serde_json::to_value(a).unwrap();let mut b=serde_json::to_value(b).unwrap();a.as_object_mut().unwrap().remove("rawStaggerPoints");b.as_object_mut().unwrap().remove("rawStaggerPoints");assert_eq!(a,b);}
            assert!((new.skill_stats[&0].raw_stagger_points-old.skill_stats[&0].raw_stagger_points*1.4).abs()<1e-9);
            let summed=engine::simulate(&profile,3,10000,42,None).skill_stats[&0].raw_stagger_points;assert!((summed-new.skill_stats[&0].raw_stagger_points*3.).abs()<1e-9);
            let mut missed=profile.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=missed.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&missed,42,10000).0.skill_stats.values().all(|s|s.raw_stagger_points==0.));
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();let mut reverse=builder::build(&input.to_string()).unwrap();reverse.skills[0].max_uses=1;
            if fast {reverse.stat.attack_speed.insert("test".into(),50.);reverse.stat.bake();}
            assert_eq!(serde_json::to_value(engine::trace_one(&reverse,42,10000).0).unwrap(),serde_json::to_value(&new).unwrap());
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().iter_mut().find(|t|t["name"]==choice).unwrap()["level"]=json!(2);assert!(builder::build(&input.to_string()).is_err());
        }}}
    }
}

#[test]
fn blink_death_pc_condition_never_boosts_fixed_pve_npc() {
    for level in [10,14] {for first in [None,Some("그림자 충전"),Some("꿰뚫는 일격"),Some("발목 절단")] {for second in [None,Some("전진 베기"),Some("소용돌이 표창"),Some("분쇄 표창")] {for assumptions in [false,true] {
        let mut input=corrosion_input(26520,"블링크",&[265400;6]);
        input["character"]["skills"][0]["level"]=json!(level);
        input["character"]["skills"][0]["tripods"]=json!(first.into_iter().chain(second).map(|name|json!({"name":name,"level":1})).collect::<Vec<_>>());
        input["assumedTargetSeedOrLower"]=json!(assumptions);input["assumedTargetAbnormalMoveImmune"]=json!(assumptions);input["assumedTargetSlowVulnerable"]=json!(true);
        let baseline=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"죽음의 그림자","level":1,"values":[999999]}));
        for reverse in [false,true] {
            if reverse {input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();}
            let profile=builder::build(&input.to_string()).unwrap();
            assert_eq!(serde_json::to_value(&profile.skills).unwrap(),serde_json::to_value(&baseline.skills).unwrap());
            let policy=&profile.reaper_reference.as_ref().unwrap().metadata["blink_death_policy"];
            assert_eq!(policy["targetIsPc"],false);assert_eq!(policy["damageEffectApplied"],false);assert_eq!(policy["combatSourceRows"].as_array().unwrap().len(),3);
            let old=engine::trace_one(&baseline,42,1000);let new=engine::trace_one(&profile,42,1000);
            assert_eq!(serde_json::to_value(&old.0).unwrap(),serde_json::to_value(&new.0).unwrap());
            assert_eq!(serde_json::to_value(&old.1.damage_events).unwrap(),serde_json::to_value(&new.1.damage_events).unwrap());
        }
    }}}}
}

#[test]
fn blink_strength_max_distance_scopes_damage_to_direct_effects() {
    for (second,boosted_ids) in [
        (None,vec![265400]),
        (Some("소용돌이 표창"),vec![265400,265253]),
        (Some("분쇄 표창"),vec![265261]),
        (Some("전진 베기"),vec![265240,265243]),
    ] {
        for level in [10,14] {
            let mut input=corrosion_input(26520,"블링크",&[265400;6]);
            input["character"]["skills"][0]["level"]=json!(level);
            let choices:Vec<_>=[Some("그림자 충전"),second].into_iter().flatten().map(|name|json!({"name":name,"level":1})).collect();
            input["character"]["skills"][0]["tripods"]=json!(choices);
            let baseline=builder::build(&input.to_string()).unwrap();
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"그림자 강화","level":1,"values":[999999]}));
            let selected=builder::build(&input.to_string()).unwrap();
            let policy=&selected.reaper_reference.as_ref().unwrap().metadata["blink_strength_policy"];
            assert_eq!(policy["maxDistanceBonus"],1.44);
            assert_eq!(policy["areaEffectIds"],json!([265253]));
            assert_eq!(policy["forwardEffectIds"],json!([265240,265243]));
            assert_eq!(selected.skills[0].hits.len(),baseline.skills[0].hits.len());
            for (old,new) in baseline.skills[0].hits.iter().zip(&selected.skills[0].hits) {
                assert_eq!((new.effect_id,&new.hit_id,new.action_delay_ms,new.fixed_delay_ms),(old.effect_id,&old.hit_id,old.action_delay_ms,old.fixed_delay_ms));
                let ratio=(1.+new.damage_increase)/(1.+old.damage_increase);
                let expected=if boosted_ids.contains(&old.effect_id) {if [265240,265243].contains(&old.effect_id) {2.35} else {2.44}} else {1.};
                assert!((ratio-expected).abs()<1e-12,"{second:?} / {level} / {}: {ratio}",old.hit_id);
            }
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
            let reversed=builder::build(&input.to_string()).unwrap();
            assert_eq!(serde_json::to_value(&selected.skills).unwrap(),serde_json::to_value(&reversed.skills).unwrap());
        }
    }
}

#[test]
fn reaper_holding_category_is_trusted_and_normal_choices_remove_holding_tag() {
    use crate::profile::skill::{SkillType,SkillTag};
    for (id,name,normal_choice,effect) in [(26560,"블랙 미스트","거짓 안개",265600),(26600,"쉐도우 스톰","그림자 지대",266001)] {
        for normal in [false,true] {for incoming in ["Normal","Holding","Charge"] {
            let mut input=corrosion_input(id,name,&[effect]);
            input["skillData"][0]["castTimesMs"]=json!([2567]);
            input["skillData"][0]["skillControlType"]=json!(incoming);
            input["character"]["skills"][0]["tripods"]=if normal {json!([{"name":normal_choice,"level":1}])} else {json!([])};
            let profile=builder::build(&input.to_string()).unwrap();
            let skill=profile.skills.iter().find(|s|s.name==name).unwrap();
            assert_eq!(skill.tags.contains(&SkillTag::HoldingCasting),!normal);
            assert!(if normal {matches!(skill.skill_type,SkillType::Normal)} else {matches!(skill.skill_type,SkillType::Holding{..})});
            assert!(!skill.tags.contains(&SkillTag::Charging));
            let common:Value=serde_json::from_str(include_str!("../skill-data/common/backend/engravings.json")).unwrap();
            let engraving=common.as_array().unwrap().iter().find(|e|e["name"]=="속전속결").unwrap();
            input["character"]["engravings"]=json!([{"name":"속전속결","grade":"","level":3,"featureData":{"baseLevels":engraving["baseLevels"]}}]);
            let boosted=builder::build(&input.to_string()).unwrap();
            let after=boosted.skills.iter().find(|s|s.name==name).unwrap();
            for (old,new) in skill.hits.iter().zip(&after.hits) {
                assert!(((1.+new.damage_increase)/(1.+old.damage_increase)-if normal {1.} else {1.2}).abs()<1e-12);
            }
            assert_eq!(after.cast_times_ms,skill.cast_times_ms.iter().map(|v|if normal {*v} else {(*v as f64*0.8).round() as u32}).collect::<Vec<_>>());
        }}
    }
}

#[test]
fn blink_forward_replaces_untrusted_hits_and_applies_owner_bonuses_once() {
    for level in [7,10,11,14] {for first in [None,Some("그림자 충전"),Some("꿰뚫는 일격"),Some("발목 절단")] {
        let mut input=corrosion_input(26520,"블링크",&[265400;6]);
        input["character"]["skills"][0]["level"]=json!(level);input["skillData"][0]["identityCategory"]=json!(44);
        let mut choices=vec![json!({"name":"전진 베기","level":1,"values":[999999]})];
        if let Some(name)=first {choices.push(json!({"name":name,"level":1}));}
        input["character"]["skills"][0]["tripods"]=json!(choices);
        input["aplConfig"]=json!("actions:\n - skill: 블링크\n");input["assumedTargetSlowVulnerable"]=json!(true);
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"heatLossPerSecond":0,"personaDarkLossPerSecond":0,"skillGains":{"26520":{"dark":0.1,"heat":0.1}}});
        let p=builder::build(&input.to_string()).unwrap();let s=&p.skills[0];
        assert_eq!(s.cast_times_ms,vec![1000]);assert_eq!(s.hits.len(),2);
        assert_eq!(s.hits.iter().map(|h|(h.effect_id,h.hit_id.as_str(),h.action_delay_ms,h.fixed_delay_ms)).collect::<Vec<_>>(),vec![(265240,"26520:forward:pc",470,0),(265243,"26520:forward:npc",0,470)]);
        assert!(s.hits.iter().all(|h|(h.damage_increase-0.995).abs()<1e-12 && h.defense_ignore==if first==Some("꿰뚫는 일격") {0.62} else {0.}),"{level} / {first:?}: {:?}",s.hits.iter().map(|h|(h.damage_increase,h.defense_ignore)).collect::<Vec<_>>());
        assert_eq!(p.reaper_reference.as_ref().unwrap().metadata["blink_forward_policy"]["chainChangeDelayMsRaw"],1000);
        assert_eq!((s.timed_effects[0].action_delay_ms,s.timed_effects[0].action_end_ms),(0,Some(830)));
        let (_,trace)=engine::trace_one(&p,42,1000);assert_eq!(trace.damage_events.len(),2);
        let gain=if first==Some("그림자 충전") {0.145} else {0.1};
        assert!(trace.damage_events.iter().all(|e|e.resource_gains.iter().any(|g|g.resource=="ReaperDark" && (g.amount-gain).abs()<1e-12)));
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        let reversed=builder::build(&input.to_string()).unwrap();assert_eq!(serde_json::to_value(&p.skills).unwrap(),serde_json::to_value(&reversed.skills).unwrap());
        let mut miss=p.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=s.hits.iter().map(|h|h.hit_id.clone()).collect();
        let (_,missed)=engine::trace_one(&miss,42,1000);assert!(missed.damage_events.iter().all(|e|e.resource_gains.is_empty()));assert!(missed.casts[0].active_buffs.contains(&"reaper:protection:26520:4".into()));
    }}
}

#[test]
fn blink_swirl_replaces_untrusted_hits_and_applies_owner_bonuses_once() {
    for level in [7,10,11,14] {for first in [None,Some("그림자 충전"),Some("꿰뚫는 일격"),Some("발목 절단")] {
        let mut input=corrosion_input(26520,"블링크",&[265400;6]);
        input["character"]["skills"][0]["level"]=json!(level);input["skillData"][0]["identityCategory"]=json!(44);input["skillData"][0]["cooldown"]=json!(18);
        let mut choices=vec![json!({"name":"소용돌이 표창","level":1,"values":[999999]})];
        if let Some(name)=first {choices.push(json!({"name":name,"level":1}));}
        input["character"]["skills"][0]["tripods"]=json!(choices);
        input["aplConfig"]=json!("actions:\n - skill: 블링크\n");input["assumedTargetSlowVulnerable"]=json!(true);
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"heatLossPerSecond":0,"personaDarkLossPerSecond":0,"skillGains":{"26520":{"dark":0.01,"heat":0.01}}});
        let p=builder::build(&input.to_string()).unwrap();let s=&p.skills[0];
        assert_eq!(s.cast_times_ms,vec![1000]);let count=if level<=10 {12} else {14};assert_eq!(s.hits.len(),count);assert_eq!(s.hits.iter().filter(|h|h.effect_id==265253).count(),8);
        assert_eq!(s.hits.iter().filter(|h|h.effect_id==265253).map(|h|(h.action_delay_ms,h.fixed_delay_ms)).collect::<Vec<_>>(),vec![(0,199),(0,499),(0,799),(0,1099),(350,0),(350,300),(350,600),(350,900)]);
        assert!(s.hits.iter().all(|h|(h.damage_increase-0.68).abs()<1e-12 && h.defense_ignore==if first==Some("꿰뚫는 일격") {0.62} else {0.}),"{level} / {first:?}: {:?}",s.hits.iter().map(|h|(h.damage_increase,h.defense_ignore)).collect::<Vec<_>>());
        assert_eq!(p.reaper_reference.as_ref().unwrap().metadata["blink_swirl_policy"]["timerRaw"]["raw_final_flag"],1);
        let (_,trace)=engine::trace_one(&p,42,2000);assert_eq!(trace.damage_events.len(),count);assert!(trace.damage_events.iter().any(|e|e.time>1.0));
        let gain=if first==Some("그림자 충전") {0.0145} else {0.01};
        assert!(trace.damage_events.iter().all(|e|e.resource_gains.iter().any(|g|g.resource=="ReaperDark" && (g.amount-gain).abs()<1e-12)));
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        let reversed=builder::build(&input.to_string()).unwrap();assert_eq!(serde_json::to_value(&p.skills).unwrap(),serde_json::to_value(&reversed.skills).unwrap());
        let mut miss=p.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=s.hits.iter().map(|h|h.hit_id.clone()).collect();
        let (_,missed)=engine::trace_one(&miss,42,2000);assert!(missed.damage_events.iter().all(|e|e.resource_gains.is_empty()));
    }}
}

#[test]
fn blink_crush_replaces_untrusted_hits_and_applies_owner_bonuses_once() {
    for level in [7,10,11,14] {for first in [None,Some("그림자 충전"),Some("꿰뚫는 일격"),Some("발목 절단")] {
        let mut input=corrosion_input(26520,"블링크",&[265400;6]);
        input["character"]["skills"][0]["level"]=json!(level);input["skillData"][0]["identityCategory"]=json!(44);
        let mut choices=vec![json!({"name":"분쇄 표창","level":1,"values":[999999]})];
        if let Some(name)=first {choices.push(json!({"name":name,"level":1}));}
        input["character"]["skills"][0]["tripods"]=json!(choices);
        input["aplConfig"]=json!("actions:\n - skill: 블링크\n");input["assumedTargetSlowVulnerable"]=json!(true);
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"heatLossPerSecond":0,"personaDarkLossPerSecond":0,"skillGains":{"26520":{"dark":0.1,"heat":0.1}}});
        let p=builder::build(&input.to_string()).unwrap();let s=&p.skills[0];
        assert_eq!(s.cast_times_ms,vec![1000]);assert_eq!(s.hits.len(),2);
        assert_eq!(s.hits.iter().map(|h|(h.effect_id,h.hit_id.as_str(),h.action_delay_ms,h.fixed_delay_ms)).collect::<Vec<_>>(),vec![(265261,"26520:crush:npc",0,199),(265261,"26520:crush:pc",350,0)]);
        assert!(s.hits.iter().all(|h|(h.damage_increase-0.8375).abs()<1e-12 && h.defense_ignore==if first==Some("꿰뚫는 일격") {0.62} else {0.}),"{level} / {first:?}: {:?}",s.hits.iter().map(|h|(h.damage_increase,h.defense_ignore)).collect::<Vec<_>>());
        assert_eq!(p.reaper_reference.as_ref().unwrap().metadata["blink_crush_policy"]["callbackRaw"][0]["raw_word9"],4);
        let (_,trace)=engine::trace_one(&p,42,1000);assert_eq!(trace.damage_events.len(),2);
        let gain=if first==Some("그림자 충전") {0.145} else {0.1};
        assert!(trace.damage_events.iter().all(|e|e.resource_gains.iter().any(|g|g.resource=="ReaperDark" && (g.amount-gain).abs()<1e-12)));
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        let reversed=builder::build(&input.to_string()).unwrap();assert_eq!(serde_json::to_value(&p.skills).unwrap(),serde_json::to_value(&reversed.skills).unwrap());
        let mut miss=p.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=s.hits.iter().map(|h|h.hit_id.clone()).collect();
        let (_,missed)=engine::trace_one(&miss,42,1000);assert!(missed.damage_events.iter().all(|e|e.resource_gains.is_empty()));
    }}
}

#[test]
fn blink_forward_scales_pc_protection_and_hit_but_keeps_npc_command_time_fixed() {
    for fast in [false,true] {
        let mut input=corrosion_input(26520,"블링크",&[265400;6]);
        input["character"]["skills"][0]["tripods"]=json!([{"name":"전진 베기","level":1}]);
        input["skillData"][0]["identityCategory"]=json!(44);input["aplConfig"]=json!("actions:\n - skill: 블링크\n");
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"heatLossPerSecond":0,"personaDarkLossPerSecond":0,"skillGains":{}});
        let mut p=builder::build(&input.to_string()).unwrap();p.stat.swiftness=0.;p.stat.attack_speed.clear();p.stat.attack_move_speed_percent=0.;
        if fast {p.stat.attack_speed.insert("test".into(),40.);}p.stat.bake();p.skills[0].max_uses=1;
        let actual_cast=if fast {714} else {1000};let scale=|time:u32|time*actual_cast/1000;
        let t=engine::trace_one(&p,42,1000).1;
        assert_eq!(t.damage_events.iter().find(|h|h.hit_id=="26520:forward:pc").unwrap().time,scale(470) as f64/1000.);
        assert_eq!(t.damage_events.iter().find(|h|h.hit_id=="26520:forward:npc").unwrap().time,0.470);
        let template=p.skills[0].hits[0].clone();p.skills[0].hits.clear();
        for time in [828,830] {let mut hit=template.clone();hit.hit_id=format!("test:forward:protection:{time}");hit.action_delay_ms=time;hit.fixed_delay_ms=0;p.skills[0].hits.push(hit);}
        let before=engine::trace_one(&p,42,scale(828)).1;assert!(before.casts[0].active_buff_details_after_hit.iter().any(|b|b.id=="reaper:protection:26520:4"));
        let end=engine::trace_one(&p,42,scale(830)).1;assert_eq!(end.damage_events.len(),2);assert!(!end.casts[0].active_buff_details_after_hit.iter().any(|b|b.id=="reaper:protection:26520:4"));
    }
}

fn armor_input(id:u32,first:Option<&str>,third:Option<&str>) -> Value {
    let (name,ids,delays,cast)=if id==26080 {("이블리스토",vec![260800,260801,260802,260803],vec![220,1708,2655,4153],4766)} else {("블랙 미스트",vec![265600;5],vec![743,1294,1846,2397,2949],3833)};
    let mut input=corrosion_input(id,name,&ids);input["skillData"][0]["castTimesMs"]=json!([cast]);input["skillData"][0]["properties"]["attackType"]=json!(if id==26080 {"백 어택"} else {""});
    for (i,hit) in input["skillData"][0]["hits"].as_array_mut().unwrap().iter_mut().enumerate() {hit["actionDelayMs"]=json!(delays[i]);hit["hitId"]=json!(if id==26080 {format!("26080:{i}:0")} else {format!("26560:1:0:repeat:{i}")});}
    let mut choices=vec![json!({"name":"강인함","level":1,"values":[999999]})];for name in [first,third].into_iter().flatten() {choices.push(json!({"name":name,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);input["assumedTargetNerveVulnerable"]=json!(true);input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n"));input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"heatLossPerSecond":0,"personaDarkLossPerSecond":0,"skillGains":{}});input
}

#[test]
fn reaper_armor_choices_keep_poison_layers_callbacks_and_whole_combat_unchanged() {
    for (id,firsts,thirds,excluded) in [(26080,vec![None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")],vec![None,Some("기습"),Some("속전속결")],vec!["심호흡","날카로운 단검"]),(26560,vec![None,Some("넓은 안개")],vec![None,Some("거짓 안개"),Some("안개 폭발")],vec!["안개의 눈","퍼지는 안개"])] {
        for first in firsts {for &third in &thirds {let input=armor_input(id,first,third);let p=builder::build(&input.to_string()).unwrap();let mut plain=input.clone();plain["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|t|t["name"]!="강인함");let base=builder::build(&plain.to_string()).unwrap();let skill=&p.skills[0];assert_eq!(serde_json::to_value(&skill.hits).unwrap(),serde_json::to_value(&base.skills[0].hits).unwrap());assert_eq!((skill.cast_times_ms.clone(),skill.cooldown_ms),(base.skills[0].cast_times_ms.clone(),base.skills[0].cooldown_ms));assert_eq!(skill.timed_effects.len(),if third==Some("거짓 안개") {1} else {2});assert!(skill.timed_effects.iter().all(|t|t.until_cast_end&&matches!(&t.effect,crate::profile::TriggerEffect::ApplyBuff(b) if b.effects.is_empty())));
            let mut reversed=input.clone();reversed["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(&p.skills).unwrap(),serde_json::to_value(builder::build(&reversed.to_string()).unwrap().skills).unwrap());
            for fast in [false,true] {for all_miss in [false,true] {let mut observed=p.clone();if fast {observed.stat.attack_speed.insert("test".into(),40.);}if all_miss {observed.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();}let mut old=observed.clone();old.skills[0].timed_effects.clear();let (r,e)=engine::trace_one(&observed,42,9000);let (b,t)=engine::trace_one(&old,42,9000);assert_eq!(serde_json::to_value(r).unwrap(),serde_json::to_value(b).unwrap());assert_eq!(serde_json::to_value(e.damage_events).unwrap(),serde_json::to_value(t.damage_events).unwrap());assert_eq!(serde_json::to_value(e.class_events).unwrap(),serde_json::to_value(t.class_events).unwrap());assert!(e.casts.iter().all(|c|c.active_buffs.iter().any(|b|b.starts_with(&format!("reaper:protection:{id}:")))));}}
            for name in &excluded {let mut bad=input.clone();bad["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":name,"level":1}));assert!(builder::build(&bad.to_string()).unwrap_err().contains("중복"));}
        }}
    }
}

#[test]
fn reaper_armor_delayed_start_precedes_same_time_hit_and_gaps_and_ends_are_inactive() {
    for (id,third) in [(26080,None),(26080,Some("속전속결")),(26560,None),(26560,Some("거짓 안개")),(26560,Some("안개 폭발"))] {for fast in [false,true] {
        let mut p=builder::build(&armor_input(id,None,third).to_string()).unwrap();p.stat.swiftness=0.;p.stat.attack_speed.clear();p.stat.attack_move_speed_percent=0.;if fast {p.stat.attack_speed.insert("test".into(),40.);}p.stat.bake();p.skills[0].max_uses=1;
        let windows=p.skills[0].timed_effects.iter().map(|t|(t.action_delay_ms,t.action_end_ms.unwrap())).collect::<Vec<_>>();let cast=p.skills[0].cast_times_ms[0];let actual_cast=if fast {(cast as f64/1.4) as u32} else {cast};let scaled=|time:u32|(time as u64*actual_cast as u64/cast as u64) as u32;let expected=windows.iter().map(|&(start,end)|(scaled(start),scaled(end))).collect::<Vec<_>>();
        let template=p.skills[0].hits[0].clone();p.skills[0].hits.clear();let mut times=Vec::new();for &(start,end) in &windows {if start>0 {times.push(start-2);}times.extend([start,end-2,end]);}times.sort_unstable();times.dedup();
        for (i,time) in times.iter().enumerate() {let mut hit=template.clone();hit.hit_id=format!("test:armor:probe:{i}");hit.action_delay_ms=*time;hit.fixed_delay_ms=0;hit.triggers.clear();p.skills[0].hits.push(hit);}
        for time in times {let when=scaled(time);let trace=engine::trace_one(&p,42,when).1;assert!(trace.damage_events.iter().any(|h|(h.time*1000.).round() as u32==when));let active=trace.casts[0].active_buff_details_after_hit.iter().any(|b|b.id.starts_with(&format!("reaper:protection:{id}:")));assert_eq!(active,expected.iter().any(|&(start,end)|when>=start&&when<end),"{id} {third:?} fast={fast} at {when}");}
        let mut observer=p.skills[0].clone();observer.skill_id=99;observer.name="중단 없는 종료 관측".into();observer.timed_effects.clear();observer.hits.truncate(1);observer.hits[0].action_delay_ms=0;p.skills.push(observer);let mut action=p.apl_actions[0].clone();action.skill_id=99;p.apl_actions.push(action);assert!(!engine::trace_one(&p,42,actual_cast+100).1.casts[1].active_buffs.iter().any(|b|b.starts_with("reaper:protection:")));
    }}
}

#[test]
fn reaper_mist_owner_protection_excludes_projectiles_after_cast_and_reports_all_selected_skills() {
    let mut input=armor_input(26560,Some("넓은 안개"),Some("거짓 안개"));let mut p=builder::build(&input.to_string()).unwrap();p.skills[0].max_uses=1;let t=engine::trace_one(&p,42,7000).1;assert_eq!(t.damage_events.len(),5);assert!(t.damage_events.iter().any(|h|h.time>1.));assert!(t.casts[0].active_buff_details_after_hit.iter().all(|b|!b.id.starts_with("reaper:protection:")));
    let other=armor_input(26080,Some("독 : 출혈"),Some("속전속결"));input["character"]["skills"].as_array_mut().unwrap().push(other["character"]["skills"][0].clone());input["skillData"].as_array_mut().unwrap().push(other["skillData"][0].clone());let p=builder::build(&input.to_string()).unwrap();let policies=&p.reaper_reference.as_ref().unwrap().metadata["protection_window_policies"];assert!(policies["이블리스토"]["selectedVariant"].is_object());assert!(policies["블랙 미스트"]["selectedVariant"].is_object());assert!(p.skills.iter().all(|s|!s.timed_effects.is_empty()));
}

#[test]
fn reaper_rage_protection_composes_main_routes_without_damage_gauge_or_rng_changes() {
    for second in [None,Some("집중 공격"),Some("처형"),Some("기습")] {for third in [None,Some("치명적인 단검"),Some("순살")] {
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);input["skillData"][0]["castTimesMs"]=json!([2067]);input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"heatLossPerSecond":0,"personaDarkLossPerSecond":0,"skillGains":{}});
        let mut choices=vec![json!({"name":"암살자의 거래","level":1,"values":[999999]})];for name in [second,third].into_iter().flatten() {choices.push(json!({"name":name,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];let end=match second {Some("집중 공격")=>1420,Some("처형")=>575,_=>1120};
        assert_eq!(skill.timed_effects.len(),1);let timed=&skill.timed_effects[0];assert_eq!((timed.action_delay_ms,timed.action_end_ms,timed.until_cast_end),(0,Some(end),true));assert!(matches!(&timed.effect,crate::profile::TriggerEffect::ApplyBuff(b) if b.effects.is_empty()&&b.max_stacks==1&&b.duration_ms==0));
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&profile.skills).unwrap());
        let mut plain=input.clone();plain["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|c|c["name"]!="암살자의 거래");let base=builder::build(&plain.to_string()).unwrap();assert_eq!(serde_json::to_value(&skill.hits).unwrap(),serde_json::to_value(&base.skills[0].hits).unwrap());assert_eq!((skill.cast_times_ms.clone(),skill.cooldown_ms),(base.skills[0].cast_times_ms.clone(),base.skills[0].cooldown_ms));
        for fast in [false,true] {for all_miss in [false,true] {let mut p=profile.clone();if fast {p.stat.attack_speed.insert("test".into(),40.);}if all_miss {p.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();}let mut old=p.clone();old.skills[0].timed_effects.clear();let (r,e)=engine::trace_one(&p,42,9000);let (b,t)=engine::trace_one(&old,42,9000);assert_eq!(serde_json::to_value(r).unwrap(),serde_json::to_value(b).unwrap());assert_eq!(serde_json::to_value(e.damage_events).unwrap(),serde_json::to_value(t.damage_events).unwrap());assert_eq!(serde_json::to_value(e.class_events).unwrap(),serde_json::to_value(t.class_events).unwrap());assert!(e.casts.iter().all(|c|c.active_buffs.contains(&"reaper:protection:26810:1".into())));}}
        let mut bad=input.clone();bad["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"은밀한 암살","level":1}));assert!(builder::build(&bad.to_string()).unwrap_err().contains("중복"));input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().iter_mut().find(|c|c["name"]=="암살자의 거래").unwrap()["level"]=json!(2);assert!(builder::build(&input.to_string()).is_err());
    }}
}

#[test]
fn reaper_rage_protection_ends_before_cast_end_and_is_inactive_at_exact_boundary() {
    for second in [None,Some("집중 공격"),Some("처형")] {for fast in [false,true] {for stage_speed in [false,true] {
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);input["skillData"][0]["castTimesMs"]=json!([2067]);input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"heatLossPerSecond":0,"personaDarkLossPerSecond":0,"skillGains":{}});let mut choices=vec![json!({"name":"암살자의 거래","level":1})];if let Some(name)=second {choices.push(json!({"name":name,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        let mut p=builder::build(&input.to_string()).unwrap();let raw_end=p.skills[0].timed_effects[0].action_end_ms.unwrap();
        if stage_speed {input["reaperBuild"]=json!("dark_shadow_dot_legacy");let mut core=destiny_slot("더블 코어","ancient",20);core["coreSymbol"]=json!("달");input["character"]["arkGrids"]=json!({"reaperSlots":[core]});p=builder::build(&input.to_string()).unwrap();assert_eq!(p.skills[0].timed_effects[0].action_end_ms,Some((raw_end as f64/1.1).round() as u32));}
        p.stat.swiftness=0.;p.stat.attack_speed.clear();p.stat.attack_move_speed_percent=0.;if fast {p.stat.attack_speed.insert("test".into(),40.);}p.stat.bake();
        p.skills[0].max_uses=1;let end=p.skills[0].timed_effects[0].action_end_ms.unwrap();p.skills[0].hits.truncate(1);p.skills[0].hits[0].action_delay_ms=end-2;let mut last=p.skills[0].hits[0].clone();last.hit_id="test:protection:end".into();last.action_delay_ms=end;p.skills[0].hits.push(last);
        let e=engine::trace_one(&p,42,4000).1;assert_eq!(e.damage_events.len(),2);let before=(e.damage_events[0].time*1000.).round() as u32;let actual_end=(e.damage_events[1].time*1000.).round() as u32;let cast=p.skills[0].cast_times_ms[0];let actual_cast=if fast {(cast as f64/1.4) as u32} else {cast};assert_eq!(actual_end,(end as u64*actual_cast as u64/cast as u64) as u32);assert!(actual_end<actual_cast);
        assert!(engine::trace_one(&p,42,before).1.casts[0].active_buff_details_after_hit.iter().any(|b|b.id=="reaper:protection:26810:1"));assert!(!e.casts[0].active_buff_details_after_hit.iter().any(|b|b.id=="reaper:protection:26810:1"));assert!(e.casts[0].active_buffs.contains(&"reaper:protection:26810:1".into()));
        let mut observer=p.skills[0].clone();observer.skill_id=99;observer.name="보호 종료 관측".into();observer.timed_effects.clear();p.skills.push(observer);let mut action=p.apl_actions[0].clone();action.skill_id=99;p.apl_actions.push(action);assert!(!engine::trace_one(&p,42,actual_cast+100).1.casts[1].active_buffs.contains(&"reaper:protection:26810:1".into()));
    }}}
}

#[test]
fn reaper_rage_protection_is_capped_at_cast_end_and_old_expiry_cannot_remove_recast() {
    let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);input["character"]["skills"][0]["tripods"]=json!([{"name":"암살자의 거래","level":1}]);input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");let mut p=builder::build(&input.to_string()).unwrap();p.stat.swiftness=0.;p.stat.attack_speed.clear();p.stat.attack_move_speed_percent=0.;p.stat.bake();p.skills[0].hits[0].action_delay_ms=100;p.skills[0].max_uses=3;
    let t=engine::trace_one(&p,42,1000).1;assert_eq!(t.casts.len(),3);assert!(t.casts.iter().all(|c|c.active_buffs.contains(&"reaper:protection:26810:1".into())));assert!(t.casts.iter().all(|c|c.active_buff_details_after_hit.iter().all(|b|b.id!="reaper:protection:26810:1")));assert_eq!(serde_json::to_value(engine::trace_one(&p,42,1000)).unwrap(),serde_json::to_value(engine::trace_one(&p,42,1000)).unwrap());
}

#[test]
fn reaper_destruction_levels_consume_exact_landed_hits_without_changing_combat() {
    let strip_levels=|mut value:Value| {
        if let Some(stats)=value["result"]["skill_stats"].as_object_mut() { for stat in stats.values_mut() { stat.as_object_mut().unwrap().remove("destruction_hits"); } }
        for hit in value["damage"].as_array_mut().unwrap() { hit.as_object_mut().unwrap().remove("destructionLevel"); }
        value
    };
    for (id,name,ids,second_choices,third) in [
        (26820,"사일런트 스매셔",vec![268200],vec![None,Some("빠른 준비"),Some("추진력"),Some("그림자 잔상")],"지면 강타"),
        (26830,"댄싱 오브 퓨리",vec![268300,268300,268300,268300,268301],vec![None,Some("혼돈 강화"),Some("약육강식"),Some("치명적인 단검")],"절제된 동작"),
    ] { for second in second_choices { for final_selected in [false,true] { for level in [10,12,14] {
        let mut input=corrosion_input(id,name,&ids);input["character"]["skills"][0]["level"]=json!(level);
        input["skillData"][0]["cooldown"]=json!(300);input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n"));
        input["assumedTargetSeedOrLower"]=json!(true);
        let mut choices=Vec::new();if let Some(c)=second {choices.push(json!({"name":c,"level":1}));}if final_selected {choices.push(json!({"name":third,"level":1}));}
        input["character"]["skills"][0]["tripods"]=json!(choices);let baseline=builder::build(&input.to_string()).unwrap();
        choices.push(json!({"name":"부위파괴 강화","level":1,"values":[999999]}));input["character"]["skills"][0]["tripods"]=json!(choices);
        let profile=builder::build(&input.to_string()).unwrap();let expected=if id==26820 {1} else {2};
        assert!(profile.skills[0].hits.iter().any(|h|h.destruction==expected));
        for hit in &profile.skills[0].hits {
            let target=if id==26820 {[268200,268210,268220].contains(&hit.effect_id)} else {[268301,268371,268381].contains(&hit.effect_id)};
            assert_eq!(hit.destruction,if target {expected} else {0});
            let old=baseline.skills[0].hits.iter().find(|h|h.hit_id==hit.hit_id).unwrap();assert_eq!(old.destruction,if target&&id==26830 {1} else {0});
        }
        let (result,events)=engine::trace_one(&profile,42,10000);let (old,old_events)=engine::trace_one(&baseline,42,10000);
        let wanted=profile.skills[0].hits.iter().filter(|h|h.destruction>0).count() as u32;
        assert_eq!(result.skill_stats[&0].destruction_hits,std::collections::BTreeMap::from([(expected,wanted)]));
        assert_eq!(events.damage_events.iter().filter(|h|h.destruction_level==expected).count() as u32,wanted);
        assert_eq!(strip_levels(json!({"result":result,"damage":events.damage_events})),strip_levels(json!({"result":old,"damage":old_events.damage_events})));
        assert_eq!(serde_json::to_value(events.casts).unwrap(),serde_json::to_value(old_events.casts).unwrap());
        assert_eq!(serde_json::to_value(events.class_events).unwrap(),serde_json::to_value(old_events.class_events).unwrap());
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&profile.skills).unwrap());
        let mut missed=profile.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=profile.skills[0].hits.iter().filter(|h|h.destruction>0).map(|h|h.hit_id.clone()).collect();let (miss_result,miss_trace)=engine::trace_one(&missed,42,10000);assert!(miss_result.skill_stats[&0].destruction_hits.is_empty());assert!(miss_trace.damage_events.iter().all(|h|h.destruction_level==0));
    }}}}
}

#[test]
fn reaper_destruction_aggregates_iterations_and_does_not_leak_between_runs() {
    let mut input=corrosion_input(26830,"댄싱 오브 퓨리",&[268300,268301]);input["skillData"][0]["cooldown"]=json!(300);input["character"]["skills"][0]["tripods"]=json!([{"name":"부위파괴 강화","level":1}]);
    let profile=builder::build(&input.to_string()).unwrap();let result=engine::simulate(&profile,3,10000,42,None);assert_eq!(result.skill_stats[&0].destruction_hits,std::collections::BTreeMap::from([(2,3)]));
    for _ in 0..3 {assert_eq!(engine::trace_one(&profile,42,10000).0.skill_stats[&0].destruction_hits,std::collections::BTreeMap::from([(2,1)]));}
    input["character"]["skills"][0]["tripods"]=json!([]);let base=builder::build(&input.to_string()).unwrap();assert_eq!(engine::trace_one(&base,42,10000).0.skill_stats[&0].destruction_hits,std::collections::BTreeMap::from([(1,1)]));
    input["character"]["skills"][0]["tripods"]=json!([{"name":"부위파괴 강화","level":2}]);assert!(builder::build(&input.to_string()).is_err());
    input["character"]["skills"][0]["level"]=json!(3);input["character"]["skills"][0]["tripods"][0]["level"]=json!(1);assert!(builder::build(&input.to_string()).is_err());
}

#[test]
fn reaper_gust_whole_choice_keeps_two_hits_and_first_tier_and_blocks_alternate_input() {
    for first in [None,Some("그림자 충전"),Some("치명적인 그림자"),Some("빠른 준비")] {for level in [10,12,14] {
        let mut input=corrosion_input(26510,"디스토션",&[265108,265108]);input["skillData"][0]["cooldown"]=json!(300);input["aplConfig"]=json!("actions:\n - skill: 디스토션\n");input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{"26510":{"dark":0.01,"heat":0.01}}});input["character"]["skills"][0]["level"]=json!(level);let mut choices=vec![json!({"name":"순풍","level":1,"values":[999999]})];if let Some(name)=first {choices.push(json!({"name":name,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);let p=builder::build(&input.to_string()).unwrap();let skill=&p.skills[0];assert_eq!(skill.cast_times_ms,vec![1333]);assert_eq!(skill.hits.len(),2);assert_eq!(skill.hits.iter().map(|h|(h.hit_id.as_str(),h.effect_id,h.action_delay_ms)).collect::<Vec<_>>(),vec![("26510:0:0",265108,284),("26510:0:1",265108,332)]);if level==12 {assert!(skill.hits.iter().all(|h|h.base_damage==189.&&h.skill_modifier==1.1712));}
        assert_eq!(skill.timed_effects.len(),2);let dodge=&skill.timed_effects[0];let movement=&skill.timed_effects[1];assert!(dodge.until_cast_end&&!movement.until_cast_end);assert_eq!((dodge.action_delay_ms,movement.action_delay_ms),(0,500));assert!(matches!(&dodge.effect,crate::profile::TriggerEffect::ApplyBuff(b) if b.effects[&StatField::DodgeRate]==0.3&&b.max_stacks==1));assert!(matches!(&movement.effect,crate::profile::TriggerEffect::ApplyBuff(b) if b.effects[&StatField::MovementSpeedMax]==0.3&&b.duration_ms==4000&&b.max_stacks==1));
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&p.skills).unwrap());
        for critical in [false,true] {for fast in [false,true] {let mut observed=p.clone();observed.stat.crit_rate.clear();observed.stat.crit_rate.insert("test".into(),if critical {100.} else {-100.});if fast {observed.stat.attack_speed.insert("test".into(),40.);}let mut baseline=observed.clone();baseline.skills[0].timed_effects.clear();let e=engine::trace_one(&observed,42,6000).1;let b=engine::trace_one(&baseline,42,6000).1;assert_eq!(serde_json::to_value(&e.damage_events).unwrap(),serde_json::to_value(&b.damage_events).unwrap());assert_eq!(e.damage_events.len(),2);assert!(e.damage_events.iter().all(|h|h.is_crit==critical&&h.resource_gains.iter().any(|g|g.resource=="ReaperDark"&&(g.amount-if first==Some("그림자 충전") {0.0145} else {0.01}).abs()<1e-12)));assert!(e.casts[0].active_buffs.contains(&"reaper:evasion:265140".into()));}}
        for name in ["유령 질주","안개 질주","그림자 질주","약육강식"] {let mut bad=input.clone();bad["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":name,"level":1}));assert_eq!(builder::build(&bad.to_string()).is_err(),name=="약육강식");}
    }}
}

#[test]
fn reaper_gust_self_movement_notify_is_miss_independent_and_used_until_fixed_expiry() {
    let mut input=corrosion_input(26510,"디스토션",&[265108,265108]);input["character"]["skills"][0]["tripods"]=json!([{"name":"순풍","level":1}]);input["aplConfig"]=json!("actions:\n - skill: 디스토션\n");input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{}});let mut p=builder::build(&input.to_string()).unwrap();p.skills[0].max_uses=1;p.stat.attack_speed.clear();p.stat.attack_move_speed_percent=0.;p.stat.movement_speed.clear();p.stat.movement_speed_max.clear();p.stat.swiftness=0.;p.stat.bake();p.stat.movement_speed_damage_coefficient=0.5;
    let mut observer=p.skills[0].clone();observer.skill_id+=1;observer.name="후속 관측".into();observer.timed_effects.clear();observer.cooldown_ms=0;observer.max_uses=0;observer.cast_times_ms=vec![100];observer.hits.truncate(1);observer.hits[0].hit_id="test:gust:probe".into();observer.hits[0].action_delay_ms=0;observer.hits[0].triggers.clear();observer.resource_gains.clear();p.skills.push(observer);p.apl_actions.push(p.apl_actions[0].clone());p.apl_actions[1].skill_id=p.skills[1].skill_id;
    for fast in [false,true] {for all_miss in [false,true] {let mut observed=p.clone();if fast {observed.stat.attack_speed.insert("test".into(),40.);}if all_miss {observed.reaper_reference.as_mut().unwrap().missed_hit_ids=observed.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();}let e=engine::trace_one(&observed,42,7000).1;let end=(e.casts[1].time*1000.).round() as u64;assert_eq!(end,if fast {952} else {1333});let start=(500.*end as f64/1333.).round() as u64;for c in &e.casts[1..] {let t=(c.time*1000.).round() as u64;assert!(!c.active_buffs.contains(&"reaper:evasion:265140".into()));assert_eq!(c.active_buffs.contains(&"reaper:movement:265150".into()),t>=start&&t<start+4000);}
        let subsequent:Vec<_>=e.damage_events.iter().filter(|h|h.skill_name=="후속 관측").collect();assert!(subsequent.iter().any(|h|h.time>(start+4000) as f64/1000.));for h in subsequent {let t=(h.time*1000.).round() as u64;assert!((h.formula.movement_speed_damage_multiplier-if t<start+4000 {1.15} else {1.}).abs()<1e-12);}assert!(e.damage_events.iter().filter(|h|h.skill_name=="디스토션").all(|h|h.formula.movement_speed_damage_multiplier==1.));
    }}
}

#[test]
fn reaper_shabel_evasion_composes_poison_and_third_tier_without_damage_or_rng_change() {
    for poison in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")] {for third in [None,Some("낙인 활성"),Some("속전속결")] {
        let mut input=corrosion_input(26110,"샤벨 스팅거",&[261100,261100,261101,261101]);input["skillData"][0]["cooldown"]=json!(300);input["aplConfig"]=json!("actions:\n - skill: 샤벨 스팅거\n");input["assumedTargetNerveVulnerable"]=json!(true);
        let mut choices=vec![json!({"name":"회피의 달인","level":1,"values":[999999]})];for name in [poison,third].into_iter().flatten() {choices.push(json!({"name":name,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        let p=builder::build(&input.to_string()).unwrap();let mut plain=input.clone();plain["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|c|c["name"]!="회피의 달인");let base=builder::build(&plain.to_string()).unwrap();let skill=&p.skills[0];assert_eq!(serde_json::to_value(&skill.hits).unwrap(),serde_json::to_value(&base.skills[0].hits).unwrap());assert_eq!((skill.cast_times_ms.clone(),skill.cooldown_ms),(base.skills[0].cast_times_ms.clone(),base.skills[0].cooldown_ms));
        assert_eq!(skill.timed_effects.len(),1);let timed=&skill.timed_effects[0];assert!(timed.until_cast_end);assert_eq!((timed.phase,timed.action_delay_ms),(0,0));let buff=match &timed.effect {crate::profile::TriggerEffect::ApplyBuff(b)=>b,_=>panic!("self buff required")};assert_eq!((buff.effects[&StatField::DodgeRate],buff.duration_ms,buff.max_stacks),(0.2,0,1));assert!(p.stat.dodge_rate.is_empty());
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&p.skills).unwrap());
        for fast in [false,true] {for all_miss in [false,true] {let mut observed=p.clone();let mut baseline=base.clone();if fast {observed.stat.attack_speed.insert("test".into(),40.);baseline.stat.attack_speed.insert("test".into(),40.);}if all_miss {let missed=skill.hits.iter().map(|h|h.hit_id.clone()).collect::<Vec<_>>();observed.reaper_reference.as_mut().unwrap().missed_hit_ids=missed.clone();baseline.reaper_reference.as_mut().unwrap().missed_hit_ids=missed;}
            let e=engine::trace_one(&observed,42,10000).1;let b=engine::trace_one(&baseline,42,10000).1;assert_eq!(serde_json::to_value(&e.damage_events).unwrap(),serde_json::to_value(&b.damage_events).unwrap());assert_eq!(serde_json::to_value(&e.class_events).unwrap(),serde_json::to_value(&b.class_events).unwrap());assert!(e.casts[0].active_buffs.contains(&buff.id));assert!(e.casts[0].active_buff_details.iter().any(|b|b.id==buff.id&&b.name=="회피율 증가 (20%)"&&b.stacks==1));if !all_miss {assert!(e.casts[0].active_buff_details_after_hit.iter().any(|b|b.id==buff.id));}
        }}
        for name in ["두개의 그림자","날카로운 단검"] {let mut bad=input.clone();bad["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":name,"level":1}));assert!(builder::build(&bad.to_string()).is_err());}
    }}
}

#[test]
fn reaper_shabel_evasion_expires_at_speed_adjusted_cast_end_before_next_action() {
    let mut input=corrosion_input(26110,"샤벨 스팅거",&[261100,261100,261101,261101]);input["character"]["skills"][0]["tripods"]=json!([{"name":"회피의 달인","level":1}]);input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{}});input["aplConfig"]=json!("actions:\n - skill: 샤벨 스팅거\n");let mut p=builder::build(&input.to_string()).unwrap();p.skills[0].cast_times_ms=vec![1000];p.skills[0].cooldown_ms=300000;p.skills[0].hits.truncate(2);for (h,delay) in p.skills[0].hits.iter_mut().zip([999,1000]) {h.action_delay_ms=delay;h.fixed_delay_ms=0;}
    p.stat.attack_speed.clear();p.stat.attack_move_speed_percent=0.;p.stat.swiftness=0.;p.stat.bake();
    for fast in [false,true] {let mut observed=p.clone();if fast {observed.stat.attack_speed.insert("test".into(),40.);}let e=engine::trace_one(&observed,42,2000).1;assert_eq!(e.damage_events.len(),2);let before=(e.damage_events[0].time*1000.).round() as u32;let end=(e.damage_events[1].time*1000.).round() as u32;assert!(before<end);assert_eq!(end,if fast {714} else {1000});assert!(engine::trace_one(&observed,42,before).1.casts[0].active_buff_details_after_hit.iter().any(|b|b.id=="reaper:evasion:261160"));assert!(!e.casts[0].active_buff_details_after_hit.iter().any(|b|b.id=="reaper:evasion:261160"));
        let mut observer=observed.skills[0].clone();observer.skill_id+=1;observer.name="만료 관측".into();observer.timed_effects.clear();observed.skills.push(observer);observed.apl_actions.push(observed.apl_actions[0].clone());observed.apl_actions[1].skill_id=observed.skills[1].skill_id;let e=engine::trace_one(&observed,42,2000).1;assert_eq!(e.casts[1].time,end as f64/1000.);assert!(!e.casts[1].active_buffs.iter().any(|id|id=="reaper:evasion:261160"));
    }
}

#[test]
fn reaper_trap_swamp_composes_all_tiers_fixed_callbacks_and_slow_hit_gate() {
    for first in [None,Some("치명적인 그림자"),Some("날렵한 몸놀림"),Some("덫 강화")] {for third in [None,Some("연속 공격"),Some("그림자 활성")] {
        let mut input=corrosion_input(26570,"쉐도우 트랩",&[265700]);input["skillData"][0]["cooldown"]=json!(300);input["aplConfig"]=json!("actions:\n - skill: 쉐도우 트랩\n");input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{"26570":{"dark":0.01,"heat":0.01}}});let mut choices=vec![json!({"name":"그림자 늪","level":1,"values":[999999]})];for name in [first,third].into_iter().flatten() {choices.push(json!({"name":name,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        for level in [10,12,14] {input["character"]["skills"][0]["level"]=json!(level);let off=builder::build(&input.to_string()).unwrap();input["assumedTargetSlowVulnerable"]=json!(true);let p=builder::build(&input.to_string()).unwrap();input["assumedTargetSlowVulnerable"]=json!(false);let skill=&p.skills[0];assert_eq!(skill.hits.len(),7);assert_eq!(skill.cast_times_ms,vec![500]);assert_eq!(skill.hits[0].hit_id,"26570:0:0");assert_eq!(skill.hits.iter().map(|h|h.hit_id.clone()).collect::<std::collections::HashSet<_>>().len(),7);
            assert_eq!(skill.hits.iter().map(|h|(h.effect_id,h.action_delay_ms,h.fixed_delay_ms)).collect::<Vec<_>>(),vec![(if first==Some("덫 강화") {265754} else {265751},30,400)].into_iter().chain([450,950,1450,1950,2450,2950].map(|delay|(if first==Some("덫 강화") {265755} else {265752},30,delay))).collect::<Vec<_>>());if level==12 {assert_eq!((skill.hits[0].base_damage,skill.hits[0].skill_modifier),(739.,4.6319));assert!(skill.hits.iter().skip(1).all(|h|(h.base_damage,h.skill_modifier)==(26.5,0.1544)));}
            let buff=skill.hits[0].triggers.iter().find_map(|t|match &t.effect {crate::profile::TriggerEffect::ApplyTargetBuff(b) if b.id=="reaper:slow:265726"=>Some(b.clone()),_=>None}).unwrap();assert_eq!((buff.effects[&StatField::TargetMovementSpeedReduction],buff.duration_ms,buff.max_stacks),(0.69,3000,1));assert_eq!(buff.effects.len(),1);assert!(skill.hits.iter().all(|h|h.triggers.iter().filter(|t|matches!(&t.effect,crate::profile::TriggerEffect::ApplyTargetBuff(b) if b.id==buff.id)).count()==1));
            assert_eq!((skill.cooldown_ms,skill.max_stacks,skill.charge_recovery_ms),(off.skills[0].cooldown_ms,off.skills[0].max_stacks,off.skills[0].charge_recovery_ms));assert_eq!(skill.max_stacks,if third==Some("연속 공격") {2} else {0});
            for critical in [false,true] {let mut observed=p.clone();observed.stat.crit_rate.clear();observed.stat.crit_rate.insert("test".into(),if critical {100.} else {-100.});let mut inactive=off.clone();inactive.stat=observed.stat.clone();for fast in [false,true] {if fast {observed.stat.attack_speed.insert("test".into(),40.);inactive.stat.attack_speed.insert("test".into(),40.);}let e=engine::trace_one(&observed,42,7000).1;let a=engine::trace_one(&inactive,42,7000).1;assert_eq!(serde_json::to_value(&e.damage_events).unwrap(),serde_json::to_value(&a.damage_events).unwrap());let hits:Vec<_>=e.damage_events.iter().filter(|h|h.cast_id==Some(e.casts[0].cast_id)).collect();assert_eq!(hits.len(),7);assert!(hits.iter().all(|h|h.is_crit==critical&&h.resource_gains.iter().any(|g|g.resource=="ReaperDark"&&(g.amount-0.01).abs()<1e-12)));assert!(hits[1..].windows(2).all(|pair|(pair[1].time-pair[0].time-0.5).abs()<1e-12));assert!(e.casts[0].target_debuff_details_after_hit.iter().any(|b|b.id==buff.id));}}
            let mut missed=p.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[0].hit_id.clone()];let e=engine::trace_one(&missed,42,3500).1;assert_eq!(e.damage_events.iter().filter(|h|h.cast_id==Some(e.casts[0].cast_id)).count(),6);assert!(e.casts[0].target_debuff_details_after_hit.iter().any(|b|b.id==buff.id));missed.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();let e=engine::trace_one(&missed,42,7000).1;assert!(e.damage_events.is_empty());assert!(e.casts.iter().all(|c|c.target_debuff_details_after_hit.iter().all(|b|b.id!=buff.id)));
            input["assumedTargetSlowVulnerable"]=json!(true);input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&p.skills).unwrap());input["assumedTargetSlowVulnerable"]=json!(false);
        }
    }}
}

#[test]
fn reaper_trap_swamp_slow_keeps_independent_expiry_and_strongest_target_state() {
    use crate::profile::{BuffSpec,StackType};let mut buffs=crate::engine::state::BuffManager::new();let specs=[("weak",0.462),("reaper:slow:265726",0.69)].map(|(id,value)|BuffSpec {id:id.into(),effects:hashbrown::HashMap::from([(StatField::TargetMovementSpeedReduction,value)]),max_stacks:1,stack_type:StackType::Refresh,duration_ms:3000});buffs.apply(specs[0].clone(),7000,1);buffs.apply(specs[1].clone(),3000,1);assert_eq!(buffs.target_movement_speed_reduction(2999),Some(0.69));buffs.apply(specs[1].clone(),5000,1);buffs.expire_if("reaper:slow:265726",3000);assert_eq!(buffs.target_movement_speed_reduction(3000),Some(0.69));assert_eq!(buffs.target_movement_speed_reduction(4999),Some(0.69));assert_eq!(buffs.target_movement_speed_reduction(5000),Some(0.462));assert_eq!(buffs.target_movement_speed_reduction(7000),None);
}

#[test]
fn reaper_shabel_mark_does_not_reduce_the_cast_that_first_activated_it() {
    let mut input=corrosion_input(26110,"샤벨 스팅거",&[261100,261100,261101,261101]);input["character"]["skills"][0]["tripods"]=json!([{"name":"낙인 활성","level":1}]);input["aplConfig"]=json!("actions:\n - skill: 샤벨 스팅거\n");let mut p=builder::build(&input.to_string()).unwrap();p.skills[0].cooldown_ms=1000;p.stat.swiftness=0.;p.stat.cooldown_reduction.clear();p.stat.cooldown_penalty=0.;
    let e=engine::trace_one(&p,42,5000).1;assert!(e.casts.len()>4);assert!((e.casts[1].time-e.casts[0].time-1.).abs()<1e-12);assert!(e.casts.windows(2).skip(1).all(|c|(c[1].time-c[0].time-0.79).abs()<1e-12));let mut missed=p.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=p.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&missed,42,5000).1.casts.windows(2).all(|c|(c[1].time-c[0].time-1.).abs()<1e-12));
}

#[test]
fn reaper_shabel_mark_composes_poison_and_first_second_tier_without_poison_reapplication() {
    for poison in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")] {for second in [None,Some("두개의 그림자"),Some("날카로운 단검")] {
        let mut input=corrosion_input(26110,"샤벨 스팅거",&[261100,261100,261101,261101]);input["skillData"][0]["cooldown"]=json!(300);input["assumedTargetNerveVulnerable"]=json!(true);let mut choices=vec![json!({"name":"낙인 활성","level":1,"values":[999999]})];for name in [poison,second].into_iter().flatten() {choices.push(json!({"name":name,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        let mut plain=input.clone();plain["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|t|t["name"]!="낙인 활성");let baseline=builder::build(&plain.to_string()).unwrap();let p=builder::build(&input.to_string()).unwrap();assert_eq!(p.skills[0].cooldown_ms,baseline.skills[0].cooldown_ms);assert_eq!(p.skills[0].hits.len(),baseline.skills[0].hits.len());assert!(p.skills[0].hits.iter().all(|h|h.triggers.iter().filter(|t|matches!(&t.effect,crate::profile::TriggerEffect::ApplyBuff(b) if b.id=="reaper:shabel_mark")).count()==1));
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&p.skills).unwrap());let e=engine::trace_one(&p,42,10000).1;assert!(e.casts[0].active_buff_details_after_hit.iter().any(|b|b.id=="reaper:shabel_mark"));let mut miss=p.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=miss.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&miss,42,10000).1.casts.iter().all(|c|!c.active_buff_details_after_hit.iter().any(|b|b.id=="reaper:shabel_mark")));
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"속전속결","level":1}));assert!(builder::build(&input.to_string()).is_err());
    }}
}

#[test]
fn reaper_fury_shadow_repeat_ratio_composes_stats_and_keeps_one_final_hit() {
    for first in [None,Some("날렵한 움직임"),Some("부위파괴 강화")] {for second in [None,Some("혼돈 강화"),Some("약육강식"),Some("치명적인 단검")] {
        let mut input=corrosion_input(26830,"댄싱 오브 퓨리",&[268300,268300,268300,268300,268301]);
        input["character"]["skills"][0]["level"]=json!(12);input["skillData"][0]["cooldown"]=json!(25);
        input["character"]["skills"][0]["tripods"]=json!([Some("그림자의 춤"),first,second].into_iter().flatten().map(|name|json!({"name":name,"level":1,"values":[999999]})).collect::<Vec<_>>());
        input["assumedTargetSeedOrLower"]=json!(true);input["aplConfig"]=json!("actions:\n - skill: 댄싱 오브 퓨리\n");
        let p=builder::build(&input.to_string()).unwrap();let s=&p.skills[0];let saved=if first==Some("날렵한 움직임") {222} else {0};
        assert_eq!(s.cast_times_ms,vec![4200-saved]);assert_eq!(s.hits.len(),7);assert_eq!(s.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),[vec![268370;6],vec![268371]].concat());
        assert_eq!(s.hits.iter().map(|h|h.action_delay_ms).collect::<Vec<_>>(),[770,1103,1436,1770,2103,2436,3295].map(|n|n-saved));assert!(s.hits.iter().all(|h|h.fixed_delay_ms==0));
        assert!(s.hits[..6].iter().all(|h|(h.skill_modifier-2.3949).abs()<1e-9));assert!((s.hits[6].skill_modifier-14.3636).abs()<1e-9);
        let bonus=1.05*1.75*(if second==Some("치명적인 단검") {1.35} else if second==Some("약육강식") {1.948} else {1.})-1.;assert!(s.hits.iter().all(|h|(h.damage_increase-bonus).abs()<1e-9));
        assert!(s.hits[..6].iter().all(|h|h.destruction==0));assert_eq!(s.hits[6].destruction,if first==Some("부위파괴 강화") {2} else {1});
        assert_eq!(s.timed_effects.len(),1);assert!(s.timed_effects[0].until_cast_end);assert!(p.reaper_reference.as_ref().unwrap().metadata["fury_shadow_policy"]["policy"].as_str().unwrap().contains("Provisional"));
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();let reverse=builder::build(&input.to_string()).unwrap();
        for fast in [false,true] {let mut a=p.clone();let mut b=reverse.clone();for profile in [&mut a,&mut b] {profile.skills[0].max_uses=1;if fast {profile.stat.attack_speed.insert("test".into(),40.);profile.stat.bake();}}
            let (result,trace)=engine::trace_one(&a,42,30000);let (other,opposite)=engine::trace_one(&b,42,30000);assert_eq!(serde_json::to_value(result).unwrap(),serde_json::to_value(other).unwrap());assert_eq!(serde_json::to_value(&trace).unwrap(),serde_json::to_value(opposite).unwrap());assert_eq!(trace.damage_events.len(),7);
            a.reaper_reference.as_mut().unwrap().missed_hit_ids=a.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();let miss=engine::trace_one(&a,42,30000).1;assert!(miss.damage_events.is_empty());
        }
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"절제된 동작","level":1}));assert!(builder::build(&input.to_string()).is_err());
    }}
}

#[test]
fn reaper_distortion_routes_compose_gust_and_ignore_choice_order() {
    for (name,cast,ids,times,gust_time) in [("유령 질주",2100,vec![265180],vec![(624,0)],1267),("안개 질주",2666,vec![265108,265108,265151,265151,265153,265153,265153],vec![(284,0),(332,0),(1616,0),(1666,0),(1333,500),(1333,800),(1333,1100)],1833),("그림자 질주",2433,vec![265174,265174],vec![(733,0),(733,350)],1600)] {
        for first in [None,Some("그림자 충전"),Some("치명적인 그림자"),Some("빠른 준비")] {for third in [None,Some("순풍"),Some("약육강식")] {
            let mut input=corrosion_input(26510,"디스토션",&[265108,265108]);
            input["skillData"][0]["cooldown"]=json!(12);
            input["aplConfig"]=json!("actions:\n - skill: 디스토션\n");
            input["assumedTargetSeedOrLower"]=json!(true);
            input["character"]["skills"][0]["tripods"]=json!([Some(name),first,third].into_iter().flatten().map(|name|json!({"name":name,"level":1,"values":[999999]})).collect::<Vec<_>>());
            let p=builder::build(&input.to_string()).unwrap();let s=&p.skills[0];
            assert_eq!(s.cast_times_ms,vec![cast]);assert_eq!(s.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),ids);
            assert_eq!(s.hits.iter().map(|h|(h.action_delay_ms,h.fixed_delay_ms)).collect::<Vec<_>>(),times);
            assert_eq!(s.cooldown_ms,if name=="안개 질주" {20000} else {12000}-if first==Some("빠른 준비") {4000} else {0});
            for h in &s.hits {let expected=1.05*(if name=="그림자 질주" {1.6} else {1.})*(if third==Some("약육강식") {3.64} else {1.})-1.;assert!((h.damage_increase-expected).abs()<1e-10,"{name} {first:?} {third:?}: {} != {expected}",h.damage_increase);}
            assert!(p.reaper_reference.as_ref().unwrap().metadata["distortion_policy"]["policy"].as_str().unwrap().contains("User-approved"));
            if third==Some("순풍") {assert_eq!(s.timed_effects.len(),2);assert_eq!(s.timed_effects[1].action_delay_ms,gust_time);}
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();let reverse=builder::build(&input.to_string()).unwrap();
            for fast in [false,true] {let mut a=p.clone();let mut b=reverse.clone();if fast {for p in [&mut a,&mut b] {p.stat.attack_speed.insert("test".into(),40.);p.stat.bake();}}
                let (result,trace)=engine::trace_one(&a,42,30000);let (other,opposite)=engine::trace_one(&b,42,30000);
                assert_eq!(serde_json::to_value(result).unwrap(),serde_json::to_value(other).unwrap());assert_eq!(serde_json::to_value(&trace).unwrap(),serde_json::to_value(opposite).unwrap());
                if name=="안개 질주" {let events:Vec<_>=trace.damage_events.iter().filter(|h|matches!(h.hit_id.as_str(),"26510:5:4"|"26510:5:5"|"26510:5:6")).collect();assert!(events.len()>=3);assert!((events[1].time-events[0].time-0.3).abs()<1e-9);}
            }
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":if name=="유령 질주" {"그림자 질주"} else {"유령 질주"},"level":1}));assert!(builder::build(&input.to_string()).is_err());
        }}
    }
}

#[test]
fn reaper_spirit_leap_scales_all_owner_times_and_preserves_fixed_delays() {
    for first in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")] {for third in [None,Some("기습"),Some("날카로운 단검")] {for grid in [false,true] {
        let mut input=corrosion_input(26060,"스피릿 캐치",&[260600,260612]);
        input["aplConfig"]=json!("actions:\n - skill: 스피릿 캐치\n");
        input["character"]["skills"][0]["tripods"]=json!([first,third].into_iter().flatten().map(|name|json!({"name":name,"level":1})).collect::<Vec<_>>());
        let base=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"도약","level":1,"values":[999]}));
        if grid {let mut slot=trace_hero_slot("출혈독","ancient",20);slot["coreAlignment"]=json!("질서");for gem in slot["gems"].as_array_mut().unwrap() {gem["name"]=json!("안정");}input["character"]["arkGrids"]=json!({"reaperSlots":[slot]});}
        let profile=builder::build(&input.to_string()).unwrap();
        let mut same_grid=input.clone();same_grid["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|c|c["name"]!="도약");
        let baseline=builder::build(&same_grid.to_string()).unwrap();
        let scale=|time:u32| {let time=(time as f64/1.3).round() as u32;if grid {(time as f64/1.15).round() as u32} else {time}};
        let selected=&profile.skills[0];let old=&base.skills[0];
        assert_eq!(selected.cast_times_ms,old.cast_times_ms.iter().copied().map(scale).collect::<Vec<_>>());
        assert_eq!((selected.cooldown_ms,selected.charge_recovery_ms),(old.cooldown_ms,old.charge_recovery_ms));
        for (index,(a,b)) in selected.hits.iter().zip(&baseline.skills[0].hits).enumerate() {
            assert_eq!((a.action_delay_ms,a.fixed_delay_ms),(scale(old.hits[index].action_delay_ms),b.fixed_delay_ms));
            let mut expected=b.clone();expected.action_delay_ms=scale(old.hits[index].action_delay_ms);assert_eq!(serde_json::to_value(a).unwrap(),serde_json::to_value(expected).unwrap());
        }
        for (a,b) in selected.timed_effects.iter().zip(&old.timed_effects) {assert_eq!((a.action_delay_ms,a.action_end_ms),(scale(b.action_delay_ms),b.action_end_ms.map(scale)));}
        assert!(profile.reaper_reference.as_ref().unwrap().metadata["spirit_leap_policy"]["policy"].as_str().unwrap().contains("User-approved"));
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();let reverse=builder::build(&input.to_string()).unwrap();
        for fast in [false,true] {let mut a=profile.clone();let mut b=reverse.clone();if fast {for p in [&mut a,&mut b] {p.stat.attack_speed.insert("test".into(),40.);p.stat.bake();}}
            let (result,trace)=engine::trace_one(&a,42,10000);let (other,opposite)=engine::trace_one(&b,42,10000);
            assert_eq!(serde_json::to_value(result).unwrap(),serde_json::to_value(other).unwrap());
            assert_eq!(serde_json::to_value(trace).unwrap(),serde_json::to_value(opposite).unwrap());
        }
    }}}
}

#[test]
fn reaper_mist_mobility_adds_speed_during_cast_caps_damage_and_expires() {
    for third in [None,Some("거짓 안개"),Some("안개 폭발")] {for fast in [false,true] {for speed in [0.,0.3,0.4] {
        let mut input=corrosion_input(26560,"블랙 미스트",&[265600;5]);
        input["aplConfig"]=json!("actions:\n - skill: 블랙 미스트\n");
        input["character"]["skills"][0]["tripods"]=json!(third.into_iter().map(|name|json!({"name":name,"level":1})).collect::<Vec<_>>());
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{}});
        let mut base=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"탁월한 기동성","level":1,"values":[999]}));
        let mut profile=builder::build(&input.to_string()).unwrap();
        assert_eq!(serde_json::to_value(&profile.skills[0].hits).unwrap(),serde_json::to_value(&base.skills[0].hits).unwrap());
        for p in [&mut base,&mut profile] {
            p.stat.swiftness=0.;p.stat.attack_move_speed_percent=0.;p.stat.movement_speed.clear();p.stat.movement_speed_max.clear();p.stat.movement_speed.insert("test".into(),speed);p.stat.attack_speed.clear();if fast {p.stat.attack_speed.insert("test".into(),40.);}p.stat.bake();p.stat.movement_speed_damage_coefficient=0.5;
            p.skills[0].max_uses=1;
            let mut observer=p.skills[0].clone();observer.skill_id+=1;observer.name="버프 만료 관측".into();observer.timed_effects.clear();p.skills.push(observer);
            let mut action=p.apl_actions[0].clone();action.skill_id=p.skills[1].skill_id;p.apl_actions.push(action);
        }
        let trace=engine::trace_one(&profile,42,12000).1;let old=engine::trace_one(&base,42,12000).1;
        let end=trace.casts[1].time;
        assert_eq!(trace.casts[1].time,old.casts[1].time);
        assert!(trace.casts[0].active_buffs.contains(&"reaper:movement:mist-mobility".into()));
        assert!(!trace.casts[1].active_buffs.contains(&"reaper:movement:mist-mobility".into()));
        assert_eq!(trace.damage_events.len(),old.damage_events.len());
        let ratio=(1.+(speed+0.302).min(0.4)*0.5)/(1.+speed*0.5);
        for (hit,baseline) in trace.damage_events.iter().zip(&old.damage_events) {
            assert_eq!((hit.time,hit.hit_id.clone()),(baseline.time,baseline.hit_id.clone()));
            let expected=if hit.time<end {ratio} else {1.};
            assert!((hit.result_damage-baseline.result_damage*expected).abs()<1e-6);
        }
        let missed=profile.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();profile.reaper_reference.as_mut().unwrap().missed_hit_ids=missed;
        assert!(engine::trace_one(&profile,42,12000).1.casts[0].active_buffs.contains(&"reaper:movement:mist-mobility".into()));
    }}}
}

#[test]
fn reaper_mist_eye_selects_one_region_preserving_layers_explosion_damage_and_gauge() {
    for level in [10,12,14] {for wide in [false,true] {for third in [None,Some("거짓 안개"),Some("안개 폭발")] {for centre in [false,true] {
        let mut input=corrosion_input(26560,"블랙 미스트",&[265600;5]);input["skillData"][0]["cooldown"]=json!(300);input["character"]["skills"][0]["level"]=json!(level);input["aplConfig"]=json!("actions:\n - skill: 블랙 미스트\n");input["assumedTargetMistCentre"]=json!(centre);input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{"26560":{"dark":0.01,"heat":0.01}}});
        let mut choices=vec![json!({"name":"안개의 눈","level":1,"values":[999999]})];if wide {choices.push(json!({"name":"넓은 안개","level":1}));}if let Some(name)=third {choices.push(json!({"name":name,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        let mut plain=input.clone();plain["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|t|t["name"]!="안개의 눈");let baseline=builder::build(&plain.to_string()).unwrap();let p=builder::build(&input.to_string()).unwrap();let s=&p.skills[0];let old=&baseline.skills[0];assert_eq!(s.hits.len(),old.hits.len());assert_eq!(s.cast_times_ms,old.cast_times_ms);assert_eq!(s.hits.len(),if third==Some("안개 폭발") {8} else {5});
        let (base,ghost,explosion)=match (wide,centre) {(false,true)=>(265640,265677,[265690,265692,265663]),(false,false)=>(265641,265678,[265691,265693,265664]),(true,true)=>(265642,265679,[265695,265697,265699]),(true,false)=>(265643,265680,[265696,265698,265662])};let mut ids=vec![if third==Some("거짓 안개") {ghost} else {base};5];if third==Some("안개 폭발") {ids.extend(explosion);}assert_eq!(s.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),ids);
        for (a,b) in s.hits.iter().zip(&old.hits) {assert_eq!((a.base_damage,a.skill_modifier,a.action_delay_ms,a.fixed_delay_ms,a.hit_id.as_str()),(b.base_damage,b.skill_modifier,b.action_delay_ms,b.fixed_delay_ms,b.hit_id.as_str()));assert!(((1.+a.damage_increase)/(1.+b.damage_increase)-if centre {1.8} else {1.}).abs()<1e-12);}
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&p.skills).unwrap());
        for critical in [false,true] {let mut observed=p.clone();observed.stat.crit_rate.clear();observed.stat.crit_rate.insert("test".into(),if critical {100.} else {-100.});let e=engine::trace_one(&observed,42,10000).1;assert_eq!(e.damage_events.len(),s.hits.len());assert!(e.damage_events.iter().all(|h|h.is_crit==critical&&h.resource_gains.iter().any(|g|g.resource=="ReaperDark"&&(g.amount-0.01).abs()<1e-12)));
            observed.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![s.hits[0].hit_id.clone()];assert_eq!(engine::trace_one(&observed,42,10000).1.damage_events.len(),s.hits.len()-1);observed.reaper_reference.as_mut().unwrap().missed_hit_ids=s.hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&observed,42,10000).1.damage_events.is_empty());
        }
    }}}}
}

#[test]
fn reaper_mist_eye_keeps_old_default_false_and_rejects_level_one_and_same_tier_spread() {
    let mut input=corrosion_input(26560,"블랙 미스트",&[265600;5]);input["character"]["skills"][0]["tripods"]=json!([]);let old=builder::build(&input.to_string()).unwrap();input["assumedTargetMistCentre"]=json!(true);assert_eq!(serde_json::to_value(old.skills).unwrap(),serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap());
    input["character"]["skills"][0]["level"]=json!(12);input["character"]["skills"][0]["tripods"]=json!([{"name":"안개의 눈","level":1},{"name":"넓은 안개","level":1},{"name":"거짓 안개","level":1}]);let p=builder::build(&input.to_string()).unwrap();input["assumedTargetMistCentre"]=json!(false);assert_ne!(p.skills[0].hits[0].effect_id,builder::build(&input.to_string()).unwrap().skills[0].hits[0].effect_id);
    input["character"]["skills"][0]["level"]=json!(1);assert!(builder::build(&input.to_string()).unwrap_err().contains("스킬 7레벨 필요"));input["character"]["skills"][0]["level"]=json!(12);input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"퍼지는 안개","level":1}));assert!(builder::build(&input.to_string()).is_err());
}

#[test]
fn reaper_storm_zone_six_hits_keep_fixed_callbacks_final_scope_and_owner_compositions() {
    for first in [None,Some("그림자 충전"),Some("치명적인 그림자"),Some("꿰뚫는 일격")] {for second in [None,Some("난무 강화"),Some("빠른 준비"),Some("마침표")] {
        let mut input=corrosion_input(26600,"쉐도우 스톰",&[266000]);input["skillData"][0]["cooldown"]=json!(300);input["character"]["skills"][0]["level"]=json!(12);input["aplConfig"]=json!("actions:\n - skill: 쉐도우 스톰\n");input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{"26600":{"dark":0.01,"heat":0.01}}});
        let mut choices=vec![json!({"name":"그림자 지대","level":1,"values":[999999]})];if let Some(f)=first {choices.push(json!({"name":f,"level":1}));}if let Some(s)=second {choices.push(json!({"name":s,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        let mut plain=input.clone();plain["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|t|t["name"]!="그림자 지대");let baseline=builder::build(&plain.to_string()).unwrap();let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];assert_eq!(skill.cast_times_ms,vec![1667]);assert_eq!(skill.hits.len(),6);assert_eq!(skill.hits.iter().map(|h|(h.effect_id,h.action_delay_ms,h.fixed_delay_ms)).collect::<Vec<_>>(),vec![(266083,430,0),(266081,519,300),(266081,519,500),(266081,519,700),(266081,519,900),(266082,519,1100)]);assert_eq!(skill.hits.iter().map(|h|h.base_damage).collect::<Vec<_>>(),vec![1282.,242.,242.,242.,242.,961.]);assert_eq!(skill.hits.iter().map(|h|h.skill_modifier).collect::<Vec<_>>(),vec![8.0078,1.5011,1.5011,1.5011,1.5011,6.0067]);for (i,h) in skill.hits.iter().enumerate() {assert!(((1.+h.damage_increase)/(1.+baseline.skills[0].hits[0].damage_increase)-1.7*if i==5&&second==Some("마침표") {2.5} else {1.}).abs()<1e-12);assert_eq!(h.crit_damage_bonus,if second==Some("난무 강화") {1.2} else {0.});assert_eq!(h.defense_ignore,if first==Some("꿰뚫는 일격") {0.62} else {0.});}assert_eq!(skill.on_end_if_hit.len(),usize::from(second==Some("빠른 준비")));
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&profile.skills).unwrap());
        for crit in [false,true] {let mut p=profile.clone();p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),if crit {100.} else {-100.});let e=engine::trace_one(&p,42,1800).1;assert_eq!(e.damage_events.len(),6);assert!(e.damage_events.iter().all(|h|h.is_crit==crit));let callbacks:Vec<_>=e.damage_events.iter().skip(1).collect();for pair in callbacks.windows(2) {assert!((pair[1].time-pair[0].time-0.2).abs()<1e-12);}assert!(e.damage_events.iter().all(|h|h.resource_gains.iter().any(|g|g.resource=="ReaperDark"&&(g.amount-if first==Some("그림자 충전") {0.0145} else {0.01}).abs()<1e-12)));
            p.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[0].hit_id.clone()];assert_eq!(engine::trace_one(&p,42,1800).1.damage_events.len(),5);p.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&p,42,10000).1.damage_events.is_empty());
        }
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"검은 폭풍","level":1}));assert!(builder::build(&input.to_string()).is_err());
    }}
}

#[test]
fn reaper_storm_zone_end_cooldown_does_not_count_late_explosion_retroactively() {
    for speed in [0.,0.4] {for mode in ["all","explosion","miss"] {
        let mut input=corrosion_input(26600,"쉐도우 스톰",&[266000]);input["skillData"][0]["cooldown"]=json!(20);input["character"]["skills"][0]["tripods"]=json!([{"name":"그림자 지대","level":1},{"name":"빠른 준비","level":1}]);input["aplConfig"]=json!("actions:\n - skill: 쉐도우 스톰\n");input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{}});
        let mut p=builder::build(&input.to_string()).unwrap();p.stat.swiftness=0.;p.stat.baked_swiftness_cd_reduction=0.;p.stat.attack_speed.clear();p.stat.attack_move_speed_percent=0.;p.stat.attack_speed.insert("test".into(),speed);
        if mode!="all" {p.reaper_reference.as_mut().unwrap().missed_hit_ids=p.skills[0].hits.iter().take(if mode=="miss" {6} else {5}).map(|h|h.hit_id.clone()).collect();}
        let e=engine::trace_one(&p,42,50000).1;assert!(e.casts.len()>=3);let interval=if mode=="all"||mode=="explosion"&&speed==0. {18.7} else {20.};for pair in e.casts.windows(2) {assert!((pair[1].time-pair[0].time-interval).abs()<1e-12);}
    }}
}

#[test]
fn reaper_death_eye_two_hits_keep_conditional_damage_out_of_poison_and_extra_packets() {
    for (i,poison) in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")].iter().enumerate() {for third in [None,Some("기습"),Some("치명적인 단검")] {for back in [false,true] {
        let mut input=corrosion_input(26120,"데스 사이드",&[261200,261201]);input["skillData"][0]["cooldown"]=json!(300);input["skillData"][0]["properties"]["attackType"]=json!(if back {"백 어택"} else {""});input["character"]["skills"][0]["level"]=json!(12);input["aplConfig"]=json!("actions:\n - skill: 데스 사이드\n");input["assumedTargetNerveVulnerable"]=json!(true);
        let mut choices=vec![json!({"name":"폭풍의 눈","level":1,"values":[999999]})];if let Some(p)=poison {choices.push(json!({"name":p,"level":1}));}if let Some(t)=third {choices.push(json!({"name":t,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        let base=builder::build(&input.to_string()).unwrap();let skill=&base.skills[0];assert_eq!(skill.cast_times_ms,vec![1367]);assert_eq!(skill.hits.len(),2);assert_eq!(skill.hits.iter().map(|h|h.action_delay_ms).collect::<Vec<_>>(),vec![163,976]);assert_eq!(skill.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),if i<2 {vec![261280,261281]} else if i==2 {vec![261282,261283]} else {vec![261284,261285]});assert_eq!((skill.hits[0].base_damage,skill.hits[1].base_damage),(235.5,1098.));assert_eq!((skill.hits[0].skill_modifier,skill.hits[1].skill_modifier),(1.4669,6.844));
        input["assumedTargetSeedOrLower"]=json!(true);let selected=builder::build(&input.to_string()).unwrap();for (hit,old) in selected.skills[0].hits.iter().zip(&skill.hits) {assert!(((1.+hit.damage_increase)/(1.+old.damage_increase)-2.05).abs()<1e-12);}
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&selected.skills).unwrap());
        for crit in [false,true] {let force=|p:&crate::profile::CharacterProfiles| {let mut x=p.clone();x.stat.crit_rate.clear();x.stat.crit_rate.insert("test".into(),if crit {100.} else {-100.});x};let p=force(&selected);let e=engine::trace_one(&p,42,1500).1;let old=engine::trace_one(&force(&base),42,1500).1;let direct:Vec<_>=e.damage_events.iter().filter(|h|h.hit_id.starts_with("26120:")).collect();assert_eq!(direct.len(),2);assert!(direct.iter().all(|h|h.is_crit==crit));let poison_events=|e:&crate::constants::TraceData|e.damage_events.iter().filter(|h|h.hit_id.starts_with("reaper:")).map(|h|serde_json::to_value(h).unwrap()).collect::<Vec<_>>();assert_eq!(poison_events(&e),poison_events(&old));
            let states=&e.casts[0].target_debuff_details_after_hit;if i>0 {let extra=if i==2 {if third==Some("기습")&&back {2} else if third==Some("치명적인 단검")&&crit {1} else {0}} else {0};assert_eq!(states.iter().find(|b|b.id==["","reaper:corrosion","reaper:bleeding","reaper:nerve:darkness"][i]).unwrap().stacks,1+extra);}else {assert!(states.is_empty());}
        }
        let mut miss=selected.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[1].hit_id.clone()];assert!(engine::trace_one(&miss,42,1500).1.casts[0].target_debuff_details_after_hit.is_empty());miss.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&miss,42,10000).1.damage_events.is_empty());
        for name in ["데스 콜","변칙 공격"] {let mut bad=input.clone();bad["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":name,"level":1}));assert!(builder::build(&bad.to_string()).is_err());}
    }}}
}

#[test]
fn reaper_death_eye_back_bleeding_consumes_at_cap_and_keeps_overflow() {
    let mut input=corrosion_input(26120,"데스 사이드",&[261200,261201]);input["skillData"][0]["cooldown"]=json!(3);input["character"]["skills"][0]["tripods"]=json!([{"name":"폭풍의 눈","level":1},{"name":"독 : 출혈","level":1},{"name":"기습","level":1}]);input["assumedTargetSeedOrLower"]=json!(true);input["aplConfig"]=json!("actions:\n - skill: 데스 사이드\n");
    let mut profile=builder::build(&input.to_string()).unwrap();profile.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![profile.skills[0].hits[0].hit_id.clone()];let e=engine::trace_one(&profile,42,11800).1;assert_eq!(e.casts.len(),4);let burst:Vec<_>=e.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:burst").collect();assert_eq!(burst.len(),1);assert_eq!(burst[0].formula.skill_damage_multiplier,1.);assert!(burst[0].resource_gains.is_empty());assert_eq!(e.casts[3].target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:bleeding").unwrap().stacks,2);
    profile.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![profile.skills[0].hits[1].hit_id.clone()];let miss=engine::trace_one(&profile,42,11800).1;assert!(miss.damage_events.iter().all(|h|!h.hit_id.starts_with("reaper:")));assert!(miss.casts.iter().all(|c|c.target_debuff_details_after_hit.is_empty()));
}

#[test]
fn reaper_shadow_dot_wide_keeps_two_hits_final_poison_and_third_tier_composition() {
    for (i,poison) in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")].iter().enumerate() {for mode in ["plain","immune-off","immune-on","active"] {
        let mut input=corrosion_input(26070,"쉐도우 닷",&[260701,260702]);input["skillData"][0]["cooldown"]=json!(300);input["skillData"][0]["castTimesMs"]=json!([1333]);input["character"]["skills"][0]["level"]=json!(12);input["aplConfig"]=json!("actions:\n - skill: 쉐도우 닷\n");input["assumedTargetNerveVulnerable"]=json!(true);input["assumedTargetAbnormalMoveImmune"]=json!(mode=="immune-on");
        for (index,h) in input["skillData"][0]["hits"].as_array_mut().unwrap().iter_mut().enumerate() {h["hitId"]=json!(format!("26070:0:{index}"));h["actionDelayMs"]=json!([270,420][index]);h["fixedDamage"]=json!(vec![576.5;20]);h["damageRatio"]=json!(vec![3.5853;20]);}
        let mut choices=vec![json!({"name":"넓은 공격","level":1,"values":[999999]})];if let Some(p)=poison {choices.push(json!({"name":p,"level":1}));}if mode.starts_with("immune") {choices.push(json!({"name":"죽음의 일격","level":1}));}if mode=="active" {choices.push(json!({"name":"급습 활성","level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];assert_eq!(skill.cast_times_ms,vec![1333]);assert_eq!(skill.hits.len(),2);assert_eq!(skill.hits.iter().map(|h|h.action_delay_ms).collect::<Vec<_>>(),vec![296,399]);assert_eq!(skill.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),if i<2 {vec![260761,260762]} else if i==2 {vec![260764,260765]} else {vec![260767,260768]});assert!(skill.hits.iter().all(|h|h.base_damage==576.5&&(h.skill_modifier-3.5853).abs()<1e-12&&h.fixed_delay_ms==0));
        let mut plain=input.clone();plain["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|t|t["name"]!="넓은 공격");let base=builder::build(&plain.to_string()).unwrap();assert_eq!(skill.hits.iter().map(|h|h.damage_increase).collect::<Vec<_>>(),base.skills[0].hits.iter().map(|h|h.damage_increase).collect::<Vec<_>>());assert_eq!(serde_json::to_value(&profile.global_triggers).unwrap(),serde_json::to_value(&base.global_triggers).unwrap());
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&profile.skills).unwrap());
        for crit in [false,true] {let mut p=profile.clone();p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),if crit {100.} else {-100.});let e=engine::trace_one(&p,42,1500).1;let direct:Vec<_>=e.damage_events.iter().filter(|h|h.hit_id.starts_with("26070:")).collect();assert_eq!(direct.len(),2);assert!(direct.iter().all(|h|h.is_crit==crit));let states=&e.casts[0].target_debuff_details_after_hit;if i>0 {assert_eq!(states.iter().find(|b|b.id==["","reaper:corrosion","reaper:bleeding","reaper:nerve:darkness"][i]).unwrap().stacks,1);}else {assert!(states.is_empty());}}
        let mut miss=profile.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[0].hit_id.clone()];let e=engine::trace_one(&miss,42,1500).1;assert_eq!(e.casts[0].target_debuff_details_after_hit.len(),if i>0 {if i==1 {2} else {1}} else {0});miss.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[1].hit_id.clone()];assert!(engine::trace_one(&miss,42,1500).1.casts[0].target_debuff_details_after_hit.is_empty());miss.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&miss,42,10000).1.damage_events.is_empty());
        for name in ["빠른 준비","치명적인 단검"] {let mut bad=input.clone();bad["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":name,"level":1}));assert!(builder::build(&bad.to_string()).is_err());}
    }}
}

#[test]
fn reaper_phantom_finishing_composes_layers_poison_and_fixed_callback_delays() {
    for (layer_index,layer) in [None,Some("발목 절단"),Some("넓은 공격"),Some("날카로운 단검")].iter().enumerate() {for (i,poison) in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")].iter().enumerate() {
        let mut input=corrosion_input(26140,"팬텀 댄서",&[261400,261401,261402,261403]);input["skillData"][0]["cooldown"]=json!(300);input["skillData"][0]["castTimesMs"]=json!([3067]);for (index,hit) in input["skillData"][0]["hits"].as_array_mut().unwrap().iter_mut().enumerate() {hit["hitId"]=json!(format!("{}:0:{}",if index<2 {26140} else {26150},index%2));hit["actionDelayMs"]=json!([197,700,1864,2367][index]);hit["fixedDamage"]=json!(vec![339.;20]);hit["damageRatio"]=json!(vec![2.1155;20]);}input["character"]["skills"][0]["level"]=json!(12);input["aplConfig"]=json!("actions:\n - skill: 팬텀 댄서\n");input["assumedTargetNerveVulnerable"]=json!(true);input["assumedTargetSlowVulnerable"]=json!(true);
        let mut choices=vec![json!({"name":"마무리 동작","level":1,"values":[999999]})];if let Some(p)=poison {choices.push(json!({"name":p,"level":1}));}if let Some(l)=layer {choices.push(json!({"name":l,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];assert_eq!(skill.cast_times_ms,vec![3067]);assert_eq!(skill.hits.len(),6);assert_eq!(skill.hits.iter().map(|h|(h.action_delay_ms,h.fixed_delay_ms)).collect::<Vec<_>>(),vec![(197,0),(700,0),(700,500),(1864,0),(2367,0),(2367,500)]);
        let shadow=if layer_index==2 {261473+i.saturating_sub(1) as u32} else {261470+i.saturating_sub(1) as u32};assert_eq!(skill.hits[2].effect_id,shadow);assert_eq!(skill.hits[5].effect_id,shadow);assert!(skill.hits.iter().all(|h|h.base_damage==339.&&(h.skill_modifier-2.1155).abs()<1e-12));
        let mut plain=input.clone();plain["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|t|t["name"]!="마무리 동작");let base=builder::build(&plain.to_string()).unwrap();let direct:Vec<_>=skill.hits.iter().filter(|h|h.fixed_delay_ms==0).cloned().collect();assert_eq!(serde_json::to_value(direct).unwrap(),serde_json::to_value(&base.skills[0].hits).unwrap());
        for h in [ &skill.hits[2],&skill.hits[5] ] {assert!(((1.+h.damage_increase)/(1.+skill.hits[0].damage_increase)-1.6).abs()<1e-12);assert_eq!(h.crit_damage_bonus,if layer_index==3 {0.9} else {0.});assert!(h.triggers.iter().all(|t|!serde_json::to_string(t).unwrap().contains("reaper:bleeding")&&!serde_json::to_string(t).unwrap().contains("reaper:corrosion")&&!serde_json::to_string(t).unwrap().contains("reaper:nerve")));}
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&profile.skills).unwrap());
        for crit in [false,true] {let mut p=profile.clone();p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),if crit {100.} else {-100.});let events=engine::trace_one(&p,42,3300).1;let damage:Vec<_>=events.damage_events.iter().filter(|h|h.hit_id.starts_with("26140:")||h.hit_id.starts_with("26150:")).collect();assert_eq!(damage.len(),6);assert!(damage.iter().all(|h|h.is_crit==crit));assert!(((damage[2].time-damage[1].time)-0.5).abs()<1e-12);assert!(((damage[5].time-damage[4].time)-0.5).abs()<1e-12);
            let states=&events.casts[0].target_debuff_details_after_hit;if i>0 {assert_eq!(states.iter().find(|b|b.id==["","reaper:corrosion","reaper:bleeding","reaper:nerve:darkness"][i]).unwrap().stacks,1);}else {assert!(states.iter().all(|b|b.id=="reaper:slow:261440"));}
        }
        let mut miss=profile.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[4].hit_id.clone()];let e=engine::trace_one(&miss,42,3300).1;assert!(!e.casts[0].target_debuff_details_after_hit.iter().any(|b|["reaper:corrosion","reaper:bleeding","reaper:nerve:darkness"].contains(&b.id.as_str())));assert!(e.damage_events.iter().any(|h|h.hit_id==skill.hits[5].hit_id));
        miss.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[2].hit_id.clone(),skill.hits[5].hit_id.clone()];assert_eq!(engine::trace_one(&miss,42,3300).1.damage_events.iter().filter(|h|h.hit_id.starts_with("26140:")||h.hit_id.starts_with("26150:")).count(),4);miss.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&miss,42,10000).1.damage_events.is_empty());
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"폭풍의 춤","level":1}));assert!(builder::build(&input.to_string()).is_err());
    }}
}

#[test]
fn reaper_phantom_storm_corrected_callback_composes_layers_poison_and_fixed_delays() {
    for (layer_index,layer) in [None,Some("발목 절단"),Some("넓은 공격"),Some("날카로운 단검")].iter().enumerate() {for (i,poison) in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")].iter().enumerate() {
        let mut input=corrosion_input(26140,"팬텀 댄서",&[261400,261401,261402,261403]);input["skillData"][0]["cooldown"]=json!(300);input["skillData"][0]["castTimesMs"]=json!([3067]);for (index,hit) in input["skillData"][0]["hits"].as_array_mut().unwrap().iter_mut().enumerate() {hit["hitId"]=json!(format!("{}:0:{}",if index<2 {26140} else {26150},index%2));hit["actionDelayMs"]=json!([197,700,1864,2367][index]);hit["fixedDamage"]=json!(vec![339.;20]);hit["damageRatio"]=json!(vec![2.1155;20]);}input["character"]["skills"][0]["level"]=json!(12);input["aplConfig"]=json!("actions:\n - skill: 팬텀 댄서\n");input["assumedTargetNerveVulnerable"]=json!(true);input["assumedTargetSlowVulnerable"]=json!(true);
        let mut choices=vec![json!({"name":"폭풍의 춤","level":1,"values":[999999]})];if let Some(p)=poison {choices.push(json!({"name":p,"level":1}));}if let Some(l)=layer {choices.push(json!({"name":l,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];assert_eq!(skill.cast_times_ms,vec![3067]);assert_eq!(skill.hits.len(),14);
        let shadows:Vec<_>=skill.hits.iter().filter(|h|h.fixed_delay_ms>0).collect();assert_eq!(shadows.len(),10);assert!(shadows.iter().all(|h|h.effect_id!=261450));
        assert_eq!(shadows.iter().filter(|h|h.effect_id==261500).count(),if layer_index==2&&i==2 {2} else {0});
        assert!((shadows.iter().map(|h|h.skill_modifier).sum::<f64>()-5.0766).abs()<1e-9);
        let direct=skill.hits.iter().find(|h|h.fixed_delay_ms==0).unwrap();assert!(shadows.iter().all(|h|((1.+h.damage_increase)/(1.+direct.damage_increase)-1.58).abs()<1e-9));
        assert!(shadows.iter().all(|h|h.triggers.iter().all(|t|!serde_json::to_string(t).unwrap().contains("reaper:bleeding")&&!serde_json::to_string(t).unwrap().contains("reaper:corrosion")&&!serde_json::to_string(t).unwrap().contains("reaper:nerve"))));
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&profile.skills).unwrap());
        for fast in [false,true] {let mut p=profile.clone();p.skills[0].max_uses=1;if fast {p.stat.attack_speed.insert("test".into(),40.);p.stat.bake();}let e=engine::trace_one(&p,42,10000).1;
            assert_eq!(e.damage_events.iter().filter(|h|h.hit_id.contains(":shadow:")).count(),10);
            if i>0 {assert_eq!(e.casts[0].target_debuff_details_after_hit.iter().find(|b|b.id==["","reaper:corrosion","reaper:bleeding","reaper:nerve:darkness"][i]).unwrap().stacks,1);}
            p.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&p,42,10000).1.damage_events.is_empty());
        }
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"마무리 동작","level":1}));assert!(builder::build(&input.to_string()).is_err());
    }}
}

#[test]
fn reaper_iblis_breath_normal_swift_and_back_keep_poison_chance_scope() {
    for (i,poison) in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")].iter().enumerate() {for third in [None,Some("기습"),Some("속전속결")] {
        let swift=third==Some("속전속결");let back=third==Some("기습");let mut input=corrosion_input(26080,"이블리스토",&[260800,260801,260802,260803]);input["skillData"][0]["cooldown"]=json!(300);input["character"]["skills"][0]["level"]=json!(12);input["aplConfig"]=json!("actions:\n - skill: 이블리스토\n");input["assumedTargetNerveVulnerable"]=json!(true);
        let mut choices=vec![json!({"name":"심호흡","level":1,"values":[999999]})];if let Some(p)=poison {choices.push(json!({"name":p,"level":1}));}if let Some(t)=third {choices.push(json!({"name":t,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];let catalog:Vec<Value>=serde_json::from_str(include_str!("../skill-data/reaper/backend/tripod-catalog.json")).unwrap();assert!(profile.reaper_reference.as_ref().unwrap().metadata["damage_scope"].as_str().unwrap().contains(&format!("{}개 Lv1",catalog.iter().filter(|t|t["simulationSupported"]==true).count())));
        let ids=if swift {if i==0 {vec![260880,260881]} else {vec![260878+4*i as u32,260879+4*i as u32]}} else {if i==0 {vec![260800,260801,260802,260803]} else {(260856+4*i as u32..260860+4*i as u32).collect()}};
        assert_eq!(skill.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),ids);assert_eq!(skill.cast_times_ms,vec![if swift {1837} else {3753}]);assert_eq!(skill.hits.iter().map(|h|h.action_delay_ms).collect::<Vec<_>>(),if swift {vec![173,1354]} else {vec![173,1345,2090,3270]});assert!(skill.hits.iter().all(|h|h.base_damage==if swift {933.5} else {468.}&&(h.skill_modifier-if swift {5.8241} else {2.9116}).abs()<1e-12));
        let mut plain=input.clone();plain["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|t|t["name"]!="심호흡");let base=builder::build(&plain.to_string()).unwrap();assert_eq!(skill.hits.iter().map(|h|h.damage_increase).collect::<Vec<_>>(),base.skills[0].hits.iter().map(|h|h.damage_increase).collect::<Vec<_>>());
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&profile.skills).unwrap());
        for probability in [0.,1.] {for crit in [false,true] {
            let mut p=profile.clone();p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),if crit {100.} else {-100.});for h in &mut p.skills[0].hits {for t in &mut h.triggers {if t.chance<1. {t.chance=probability;}}}
            let events=engine::trace_one(&p,42,4000).1;let states=&events.casts[0].target_debuff_details_after_hit;
            if i==0 {assert!(states.is_empty());assert_eq!(events.damage_events.len(),ids.len());} else {let expected=if i==2 {if swift {2+4*probability as u32} else {1+2*u32::from(back)+4*probability as u32}} else {1+u32::from(swift)};assert_eq!(states.iter().find(|b|b.id==["","reaper:corrosion","reaper:bleeding","reaper:nerve:darkness"][i]).unwrap().stacks,expected);}
            if i==2 {for index in [0,skill.hits.len()-1] {
                let mut miss=p.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[index].hit_id.clone()];let e=engine::trace_one(&miss,42,4000).1;
                let total=if swift {2+4*probability as u32} else {1+2*u32::from(back)+4*probability as u32};let removed=if swift {1+2*probability as u32} else if index==0 {probability as u32} else {1+2*u32::from(back)+probability as u32};
                assert_eq!(e.casts[0].target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:bleeding").map(|b|b.stacks).unwrap_or(0),total-removed);
            }}
            p.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&p,42,4000).1.damage_events.is_empty());
        }}
        if i==2 {assert!(skill.hits.iter().all(|h|h.triggers.iter().filter(|t|t.chance==0.5).count()==if swift {2} else {1}));}
        if i==3 {input["assumedTargetNerveVulnerable"]=json!(false);assert!(engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,4000).1.casts[0].target_debuff_details_after_hit.is_empty());}
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"날카로운 단검","level":1}));assert!(builder::build(&input.to_string()).unwrap_err().contains("중복 선택"));
    }}
}

#[test]
fn reaper_iblis_breath_swift_chance_packets_burst_and_keep_overflow() {
    let mut input=corrosion_input(26080,"이블리스토",&[260800,260801,260802,260803]);input["skillData"][0]["cooldown"]=json!(3);input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 출혈","level":1},{"name":"심호흡","level":1},{"name":"속전속결","level":1}]);input["aplConfig"]=json!("actions:\n - skill: 이블리스토\n");
    let mut profile=builder::build(&input.to_string()).unwrap();for h in &mut profile.skills[0].hits {for t in &mut h.triggers {if t.chance<1. {t.chance=1.;}}}let events=engine::trace_one(&profile,42,4800).1;assert_eq!(events.casts.len(),2);assert_eq!(events.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:burst").count(),1);assert_eq!(events.casts[1].target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:bleeding").unwrap().stacks,2);
}

#[test]
fn reaper_spinning_rotation_composes_poison_condition_shadow_and_charge() {
    for (i,poison) in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")].iter().enumerate() {for third in [None,Some("그림자 잔상"),Some("끝없는 춤")] {
        let shadow=third==Some("그림자 잔상");let nerve=i==3;
        let mut input=corrosion_input(26050,"스피닝 대거",&[260500,260501]);input["skillData"][0]["cooldown"]=json!(300);input["character"]["skills"][0]["level"]=json!(12);input["aplConfig"]=json!("actions:\n - skill: 스피닝 대거\n");input["assumedTargetNerveVulnerable"]=json!(true);
        let mut choices=vec![json!({"name":"회전력 강화","level":1,"values":[999999]})];if let Some(p)=poison {choices.push(json!({"name":p,"level":1}));}if let Some(t)=third {choices.push(json!({"name":t,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];
        let mut ids=if i==0 {vec![260500,260501]} else {vec![260538+2*i as u32,260539+2*i as u32]};if shadow {ids.push(if i==0 {260570} else {260572+i as u32});}
        assert_eq!(skill.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),ids);assert_eq!(skill.cast_times_ms,vec![if nerve {1120} else {1400}]);
        let mut times=if nerve {vec![331,if shadow {646} else {645}]} else {vec![414,807]};if shadow {times.push(if nerve {887} else {1109});}assert_eq!(skill.hits.iter().map(|h|h.action_delay_ms).collect::<Vec<_>>(),times);
        assert_eq!(skill.hits[0].base_damage,408.);assert!((skill.hits[0].skill_modifier-2.5519).abs()<1e-12);assert_eq!(skill.hits[1].base_damage,524.5);assert!((skill.hits[1].skill_modifier-3.281).abs()<1e-12);
        let mut plain=input.clone();plain["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|t|t["name"]!="회전력 강화");let baseline=builder::build(&plain.to_string()).unwrap();
        for (h,b) in skill.hits.iter().zip(&baseline.skills[0].hits) {assert!(((1.+h.damage_increase)/(1.+b.damage_increase)-1.45).abs()<1e-12,"global once, range does not multiply damage");}if shadow {assert!(((1.+skill.hits[2].damage_increase)/(1.+skill.hits[0].damage_increase)-1.6).abs()<1e-12);assert_eq!(skill.hits[2].base_damage,878.);}
        if third==Some("끝없는 춤") {assert_eq!(skill.max_stacks,2);assert_eq!(skill.charge_recovery_ms,12000);}
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&profile.skills).unwrap());
        for probability in [0.,1.] {for critical in [false,true] {
            let mut p=profile.clone();p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),if critical {100.} else {-100.});
            for h in &mut p.skills[0].hits {for t in &mut h.triggers {if t.chance<1. {t.chance=probability;}}}
            let (_,events)=engine::trace_one(&p,42,1800);let states=&events.casts[0].target_debuff_details_after_hit;
            if i==0 {assert!(states.is_empty());assert_eq!(events.damage_events.len(),ids.len());} else {
                let expected=1+u32::from(shadow)+if i==2 {probability as u32} else {0};assert_eq!(states.iter().find(|b|b.id==["","reaper:corrosion","reaper:bleeding","reaper:nerve:darkness"][i]).unwrap().stacks,expected);
            }
            let mut miss=p.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[0].hit_id.clone()];let e=engine::trace_one(&miss,42,1800).1;if i==2 {assert_eq!(e.casts[0].target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:bleeding").unwrap().stacks,1+u32::from(shadow));}
            miss.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&miss,42,1800).1.damage_events.is_empty());
        }}
        if i==2 {assert_eq!(skill.hits.iter().map(|h|h.triggers.iter().filter(|t|t.chance==0.3).count()).collect::<Vec<_>>(),if shadow {vec![1,0,0]} else {vec![1,0]});}
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"기습","level":1}));assert!(builder::build(&input.to_string()).unwrap_err().contains("중복 선택"));
    }}
}

#[test]
fn reaper_spinning_rotation_chance_packet_consumes_cap_before_later_hits() {
    let mut input=corrosion_input(26050,"스피닝 대거",&[260500,260501]);input["skillData"][0]["cooldown"]=json!(2);input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 출혈","level":1},{"name":"회전력 강화","level":1},{"name":"그림자 잔상","level":1}]);input["aplConfig"]=json!("actions:\n - skill: 스피닝 대거\n");
    let mut profile=builder::build(&input.to_string()).unwrap();for h in &mut profile.skills[0].hits {for t in &mut h.triggers {if t.chance<1. {t.chance=1.;}}}
    let events=engine::trace_one(&profile,42,7200).1;assert_eq!(events.casts.len(),4);assert_eq!(events.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:burst").count(),1);assert_eq!(events.casts[3].target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:bleeding").unwrap().stacks,2);
}

#[test]
fn reaper_phantom_layers_keep_parent_reinput_four_hits_and_final_poison_once() {
    for (choice,ids) in [("넓은 공격",vec![vec![261450,261459,261451,261452],vec![261450,261459,261451,261452],vec![261453,261516,261454,261455],vec![261456,261517,261457,261458]]),("날카로운 단검",vec![vec![261410,261411,261412,261413],vec![261410,261411,261412,261413],vec![261420,261423,261421,261422],vec![261430,261433,261431,261432]])] {
        for (i,poison) in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")].iter().enumerate() {
            let mut input=corrosion_input(26140,"팬텀 댄서",&[261400,261401,261402,261403]);input["skillData"][0]["cooldown"]=json!(300);input["character"]["skills"][0]["level"]=json!(12);
            let mut choices=vec![json!({"name":choice,"level":1,"values":[999999]})];if let Some(name)=poison {choices.push(json!({"name":name,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);input["aplConfig"]=json!("actions:\n - skill: 팬텀 댄서\n");input["assumedTargetNerveVulnerable"]=json!(true);
            let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];assert_eq!(skill.cast_times_ms,vec![3067]);assert_eq!(skill.hits.iter().map(|h|h.action_delay_ms).collect::<Vec<_>>(),vec![197,700,1864,2367]);assert_eq!(skill.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),ids[i]);assert_eq!(skill.hits.iter().map(|h|h.hit_id.clone()).collect::<std::collections::HashSet<_>>().len(),4);
            assert_eq!(skill.tags.contains(&crate::profile::SkillTag::ElementElectricity),choice=="날카로운 단검");assert!(skill.hits.iter().all(|h|(h.crit_damage_bonus-if choice=="날카로운 단검" {0.9} else {0.}).abs()<1e-12));assert!(skill.hits.iter().all(|h|(h.skill_modifier-2.1155).abs()<1e-12&&h.base_damage==339.));
            let mut base_input=input.clone();base_input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|t|t["name"]!=choice);let base=builder::build(&base_input.to_string()).unwrap();assert_eq!(skill.hits.iter().map(|h|h.damage_increase).collect::<Vec<_>>(),base.skills[0].hits.iter().map(|h|h.damage_increase).collect::<Vec<_>>(),"range is not a damage modifier");
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&profile.skills).unwrap());
            for critical in [false,true] {let mut p=profile.clone();p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),if critical {100.} else {-100.});let events=engine::trace_one(&p,42,3400).1;let direct:Vec<_>=events.damage_events.iter().filter(|h|h.hit_id.starts_with("26140:")||h.hit_id.starts_with("26150:")).collect();assert_eq!(direct.len(),4);let states=&events.casts[0].target_debuff_details_after_hit;if poison.is_none(){assert!(states.is_empty());assert_eq!(events.damage_events.len(),4);} else {assert_eq!(states.iter().find(|b|b.id==["","reaper:corrosion","reaper:bleeding","reaper:nerve:darkness"][i]).unwrap().stacks,1,"final selected poison applies once");}
                assert!(direct.iter().all(|h|h.is_crit==critical));
            }
            let mut miss=profile.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[3].hit_id.clone()];assert!(engine::trace_one(&miss,42,10000).1.casts[0].target_debuff_details_after_hit.is_empty());miss.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&miss,42,10000).1.damage_events.is_empty());
            if i==3 {input["assumedTargetNerveVulnerable"]=json!(false);assert!(engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,3400).1.casts[0].target_debuff_details_after_hit.is_empty());}
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":if choice=="넓은 공격" {"날카로운 단검"} else {"넓은 공격"},"level":1}));assert!(builder::build(&input.to_string()).unwrap_err().contains("중복 선택"));
        }
    }
}

#[test]
fn reaper_death_call_four_hits_critical_followup_and_selected_final_poison() {
    for (poison,ids,buff) in [(None,vec![261200,261200,261270,261271],""),(Some("독 : 부식"),vec![261210,261210,261270,261271],"reaper:corrosion"),(Some("독 : 출혈"),vec![261220,261220,261272,261273],"reaper:bleeding"),(Some("독 : 신경"),vec![261230,261230,261274,261275],"reaper:nerve:darkness")] {for bonus in [None,Some("기습"),Some("치명적인 단검")] {
        let mut input=corrosion_input(26120,"데스 사이드",&[261200,261200,261201]);input["skillData"][0]["cooldown"]=json!(300);input["character"]["skills"][0]["level"]=json!(12);
        let mut choices=vec![json!({"name":"데스 콜","level":1,"values":[999999]})];if let Some(p)=poison {choices.push(json!({"name":p,"level":1}));}if let Some(b)=bonus {choices.push(json!({"name":b,"level":1}));}
        input["character"]["skills"][0]["tripods"]=json!(choices);input["aplConfig"]=json!("actions:\n - skill: 데스 사이드\n");input["assumedTargetNerveVulnerable"]=json!(true);
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];assert_eq!(skill.cast_times_ms,vec![2000]);assert_eq!(skill.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),ids);
        assert_eq!(skill.hits.iter().map(|h|(h.action_delay_ms,h.fixed_delay_ms)).collect::<Vec<_>>(),vec![(200,0),(200,300),(1100,0),(1152,0)]);
        assert_eq!(skill.hits.iter().map(|h|h.crit_damage_bonus).collect::<Vec<_>>(),vec![0.,0.,1.,1.]);assert!((skill.hits[2].skill_modifier-2.3456).abs()<1e-12);assert!((skill.hits[3].skill_modifier-5.4752).abs()<1e-12);
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&profile.skills).unwrap());
        for critical in [false,true] {let mut p=profile.clone();p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),if critical {100.} else {-100.});let events=engine::trace_one(&p,42,2100).1;assert_eq!(events.casts.len(),1);
            // Fixture has10% attack speed; the repeated hit's300ms stays fixed.
            assert_eq!(events.damage_events.iter().filter(|h|h.hit_id.starts_with("26120:")).map(|h|(h.time*1000.).round() as u32).collect::<Vec<_>>(),vec![181,481,999,1047]);
            let states=&events.casts[0].target_debuff_details_after_hit;if poison.is_none() {assert!(states.is_empty());assert_eq!(events.damage_events.len(),4);} else {let expected=if poison==Some("독 : 출혈") {3+if bonus==Some("기습") {2} else if bonus==Some("치명적인 단검")&&critical {1} else {0}} else {1};assert_eq!(states.iter().find(|s|s.id==buff).unwrap().stacks,expected);}
        }
        let mut miss=profile.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[3].hit_id.clone()];assert!(engine::trace_one(&miss,42,10000).1.casts[0].target_debuff_details_after_hit.is_empty());
        miss.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&miss,42,10000).1.damage_events.is_empty());
        if poison==Some("독 : 신경") {input["assumedTargetNerveVulnerable"]=json!(false);assert!(engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,2100).1.casts[0].target_debuff_details_after_hit.is_empty());}
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"변칙 공격","level":1}));assert!(builder::build(&input.to_string()).unwrap_err().contains("중복 선택"));
    }}
}

#[test]
fn reaper_death_call_bleed_consumes_max_between_two_extra_packets() {
    let mut input=corrosion_input(26120,"데스 사이드",&[261200,261200,261201]);input["skillData"][0]["cooldown"]=json!(3);input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 출혈","level":1},{"name":"데스 콜","level":1}]);input["aplConfig"]=json!("actions:\n - skill: 데스 사이드\n");
    let (_,events)=trace(&input,11800);assert_eq!(events.casts.len(),4);assert_eq!(events.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:burst").count(),1);assert_eq!(events.casts.last().unwrap().target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:bleeding").unwrap().stacks,2);
}

#[test]
fn reaper_death_turn_adds_one_landed_bleed_packet_and_orders_other_chains() {
    for poison in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")] {for bonus in [None,Some("기습"),Some("치명적인 단검")] {
        let mut input=corrosion_input(26120,"데스 사이드",&[261200,261200,261201]);input["skillData"][0]["cooldown"]=json!(300);
        let mut choices=vec![json!({"name":"변칙 공격","level":1,"values":[999999]})];if let Some(p)=poison {choices.push(json!({"name":p,"level":1}));}if let Some(b)=bonus {choices.push(json!({"name":b,"level":1}));}
        input["character"]["skills"][0]["tripods"]=json!(choices);input["aplConfig"]=json!("actions:\n - skill: 데스 사이드\n");input["assumedTargetNerveVulnerable"]=json!(true);
        let on=builder::build(&input.to_string()).unwrap();input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&on.skills).unwrap());
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().retain(|t|t["name"]!="변칙 공격");let off=builder::build(&input.to_string()).unwrap();
        let strip=|p:&crate::profile::CharacterProfiles|{let mut skills=p.skills.clone();for h in &mut skills[0].hits {h.triggers.clear();}serde_json::to_value(skills).unwrap()};assert_eq!(strip(&on),strip(&off));
        for crit in [false,true] {let mut on=on.clone();let mut off=off.clone();for p in [&mut on,&mut off] {p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),if crit {100.} else {-100.});}
            let a=engine::trace_one(&on,42,2500).1;let b=engine::trace_one(&off,42,2500).1;
            if poison==Some("독 : 출혈") {let stacks=|e:&crate::constants::TraceData|e.casts[0].target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:bleeding").unwrap().stacks;assert_eq!(stacks(&a),stacks(&b)+1);if bonus.is_none(){assert_eq!(stacks(&a),2);}}
            else {assert_eq!(serde_json::to_value(a.damage_events).unwrap(),serde_json::to_value(b.damage_events).unwrap());}
        }
        let mut miss=on.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![on.skills[0].hits.last().unwrap().hit_id.clone()];assert!(engine::trace_one(&miss,42,10000).1.damage_events.iter().all(|h|!h.hit_id.starts_with("reaper:bleeding:")));
    }}
    let mut input=corrosion_input(26120,"데스 사이드",&[261200,261200,261201]);input["skillData"][0]["cooldown"]=json!(3);input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 출혈","level":1},{"name":"변칙 공격","level":1}]);input["aplConfig"]=json!("actions:\n - skill: 데스 사이드\n");
    let (_,events)=trace(&input,14900);assert_eq!(events.casts.len(),5);assert_eq!(events.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:burst").count(),1);assert!(!events.casts.last().unwrap().target_debuff_details_after_hit.iter().any(|b|b.id=="reaper:bleeding"));
}

#[test]
fn reaper_poison_layers_compose_selected_chains_bonuses_and_misses() {
    for (sid,name,choice,base,times,cast,ids_by_poison,bonuses) in [
        (26050,"스피닝 대거","그림자 잔상",vec![260500,260501],vec![414,807,1109],1400,vec![vec![260500,260501,260570],vec![260510,260511,260570],vec![260520,260521,260571],vec![260530,260531,260572]],vec![None,Some("기습"),Some("치명적인 단검")]),
        (26060,"스피릿 캐치","그림자 잔상",vec![260600,260612],vec![649,992],1567,vec![vec![260663,260664],vec![260663,260664],vec![260668,260669],vec![260673,260674]],vec![None,Some("날카로운 단검"),Some("기습")]),
        (26110,"샤벨 스팅거","속전속결",vec![261100,261101],vec![200,500],1467,vec![vec![261150,261161],vec![261150,261161],vec![261151,261162],vec![261152,261163]],vec![None,Some("두개의 그림자"),Some("날카로운 단검")]),
    ] {for (poison_index,poison) in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")].iter().enumerate() {for bonus in &bonuses {
        let mut input=corrosion_input(sid,name,&base);input["skillData"][0]["cooldown"]=json!(300);input["character"]["skills"][0]["level"]=json!(12);
        let mut choices=vec![json!({"name":choice,"level":1,"values":[999999]})];if let Some(p)=poison {choices.push(json!({"name":p,"level":1}));}if let Some(b)=bonus {choices.push(json!({"name":b,"level":1}));}
        input["character"]["skills"][0]["tripods"]=json!(choices);input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n"));input["assumedTargetNerveVulnerable"]=json!(true);
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];
        assert_eq!(skill.cast_times_ms,vec![cast]);assert_eq!(skill.hits.iter().map(|h|h.action_delay_ms).collect::<Vec<_>>(),times);assert_eq!(skill.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),ids_by_poison[poison_index]);
        if sid!=26110 {assert!(((1.+skill.hits.last().unwrap().damage_increase)/(1.+skill.hits[0].damage_increase)-1.6).abs()<1e-12,"{sid}/{poison:?}/{bonus:?}: {:?}",skill.hits.iter().map(|h|h.damage_increase).collect::<Vec<_>>());} else {assert!(skill.hits.iter().all(|h|h.damage_increase>=0.45));}
        let events=engine::trace_one(&profile,42,1800).1;assert_eq!(events.casts.len(),1);
        let states=&events.casts[0].target_debuff_details_after_hit;
        if poison.is_none() {assert!(states.is_empty());assert_eq!(events.damage_events.len(),skill.hits.len());}
        else {let buff=["","reaper:corrosion","reaper:bleeding","reaper:nerve:darkness"][poison_index];let stacks=states.iter().find(|b|b.id==buff).unwrap().stacks;if bonus.is_none()||poison_index!=2 {assert_eq!(stacks,2);} else {assert!((2..=5).contains(&stacks),"{sid}/{poison:?}/{bonus:?}: {stacks}");}}
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&profile.skills).unwrap());
        let mut miss=profile.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&miss,42,10000).1.damage_events.is_empty());
        if sid==26110&&poison_index==2&&bonus.is_some() {for hit in &skill.hits {assert_eq!(hit.triggers.iter().filter(|t|t.chance>0.&&t.chance<1.).count(),1,"one independent chance roll per swift hit");}}
    }}}
}

#[test]
fn reaper_iblis_swift_two_hits_apply_only_selected_poison_and_preserve_order() {
    for (poison,ids,buff) in [(None,[260880,260881],""),(Some("독 : 부식"),[260880,260881],"reaper:corrosion"),(Some("독 : 출혈"),[260884,260885],"reaper:bleeding"),(Some("독 : 신경"),[260888,260889],"reaper:nerve:darkness")] {
        for bonus in [None,Some("날카로운 단검")] {
            let mut input=corrosion_input(26080,"이블리스토",&[260800,260801,260802,260803]);
            input["skillData"][0]["cooldown"]=json!(300);
            input["character"]["skills"][0]["level"]=json!(12);
            let mut choices=vec![json!({"name":"속전속결","level":1,"values":[999999]})];
            if let Some(name)=poison {choices.push(json!({"name":name,"level":1}));}
            if let Some(name)=bonus {choices.push(json!({"name":name,"level":1}));}
            input["character"]["skills"][0]["tripods"]=json!(choices);input["aplConfig"]=json!("actions:\n - skill: 이블리스토\n");input["assumedTargetNerveVulnerable"]=json!(true);
            let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];
            assert_eq!(skill.cast_times_ms,vec![2333]);assert_eq!(skill.hits.iter().map(|h|h.action_delay_ms).collect::<Vec<_>>(),vec![220,1720]);assert_eq!(skill.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),ids);
            assert!(skill.hits.iter().all(|h|h.damage_increase>=0.6));
            let events=engine::trace_one(&profile,42,2500).1;assert_eq!(events.casts.len(),1);
            let states=&events.casts[0].target_debuff_details_after_hit;
            if poison.is_some() {assert_eq!(states.iter().find(|b|b.id==buff).unwrap().stacks,2);} else {assert!(states.is_empty());assert_eq!(events.damage_events.len(),2);}
            let direct:Vec<_>=events.damage_events.iter().filter(|h|h.hit_id.starts_with("26080:")).collect();assert_eq!(direct.len(),2);
            assert!(direct.iter().all(|h|(h.formula.skill_modifier-5.8241).abs()<1e-12));
            for (index,event) in direct.iter().enumerate() {assert!((event.formula.skill_damage_multiplier-(1.+skill.hits[index].damage_increase)).abs()<1e-12);}
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&profile.skills).unwrap());
            for index in [0,1] {let mut miss=profile.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[index].hit_id.clone()];let trace=engine::trace_one(&miss,42,2500).1;
                if poison.is_some() {assert_eq!(trace.casts[0].target_debuff_details_after_hit.iter().find(|b|b.id==buff).unwrap().stacks,1);} else {assert!(trace.casts[0].target_debuff_details_after_hit.is_empty());}
            }
            let mut miss=profile.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&miss,42,10000).1.damage_events.is_empty());
            if poison==Some("독 : 신경") {input["assumedTargetNerveVulnerable"]=json!(false);assert!(engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,2500).1.casts[0].target_debuff_details_after_hit.is_empty());}
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"기습","level":1}));assert!(builder::build(&input.to_string()).unwrap_err().contains("중복 선택"));
        }
    }
}

#[test]
fn reaper_iblis_swift_checks_shared_poison_cap_between_its_two_hits() {
    for (name,buff) in [("독 : 출혈","reaper:bleeding"),("독 : 신경","reaper:nerve:darkness")] {
        let mut input=corrosion_input(26080,"이블리스토",&[260800,260801,260802,260803]);
        input["character"]["skills"][0]["tripods"]=json!([{"name":"속전속결","level":1},{"name":name,"level":1}]);input["skillData"][0]["cooldown"]=json!(3);input["assumedTargetNerveVulnerable"]=json!(true);input["aplConfig"]=json!("actions:\n - skill: 이블리스토\n");
        let (_,events)=trace(&input,14900);
        assert_eq!(events.casts.len(),5);
        if name=="독 : 출혈" {
            assert_eq!(events.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:burst").count(),1);
            assert!(!events.casts[4].target_debuff_details_after_hit.iter().any(|b|b.id==buff));
        } else {
            assert_eq!(events.casts[1].target_debuff_details_after_hit.iter().find(|b|b.id==buff).unwrap().stacks,1);
            assert!(events.casts[1].target_debuff_details_after_hit.iter().any(|b|b.id=="reaper:nerve:fear"));
        }
    }
}

#[test]
fn reaper_silent_shadow_target_stacks_only_affect_later_silent_hits_and_compose_ranges() {
    for range in [None,Some("넓은 공격"),Some("가벼운 동작")] {for ground in [false,true] {
        let mut input=corrosion_input(26820,"사일런트 스매셔",&[268200]);
        let mut choices=vec![json!({"name":"그림자 잔상","level":1,"values":[999999]})];
        if let Some(name)=range {choices.push(json!({"name":name,"level":1}));}
        if ground {choices.push(json!({"name":"지면 강타","level":1}));}
        input["character"]["skills"][0]["tripods"]=json!(choices);
        input["aplConfig"]=json!("actions:\n - skill: 사일런트 스매셔\n");
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{"26820":{"dark":0.01,"heat":0.01}}});
        let on=builder::build(&input.to_string()).unwrap();let skill=&on.skills[0];
        assert_eq!(skill.cast_times_ms,vec![2500]);assert_eq!(skill.hits.iter().map(|h|h.action_delay_ms).collect::<Vec<_>>(),vec![300,700,1500]);
        let (clone,main)=match range {Some("넓은 공격")=>(268261,268210),Some("가벼운 동작")=>(268262,268220),_=>(268260,268200)};
        assert_eq!(skill.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),vec![clone,clone,main]);
        let trace=engine::trace_one(&on,42,2600).1;
        assert_eq!(trace.damage_events.len(),3);
        let static_bonus=skill.hits[0].damage_increase;
        for (i,event) in trace.damage_events.iter().enumerate() {assert!((event.formula.skill_damage_multiplier-(1.+skill.hits[i].damage_increase+[0.,0.22,0.44][i])).abs()<1e-12,"range={range:?},ground={ground},hit={i},actual={},static={}",event.formula.skill_damage_multiplier,skill.hits[i].damage_increase);}
        assert_eq!(trace.casts[0].target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:silent:shadow").unwrap().stacks,2);
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&on.skills).unwrap());
        let mut first_miss=on.clone();first_miss.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![skill.hits[0].hit_id.clone()];
        let missed=engine::trace_one(&first_miss,42,2600).1;assert_eq!(missed.damage_events.len(),2);
        for (i,event) in missed.damage_events.iter().enumerate() {assert!((event.formula.skill_damage_multiplier-(1.+static_bonus+[0.,0.22][i])).abs()<1e-12);}
        let mut clones_miss=on.clone();clones_miss.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits[..2].iter().map(|h|h.hit_id.clone()).collect();
        let missed=engine::trace_one(&clones_miss,42,2600).1;assert_eq!(missed.damage_events.len(),1);assert!((missed.damage_events[0].formula.skill_damage_multiplier-(1.+static_bonus)).abs()<1e-12);
        assert!(missed.casts[0].target_debuff_details_after_hit.is_empty());
        let mut expired=on.clone();expired.skills[0].hits[2].action_delay_ms=3700;expired.skills[0].cast_times_ms[0]=4000;
        let late=engine::trace_one(&expired,42,4100).1;assert!((late.damage_events[2].formula.skill_damage_multiplier-(1.+static_bonus)).abs()<1e-12);
        let mut repeated=on.clone();repeated.skills[0].cooldown_ms=2500;
        let repeated_trace=engine::trace_one(&repeated,42,6000).1;
        assert!(repeated_trace.casts.iter().all(|c|c.target_debuff_details_after_hit.iter().any(|b|b.id=="reaper:silent:shadow"&&b.stacks==2)));
        assert!((repeated_trace.damage_events[3].formula.skill_damage_multiplier-(1.+static_bonus+0.44)).abs()<1e-12);
        let mut probe=on.clone();let mut other=skill.clone();other.skill_id=999;other.name="대상 상태 비상속 검사".into();other.hits=vec![skill.hits[2].clone()];other.hits[0].triggers.clear();other.hits[0].action_delay_ms=100;other.hits[0].hit_id="probe".into();probe.skills.push(other);
        probe.apl_actions.insert(0,crate::profile::AplAction {skill_id:999,conditions:vec![crate::profile::AplCondition::TargetDebuffStacks {debuff_id:"reaper:silent:shadow".into(),operator:crate::profile::CompareOperator::Gte,value:1.}]});
        let probe_trace=engine::trace_one(&probe,42,6000).1;
        assert!(probe_trace.damage_events.iter().any(|e|e.skill_id==999));
        assert!(probe_trace.damage_events.iter().filter(|e|e.skill_id==999).all(|e|(e.formula.skill_damage_multiplier-(1.+static_bonus)).abs()<1e-12));
    }}
}

#[test]
fn reaper_silent_light_swaps_both_layers_without_changing_damage_gauge_or_timing() {
    for other in [None,Some("빠른 준비"),Some("추진력")] {for ground in [false,true] {
        let mut input=corrosion_input(26820,"사일런트 스매셔",&[268200]);
        input["aplConfig"]=json!("actions:\n - skill: 사일런트 스매셔\n");
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{"26820":{"dark":0.01,"heat":0.01}}});
        let mut choices:Vec<Value>=other.map(|name|vec![json!({"name":name,"level":1})]).unwrap_or_default();
        if ground {choices.push(json!({"name":"지면 강타","level":1}));}
        input["character"]["skills"][0]["tripods"]=json!(choices);
        let off=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"가벼운 동작","level":1,"values":[999999]}));
        let on=builder::build(&input.to_string()).unwrap();
        assert_eq!(on.skills[0].hits.len(),1);assert_eq!(on.skills[0].hits[0].effect_id,268220);
        let mut expected=serde_json::to_value(&off.skills).unwrap();expected[0]["hits"][0]["effect_id"]=json!(268220);
        assert_eq!(serde_json::to_value(&on.skills).unwrap(),expected);
        assert_eq!(on.reaper_reference.as_ref().unwrap().metadata["silent_light_policy"]["attackDistanceBonusM"],3);
        let a=engine::trace_one(&off,42,30000).1;let b=engine::trace_one(&on,42,30000).1;
        assert_eq!(serde_json::to_value(a).unwrap(),serde_json::to_value(b).unwrap());
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        assert_eq!(serde_json::to_value(builder::build(&input.to_string()).unwrap().skills).unwrap(),serde_json::to_value(&on.skills).unwrap());
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"넓은 공격","level":1}));
        assert!(builder::build(&input.to_string()).unwrap_err().contains("중복 선택"));
        let mut miss=on.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=miss.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();
        assert!(engine::trace_one(&miss,42,10000).1.damage_events.is_empty());
    }}
}

#[test]
fn reaper_call_wide_three_conditional_paths_preserve_callbacks_damage_gauge_and_order() {
    let catalog:Value=serde_json::from_str(include_str!("../skill-data/reaper/backend/tripod-catalog.json")).unwrap();
    let wide=catalog.as_array().unwrap().iter().find(|t|t["gameSkillId"]==26550&&t["choiceId"]==8).unwrap();
    let base=&wide["runtimeEffects"][0]["variants"][0]["route"];
    for (other,wrapper,count) in [(None,265467,5),(Some("지속되는 부름"),265465,7),(Some("치명적인 그림자"),265466,5)] {for charge in [false,true] {for movement in [false,true] {
        let mut input=corrosion_input(26550,"콜 오브 나이프",&[265420]);
        input["skillData"][0]["hits"]=base["hits"].clone();input["skillData"][0]["castTimesMs"]=base["castTimesMs"].clone();input["skillData"][0]["cooldown"]=json!(20);
        let mut choices:Vec<Value>=other.map(|name|vec![json!({"name":name,"level":1})]).unwrap_or_default();
        if charge {choices.push(json!({"name":"절망의 부름","level":1}));}
        if movement {choices.push(json!({"name":"그림자 강탈","level":1}));}
        input["character"]["skills"][0]["tripods"]=json!(choices);
        input["aplConfig"]=json!("actions:\n - skill: 콜 오브 나이프\n");
        input["assumedTargetSlowVulnerable"]=json!(true);input["assumedTargetNerveVulnerable"]=json!(true);
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{"26550":{"dark":0.01,"heat":0.01}}});
        let off=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"공포의 부름","level":1,"values":[999999]}));
        if charge {assert!(builder::build(&input.to_string()).unwrap_err().contains("중복 선택"));continue;}
        let on=builder::build(&input.to_string()).unwrap();
        assert_eq!(on.skills[0].hits.len(),count);
        assert_eq!(serde_json::to_value(&off.skills).unwrap(),serde_json::to_value(&on.skills).unwrap());
        let metadata=&on.reaper_reference.as_ref().unwrap().metadata["call_wide_policy"];
        assert_eq!(metadata["wideWrapperId"],wrapper);assert_eq!(metadata["rangeMultiplier"],1.5);
        let normalize_trace=|mut trace:crate::constants::TraceData| {
            for cast in &mut trace.casts {
                cast.active_buffs.sort();cast.target_debuffs.sort();
                for details in [&mut cast.active_buff_details,&mut cast.target_debuff_details,&mut cast.active_buff_details_after_hit,&mut cast.target_debuff_details_after_hit] {details.sort_by(|a,b|a.id.cmp(&b.id));}
            }
            serde_json::to_value(trace).unwrap()
        };
        let a=normalize_trace(engine::trace_one(&off,42,30000).1);
        let b=normalize_trace(engine::trace_one(&on,42,30000).1);
        for key in ["damageEvents","classEvents","casts"] {assert!(!a[key].is_null(),"missing {key}");assert_eq!(a[key],b[key],"{other:?}/{charge}/{movement}: {key}");}
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        let reverse=builder::build(&input.to_string()).unwrap();assert_eq!(serde_json::to_value(&on.skills).unwrap(),serde_json::to_value(&reverse.skills).unwrap());
        let mut miss=on.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=miss.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();
        let missed=engine::trace_one(&miss,42,10000).1;
        assert!(missed.damage_events.is_empty());assert!(missed.casts.iter().all(|c|c.target_debuff_details_after_hit.is_empty()));
    }}}
}

#[test]
fn reaper_call_control_routes_gate_independent_target_conditions_and_preserve_damage() {
    for choice in [None,Some("지속되는 부름"),Some("치명적인 그림자")] {
        let mut input=corrosion_input(26550,"콜 오브 나이프",&[265420,265421,265421,265421,265422]);
        input["character"]["skills"][0]["tripods"]=json!(choice.map(|name|vec![json!({"name":name,"level":1})]).unwrap_or_default());
        input["skillData"][0]["cooldown"]=json!(20);
        input["aplConfig"]=json!("actions:\n - skill: 콜 오브 나이프\n");
        let off=builder::build(&input.to_string()).unwrap();
        let off_damage=serde_json::to_value(engine::trace_one(&off,42,6000).1.damage_events).unwrap();
        for (slow,fear) in [(false,false),(true,false),(false,true),(true,true)] {
            input["assumedTargetSlowVulnerable"]=json!(slow);input["assumedTargetNerveVulnerable"]=json!(fear);
            let mut on=builder::build(&input.to_string()).unwrap();
            let trace=engine::trace_one(&on,42,6000).1;
            assert_eq!(serde_json::to_value(&trace.damage_events).unwrap(),off_damage);
            let final_states=&trace.casts[0].target_debuff_details_after_hit;
            assert_eq!(final_states.iter().any(|b|b.id=="reaper:call:fear"),fear);
            let count=if choice==Some("지속되는 부름") && (slow||fear) {2} else {0};
            assert_eq!(final_states.iter().find(|b|b.id=="reaper:call:mark").map_or(0,|b|b.stacks),count);
            for hit in &on.skills[0].hits {
                let mark=hit.triggers.iter().find_map(|t|match &t.effect {crate::profile::skill::TriggerEffect::ApplyTargetBuff(b) if b.id=="reaper:call:mark"=>Some(b),_=>None});
                assert_eq!(mark.is_some(),slow||fear);
                if let Some(mark)=mark {assert_eq!(mark.effects.get(&StatField::TargetMovementSpeedReduction).copied(),slow.then_some(0.05));assert_eq!((mark.duration_ms,mark.max_stacks),(2000,5));}
            }
            on.reaper_reference.as_mut().unwrap().missed_hit_ids=on.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();
            assert!(engine::trace_one(&on,42,6000).1.casts.iter().all(|c|c.target_debuff_details_after_hit.iter().all(|b|!b.id.starts_with("reaper:call:"))));
            if slow||fear {
                on.reaper_reference.as_mut().unwrap().missed_hit_ids=on.skills[0].hits.iter().skip(4).map(|h|h.hit_id.clone()).collect();
                let four=engine::trace_one(&on,42,6000).1;
                assert_eq!(four.casts[0].target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:call:mark").unwrap().stacks,4);
                assert!(!four.casts[0].target_debuff_details_after_hit.iter().any(|b|b.id=="reaper:call:fear"));
            }
        }
    }
}

#[test]
fn reaper_call_control_natural_expiry_never_causes_fear_and_refresh_reaches_five() {
    let mut input=corrosion_input(26550,"콜 오브 나이프",&[265420]);
    input["character"]["skills"][0]["tripods"]=json!([]);
    input["assumedTargetNerveVulnerable"]=json!(true);
    input["aplConfig"]=json!("actions:\n - skill: 콜 오브 나이프\n");
    input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{}});
    for cooldown in [499,2000,2500] {
        let mut p=builder::build(&input.to_string()).unwrap();p.skills[0].cooldown_ms=cooldown;
        let trace=engine::trace_one(&p,42,7000).1;
        if cooldown==499 {
            assert!(trace.casts[4].target_debuff_details_after_hit.iter().any(|b|b.id=="reaper:call:fear"));
            assert!(!trace.casts[4].target_debuff_details_after_hit.iter().any(|b|b.id=="reaper:call:mark"));
        } else {
            assert!(trace.casts.iter().all(|c|c.target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:call:mark").unwrap().stacks==1));
            assert!(trace.casts.iter().all(|c|c.target_debuff_details_after_hit.iter().all(|b|b.id!="reaper:call:fear")));
        }
    }
}

#[test]
fn reaper_call_slow_counts_stacks_and_compares_with_ankle_cut_separate_timers() {
    use crate::profile::{BuffSpec,StackType};
    let mut buffs=crate::engine::state::BuffManager::new();
    let make=|id:&str,value,max_stacks|BuffSpec {id:id.into(),effects:hashbrown::HashMap::from([(StatField::TargetMovementSpeedReduction,value)]),max_stacks,stack_type:StackType::Refresh,duration_ms:2000};
    buffs.apply(make("call",0.05,5),4000,4);buffs.apply(make("ankle",0.462,1),3000,1);
    assert_eq!(buffs.target_movement_speed_reduction(2999),Some(0.462));
    assert_eq!(buffs.target_movement_speed_reduction(3000),Some(0.2));
    buffs.apply(make("call",0.05,5),5000,1);buffs.expire_if("call",4000);
    assert_eq!(buffs.target_movement_speed_reduction(4000),Some(0.25));
    assert_eq!(buffs.target_movement_speed_reduction(5000),None);
}

#[test]
fn reaper_slow_three_choices_gate_target_hits_and_leave_actor_damage_unchanged() {
    for (id,name,ids,value,duration) in [
        (26060,"스피릿 캐치",vec![260600,260612],0.462,6000),
        (26140,"팬텀 댄서",vec![261400,261401,261402,261403],0.35,3000),
        (26520,"블링크",vec![265400;6],0.294,3000),
    ] {
        let mut input=corrosion_input(id,name,&ids);
        input["character"]["skills"][0]["tripods"]=json!([]);
        input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n"));
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{}});
        let baseline=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"]=json!([{"name":"발목 절단","level":1,"values":[999999]}]);
        let off=builder::build(&input.to_string()).unwrap();
        assert_eq!(serde_json::to_value(&baseline.skills).unwrap(),serde_json::to_value(&off.skills).unwrap());
        input["assumedTargetSlowVulnerable"]=json!(true);
        let mut on=builder::build(&input.to_string()).unwrap();
        let skill=&on.skills[0];
        for hit in &skill.hits {
            let buff=hit.triggers.iter().find_map(|t|match &t.effect {crate::profile::skill::TriggerEffect::ApplyTargetBuff(b) if b.id.starts_with("reaper:slow:")=>Some(b),_=>None}).unwrap();
            assert_eq!((buff.effects[&StatField::TargetMovementSpeedReduction],buff.duration_ms,buff.max_stacks),(value,duration,1));
            assert!(!buff.effects.contains_key(&StatField::MovementSpeed));
        }
        let a=engine::trace_one(&off,42,30000).1;let b=engine::trace_one(&on,42,30000).1;
        assert_eq!(serde_json::to_value(a.damage_events).unwrap(),serde_json::to_value(b.damage_events).unwrap());
        assert!(b.casts.iter().any(|c|c.target_debuff_details_after_hit.iter().any(|b|b.id.starts_with("reaper:slow:"))));
        on.reaper_reference.as_mut().unwrap().missed_hit_ids=on.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();
        assert!(engine::trace_one(&on,42,30000).1.casts.iter().all(|c|c.target_debuff_details_after_hit.iter().all(|b|!b.id.starts_with("reaper:slow:"))));
    }
}

#[test]
fn reaper_slow_strongest_resumes_weaker_and_ignores_stale_refresh_expiry() {
    use crate::profile::{BuffSpec,StackType};
    let mut buffs=crate::engine::state::BuffManager::new();
    let specs=[("weak",0.294),("strong",0.462)].map(|(id,value)|BuffSpec {id:id.into(),effects:hashbrown::HashMap::from([(StatField::TargetMovementSpeedReduction,value)]),max_stacks:1,stack_type:StackType::Refresh,duration_ms:3000});
    buffs.apply(specs[0].clone(),6000,1);buffs.apply(specs[1].clone(),3000,1);
    assert_eq!(buffs.target_movement_speed_reduction(2999),Some(0.462));
    assert_eq!(buffs.target_movement_speed_reduction(3000),Some(0.294));
    buffs.apply(specs[0].clone(),7000,1);buffs.expire_if("weak",6000);
    assert_eq!(buffs.target_movement_speed_reduction(6000),Some(0.294));
    assert_eq!(buffs.stacks("weak",6000),1);
    assert_eq!(buffs.target_movement_speed_reduction(7000),None);
}

#[test]
fn reaper_nerve_seven_routes_preserve_damage_and_gate_last_hit_shared_control() {
    for (id,name,ids,last) in [
        (26050,"스피닝 대거",vec![260500,260501],260531),
        (26060,"스피릿 캐치",vec![260600,260612],260633),
        (26070,"쉐도우 닷",vec![260701,260702],260732),
        (26080,"이블리스토",vec![260800,260801,260802,260803],260833),
        (26110,"샤벨 스팅거",vec![261100,261100,261101,261101],261132),
        (26120,"데스 사이드",vec![261200,261200,261201],261231),
        (26140,"팬텀 댄서",vec![261400,261401,261402,261403],261432),
    ] {
        let mut input=corrosion_input(id,name,&ids);
        input["character"]["skills"][0]["tripods"]=json!([]);
        input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n"));
        let baseline=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 신경","level":1,"values":[999999]}]);
        let mut disabled=builder::build(&input.to_string()).unwrap();
        assert_eq!(disabled.skills[0].hits.last().unwrap().effect_id,last);
        for (hit,old) in disabled.skills[0].hits.iter().zip(&baseline.skills[0].hits) {
            assert_eq!((hit.base_damage,hit.skill_modifier,hit.action_delay_ms,hit.damage_increase),(old.base_damage,old.skill_modifier,old.action_delay_ms,old.damage_increase));
        }
        input["assumedTargetNerveVulnerable"]=json!(true);
        let mut enabled=builder::build(&input.to_string()).unwrap();
        for profile in [&mut disabled,&mut enabled] {profile.skills[0].cooldown_ms=1000;profile.skills[0].cast_times_ms=vec![100];}
        assert!(disabled.skills[0].hits.iter().all(|h|h.triggers.is_empty()));
        assert_eq!(enabled.skills[0].hits.iter().filter(|h|!h.triggers.is_empty()).count(),1);
        let (_,off)=engine::trace_one(&disabled,42,5000);
        let (_,on)=engine::trace_one(&enabled,42,5000);
        assert_eq!(serde_json::to_value(off.damage_events).unwrap(),serde_json::to_value(&on.damage_events).unwrap());
        let states:Vec<_>=on.casts.iter().take(5).map(|cast|cast.target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:nerve:darkness").map_or(0,|b|b.stacks)).collect();
        assert_eq!(states,vec![1,2,0,1,2]);
        assert!(on.casts[2].target_debuff_details_after_hit.iter().any(|b|b.id=="reaper:nerve:fear" && b.stacks==1));
        enabled.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![enabled.skills[0].hits.last().unwrap().hit_id.clone()];
        let (_,missed)=engine::trace_one(&enabled,42,5000);
        assert!(missed.casts.iter().all(|c|c.target_debuff_details_after_hit.is_empty()));
    }
}

#[test]
fn reaper_nerve_shared_refresh_expiry_consumption_and_fear_duration() {
    let mut input=corrosion_input(26070,"쉐도우 닷",&[260701,260702]);
    input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 신경","level":1}]);
    input["assumedTargetNerveVulnerable"]=json!(true);
    let mut profile=builder::build(&input.to_string()).unwrap();
    profile.skills[0].cast_times_ms=vec![100];profile.skills[0].cooldown_ms=2500;
    let mut other=profile.skills[0].clone();other.skill_id+=100;other.name="공유 대상 확인".into();
    profile.skills.push(other);
    // Alternate source skills at 0/100/2500ms against the same target.
    profile.apl_actions.clear();profile.apl_order=profile.skills.iter().map(|s|s.skill_id).collect();
    let (_,events)=engine::trace_one(&profile,42,6000);
    assert_eq!(events.casts[0].target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:nerve:darkness").unwrap().stacks,1);
    assert_eq!(events.casts[1].target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:nerve:darkness").unwrap().stacks,2);
    assert!(events.casts[2].target_debuff_details_after_hit.iter().any(|b|b.id=="reaper:nerve:fear"));
    for cd in [3999,4000,5000] {
        let mut single=profile.clone();single.skills.truncate(1);single.skills[0].cooldown_ms=cd;
        single.apl_actions.clear();single.apl_order=vec![single.skills[0].skill_id];
        let (_,execution)=engine::trace_one(&single,42,13000);
        let second=&execution.casts[1];
        assert_eq!(second.target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:nerve:darkness").unwrap().stacks,if cd<4000 {2} else {1});
        if cd==3999 {
            assert!(execution.casts[2].target_debuff_details_after_hit.iter().any(|b|b.id=="reaper:nerve:fear"));
            assert!(!execution.casts[3].target_debuffs.contains(&"reaper:nerve:fear".into()));
            assert_eq!(execution.casts[3].target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:nerve:darkness").unwrap().stacks,1);
        }
    }
}

#[test]
fn reaper_storm_end_cooldown_counts_one_target_once_and_respects_hit_end_boundary() {
    for mode in ["all", "last", "miss", "late"] {for black in [false,true] {
        let ids=[vec![266000],vec![266001;10],vec![266002]].concat();
        let mut input=corrosion_input(26600,"쉐도우 스톰",&ids);
        input["skillData"][0]["cooldown"]=json!(20);input["skillData"][0]["castTimesMs"]=json!([1000]);
        input["skillData"][0]["hits"][11]["actionDelayMs"]=json!(if mode=="late" {1200} else {1000});
        input["character"]["skills"][0]["tripods"]=json!([{"name":"빠른 준비","level":1,"values":[999999]}]);
        if black {input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"검은 폭풍","level":1}));}
        input["aplConfig"]=json!("actions:\n - skill: 쉐도우 스톰\n");
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{}});
        let mut profile=builder::build(&input.to_string()).unwrap();
        profile.stat.swiftness=0.;profile.stat.baked_swiftness_cd_reduction=0.;profile.stat.attack_speed.clear();profile.stat.attack_move_speed_percent=0.;
        let skill=&profile.skills[0];assert_eq!(skill.on_end_if_hit.len(),1);
        if mode!="all" {profile.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().take(if mode=="miss" {12} else {11}).map(|h|h.hit_id.clone()).collect();}
        let (_,execution)=engine::trace_one(&profile,42,50000);
        let expected=if matches!(mode,"all"|"last") {18.7} else {20.};
        assert!(execution.casts.len()>=3);
        for pair in execution.casts.windows(2) {assert!((pair[1].time-pair[0].time-expected).abs()<1e-12);}
        if mode=="all" {assert_eq!(execution.damage_events.iter().filter(|h|h.cast_id==Some(execution.casts[0].cast_id)).count(),12);}
    }}
}

#[test]
fn reaper_storm_end_cooldown_does_not_create_a_timer_when_already_ready() {
    let mut input=corrosion_input(26600,"쉐도우 스톰",&[266000]);
    input["skillData"][0]["cooldown"]=json!(0);input["skillData"][0]["castTimesMs"]=json!([1000]);
    input["character"]["skills"][0]["tripods"]=json!([]);input["aplConfig"]=json!("actions:\n - skill: 쉐도우 스톰\n");
    let baseline=builder::build(&input.to_string()).unwrap();
    input["character"]["skills"][0]["tripods"]=json!([{"name":"빠른 준비","level":1}]);
    let selected=builder::build(&input.to_string()).unwrap();
    assert_eq!(serde_json::to_value(engine::trace_one(&baseline,42,10000).1.casts).unwrap(),serde_json::to_value(engine::trace_one(&selected,42,10000).1.casts).unwrap());
}

#[test]
fn reaper_focus_has_two_damage_hits_and_composes_critical_or_chaos_in_both_orders() {
    for other in ["", "치명적인 단검", "순살"] {
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);
        input["character"]["skills"][0]["tripods"]=json!([{"name":"집중 공격","level":1,"values":[999999]}]);
        if !other.is_empty() {input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":other,"level":1}));}
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];
        assert_eq!(skill.cast_times_ms,vec![2467]);
        assert_eq!(skill.hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),vec![268171,268172]);
        assert!(((1.+skill.hits[1].damage_increase)/(1.+skill.hits[0].damage_increase)-1.65).abs()<1e-12);
        assert!(skill.timed_effects.is_empty());
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        let reverse=builder::build(&input.to_string()).unwrap();
        assert_eq!(serde_json::to_value(&profile.skills).unwrap(),serde_json::to_value(&reverse.skills).unwrap());
        input["character"]["skills"][0]["tripods"]=json!([{"name":"집중 공격","level":1},{"name":"처형","level":1}]);
        assert!(builder::build(&input.to_string()).is_err());
    }
}

#[test]
fn reaper_rage_self_notify_occurs_after_damage_even_on_miss_and_scales_with_cast_speed() {
    for choice in ["", "집중 공격", "처형"] {for fast in [false,true] {
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);
        input["character"]["skills"][0]["tripods"]=if choice.is_empty() {json!([{"name":"은밀한 암살","level":1}])} else {json!([{"name":"은밀한 암살","level":1},{"name":choice,"level":1}])};
        let mut filler=input["skillData"][0].clone();filler["gameSkillId"]=json!(26070);filler["name"]=json!("쉐도우 닷");filler["hits"][0]["effectId"]=json!(260701);filler["hits"][0]["hitId"]=json!("filler");
        input["skillData"].as_array_mut().unwrap().push(filler);input["character"]["skills"].as_array_mut().unwrap().push(json!({"name":"쉐도우 닷","level":10,"tripods":[]}));
        input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n - skill: 쉐도우 닷\n");
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{}});
        let mut profile=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        assert_eq!(serde_json::to_value(&profile.skills).unwrap(),serde_json::to_value(&builder::build(&input.to_string()).unwrap().skills).unwrap());
        profile.skills[0].max_uses=1;
        profile.stat.swiftness=0.;profile.stat.attack_speed.clear();profile.stat.attack_move_speed_percent=0.;profile.stat.movement_speed_damage_coefficient=0.5;
        if fast {profile.stat.attack_speed.insert("test".into(),50.);}
        let (_,execution)=engine::trace_one(&profile,42,10000);
        let end=(execution.casts[1].time*1000.).round() as u64;
        let start=profile.skills[0].timed_effects[0].action_delay_ms as u64*end/profile.skills[0].cast_times_ms[0] as u64;
        for cast in &execution.casts {
            let time=(cast.time*1000.).round() as u64;
            assert_eq!(cast.active_buffs.iter().any(|id|id=="reaper:movement:268120"),time>=start && time<start+2000);
        }
        let mut missed=profile.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=profile.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();
        let (_,other)=engine::trace_one(&missed,42,10000);
        assert_eq!(execution.casts.iter().map(|c|c.active_buffs.contains(&"reaper:movement:268120".into())).collect::<Vec<_>>(),other.casts.iter().map(|c|c.active_buffs.contains(&"reaper:movement:268120".into())).collect::<Vec<_>>());
        assert!(execution.damage_events.iter().filter(|h|h.skill_name=="레이지 스피어").all(|h|h.formula.movement_speed_damage_multiplier==1.));
        assert!(execution.damage_events.iter().any(|h|h.skill_name=="쉐도우 닷" && (h.formula.movement_speed_damage_multiplier-1.2).abs()<1e-12));
    }}
}

#[test]
fn reaper_movement_buffs_proc_on_hit_and_do_not_proc_on_miss() {
    use crate::profile::TriggerEffect;
    for (id,name,choice,effect_id,value,duration) in [(26550,"콜 오브 나이프","그림자 강탈",265420,0.192,3000),(26570,"쉐도우 트랩","날렵한 몸놀림",265700,0.246,4000)] {
        let mut input=corrosion_input(id,name,&[effect_id]);
        input["character"]["skills"][0]["tripods"]=json!([{"name":choice,"level":1,"values":[999999]}]);
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];
        let TriggerEffect::ApplyBuff(buff)=&skill.hits[0].triggers.last().unwrap().effect else {panic!("movement buff missing")};
        assert_eq!((buff.effects[&StatField::MovementSpeedMax],buff.duration_ms,buff.max_stacks),(value,duration,1));
        let (_,events)=engine::trace_one(&profile,42,10000);
        assert!(!events.casts[0].active_buffs.contains(&buff.id));
        assert!(events.casts[1].active_buffs.contains(&buff.id));
        let mut missed=profile.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();
        let (_,events)=engine::trace_one(&missed,42,10000);
        assert!(events.casts.iter().all(|cast|!cast.active_buffs.contains(&buff.id)));
    }
}

fn request(thirst: bool) -> Value {
    serde_json::from_str(if thirst {
        include_str!("../fixtures/reaper-thirst-reference-request.json")
    } else { include_str!("../fixtures/reaper-moon-reference-request.json") }).unwrap()
}

fn trace(input: &Value, duration: u32) -> (crate::constants::SimResult, crate::constants::TraceData) {
    engine::trace_one(&builder::build(&input.to_string()).unwrap(), 42, duration)
}

fn configured(thirst: bool) -> Value {
    let mut input = request(thirst);
    input["reaperBuild"] = input["reaperReference"]["scenario"].clone();
    input.as_object_mut().unwrap().remove("reaperReference");
    input["character"]["equipments"] = json!([
        {"type":"무기","setName":"test","quality":100,"itemLevel":1660,"refinementLevel":0,"advancedRefinementLevel":0},
        {"type":"투구","setName":"test","quality":0,"itemLevel":1660,"refinementLevel":0,"advancedRefinementLevel":0}]);
    input["equipmentTables"] = json!({"test": {
        "무기":[{"itemLevel":1660,"weaponAttack":150000}],
        "투구":[{"itemLevel":1660,"mainStat":300000}]}});
    input
}

#[test]
fn reaper_dagger_choices_preserve_routes_and_apply_only_their_trusted_bonuses() {
    for (id,name,choice,ids,crit,crit_damage,damage,ambush) in [
        (26050,"스피닝 대거","기습",vec![260500,260501],0.,0.,0.,0.5),
        (26050,"스피닝 대거","치명적인 단검",vec![260500,260501],0.15,1.,0.,0.),
        (26060,"스피릿 캐치","날카로운 단검",vec![260600,260612],0.,2.1,0.,0.),
        (26060,"스피릿 캐치","기습",vec![260600,260612],0.,0.,0.,0.8),
        (26080,"이블리스토","기습",vec![260800,260801,260802,260803],0.,0.,0.,1.2),
        (26120,"데스 사이드","기습",vec![261200,261200,261201],0.,0.,0.,0.902),
        (26120,"데스 사이드","치명적인 단검",vec![261200,261200,261201],0.15,0.,0.65,0.),
    ] {
        for back in [false,true] {
            let mut input=configured(true);
            let mut data=input["skillData"].as_array().unwrap().iter().find(|s| s["name"]=="레이지 스피어").unwrap().clone();
            data["name"]=json!(name); data["gameSkillId"]=json!(id);
            data["properties"]["attackType"]=json!(if back {"백 어택"} else {""});
            let template=data["hits"][0].clone();
            data["hits"]=json!(ids.iter().enumerate().map(|(i,effect)| {
                let mut hit=template.clone(); hit["effectId"]=json!(effect);
                hit["hitId"]=json!(format!("{id}:{i}")); hit["actionDelayMs"]=json!(100+i*100);
                for field in ["fixedDamage","damageRatio"] {hit[field]=json!(vec![1.0;14]);}
                hit
            }).collect::<Vec<_>>());
            input["skillData"]=json!([data]);
            input["character"]["skills"]=json!([{"name":name,"level":10,"tripods":[]}]);
            input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n"));
            let base=builder::build(&input.to_string()).unwrap();
            input["character"]["skills"][0]["tripods"]=json!([{"name":choice,"level":1,"values":[999999]}]);
            let selected=builder::build(&input.to_string()).unwrap();
            assert_eq!(selected.skills[0].cooldown_ms,base.skills[0].cooldown_ms);
            assert_eq!(selected.skills[0].cast_times_ms,base.skills[0].cast_times_ms);
            assert_eq!(selected.skills[0].hits.len(),ids.len());
            for (hit,old) in selected.skills[0].hits.iter().zip(&base.skills[0].hits) {
                assert_eq!((hit.effect_id,hit.action_delay_ms),(old.effect_id,old.action_delay_ms));
                assert!((hit.crit_chance_bonus-old.crit_chance_bonus-crit).abs()<1e-12);
                assert!((hit.crit_damage_bonus-old.crit_damage_bonus-crit_damage).abs()<1e-12);
                assert!(((1.+hit.damage_increase)/(1.+old.damage_increase)-(1.+damage)*(1.+if back {ambush} else {0.})).abs()<1e-12);
            }
            if id==26060 && choice=="날카로운 단검" {assert!(selected.skills[0].tags.contains(&crate::profile::SkillTag::ElementElectricity));}
            input["character"]["skills"][0]["tripods"][0]["level"]=json!(5);
            assert!(builder::build(&input.to_string()).is_err());
        }
    }
}

#[test]
fn reaper_notify_range_choices_compose_after_layer_routes_in_both_input_orders() {
    for (id,name,range,other,expected) in [
        (26800,"라스트 그래피티","범위 증가","유언",268073),
        (26820,"사일런트 스매셔","넓은 공격","지면 강타",268210),
        (26820,"사일런트 스매셔","넓은 공격","추진력",268210),
    ] {
        let mut input=configured(true);
        let mut data=input["skillData"][2].clone();
        data["gameSkillId"]=json!(id); data["name"]=json!(name);
        data["hits"][0]["effectId"]=json!(if id==26800 {268000} else {268200});
        for field in ["fixedDamage","damageRatio"] {data["hits"][0][field]=json!(vec![1.;14]);}
        input["skillData"]=json!([data]);
        input["character"]["skills"]=json!([{"name":name,"level":10,"tripods":[{"name":other,"level":1}]}]);
        input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n"));
        let base=builder::build(&input.to_string()).unwrap();
        for reversed in [false,true] {
            input["character"]["skills"][0]["tripods"]=if reversed {json!([{"name":other,"level":1},{"name":range,"level":1}])}
                else {json!([{"name":range,"level":1,"values":[999999]},{"name":other,"level":1}])};
            let selected=builder::build(&input.to_string()).unwrap();
            assert_eq!(selected.skills[0].hits.len(),base.skills[0].hits.len());
            assert_eq!(selected.skills[0].cast_times_ms,base.skills[0].cast_times_ms);
            for (hit,old) in selected.skills[0].hits.iter().zip(&base.skills[0].hits) {
                assert_eq!(hit.effect_id,expected);
                assert_eq!((hit.base_damage,hit.skill_modifier,hit.damage_increase,hit.crit_chance_bonus,hit.action_delay_ms),
                           (old.base_damage,old.skill_modifier,old.damage_increase,old.crit_chance_bonus,old.action_delay_ms));
            }
        }
    }
}

#[test]
fn reaper_spreading_mist_adds_one_trusted_early_hit_before_rune_and_gem_processing() {
    let mut input=configured(true);
    let mut data=input["skillData"][2].clone();
    data["gameSkillId"]=json!(26560); data["name"]=json!("블랙 미스트");
    data["properties"]["attackType"]=json!(""); data["castTimesMs"]=json!([3833]);
    let template=data["hits"][0].clone();
    data["hits"]=json!([743,1294,1846,2397,2949].into_iter().enumerate().map(|(i,ms)| {
        let mut hit=template.clone(); hit["effectId"]=json!(265600); hit["actionDelayMs"]=json!(ms);
        hit["hitId"]=json!(format!("mist:{i}"));
        for field in ["fixedDamage","damageRatio"] {hit[field]=json!(vec![1.;14]);}
        hit
    }).collect::<Vec<_>>());
    input["skillData"]=json!([data]);
    input["character"]["skills"]=json!([{"name":"블랙 미스트","level":10,"tripods":[]}]);
    input["aplConfig"]=json!("actions:\n - skill: 블랙 미스트\n");
    let base=builder::build(&input.to_string()).unwrap();
    input["character"]["skills"][0]["tripods"]=json!([{"name":"퍼지는 안개","level":1,"values":[999999]}]);
    let selected=builder::build(&input.to_string()).unwrap();
    let skill=&selected.skills[0];
    assert_eq!(skill.hits.len(),6); assert_eq!(skill.cast_times_ms,base.skills[0].cast_times_ms);
    assert_eq!(skill.cooldown_ms,base.skills[0].cooldown_ms);
    let extra=&skill.hits[0]; assert_eq!((extra.effect_id,extra.action_delay_ms),(265661,201));
    assert_eq!((extra.base_damage,extra.skill_modifier),(961.,5.2598));
    assert!((extra.damage_increase-1.).abs()<1e-12);
    for (hit,old) in skill.hits[1..].iter().zip(&base.skills[0].hits) {
        assert_eq!((hit.effect_id,hit.action_delay_ms,hit.skill_modifier,hit.damage_increase),
                   (old.effect_id,old.action_delay_ms,old.skill_modifier,old.damage_increase));
    }
    let (_,events)=engine::trace_one(&selected,42,5000);
    let first=events.damage_events[0].cast_id;
    let hits:Vec<_>=events.damage_events.iter().filter(|hit| hit.cast_id==first).collect();
    assert_eq!(hits.len(),6); assert_eq!(hits[0].hit_id,"26560:0:notify:265661");
    assert_eq!(hits.iter().map(|hit| &hit.hit_id).collect::<std::collections::HashSet<_>>().len(),6);
}

fn corrosion_input(id:u32,name:&str,ids:&[u32]) -> Value {
    let mut input=configured(true); let mut data=input["skillData"][2].clone();
    data["gameSkillId"]=json!(id); data["name"]=json!(name);
    data["castTimesMs"]=json!([100]); data["cooldown"]=json!(0);
    let template=data["hits"][0].clone();
    data["hits"]=json!(ids.iter().enumerate().map(|(i,effect_id)| {
        let mut hit=template.clone();hit["effectId"]=json!(effect_id);hit["hitId"]=json!(format!("corrosion:{id}:{i}"));hit["actionDelayMs"]=json!(10+i*10);
        for field in ["fixedDamage","damageRatio"] {hit[field]=json!(vec![1.;14]);}hit
    }).collect::<Vec<_>>());
    if id==26110 {
        for (hit,hit_id) in data["hits"].as_array_mut().unwrap().iter_mut().zip(["26110:0:0:repeat:0","26110:0:0:repeat:1","26110:1:0","26110:1:1"]) {hit["hitId"]=json!(hit_id);}
    }
    input["skillData"]=json!([data]);input["character"]["skills"]=json!([{"name":name,"level":10,"tripods":[{"name":"독 : 부식","level":1,"values":[999999]}]}]);
    input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n   conditions:\n    - {{type: time_remaining, operator: '>=', value: 9}}\n"));input
}

#[test]
fn reaper_mist_projectiles_keep_fixed_intervals_and_compose_spreading_mist_before_callbacks() {
    use crate::profile::StatField;
    for (choice,combined,count) in [("거짓 안개",false,5),("거짓 안개",true,6),("안개 폭발",false,8),("안개 폭발",true,9)] {
        let mut input=corrosion_input(26560,"블랙 미스트",&[265600]);
        input["skillData"][0]["cooldown"]=json!(20);
        input["skillData"][0]["properties"]["attackType"]=json!("");
        input["character"]["skills"][0]["tripods"]=json!([{ "name":choice,"level":1,"values":[999999]}]);
        if combined {input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"퍼지는 안개","level":1}));}
        input["aplConfig"]=json!("actions:\n - skill: 블랙 미스트\n   conditions:\n    - {type: time_remaining, operator: '>=', value: 9.999}\n");
        let profile=builder::build(&input.to_string()).unwrap();
        let skill=&profile.skills[0];
        assert_eq!(skill.hits.len(),count);
        assert_eq!(skill.cast_times_ms,vec![if choice=="거짓 안개" {500} else {3833}]);
        assert!(profile.reaper_reference.as_ref().unwrap().metadata.get("mist_projectile_policy").is_some());
        for hit in &skill.hits {
            let expected=if hit.effect_id==265661 {1.} else if matches!(hit.effect_id,265682|265683|265684) {0.58} else {0.};
            assert!((hit.damage_increase-expected).abs()<1e-12);
        }
        let (_,events)=engine::trace_one(&profile,42,10000);
        assert_eq!(events.casts.len(),1);assert_eq!(events.damage_events.len(),count);
        assert_eq!(events.damage_events.iter().map(|h|&h.hit_id).collect::<std::collections::HashSet<_>>().len(),count);
        let is_projectile=|id:&str| id.contains(":265675:") || id.contains(":265682:") || id.contains(":265683:") || id.contains(":265684:");
        let times:Vec<_>=events.damage_events.iter().filter(|h|is_projectile(&h.hit_id)).map(|h|h.time).collect();
        for pair in times.windows(2) {assert!((pair[1]-pair[0]-if choice=="거짓 안개" {0.4} else {0.5}).abs()<1e-9);}
        let mut fast=profile.clone();fast.stat.attack_speed.insert("test".into(),50.);
        assert!(fast.stat.sum(StatField::AttackSpeed)>profile.stat.sum(StatField::AttackSpeed));
        let (_,quick)=engine::trace_one(&fast,42,10000);
        let fast_times:Vec<_>=quick.damage_events.iter().filter(|h|is_projectile(&h.hit_id)).map(|h|h.time).collect();
        assert_eq!(times.len(),fast_times.len());assert!(fast_times[0]<times[0]);
        for pair in fast_times.windows(2) {assert!((pair[1]-pair[0]-if choice=="거짓 안개" {0.4} else {0.5}).abs()<1e-9);}
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        let reversed=builder::build(&input.to_string()).unwrap();
        let (_,again)=engine::trace_one(&reversed,42,10000);
        assert_eq!(serde_json::to_value(events.damage_events).unwrap(),serde_json::to_value(again.damage_events).unwrap());
        fast.reaper_reference.as_mut().unwrap().missed_hit_ids=fast.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();
        assert!(engine::trace_one(&fast,42,10000).1.damage_events.is_empty());
    }
}

#[test]
fn reaper_corrosion_routes_share_three_stacks_and_keep_dot_independent_of_source_hits() {
    for (id,name,ids,last) in [
        (26050,"스피닝 대거",vec![260500,260501],260511),
        (26060,"스피릿 캐치",vec![260600,260612],260613),
        (26070,"쉐도우 닷",vec![260701,260702],260712),
        (26080,"이블리스토",vec![260800,260801,260802,260803],260813),
        (26120,"데스 사이드",vec![261200,261200,261201],261211),
        (26140,"팬텀 댄서",vec![261400,261401,261402,261403],261413),
        (26110,"샤벨 스팅거",vec![261100,261100,261101,261101],261112),
    ] {
        let input=corrosion_input(id,name,&ids);
        let mut profile=builder::build(&input.to_string()).unwrap();
        assert_eq!(profile.skills[0].hits.last().unwrap().effect_id,last);
        let (_,events)=engine::trace_one(&profile,42,10000);
        let dot:Vec<_>=events.damage_events.iter().filter(|h| h.hit_id=="reaper:corrosion:tick").collect();
        assert_eq!(dot.len(),8,"{name}");
        for tick in &dot {
            assert!((tick.formula.skill_modifier-0.1833*3.).abs()<1e-12,"{name}: casts={} modifier={}",events.casts.len(),tick.formula.skill_modifier);assert_eq!(tick.formula.base_damage,27.);
            assert!((tick.formula.defense_multiplier-6500.0/(6500.0+profile.target_defense*0.88)).abs()<1e-12);
            assert!(tick.resource_gains.is_empty());
        }
        for hit in &mut profile.skills[0].hits {hit.damage_increase+=999.;hit.crit_chance_bonus+=1.;hit.crit_damage_bonus+=9.;}
        profile.skills[0].evolution_damage_bonuses.push(("source-only".into(),10.));
        let (_,changed)=engine::trace_one(&profile,42,10000);
        let changed:Vec<_>=changed.damage_events.iter().filter(|h| h.hit_id=="reaper:corrosion:tick").collect();
        assert_eq!(changed.len(),8);
        for (old,new) in dot.iter().zip(&changed) {
            assert_eq!((new.formula.base_term,new.formula.skill_damage_multiplier,new.formula.evolution_damage_multiplier),
                       (old.formula.base_term,old.formula.skill_damage_multiplier,old.formula.evolution_damage_multiplier));
        }
        let repeated=engine::trace_one(&profile,42,10000);
        assert_eq!(serde_json::to_value(&changed).unwrap(),serde_json::to_value(&repeated.1.damage_events.iter().filter(|h| h.hit_id=="reaper:corrosion:tick").collect::<Vec<_>>()).unwrap());
        profile.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![profile.skills[0].hits.last().unwrap().hit_id.clone()];
        let (_,missed)=engine::trace_one(&profile,42,10000);
        assert!(!missed.damage_events.iter().any(|h| h.hit_id=="reaper:corrosion:tick"));
    }
}

#[test]
fn reaper_corrosion_last_hit_applies_after_damage_and_expires_without_more_attacks() {
    let mut input=corrosion_input(26070,"쉐도우 닷",&[260701,260702]);
    let mut probe=configured(true)["skillData"][2].clone();probe["cooldown"]=json!(300);
    input["skillData"].as_array_mut().unwrap().push(probe);
    input["character"]["skills"].as_array_mut().unwrap().push(json!({"name":"레이지 스피어","level":1,"tripods":[]}));
    input["aplConfig"]=json!("actions:\n - skill: 쉐도우 닷\n   conditions:\n    - {type: time_remaining, operator: '>=', value: 19.99}\n - skill: 레이지 스피어\n   conditions:\n    - {type: time_remaining, operator: '<=', value: 7}\n");
    let profile=builder::build(&input.to_string()).unwrap();
    let (_,events)=engine::trace_one(&profile,42,20000);
    assert_eq!(events.casts.len(),2);
    let hits=&events.damage_events;
    assert!((hits[1].formula.defense_multiplier-6500.0/(6500.0+profile.target_defense)).abs()<1e-12);
    let ticks:Vec<_>=hits.iter().filter(|h| h.hit_id=="reaper:corrosion:tick").collect();
    assert_eq!(ticks.len(),8);
    assert!((ticks[0].time-hits[1].time-0.5).abs()<1e-12);
    assert!(ticks.windows(2).all(|pair| (pair[1].time-pair[0].time-1.).abs()<1e-12));
    assert!(ticks.iter().all(|h| h.formula.base_damage==9. && (h.formula.skill_modifier-0.1833).abs()<1e-12));
    let probe=events.casts.iter().find(|cast|cast.skill_name=="레이지 스피어").unwrap();
    assert!(probe.target_debuffs.is_empty());
    let late=hits.iter().find(|hit|hit.skill_name=="레이지 스피어").unwrap();
    assert_eq!(late.formula.defense_multiplier,hits[1].formula.defense_multiplier);
}

#[test]
fn reaper_corrosion_different_skills_share_stacks_and_refresh_one_dot() {
    let mut input=corrosion_input(26050,"스피닝 대거",&[260500,260501]);
    for other in [corrosion_input(26060,"스피릿 캐치",&[260600,260612]),corrosion_input(26070,"쉐도우 닷",&[260701,260702])] {
        input["skillData"].as_array_mut().unwrap().push(other["skillData"][0].clone());
        input["character"]["skills"].as_array_mut().unwrap().push(other["character"]["skills"][0].clone());
    }
    for data in input["skillData"].as_array_mut().unwrap() {data["cooldown"]=json!(300);}
    input["aplConfig"]=json!("actions:\n - skill: 스피닝 대거\n - skill: 스피릿 캐치\n - skill: 쉐도우 닷\n");
    let (_,events)=trace(&input,10000);
    assert_eq!(events.casts.len(),3);
    assert_eq!(events.casts[2].target_debuff_details_after_hit.iter().find(|buff|buff.id=="reaper:corrosion").unwrap().stacks,3);
    let ticks:Vec<_>=events.damage_events.iter().filter(|hit|hit.hit_id=="reaper:corrosion:tick").collect();
    assert_eq!(ticks.len(),8);
    assert!(ticks.iter().all(|hit| hit.skill_name=="쉐도우 닷" && hit.cast_id==Some(events.casts[2].cast_id) && (hit.formula.skill_modifier-0.1833*3.).abs()<1e-12));
}

#[test]
fn reaper_corrosion_reapplication_at_expiry_starts_at_one_stack() {
    let mut input=corrosion_input(26070,"쉐도우 닷",&[260701,260702]);
    input["skillData"][0]["cooldown"]=json!(8);
    input["aplConfig"]=json!("actions:\n - skill: 쉐도우 닷\n");
    let (_,events)=trace(&input,17000);
    assert_eq!(events.casts.len(),3);
    assert_eq!(events.casts[1].time-events.casts[0].time,8.);
    assert!(events.damage_events.iter().filter(|hit|hit.hit_id=="reaper:corrosion:tick").all(|hit|(hit.formula.skill_modifier-0.1833).abs()<1e-12));
}

#[test]
fn reaper_bleeding_ten_stacks_burst_once_and_do_not_inherit_source_bonuses() {
    for (id,name,ids,last) in [(26050,"스피닝 대거",vec![260500,260501],260521),
        (26060,"스피릿 캐치",vec![260600,260612],260623),
        (26080,"이블리스토",vec![260800,260801,260802,260803],260823),
        (26120,"데스 사이드",vec![261200,261200,261201],261221),
        (26070,"쉐도우 닷",vec![260701,260702],260722),
        (26140,"팬텀 댄서",vec![261400,261401,261402,261403],261422),
        (26110,"샤벨 스팅거",vec![261100,261100,261101,261101],261122)] {
        let mut input=corrosion_input(id,name,&ids);
        input["character"]["skills"][0]["tripods"][0]["name"]=json!("독 : 출혈");
        input["skillData"][0]["cooldown"]=json!(0.1);
        input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n   conditions:\n    - {{type: time_remaining, operator: '>=', value: 19.099}}\n"));
        let mut profile=builder::build(&input.to_string()).unwrap();
        assert_eq!(profile.skills[0].hits.last().unwrap().effect_id,last);
        let (_,events)=engine::trace_one(&profile,42,20000);
        assert_eq!(events.casts.len(),10);
        let burst:Vec<_>=events.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:burst").collect();
        assert_eq!(burst.len(),1,"{name}");
        assert_eq!((burst[0].formula.base_damage,burst[0].formula.skill_modifier),(1925.5,41.769));
        assert_eq!(burst[0].cast_id,Some(events.casts[9].cast_id));
        assert!(burst[0].resource_gains.is_empty());
        assert!(!events.damage_events.iter().any(|h|h.hit_id=="reaper:bleeding:tick"));
        assert!(!events.casts[9].target_debuff_details_after_hit.iter().any(|b|b.id=="reaper:bleeding"));
        for hit in &mut profile.skills[0].hits {hit.damage_increase+=999.;hit.crit_chance_bonus+=1.;hit.crit_damage_bonus+=9.;}
        profile.skills[0].evolution_damage_bonuses.push(("source-only".into(),10.));
        let (_,changed)=engine::trace_one(&profile,42,20000);
        let changed=changed.damage_events.iter().find(|h|h.hit_id=="reaper:bleeding:burst").unwrap();
        assert_eq!((changed.formula.base_term,changed.formula.skill_damage_multiplier,changed.formula.evolution_damage_multiplier),
            (burst[0].formula.base_term,burst[0].formula.skill_damage_multiplier,burst[0].formula.evolution_damage_multiplier));
        profile.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![profile.skills[0].hits.last().unwrap().hit_id.clone()];
        let (_,missed)=engine::trace_one(&profile,42,20000);
        assert!(!missed.damage_events.iter().any(|h|h.hit_id.starts_with("reaper:bleeding:")));
        input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n   conditions:\n    - {{type: time_remaining, operator: '>=', value: 19.199}}\n"));
        let (_,nine)=trace(&input,20000);
        assert_eq!(nine.casts.len(),9);
        assert!(!nine.damage_events.iter().any(|h|h.hit_id=="reaper:bleeding:burst"));
        let ticks:Vec<_>=nine.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:tick").collect();
        assert_eq!(ticks.len(),12);
        assert!(ticks.iter().all(|h|h.formula.base_damage==9. && (h.formula.skill_modifier-0.0169*9.).abs()<1e-12));
    }
}

#[test]
fn reaper_shabel_bleeding_keeps_last_two_hits_distinct_and_supports_provisional_proc_combinations() {
    let mut input=corrosion_input(26110,"샤벨 스팅거",&[261100,261100,261101,261101]);
    input["character"]["skills"][0]["tripods"][0]["name"]=json!("독 : 출혈");
    input["aplConfig"]=json!("actions:\n - skill: 샤벨 스팅거\n   conditions:\n    - {type: time_remaining, operator: '>=', value: 9.99}\n");
    let profile=builder::build(&input.to_string()).unwrap();
    assert_eq!(profile.skills[0].hits.iter().map(|h|h.effect_id).collect::<Vec<_>>(),vec![261120,261120,261121,261122]);
    let (_,events)=engine::trace_one(&profile,42,10000);
    for cast in &events.casts {
        let hits:Vec<_>=events.damage_events.iter().filter(|h|h.cast_id==Some(cast.cast_id) && !h.hit_id.starts_with("reaper:")).collect();
        assert_eq!(hits.len(),4);
        assert_eq!(cast.target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:bleeding").unwrap().stacks,
                   cast.target_debuff_details.iter().find(|b|b.id=="reaper:bleeding").map_or(0,|b|b.stacks)+1);
    }
    input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"날카로운 단검","level":1}));
    assert!(builder::build(&input.to_string()).unwrap().reaper_reference.unwrap().metadata.get("shabel_bleed_chance_binding").is_some());
}

#[test]
fn reaper_shabel_probability_rolls_once_per_normal_hit_and_never_recurses_from_poison() {
    use crate::profile::skill::{TriggerEffect,HitTriggerOn};
    for (choice,crit,chance,extra) in [("두개의 그림자",false,1.,4),("두개의 그림자",false,0.,0),
        ("날카로운 단검",true,1.,4),("날카로운 단검",false,1.,0),("날카로운 단검",true,0.,0)] {
        let mut input=corrosion_input(26110,"샤벨 스팅거",&[261100,261100,261101,261101]);
        input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 출혈","level":1},{"name":choice,"level":1}]);
        input["skillData"][0]["cooldown"]=json!(0.1);
        input["aplConfig"]=json!("actions:\n - skill: 샤벨 스팅거\n   conditions:\n    - {type: time_remaining, operator: '>=', value: 19.799}\n");
        let mut profile=builder::build(&input.to_string()).unwrap();
        profile.stat.crit_rate.clear();profile.stat.crit_rate.insert("test".into(),if crit {100.} else {-100.});
        for hit in &mut profile.skills[0].hits {
            let packets:Vec<_>=hit.triggers.iter_mut().filter(|t| matches!(t.effect,TriggerEffect::Sequence(_))).collect();
            assert_eq!(packets.len(),1);
            let packet=packets.into_iter().next().unwrap();
            assert_eq!(packet.chance,if choice=="두개의 그림자" {0.25} else {0.5});
            assert!(matches!(&packet.effect,TriggerEffect::Sequence(effects) if effects.len()==2));
            assert!(matches!(packet.on,HitTriggerOn::OnCrit)==(choice=="날카로운 단검"));
            packet.chance=chance;
        }
        let (_,events)=engine::trace_one(&profile,42,20000);
        assert_eq!(events.casts.len(),3);
        let total=3*(1+extra);
        let bursts:Vec<_>=events.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:burst").collect();
        assert_eq!(bursts.len(),(total/10) as usize);
        let stacks=events.casts.last().unwrap().target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:bleeding").unwrap().stacks;
        assert_eq!(stacks,total%10);
        assert!(events.casts.iter().all(|cast|cast.target_debuff_details_after_hit.iter().all(|buff|buff.id!="reaper:bleeding" || buff.stacks<10)));
        let ticks:Vec<_>=events.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:tick").collect();
        assert_eq!(ticks.len(),12);
        assert!(ticks.iter().all(|h|h.formula.base_damage==(total%10) as f64 && h.formula.skill_damage_multiplier==1. && h.resource_gains.is_empty()));
        assert!(bursts.iter().all(|h|h.formula.base_damage==1925.5 && h.formula.skill_damage_multiplier==1. && h.resource_gains.is_empty()));
        let (_,repeated)=engine::trace_one(&profile,42,20000);
        assert_eq!(serde_json::to_value(&events.damage_events).unwrap(),serde_json::to_value(&repeated.damage_events).unwrap());
        if choice=="두개의 그림자" && chance==1. {
            profile.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![profile.skills[0].hits.last().unwrap().hit_id.clone()];
            let (_,partial)=engine::trace_one(&profile,42,20000);
            assert!(!partial.damage_events.iter().any(|h|h.hit_id=="reaper:bleeding:burst"));
            let ticks:Vec<_>=partial.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:tick").collect();
            assert_eq!(ticks.len(),12);assert!(ticks.iter().all(|h|h.formula.base_damage==9.));
        }
        profile.reaper_reference.as_mut().unwrap().missed_hit_ids=profile.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();
        let (_,missed)=engine::trace_one(&profile,42,20000);
        assert!(!missed.damage_events.iter().any(|h|h.hit_id.starts_with("reaper:bleeding:")));
    }
}

#[test]
fn reaper_bleeding_bonus_conditions_apply_each_stack_and_preserve_overflow_after_burst() {
    for (id,name,ids,choice,back,crit,casts,extra) in [
        (26050,"스피닝 대거",vec![260500,260501],"기습",true,false,3,3),
        (26050,"스피닝 대거",vec![260500,260501],"기습",false,false,3,0),
        (26050,"스피닝 대거",vec![260500,260501],"치명적인 단검",true,true,4,2),
        (26050,"스피닝 대거",vec![260500,260501],"치명적인 단검",true,false,4,0),
        (26060,"스피릿 캐치",vec![260600,260612],"날카로운 단검",true,true,6,1),
        (26060,"스피릿 캐치",vec![260600,260612],"날카로운 단검",true,false,6,0),
        (26060,"스피릿 캐치",vec![260600,260612],"기습",true,false,6,1),
        (26060,"스피릿 캐치",vec![260600,260612],"기습",false,false,6,0),
        (26080,"이블리스토",vec![260800,260801,260802,260803],"기습",true,false,4,2),
        (26080,"이블리스토",vec![260800,260801,260802,260803],"기습",false,false,4,0),
        (26120,"데스 사이드",vec![261200,261200,261201],"기습",true,false,4,2),
        (26120,"데스 사이드",vec![261200,261200,261201],"기습",false,false,4,0),
        (26120,"데스 사이드",vec![261200,261200,261201],"치명적인 단검",true,true,6,1),
        (26120,"데스 사이드",vec![261200,261200,261201],"치명적인 단검",true,false,6,0)] {
        let mut input=corrosion_input(id,name,&ids);
        input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 출혈","level":1},{"name":choice,"level":1}]);
        input["skillData"][0]["cooldown"]=json!(0.1);
        input["skillData"][0]["properties"]["attackType"]=json!(if back {"백 어택"} else {""});
        input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n   conditions:\n    - {{type: time_remaining, operator: '>=', value: {}}}\n",20.-(casts-1) as f64*0.1-0.001));
        let mut profile=builder::build(&input.to_string()).unwrap();
        profile.stat.crit_rate.clear();profile.stat.crit_rate.insert("test".into(),if crit {100.} else {-100.});
        let (_,events)=engine::trace_one(&profile,42,20000);
        assert_eq!(events.casts.len(),casts as usize);
        let total=casts*(1+extra);
        let bursts:Vec<_>=events.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:burst").collect();
        assert_eq!(bursts.len(),(total/10) as usize,"{choice}/{back}/{crit}: {:?}",events.damage_events.iter().filter(|hit|!hit.hit_id.starts_with("reaper:")).map(|hit|(&hit.hit_id,hit.is_crit,hit.formula.crit_rate)).collect::<Vec<_>>());
        let stacks=events.casts.last().unwrap().target_debuff_details_after_hit.iter().find(|b|b.id=="reaper:bleeding").unwrap().stacks;
        assert_eq!(stacks,total%10);
        let ticks:Vec<_>=events.damage_events.iter().filter(|h|h.hit_id=="reaper:bleeding:tick").collect();
        assert_eq!(ticks.len(),12);
        assert!(ticks.iter().all(|h|h.formula.base_damage==(total%10) as f64 && h.formula.skill_damage_multiplier==1. && h.resource_gains.is_empty()));
        assert!(bursts.iter().all(|h|h.formula.base_damage==1925.5 && h.formula.skill_modifier==41.769 && h.formula.skill_damage_multiplier==1. && h.resource_gains.is_empty()));
    }
}

#[test]
fn reaper_dagger_choices_reject_active_unimplemented_bleed_chains() {
    let catalog:Value=serde_json::from_str(include_str!("../skill-data/reaper/backend/tripod-catalog.json")).unwrap();
    for item in catalog.as_array().unwrap().iter().filter(|item| item.get("inactiveChainEffects").is_some()) {
        let mut input=configured(true);
        let mut data=input["skillData"][2].clone();
        data["name"]=item["skillName"].clone(); data["gameSkillId"]=item["gameSkillId"].clone();
        data["hits"][0]["effectId"]=item["inactiveChainEffects"][0]["effectId"].clone();
        input["skillData"]=json!([data]);
        input["character"]["skills"]=json!([{"name":item["skillName"],"level":10,"tripods":[{"name":item["name"],"level":1}]}]);
        input["aplConfig"]=json!(format!("actions:\n - skill: {}\n",item["skillName"].as_str().unwrap()));
        if item["runtimeEffects"].as_array().unwrap().iter().any(|e|e["kind"]=="replace_route") {
            let profile=builder::build(&input.to_string()).unwrap();
            assert!(profile.skills[0].hits.iter().all(|hit|Some(hit.effect_id as u64)!=item["inactiveChainEffects"][0]["effectId"].as_u64()),"trusted neutral route removes the supplied poison-only hit");
        } else {
            assert!(builder::build(&input.to_string()).unwrap_err().contains("출혈 전용"));
        }
    }
}

fn grid_slot(name: &str, grade: &str, points: u32) -> Value {
    let mut gems = vec![];
    let mut remaining = points;
    while remaining > 0 {
        let value = remaining.min(5); remaining -= value;
        gems.push(json!({"name":"안정","gemPoint":value,"willpowerEfficiency":5,"effects":[]}));
    }
    json!({"name":name,"grade":grade,"coreAlignment":"질서","coreSymbol":"별","gems":gems})
}

#[test]
fn reaper_execution_uses_the_short_route_and_keeps_its_chain_damage_separate_from_chaos() {
    for thirst in [false,true] {
        let mut input=configured(thirst);
        let mut data=input["skillData"].as_array().unwrap().iter().find(|skill| skill["name"]=="레이지 스피어").unwrap().clone();
        data["cooldown"]=json!(25);
        for hit in data["hits"].as_array_mut().unwrap() {for field in ["fixedDamage","damageRatio"] {hit[field]=json!(vec![1.0;14]);}}
        input["skillData"]=json!([data]); input["character"]["skills"]=json!([{"name":"레이지 스피어","level":10,"tripods":[]}]);
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":if thirst {15} else {0},"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{}});
        input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");
        let base=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"]=json!([{"name":"처형","level":1,"values":[99999]}]);
        let selected=builder::build(&input.to_string()).unwrap();
        let skill=&selected.skills[0];
        assert_eq!(skill.cast_times_ms,vec![1667]); assert_eq!(skill.cooldown_ms,25000);
        assert_eq!(skill.hits.len(),1);
        assert_eq!((skill.hits[0].effect_id,skill.hits[0].hit_id.as_str(),skill.hits[0].action_delay_ms),(268180,"26810:4:0",367));
        assert_eq!(skill.hits[0].base_damage,3694.5); assert_eq!(skill.hits[0].skill_modifier,20.1825);
        assert!(((1.0+skill.hits[0].damage_increase)/(1.0+base.skills[0].hits[0].damage_increase)-1.651).abs()<1e-12);
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"순살","level":1}));
        let combined=builder::build(&input.to_string()).unwrap();
        assert_eq!(combined.skills[0].hits[0].damage_increase,skill.hits[0].damage_increase);
        assert_eq!(combined.skills[0].cooldown_ms,16000);
        assert_eq!(combined.reaper_reference.as_ref().unwrap().metadata["chaos_tripod_damage"]["26810"],json!(0.5));
        let (_,old_trace)=engine::trace_one(&selected,42,40000);
        let (_,new_trace)=engine::trace_one(&combined,42,40000);
        let expected=if thirst {1.5} else {1.0};
        assert!((new_trace.damage_events[0].formula.damage_increase_multiplier/old_trace.damage_events[0].formula.damage_increase_multiplier-expected).abs()<1e-12);
        assert!(new_trace.damage_events.iter().all(|hit| hit.hit_id=="26810:4:0"));
        assert_eq!(new_trace.damage_events.len(),new_trace.casts.len());
        // The unconditional chain modifier remains after chaos ends.
        assert!(new_trace.damage_events.last().unwrap().time>15.0);
        assert_eq!(new_trace.damage_events.last().unwrap().formula.damage_increase_multiplier,old_trace.damage_events.last().unwrap().formula.damage_increase_multiplier);
        input["character"]["gems"]=json!([{"type":"작열","level":10,"skillId":26810,"skillName":"레이지 스피어","skillEffect":"재사용 대기시간 감소"}]);
        input["character"]["skills"][0]["rune"]=json!({"name":"질풍","grade":"전설","effects":[{"kind":"cast_speed","value":14.0}]});
        let enhanced=builder::build(&input.to_string()).unwrap();
        assert_eq!(enhanced.skills[0].cooldown_ms,12160); assert_eq!(enhanced.skills[0].cast_times_ms,vec![1434]);
        assert_eq!(enhanced.skills[0].hits[0].effect_id,268180);
        input["character"]["skills"][0]["tripods"][0]["level"]=json!(5);
        assert!(builder::build(&input.to_string()).unwrap_err().contains("1레벨"));
    }
}

#[test]
fn reaper_graffiti_last_words_replaces_the_route_before_runes_gems_and_scalar_effects() {
    let mut input = configured(false);
    let mut data = input["skillData"][0].clone();
    data["name"] = json!("라스트 그래피티"); data["gameSkillId"] = json!(26800);
    data["cooldown"] = json!(30); data["castTimesMs"] = json!([1700]);
    data["hits"][0]["effectId"] = json!(268000);
    for hit in data["hits"].as_array_mut().unwrap() {
        for field in ["fixedDamage", "damageRatio"] { hit[field] = json!(vec![1.0;14]); }
    }
    input["skillData"] = json!([data]);
    input["character"]["skills"] = json!([{"name":"라스트 그래피티","level":10,"tripods":[{"name":"유언","level":1,"values":[99999]}]}]);
    input["aplConfig"] = json!("actions:\n - skill: 라스트 그래피티\n");
    let mut base_input = input.clone(); base_input["character"]["skills"][0]["tripods"] = json!([]);
    let baseline = builder::build(&base_input.to_string()).unwrap();
    let selected = builder::build(&input.to_string()).unwrap();
    let skill = &selected.skills[0];
    assert_eq!(skill.cast_times_ms, vec![2033]);
    assert_eq!(skill.hits.len(), 1);
    let hit = &skill.hits[0];
    assert_eq!(hit.effect_id, 268071); assert_eq!(hit.hit_id, "26800:4:0");
    assert_eq!(hit.action_delay_ms, 1267); assert_eq!(hit.fixed_delay_ms, 0);
    assert_eq!(hit.base_damage, 4671.0); assert!((hit.skill_modifier - 25.4971).abs() < 1e-12);
    assert!(((1.0 + hit.damage_increase) / (1.0 + baseline.skills[0].hits[0].damage_increase) - 1.2).abs() < 1e-12);
    assert!((hit.crit_chance_bonus - baseline.skills[0].hits[0].crit_chance_bonus - 0.2).abs() < 1e-12);
    assert_eq!(skill.cooldown_ms, 30000);
    let (_, execution) = trace(&input, 70000);
    assert!(execution.damage_events.iter().all(|hit| hit.hit_id == "26800:4:0"));
    assert_eq!(execution.damage_events.len(), execution.casts.len());
    // Same-tier exclusion remains enforced; neither route nor growth is client supplied.
    input["character"]["skills"][0]["tripods"][0]["level"] = json!(5);
    assert!(builder::build(&input.to_string()).unwrap_err().contains("1레벨"));
    input["character"]["skills"][0]["tripods"][0]["level"] = json!(1);
    input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"급습 강화","level":1}));
    let combined = builder::build(&input.to_string()).unwrap();
    assert_eq!(combined.skills[0].hits.len(), 1);
    assert!(((1.0 + combined.skills[0].hits[0].damage_increase) / (1.0 + baseline.skills[0].hits[0].damage_increase) - 1.2 * 1.65).abs() < 1e-12);
    assert!(combined.skills[0].tags.contains(&crate::profile::SkillTag::ElementDark));
    input["character"]["gems"] = json!([{"type":"작열","level":10,"skillId":26800,"skillName":"라스트 그래피티","skillEffect":"재사용 대기시간 감소"}]);
    input["character"]["skills"][0]["rune"] = json!({"name":"질풍","grade":"전설","effects":[{"kind":"cast_speed","value":14.0}]});
    let enhanced = builder::build(&input.to_string()).unwrap();
    assert_eq!(enhanced.skills[0].cooldown_ms, 22800);
    assert_eq!(enhanced.skills[0].cast_times_ms, vec![1748]);
    assert_eq!(enhanced.skills[0].hits[0].effect_id, 268071);
    assert_eq!(enhanced.skills[0].hits[0].damage_increase, combined.skills[0].hits[0].damage_increase);
}

#[test]
fn reaper_controlled_motion_replaces_fury_repeats_before_preparation_and_chaos_effects() {
    let mut input=configured(true);
    let mut data=input["skillData"].as_array().unwrap().iter().find(|skill| skill["name"]=="레이지 스피어").unwrap().clone();
    data["name"]=json!("댄싱 오브 퓨리"); data["gameSkillId"]=json!(26830);
    for hit in data["hits"].as_array_mut().unwrap() {for field in ["fixedDamage","damageRatio"] {hit[field]=json!(vec![1.0;14]);}}
    input["skillData"]=json!([data]); input["character"]["skills"]=json!([{"name":"댄싱 오브 퓨리","level":10,"tripods":[]}]);
    input["aplConfig"]=json!("actions:\n - skill: 댄싱 오브 퓨리\n");
    let base=builder::build(&input.to_string()).unwrap();
    input["character"]["skills"][0]["tripods"]=json!([{"name":"절제된 동작","level":1}]);
    let selected=builder::build(&input.to_string()).unwrap();
    let skill=&selected.skills[0];
    assert_eq!(skill.cast_times_ms,vec![3200]);
    assert_eq!(skill.hits.iter().map(|hit| (hit.effect_id,hit.action_delay_ms)).collect::<Vec<_>>(),vec![(268380,734),(268380,1067),(268380,1401),(268381,2295)]);
    assert_eq!(skill.hits[0].base_damage,511.0); assert_eq!(skill.hits[0].skill_modifier,2.7951);
    assert_eq!(skill.hits[3].base_damage,2301.0); assert_eq!(skill.hits[3].skill_modifier,12.5776);
    assert!(skill.hits.iter().all(|hit| ((1.0+hit.damage_increase)/(1.0+base.skills[0].hits[0].damage_increase)-1.70).abs()<1e-12));
    input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().extend([json!({"name":"날렵한 움직임","level":1}),json!({"name":"혼돈 강화","level":1})]);
    let combined=builder::build(&input.to_string()).unwrap();
    assert_eq!(combined.skills[0].cast_times_ms,vec![2978]);
    assert_eq!(combined.skills[0].hits.iter().map(|hit| hit.action_delay_ms).collect::<Vec<_>>(),vec![512,845,1179,2073]);
    assert_eq!(combined.skills[0].hits.len(),4);
    let (_,trace)=engine::trace_one(&combined,42,30000);
    let first_cast:Vec<_>=trace.damage_events.iter().filter(|hit| hit.cast_id==Some(1)).collect();
    assert_eq!(first_cast.len(),4);
    assert!(first_cast.iter().all(|hit| (hit.formula.damage_increase_multiplier/(1.06*1.80)-1.0).abs()<1e-12));
    assert!(trace.damage_events.iter().all(|hit| combined.skills[0].hits.iter().any(|raw| raw.hit_id==hit.hit_id)));
}

#[test]
fn reaper_silent_propulsion_replaces_the_route_before_ground_strike_and_preserves_one_hit() {
    let mut input=configured(true);
    let mut data=input["skillData"].as_array().unwrap().iter().find(|skill| skill["name"]=="레이지 스피어").unwrap().clone();
    data["name"]=json!("사일런트 스매셔"); data["gameSkillId"]=json!(26820);
    for hit in data["hits"].as_array_mut().unwrap() {for field in ["fixedDamage","damageRatio"] {hit[field]=json!(vec![1.0;14]);}}
    input["skillData"]=json!([data]); input["character"]["skills"]=json!([{"name":"사일런트 스매셔","level":10,"tripods":[]}]);
    input["aplConfig"]=json!("actions:\n - skill: 사일런트 스매셔\n");
    let base=builder::build(&input.to_string()).unwrap();
    input["character"]["skills"][0]["tripods"]=json!([{"name":"추진력","level":1}]);
    let selected=builder::build(&input.to_string()).unwrap();
    let skill=&selected.skills[0];
    assert_eq!(skill.cast_times_ms,vec![2500]); assert_eq!(skill.hits.len(),1);
    assert_eq!(skill.hits[0].effect_id,268200); assert_eq!(skill.hits[0].hit_id,"26820:4:0");
    assert_eq!(skill.hits[0].action_delay_ms,1500);
    assert_eq!(skill.hits[0].base_damage,3129.0); assert_eq!(skill.hits[0].skill_modifier,17.1017);
    assert!(((1.0+skill.hits[0].damage_increase)/(1.0+base.skills[0].hits[0].damage_increase)-1.60).abs()<1e-12);
    input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"지면 강타","level":1}));
    let combined=builder::build(&input.to_string()).unwrap();
    assert_eq!(combined.skills[0].hits.len(),1); assert_eq!(combined.skills[0].cast_times_ms,skill.cast_times_ms);
    assert!(((1.0+combined.skills[0].hits[0].damage_increase)/(1.0+base.skills[0].hits[0].damage_increase)-1.6*1.6).abs()<1e-12);
    assert!((combined.skills[0].hits[0].crit_chance_bonus-skill.hits[0].crit_chance_bonus-0.20).abs()<1e-12);
    let (_,events)=engine::trace_one(&combined,42,30000);
    assert!(!events.damage_events.is_empty());
    assert!(events.damage_events.iter().all(|hit| hit.hit_id=="26820:4:0"));
}

#[test]
fn reaper_rage_ambush_applies_only_to_back_attack_profiles_and_combines_with_chaos() {
    for back in [false,true] {
        let mut input=configured(true);
        let mut data=input["skillData"].as_array().unwrap().iter().find(|skill| skill["name"]=="레이지 스피어").unwrap().clone();
        data["properties"]["attackType"]=json!(if back {"백 어택"} else {""});
        for hit in data["hits"].as_array_mut().unwrap() {for field in ["fixedDamage","damageRatio"] {hit[field]=json!(vec![1.0;14]);}}
        input["skillData"]=json!([data]); input["character"]["skills"]=json!([{"name":"레이지 스피어","level":10,"tripods":[]}]);
        input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");
        let base=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"]=json!([{"name":"기습","level":1,"values":[999999]},{"name":"순살","level":1}]);
        let selected=builder::build(&input.to_string()).unwrap();
        assert_eq!(selected.skills[0].hits.len(),base.skills[0].hits.len());
        assert_eq!(selected.skills[0].cast_times_ms,base.skills[0].cast_times_ms);
        for (hit,old) in selected.skills[0].hits.iter().zip(&base.skills[0].hits) {
            assert!(((1.0+hit.damage_increase)/(1.0+old.damage_increase)-if back {1.7} else {1.0}).abs()<1e-12);
            assert_eq!(hit.effect_id,old.effect_id); assert_eq!(hit.action_delay_ms,old.action_delay_ms);
        }
        let (_,old_trace)=engine::trace_one(&base,42,10000);
        let (_,new_trace)=engine::trace_one(&selected,42,10000);
        assert!(!new_trace.damage_events.is_empty());
        for (hit,old) in new_trace.damage_events.iter().zip(&old_trace.damage_events) {
            assert!((hit.formula.damage_increase_multiplier/old.formula.damage_increase_multiplier-1.5).abs()<1e-12);
            assert!((hit.formula.skill_damage_multiplier/old.formula.skill_damage_multiplier-if back {1.7} else {1.0}).abs()<1e-12);
        }
    }
}

#[test]
fn reaper_ground_strike_keeps_one_hit_and_applies_dark_crit_and_damage_after_raw_visual_audit() {
    let mut input=configured(true);
    let mut data=input["skillData"].as_array().unwrap().iter().find(|skill| skill["name"]=="레이지 스피어").unwrap().clone();
    data["name"]=json!("사일런트 스매셔"); data["gameSkillId"]=json!(26820);
    data["hits"]=json!([data["hits"][0].clone()]); data["hits"][0]["effectId"]=json!(268200); data["hits"][0]["hitId"]=json!("26820:1:0");
    for hit in data["hits"].as_array_mut().unwrap() {
        for field in ["fixedDamage","damageRatio"] {hit[field]=json!(vec![hit[field][0].as_f64().unwrap();14]);}
    }
    input["skillData"]=json!([data]);
    input["character"]["skills"]=json!([{"name":"사일런트 스매셔","level":10,"tripods":[]}]);
    input["aplConfig"]=json!("actions:\n - skill: 사일런트 스매셔\n");
    let base=builder::build(&input.to_string()).unwrap();
    input["character"]["skills"][0]["tripods"]=json!([{"name":"지면 강타","level":1,"values":[99999]}]);
    let selected=builder::build(&input.to_string()).unwrap();
    let old=base.skills.iter().find(|skill| skill.name=="사일런트 스매셔").unwrap();
    let new=selected.skills.iter().find(|skill| skill.name=="사일런트 스매셔").unwrap();
    assert_eq!(new.hits.len(),1); assert_eq!(new.hits[0].effect_id,268200);
    assert_eq!(new.cast_times_ms,old.cast_times_ms); assert_eq!(new.cooldown_ms,old.cooldown_ms);
    assert_eq!(new.hits[0].hit_id,old.hits[0].hit_id); assert_eq!(new.hits[0].action_delay_ms,old.hits[0].action_delay_ms);
    assert!((new.hits[0].crit_chance_bonus-old.hits[0].crit_chance_bonus-0.20).abs()<1e-12);
    assert!(((1.0+new.hits[0].damage_increase)/(1.0+old.hits[0].damage_increase)-1.60).abs()<1e-12);
    assert!(new.tags.contains(&crate::profile::SkillTag::ElementDark));
    let (_,events)=engine::trace_one(&selected,42,30000);
    assert!(events.damage_events.iter().any(|hit| hit.skill_name=="사일런트 스매셔"));
    assert!(events.damage_events.iter().filter(|hit| hit.skill_name=="사일런트 스매셔").all(|hit| hit.hit_id==new.hits[0].hit_id));
}

#[test]
fn reaper_chaos_tripods_use_the_matching_skill_and_current_state_without_static_damage() {
    use crate::engine::state::class::reaper::ReaperState;
    for (id,name,choice,bonus) in [(26800,"라스트 그래피티","혼돈 강화",0.70),(26810,"레이지 스피어","순살",0.50),(26830,"댄싱 오브 퓨리","혼돈 강화",0.80)] {
        let mut input = configured(true);
        let mut data = input["skillData"].as_array().unwrap().iter().find(|data| data["name"] == "레이지 스피어").unwrap().clone();
        data["name"] = json!(name); data["gameSkillId"] = json!(id); data["cooldown"] = json!(30);
        for hit in data["hits"].as_array_mut().unwrap() { for field in ["fixedDamage","damageRatio"] { hit[field]=json!(vec![1.0;14]); } }
        input["skillData"] = json!([data]);
        input["character"]["skills"] = json!([{"name":name,"level":10,"tripods":[]}]);
        input["aplConfig"] = json!(format!("actions:\n - skill: {name}\n"));
        let base = builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"] = json!([{"name":choice,"level":1,"values":[999999]}]);
        let selected = builder::build(&input.to_string()).unwrap();
        let reference = selected.reaper_reference.as_ref().unwrap();
        assert_eq!(reference.metadata["chaos_tripod_damage"][id.to_string()],json!(bonus));
        assert_eq!(base.skills[0].hits.iter().map(|hit| hit.damage_increase).collect::<Vec<_>>(),selected.skills[0].hits.iter().map(|hit| hit.damage_increase).collect::<Vec<_>>());
        assert_eq!(selected.skills[0].cooldown_ms,base.skills[0].cooldown_ms-if id==26810 {9000} else {0});
        let mut state = ReaperState::from_reference(0.0,reference);
        let mut baseline = ReaperState::from_reference(0.0,base.reaper_reference.as_ref().unwrap());
        for now in [0,14999,15000,20000] {
            state.advance(now); baseline.advance(now);
            let ratio = state.boost(id,false).damage/baseline.boost(id,false).damage;
            assert!((ratio-if now<15000 {1.0+bonus} else {1.0}).abs()<1e-12);
            assert_eq!(state.boost(26070,false).damage,baseline.boost(26070,false).damage);
        }
        // The condition also follows explicit Moon identity state, not the build label.
        state.scenario = crate::profile::reaper::ReaperScenario::DarkShadowDotLegacy;
        state.chaos_until = state.now+1000; baseline.chaos_until = baseline.now+1000;
        assert!((state.boost(id,false).damage/baseline.boost(id,false).damage-(1.0+bonus)).abs()<1e-12);
        let (_,old_trace) = engine::trace_one(&base,42,10000);
        let (_,new_trace) = engine::trace_one(&selected,42,10000);
        assert!(!new_trace.damage_events.is_empty());
        for (hit,old) in new_trace.damage_events.iter().zip(&old_trace.damage_events) {
            assert!((hit.formula.damage_increase_multiplier/old.formula.damage_increase_multiplier-(1.0+bonus)).abs()<1e-12);
        }
        // A cast keeps its bonus even if chaos expires before the last hit.
        input["reaperIdentityModel"] = json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0.5,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{}});
        let (_,selected_trace) = trace(&input,10000);
        input["character"]["skills"][0]["tripods"] = json!([]);
        let (_,base_trace) = trace(&input,10000);
        assert!(selected_trace.damage_events.last().unwrap().time>0.5);
        for (hit,old) in selected_trace.damage_events.iter().zip(&base_trace.damage_events) {
            assert!((hit.formula.damage_increase_multiplier/old.formula.damage_increase_multiplier-(1.0+bonus)).abs()<1e-12);
        }
    }
}

#[test]
fn reaper_death_loop_applies_its_tripod_bonus_only_with_last_words_at_the_raw_threshold() {
    let mut input = configured(false);
    let mut data = input["skillData"][0].clone();
    data["name"] = json!("라스트 그래피티"); data["gameSkillId"] = json!(26800); data["cooldown"] = json!(30);
    for hit in data["hits"].as_array_mut().unwrap() { for field in ["fixedDamage","damageRatio"] {hit[field]=json!(vec![1.0;14]);} }
    input["skillData"] = json!([data]);
    input["character"]["skills"] = json!([{"name":"라스트 그래피티","level":10,"tripods":[]}]);
    input["aplConfig"] = json!("actions:\n - skill: 라스트 그래피티\n");
    for (tier, grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {
        for points in [9,10,14,16,17,18,19,20].into_iter().filter(|&points| tier > 0 || points <= 15) {
            for with_tripod in [false,true] {
                input["character"]["arkGrids"] = Value::Null;
                input["character"]["skills"][0]["tripods"] = if with_tripod {json!([{"name":"유언","level":1}])} else {json!([])};
                let baseline = builder::build(&input.to_string()).unwrap();
                input["character"]["arkGrids"] = json!({"reaperSlots":[grid_slot("데스 루프",grade,points)]});
                let selected = builder::build(&input.to_string()).unwrap();
                let mut expected = if points >= 10 {1.06} else {1.0};
                if with_tripod && points >= 17 && tier >= 2 {expected *= if tier == 2 {1.10} else {1.14};}
                if tier >= 2 && points >= 18 {expected *= 1.0017_f64.powi((points - 17) as i32);}
                for (hit,old) in selected.skills[0].hits.iter().zip(&baseline.skills[0].hits) {
                    assert!(((1.0+hit.damage_increase)/(1.0+old.damage_increase)-expected).abs()<1e-12,"{grade}/{points}/{with_tripod}");
                    assert_eq!(hit.effect_id,old.effect_id); assert_eq!(hit.action_delay_ms,old.action_delay_ms);
                }
                assert_eq!(selected.skills[0].cooldown_ms,baseline.skills[0].cooldown_ms);
            }
        }
    }
    // A Thirst build cannot activate the Moon core without its required enlightenment node.
    input["reaperBuild"] = json!("thirst_chaos_sustain");
    assert!(builder::build(&input.to_string()).unwrap_err().contains("요구 깨달음"));
}

#[test]
fn reaper_fury_preparation_speed_preserves_loop_intervals_last_attack_and_damage() {
    let mut input=configured(false);
    let mut data=input["skillData"][0].clone();
    data["name"]=json!("댄싱 오브 퓨리"); data["gameSkillId"]=json!(26830);
    data["cooldown"]=json!(25); data["castTimesMs"]=json!([3533]);
    let mut hits=vec![];
    for (i,time) in [770,1103,1436,1770,2628].into_iter().enumerate() {
        let mut hit=data["hits"][0].clone(); hit["hitId"]=json!(format!("fury:{i}"));
        hit["effectId"]=json!(if i==4 {268301} else {268300});
        hit["actionDelayMs"]=json!(time);
        for field in ["fixedDamage","damageRatio"] {hit[field]=json!(vec![hit[field][0].as_f64().unwrap();14]);}
        hits.push(hit);
    }
    data["hits"]=json!(hits); input["skillData"]=json!([data]);
    input["character"]["skills"]=json!([{"name":"댄싱 오브 퓨리","level":10,"tripods":[]}]);
    input["aplConfig"]=json!("actions:\n - skill: 댄싱 오브 퓨리\n");
    let baseline=builder::build(&input.to_string()).unwrap();
    input["character"]["skills"][0]["tripods"]=json!([{"name":"날렵한 움직임","level":1,"values":[99999]}]);
    let selected=builder::build(&input.to_string()).unwrap();
    assert_eq!(selected.skills[0].cast_times_ms,vec![3311]);
    assert_eq!(selected.skills[0].cooldown_ms,baseline.skills[0].cooldown_ms);
    assert_eq!(selected.skills[0].hits.iter().map(|h|h.action_delay_ms).collect::<Vec<_>>(),vec![548,881,1214,1548,2406]);
    for (hit,old) in selected.skills[0].hits.iter().zip(&baseline.skills[0].hits) {
        assert_eq!(hit.hit_id,old.hit_id); assert_eq!(hit.effect_id,old.effect_id);
        assert_eq!(hit.fixed_delay_ms,old.fixed_delay_ms);
        assert_eq!(hit.damage_increase,old.damage_increase);
        assert_eq!(hit.base_damage,old.base_damage); assert_eq!(hit.skill_modifier,old.skill_modifier);
    }
    let (before,old_trace)=engine::trace_one(&baseline,42,4000);
    let (after,new_trace)=engine::trace_one(&selected,42,4000);
    assert_eq!(old_trace.damage_events.len(),5); assert_eq!(new_trace.damage_events.len(),5);
    assert_eq!(before.total_damage,after.total_damage);
    assert!(new_trace.damage_events[0].time<old_trace.damage_events[0].time);
}

#[test]
fn reaper_charge_tripods_gate_usage_and_recover_serially_without_resetting_on_second_use() {
    for (id, name, tripod, cooldown, recovery) in [(26050,"스피닝 대거","끝없는 춤",12,12000),
        (26550,"콜 오브 나이프","절망의 부름",20,20000), (26570,"쉐도우 트랩","연속 공격",14,20000)] {
        let mut input = configured(false);
        let mut data = input["skillData"][0].clone();
        data["name"] = json!(name); data["gameSkillId"] = json!(id); data["cooldown"] = json!(cooldown);
        data["castTimesMs"] = json!([0]);
        for hit in data["hits"].as_array_mut().unwrap() {
            hit["actionDelayMs"] = json!(0);
            for field in ["fixedDamage","damageRatio"] { hit[field] = json!(vec![hit[field][0].as_f64().unwrap();14]); }
        }
        input["skillData"] = json!([data]);
        input["character"]["skills"] = json!([{"name":name,"level":10,"tripods":[{"name":tripod,"level":1,"values":[99999]}]}]);
        input["character"]["gems"] = json!([]);
        input["aplConfig"] = json!(format!("actions:\n - skill: {name}\n"));
        let mut profiles = builder::build(&input.to_string()).unwrap();
        assert_eq!(profiles.skills[0].max_stacks,2);
        assert_eq!(profiles.skills[0].cooldown_ms,1000);
        assert_eq!(profiles.skills[0].charge_recovery_ms,recovery);
        profiles.stat.swiftness=0.0;
        profiles.skills[0].cast_times_ms=vec![0];
        let (_,trace)=engine::trace_one(&profiles,42,recovery*3+1);
        let times:Vec<_>=trace.casts.iter().map(|cast|(cast.time*1000.0).round() as u64).collect();
        assert_eq!(times[..5],[0,1000,recovery as u64,2*recovery as u64,3*recovery as u64]);
        let (_,again)=engine::trace_one(&profiles,42,recovery*3+1);
        assert_eq!(again.casts.iter().map(|cast|(cast.time*1000.0).round() as u64).collect::<Vec<_>>(),times);
        input["character"]["gems"] = json!([{"type":"작열","level":10,"skillId":id,"skillName":name,"skillEffect":"재사용 대기시간 감소"}]);
        let mut gemmed=builder::build(&input.to_string()).unwrap();
        assert_eq!(gemmed.skills[0].cooldown_ms,760);
        assert_eq!(gemmed.skills[0].charge_recovery_ms,(recovery as f64*0.76) as u32);
        gemmed.stat.swiftness=1000.0;
        gemmed.skills[0].cast_times_ms=vec![0];
        let rate=1.0-1000.0*crate::constants::SWIFTNESS_COOLDOWN_REDUCTION;
        let interval=(recovery as f64*0.76*rate) as u64;
        let (_,trace)=engine::trace_one(&gemmed,42,recovery*3);
        let times:Vec<_>=trace.casts.iter().map(|cast|(cast.time*1000.0).round() as u64).collect();
        assert_eq!(times[..4],[0,(760.0*rate) as u64,interval,2*interval]);
    }
}

#[test]
fn reaper_grid_checks_all_grades_thresholds_skill_targets_and_fatal_base_cooldown() {
    for name in ["할루시네이션", "급습 악몽", "치명적 연계"] {
        let mut input = configured(name != "할루시네이션");
        let skills = [(26800,"라스트 그래피티"),(26810,"레이지 스피어"),(26820,"사일런트 스매셔"),(26830,"댄싱 오브 퓨리"),(26970,"피니쉬 스텝"),
            (26070,"쉐도우 닷"),(26510,"디스토션"),(26900,"월식 : 카덴차"),(26940,"카덴차 델 루나")];
        let mut data = vec![]; let mut chosen = vec![];
        for (id, skill_name) in skills.into_iter().chain((name != "할루시네이션").then_some((26980,"페이탈 스텝"))) {
            let mut row = input["skillData"][2].clone();
            row["name"] = json!(skill_name); row["gameSkillId"] = json!(id); row["cooldown"] = json!(30);
            row["skillSlot"] = json!(match id {26900 => "Awakening", 26940 => "SuperAwakening", 26970 => "HyperAwakeningTechniques", _ => "Normal"});
            for (index, hit) in row["hits"].as_array_mut().unwrap().iter_mut().enumerate() {hit["hitId"] = json!(format!("{id}:{index}"));}
            data.push(row); chosen.push(json!({"name":skill_name,"level":1,"tripods":[]}));
        }
        input["skillData"] = json!(data); input["character"]["skills"] = json!(chosen);
        input["aplConfig"] = json!("actions:\n - skill: 레이지 스피어\n");
        let baseline = builder::build(&input.to_string()).unwrap();
        for (grade, tier) in [("hero",0),("legend",1),("relic",2),("ancient",3)] {
            for points in [9,10,13,14,16,17,18,19,20] {
                if grade == "hero" && points > 15 { continue; } // Four minimum-cost gems exceed its nine willpower budget.
                input["character"]["arkGrids"] = json!({"reaperSlots":[grid_slot(name, grade, points)]});
                let profile = builder::build(&input.to_string()).unwrap();
                for skill in &profile.skills {
                    let old = baseline.skills.iter().find(|old| old.name == skill.name).unwrap();
                    let id = skill.tags.iter().find_map(|tag| if let crate::profile::SkillTag::SkillId(id) = tag {Some(*id)} else {None}).unwrap();
                    let mut expected: f64 = 1.0;
                    if points >= 10 {
                        let target = if name == "할루시네이션" {26830} else if name == "급습 악몽" {26800} else {26810};
                        if id == target { expected *= if target == 26800 {1.07} else {1.06}; }
                    }
                    if points >= 14 && tier >= 1 {
                        let target = if name == "치명적 연계" {26800} else {26830};
                        if id == target {expected *= if name == "할루시네이션" {1.09} else if target == 26800 {1.07} else {1.12};}
                    }
                    if points >= 17 && tier >= 2 {
                        if name == "할루시네이션" {
                            expected *= if tier == 2 {1.01} else {1.015};
                            if id == 26810 {expected *= if tier == 2 {1.025} else {1.05};}
                        }
                        if name == "급습 악몽" && id == 26810 {expected *= if tier == 2 {1.12} else {1.18};}
                        if name == "치명적 연계" && id == 26980 {
                            assert_eq!(skill.cooldown_ms, (30000.0 * if tier == 2 {0.92} else {0.88}) as u32);
                        }
                        if [26800,26810,26820,26830,26970].contains(&id) && points >= 18 {
                            expected *= (if name == "할루시네이션" {1.0017_f64} else {1.003_f64}).powi((points - 17) as i32);
                        }
                    }
                    for (hit, old_hit) in skill.hits.iter().zip(&old.hits) {
                        assert!(((1.0 + hit.damage_increase) / (1.0 + old_hit.damage_increase) - expected).abs() < 1e-12, "{name}/{grade}/{points}/{}",skill.name);
                    }
                    if name != "치명적 연계" || id != 26980 || points < 17 || tier < 2 {assert_eq!(skill.cooldown_ms, old.cooldown_ms);}
                }
            }
        }
    }
}

#[test]
fn reaper_chaos_stats_accumulate_thresholds_and_allow_an_order_star() {
    for name in ["공격", "무기", "속도"] {
        let mut input = configured(false);
        let baseline = builder::build(&input.to_string()).unwrap();
        for (tier, grade) in ["hero", "legend", "relic", "ancient"].into_iter().enumerate() {
            for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&points| tier > 0 || points <= 15) {
                let mut slot = grid_slot(name, grade, points);
                slot["coreAlignment"] = json!("혼돈");
                for gem in slot["gems"].as_array_mut().unwrap() {gem["name"] = json!("침식");}
                input["character"]["arkGrids"] = json!({"reaperSlots":[slot]});
                let selected = builder::build(&input.to_string()).unwrap();
                let ten = if points >= 10 {1.0} else {0.0};
                let fourteen = if points >= 14 && tier >= 1 {1.0} else {0.0};
                let seventeen = if points >= 17 && tier >= 2 {if tier == 2 {2.0} else {3.0}} else {0.0};
                let tail = if tier >= 2 {points.saturating_sub(17) as f64} else {0.0};
                let delta = |field| selected.stat.sum(field) - baseline.stat.sum(field);
                if name == "공격" {
                    assert_eq!(selected.stat.base_attack_power - baseline.stat.base_attack_power, 900.0*(ten+seventeen));
                    assert!((delta(StatField::AttackPowerMul) - (55.0*(fourteen+seventeen)+16.0*tail)/10000.0).abs()<1e-12);
                } else if name == "무기" {
                    assert_eq!(delta(StatField::WeaponAttackPower), 1300.0*(ten+seventeen));
                    assert!((selected.stat.weapon_ap_mul-baseline.stat.weapon_ap_mul-(75.0*(fourteen+seventeen)+23.0*tail)/10000.0).abs()<1e-12);
                } else {
                    assert!((delta(StatField::AttackSpeed)-(90.0*(ten+seventeen)+30.0*tail)/10000.0).abs()<1e-12);
                    assert!((delta(StatField::MovementSpeed)-(90.0*(fourteen+seventeen)+30.0*tail)/10000.0).abs()<1e-12);
                }
            }
        }
    }
    let mut input = configured(false);
    let baseline = builder::build(&input.to_string()).unwrap();
    let mut order = grid_slot("할루시네이션", "ancient", 20);
    let mut chaos = grid_slot("공격", "ancient", 20); chaos["coreAlignment"] = json!("혼돈");
    for slot in [&mut order, &mut chaos] {
        let name = if slot["coreAlignment"] == "혼돈" {"침식"} else {"안정"};
        for gem in slot["gems"].as_array_mut().unwrap() {gem["name"] = json!(name); gem["effects"] = json!([{"name":"공격력","level":5},{"name":"추가 피해","level":5}]);}
    }
    input["character"]["arkGrids"] = json!({"reaperSlots":[order,chaos]});
    let profile = builder::build(&input.to_string()).unwrap();
    assert!((profile.stat.sum(StatField::AttackPowerMul)-baseline.stat.sum(StatField::AttackPowerMul)-0.0268-0.0146).abs()<1e-12);
    assert!((profile.stat.sum(StatField::AdditionalDamage)-baseline.stat.sum(StatField::AdditionalDamage)-0.0323).abs()<1e-12); // raw Lv40=323, not two Lv20=322.
    input["character"]["arkGrids"]["reaperSlots"][1]["gems"][0]["name"] = json!("안정");
    assert!(builder::build(&input.to_string()).unwrap_err().contains("젬 이름/종류"));
}

#[test]
fn reaper_grid_rejects_invalid_loadouts_and_aggregates_trusted_gem_levels() {
    let mut input = configured(false);
    let baseline = builder::build(&input.to_string()).unwrap();
    let mut slot = grid_slot("할루시네이션", "ancient", 20);
    for gem in slot["gems"].as_array_mut().unwrap() {gem["effects"] = json!([{"name":"공격력","level":5},{"name":"추가 피해","level":5}]);}
    input["character"]["arkGrids"] = json!({"reaperSlots":[slot.clone()]});
    let selected = builder::build(&input.to_string()).unwrap();
    assert!((selected.stat.attack_power_mul.values().sum::<f64>() - baseline.stat.attack_power_mul.values().sum::<f64>() - 0.0073).abs() < 1e-12); // Aggregate Lv20=73, not four Lv5=72.
    assert!((selected.stat.additional_damage.values().sum::<f64>() - baseline.stat.additional_damage.values().sum::<f64>() - 0.0161).abs() < 1e-12); // Aggregate Lv20 raw161.
    let original = input.clone();
    for (path, value, message) in [
        (vec!["character","arkGrids","reaperSlots","0","name"],json!("달의 향기"),"미지원"),
        (vec!["character","arkGrids","reaperSlots","0","grade"],json!("hero"),"예산"),
        (vec!["character","arkGrids","reaperSlots","0","coreSymbol"],json!("해"),"미지원"),
        (vec!["character","arkGrids","reaperSlots","0","gems","0","gemPoint"],json!(6),"0~5"),
        (vec!["character","arkGrids","reaperSlots","0","gems","0","willpowerEfficiency"],json!(6),"0~5"),
        (vec!["character","arkGrids","reaperSlots","0","gems","0","effects","0","name"],json!("보스 피해"),"효과"),
        (vec!["character","arkGrids","reaperSlots","0","gems","0","effects","0","level"],json!(6),"효과"),
        (vec!["character","arkGrids","reaperSlots","0","gems","0","effects","1","name"],json!("공격력"),"중복"),
        (vec!["character","arkGrids","commonCores"],json!([{"name":"위조","points":20,"options":[]}]),"외부 효과"),
    ] {
        let mut changed = original.clone(); let mut at = &mut changed;
        for key in path {at = if let Ok(index) = key.parse::<usize>() {&mut at[index]} else {&mut at[key]};}
        *at = value; assert!(builder::build(&changed.to_string()).unwrap_err().contains(message),"{message}");
    }
    input["character"]["arkGrids"]["reaperSlots"].as_array_mut().unwrap().push(slot);
    assert!(builder::build(&input.to_string()).unwrap_err().contains("문양"));
    input = original.clone(); input["reaperBuild"] = json!("thirst_chaos_sustain");
    assert!(builder::build(&input.to_string()).unwrap_err().contains("깨달음"));
    input = original;
    for gem in input["character"]["arkGrids"]["reaperSlots"][0]["gems"].as_array_mut().unwrap() {
        gem["name"] = json!("불변"); gem["effects"] = json!([{"name":"보스 피해","level":5}]);
    }
    input["character"]["arkGrids"]["reaperSlots"][0]["gems"].as_array_mut().unwrap().truncate(3);
    let boss = builder::build(&input.to_string()).unwrap();
    assert!((boss.stat.damage_increase.values().sum::<f64>() - baseline.stat.damage_increase.values().sum::<f64>() - 0.0125).abs() < 1e-12); // Boss Lv15 raw125.
}

#[test]
fn reaper_tripods_use_trusted_values_targets_and_base_cooldown_before_gems() {
    for (id, name, tripod, kind, value, target) in [
        (26070, "쉐도우 닷", "빠른 준비", "cooldown", -3000.0, 0),
        (26510, "디스토션", "빠른 준비", "cooldown", -4000.0, 0),
        (26820, "사일런트 스매셔", "빠른 준비", "cooldown", -12000.0, 0),
        (26510, "디스토션", "치명적인 그림자", "crit", 0.40, 0),
        (26570, "쉐도우 트랩", "치명적인 그림자", "crit", 0.40, 0),
        (26600, "쉐도우 스톰", "난무 강화", "crit_damage", 1.20, 0),
        (26550, "콜 오브 나이프", "성장하는 어둠", "damage", 1.20, 265422),
        (26600, "쉐도우 스톰", "마침표", "damage", 1.50, 266002),
        (26080, "이블리스토", "날카로운 단검", "crit_damage", 0.80, 0),
        (26110, "샤벨 스팅거", "두개의 그림자", "damage", 0.60, 0),
        (26810, "레이지 스피어", "치명적인 단검", "crit", 0.30, 0),
        (26810, "레이지 스피어", "치명적인 단검", "damage", 0.60, 0),
        (26070, "쉐도우 닷", "치명적인 단검", "crit", 0.60, 0),
        (26110, "샤벨 스팅거", "날카로운 단검", "crit_damage", 1.00, 0),
        (26600, "쉐도우 스톰", "치명적인 그림자", "crit", 0.40, 0),
        (26800, "라스트 그래피티", "급습 강화", "damage", 0.65, 0),
        (26830, "댄싱 오브 퓨리", "치명적인 단검", "crit", 0.30, 0),
        (26830, "댄싱 오브 퓨리", "치명적인 단검", "damage", 0.35, 0),
        (26600, "쉐도우 스톰", "꿰뚫는 일격", "defense", 0.62, 0),
        (26520, "블링크", "꿰뚫는 일격", "defense", 0.62, 0),
    ] {
        let mut input = configured(true);
        let mut data = input["skillData"][2].clone();
        data["name"] = json!(name); data["gameSkillId"] = json!(id); data["cooldown"] = json!(30);
        data["hits"][0]["effectId"] = json!(target);
        let mut other = data["hits"][0].clone(); other["effectId"] = json!(99999); other["hitId"] = json!("other");
        data["hits"].as_array_mut().unwrap().push(other);
        for hit in data["hits"].as_array_mut().unwrap() {
            for field in ["fixedDamage", "damageRatio"] {
                hit[field] = json!(vec![hit[field][0].as_f64().unwrap(); 14]);
            }
        }
        input["skillData"] = json!([data]);
        input["character"]["skills"] = json!([{"name":name,"level":10,"tripods":[]}]);
        input["character"]["gems"] = json!([{"type":"작열","level":10,"skillId":id,"skillName":name,"skillEffect":"재사용 대기시간 감소"}]);
        input["aplConfig"] = json!(format!("actions:\n - skill: {name}\n"));
        let baseline = builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"] = json!([{"name":tripod,"level":1,"values":[999999]}]);
        let profile = builder::build(&input.to_string()).unwrap();
        let skill = &profile.skills[0]; let base = &baseline.skills[0];
        match kind {
            "cooldown" => assert_eq!(skill.cooldown_ms, ((30000.0 + value) * 0.76) as u32),
            "crit" => assert!(skill.hits.iter().zip(&base.hits).all(|(hit, old)| (hit.crit_chance_bonus - old.crit_chance_bonus - value).abs() < 1e-12)),
            "crit_damage" => assert!(skill.hits.iter().zip(&base.hits).all(|(hit, old)| (hit.crit_damage_bonus - old.crit_damage_bonus - value).abs() < 1e-12)),
            "defense" => {
                assert!(skill.hits.iter().all(|hit| hit.defense_ignore == value && hit.defense_ignore_chance == 1.0));
                assert!(skill.hits.iter().zip(&base.hits).all(|(hit, old)| hit.damage_increase == old.damage_increase && hit.crit_chance_bonus == old.crit_chance_bonus));
            }
            "damage" => {
                assert!(((1.0 + skill.hits[0].damage_increase) / (1.0 + base.hits[0].damage_increase) - 1.0 - value).abs() < 1e-12);
                if target == 0 {
                    assert!(((1.0 + skill.hits[1].damage_increase) / (1.0 + base.hits[1].damage_increase) - 1.0 - value).abs() < 1e-12);
                } else { assert_eq!(skill.hits[1].damage_increase, base.hits[1].damage_increase); }
            }, _ => unreachable!(),
        }
        assert_eq!(skill.hits.len(), base.hits.len());
        assert_eq!(skill.cast_times_ms, base.cast_times_ms);
        if id==26080 {
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"독 : 출혈","level":1}));
            let reason="독 발동 타격";
            assert!(builder::build(&input.to_string()).unwrap_err().contains(reason));
            input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().pop();
        }
        input["character"]["skills"][0]["tripods"][0]["level"] = json!(5);
        assert!(builder::build(&input.to_string()).unwrap_err().contains("1레벨"));
        input["character"]["skills"][0]["tripods"][0]["level"] = json!(1);
        input["character"]["skills"][0]["level"] = json!(3);
        assert!(builder::build(&input.to_string()).unwrap_err().contains("레벨 필요"));
        input["character"]["skills"][0]["level"] = json!(10);
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":tripod,"level":1}));
        assert!(builder::build(&input.to_string()).unwrap_err().contains("중복"));
    }
}

#[test]
fn reaper_element_tripods_replace_old_tags_without_changing_timing() {
    use crate::profile::SkillTag;
    for (id, name, tripod, element) in [
        (26070,"쉐도우 닷","치명적인 단검",SkillTag::ElementElectricity),
        (26110,"샤벨 스팅거","날카로운 단검",SkillTag::ElementElectricity),
        (26600,"쉐도우 스톰","치명적인 그림자",SkillTag::ElementElectricity),
        (26800,"라스트 그래피티","급습 강화",SkillTag::ElementDark),
        (26830,"댄싱 오브 퓨리","치명적인 단검",SkillTag::ElementElectricity),
    ] {
        let mut input=configured(true);
        let mut data=input["skillData"][2].clone();
        data["name"]=json!(name); data["gameSkillId"]=json!(id); data["properties"]["element"]=json!("성");
        for hit in data["hits"].as_array_mut().unwrap() {
            for field in ["fixedDamage","damageRatio"] { hit[field]=json!(vec![hit[field][0].as_f64().unwrap();14]); }
        }
        input["skillData"]=json!([data]); input["character"]["skills"]=json!([{"name":name,"level":10,"tripods":[]}]);
        input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n"));
        let baseline=builder::build(&input.to_string()).unwrap();
        assert!(baseline.skills[0].tags.contains(&SkillTag::ElementHoly));
        input["character"]["skills"][0]["tripods"]=json!([{"name":tripod,"level":1,"values":[99999]}]);
        let profile=builder::build(&input.to_string()).unwrap();
        assert!(profile.skills[0].tags.contains(&element));
        assert!(!profile.skills[0].tags.contains(&SkillTag::ElementHoly));
        assert_eq!(profile.skills[0].cast_times_ms,baseline.skills[0].cast_times_ms);
        assert_eq!(profile.skills[0].cooldown_ms,baseline.skills[0].cooldown_ms);
    }
}

#[test]
fn reaper_storm_armor_ignore_uses_remaining_defense_in_each_hit_formula() {
    use crate::profile::SkillTag;
    let mut input=configured(true);
    let mut data=input["skillData"][2].clone();
    data["name"]=json!("쉐도우 스톰"); data["gameSkillId"]=json!(26600);
    for hit in data["hits"].as_array_mut().unwrap() {
        for field in ["fixedDamage","damageRatio"] { hit[field]=json!(vec![hit[field][0].as_f64().unwrap();14]); }
    }
    input["skillData"]=json!([data]);
    input["character"]["skills"]=json!([{"name":"쉐도우 스톰","level":10,"tripods":[]}]);
    input["aplConfig"]=json!("actions:\n - skill: 쉐도우 스톰\n");
    for defense in [0.0,1606.0,6500.0] {
        input["targetDefense"]=json!(defense);
        input["character"]["skills"][0]["tripods"]=json!([]);
        let (before,old)=trace(&input,3000);
        input["character"]["skills"][0]["tripods"]=json!([{"name":"꿰뚫는 일격","level":1,"values":[99999]}]);
        let profile=builder::build(&input.to_string()).unwrap();
        assert!(profile.skills[0].tags.contains(&SkillTag::ElementDark));
        let (after,new)=engine::trace_one(&profile,42,3000);
        assert_eq!(new.damage_events.len(),old.damage_events.len());
        let old_factor=6500.0/(6500.0+defense);
        let new_factor=6500.0/(6500.0+defense*0.38);
        assert!(new.damage_events.iter().all(|hit| (hit.formula.defense_multiplier-new_factor).abs()<1e-12));
        assert!((after.total_damage/before.total_damage-new_factor/old_factor).abs()<1e-12);
        assert_eq!(new.casts.len(),old.casts.len());
    }
}

#[test]
fn reaper_active_cooldown_tripods_fire_once_on_cast_for_the_raw_group() {
    for (id,name,choice,target_id,target_name,amount) in [
        (26070,"쉐도우 닷","급습 활성",26810,"레이지 스피어",1900),
        (26570,"쉐도우 트랩","그림자 활성",26510,"디스토션",2400),
    ] {
        let mut input=configured(true);
        let make=|id,name,cd| {
            let mut data=input["skillData"][0].clone(); data["name"]=json!(name); data["gameSkillId"]=json!(id);
            data["cooldown"]=json!(cd); data["castTimesMs"]=json!([1000]);
            for (index,hit) in data["hits"].as_array_mut().unwrap().iter_mut().enumerate() {
                hit["hitId"]=json!(format!("{id}:{index}"));
                for field in ["fixedDamage","damageRatio"] { hit[field]=json!(vec![hit[field][0].as_f64().unwrap();14]); }
            }
            data
        };
        input["skillData"]=json!([make(target_id,target_name,10),make(26080,"이블리스토",10),make(id,name,100)]);
        input["character"]["skills"]=json!([{"name":target_name,"level":10,"tripods":[]},{"name":"이블리스토","level":10,"tripods":[]},{"name":name,"level":10,"tripods":[]}]);
        input["aplConfig"]=json!(format!("actions:\n - skill: {target_name}\n - skill: 이블리스토\n - skill: {name}\n"));
        input["reaperIdentityModel"]=identity_model();
        input["reaperIdentityModel"]["skillGains"]=json!({});
        let (_,old)=trace(&input,14000);
        input["character"]["skills"][2]["tripods"]=json!([{"name":choice,"level":1,"values":[99999]}]);
        let (_,new)=trace(&input,14000);
        let casts=|trace:&crate::constants::TraceData,skill:&str| trace.casts.iter().filter(|cast| cast.skill_name==skill).map(|cast| cast.time).collect::<Vec<_>>();
        assert_eq!(casts(&new,name).len(),1);
        assert_eq!(casts(&new,target_name)[1],casts(&old,target_name)[1]-amount as f64/1000.0);
        assert_eq!(casts(&new,"이블리스토"),casts(&old,"이블리스토"));
        assert_eq!(new.damage_events.iter().filter(|hit| hit.skill_name==name).count(),2); // Two hits, one cooldown reduction.
    }
}

#[test]
fn reaper_shadow_activation_advances_a_running_charge_without_resetting_it() {
    let mut input=configured(true);
    let make=|id,name| {
        let mut data=input["skillData"][0].clone(); data["name"]=json!(name); data["gameSkillId"]=json!(id);
        data["cooldown"]=json!(20); data["castTimesMs"]=json!([1000]);
        for (index,hit) in data["hits"].as_array_mut().unwrap().iter_mut().enumerate() {
            hit["hitId"]=json!(format!("{id}:{index}"));
            for field in ["fixedDamage","damageRatio"] { hit[field]=json!(vec![hit[field][0].as_f64().unwrap();14]); }
        }
        data
    };
    input["skillData"]=json!([make(26550,"콜 오브 나이프"),make(26570,"쉐도우 트랩")]);
    input["skillData"][1]["cooldown"]=json!(100);
    input["character"]["skills"]=json!([{"name":"콜 오브 나이프","level":10,"tripods":[{"name":"절망의 부름","level":1}]},{"name":"쉐도우 트랩","level":10,"tripods":[{"name":"그림자 활성","level":1}]}]);
    input["aplConfig"]=json!("actions:\n - skill: 콜 오브 나이프\n - skill: 쉐도우 트랩\n");
    input["reaperIdentityModel"]=identity_model();
    input["reaperIdentityModel"]["skillGains"]=json!({});
    let (_,result)=trace(&input,30000);
    let casts:Vec<_>=result.casts.iter().filter(|cast|cast.skill_name=="콜 오브 나이프").map(|cast|cast.time).collect();
    assert_eq!(casts,[0.0,1.0,17.6]);
}

fn identity_model() -> Value {
    json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,
        "personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{"26070":{"dark":0.25,"heat":0.25}}})
}

#[test]
fn reaper_shadow_charge_scales_only_explicit_landed_gains_once() {
    use crate::engine::state::class::reaper::*;
    for (id,name,thirst) in [(DISTORTION,"디스토션",false),(DISTORTION,"디스토션",true),
        (26520,"블링크",false),(26520,"블링크",true),(26600,"쉐도우 스톰",false),(26600,"쉐도우 스톰",true)] {
        let mut input=configured(thirst);
        let mut data=input["skillData"][0].clone();
        data["name"]=json!(name); data["gameSkillId"]=json!(id);
        for (index,hit) in data["hits"].as_array_mut().unwrap().iter_mut().enumerate() {
            hit["hitId"]=json!(format!("{id}:test:{index}"));
            for field in ["fixedDamage","damageRatio"] { hit[field]=json!(vec![1.0;14]); }
        }
        input["skillData"]=json!([data,input["skillData"][0].clone()]);
        input["character"]["skills"]=json!([{"name":name,"level":4,"tripods":[{"name":"그림자 충전","level":1,"values":[99999]}]},
            {"name":"쉐도우 닷","level":1,"tripods":[]}]);
        input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n"));
        input["reaperIdentityModel"]=identity_model();
        input["reaperIdentityModel"]["skillGains"]=json!({id.to_string():{"dark":0.1,"heat":0.1},"26070":{"dark":0.1,"heat":0.1}});
        let profile=builder::build(&input.to_string()).unwrap();
        let reference=profile.reaper_reference.as_ref().unwrap();
        assert_eq!(reference.metadata["identity_tripod_gain"],json!({id.to_string():0.45}));
        assert_eq!(reference.metadata["identity_model"]["skillGains"][id.to_string()]["dark"],json!(0.1));
        let mut state=ReaperState::from_reference(100.0,reference);
        state.hit(id,false); assert_eq!(state.dark,0.0);
        state.hit(id,true); assert!((state.dark-0.145).abs()<1e-12);
        state.hit(SHADOW_DOT,true); assert!((state.dark-0.245).abs()<1e-12);
        state.dark=0.99; state.hit(id,true);
        assert_eq!((state.dark,state.heat),(1.0,0.0)); // Existing overflow policy, no transfer to heat.
        state.hit(id,true); assert!((state.heat-0.2175).abs()<1e-12); // Hunger applied once, separately.
        state.persona=true; state.hit(id,true); assert!((state.heat-0.2175).abs()<1e-12);
        state.persona=false; state.chaos_until=15000; state.hit(id,true);
        assert!((state.heat-0.2175).abs()<1e-12);
        let (_,execution)=trace(&input,1000);
        assert!(execution.damage_events.iter().flat_map(|hit|&hit.resource_gains)
            .any(|gain|gain.resource=="ReaperDark" && (gain.amount-0.145).abs()<1e-12));
        input["reaperIdentityModel"]["skillGains"]=json!({});
        input["reaperIdentityModel"]["darkPerSecond"]=json!(0.2);
        let empty=builder::build(&input.to_string()).unwrap();
        let mut state=ReaperState::from_reference(100.0,empty.reaper_reference.as_ref().unwrap());
        state.hit(id,true); assert_eq!(state.dark,0.0);
        state.advance(1000); assert_eq!(state.dark,0.2); // Natural gain has no tripod multiplier.
        input.as_object_mut().unwrap().remove("reaperIdentityModel");
        let legacy=builder::build(&input.to_string()).unwrap();
        let mut state=ReaperState::from_reference(100.0,legacy.reaper_reference.as_ref().unwrap());
        state.chaos_until=0; state.dark=0.0;
        state.hit(id,true); assert_eq!(state.dark,0.0); // No invented base gain.
    }
}

fn leap_nodes(last: &str) -> Value {
    json!({"classNodes":[{"name":"달의 소리","level":3},
        {"name":"초월적인 힘","level":5}, {"name":"충전된 분노","level":2},
        {"name":"각성 증폭기","level":1}, {"name":"풀려난 힘","level":2},
        {"name":"잠재력 해방","level":1}, {"name":"즉각적인 주문","level":2},
        {"name":last,"level":3}]})
}

#[test]
fn reaper_leap_target_groups_cooldown_speed_gains_and_exclusive_selection() {
    use crate::engine::state::class::reaper::*;
    for (node, id, name) in [("비열한 칼날",26960,"쉐도우 나이프"), ("잔혹한 칼날",26960,"쉐도우 나이프"),
        ("암살자의 발자취",26970,"피니쉬 스텝"), ("그림자 맹수",26970,"피니쉬 스텝")] {
        let mut input = configured(false);
        let mut data = input["skillData"][2].clone();
        data["name"] = json!(name); data["gameSkillId"] = json!(id);
        data["skillSlot"] = json!("HyperAwakeningTechniques"); data["cooldown"] = json!(40);
        input["skillData"] = json!([data]);
        input["character"]["skills"] = json!([{"name":name,"level":1,"tripods":[]}]);
        input["aplConfig"] = json!(format!("actions:\n - skill: {name}\n"));
        input["character"]["arkPassive"] = leap_nodes(node);
        input["reaperIdentityModel"] = identity_model();
        input["reaperIdentityModel"]["skillGains"] = json!({id.to_string():{"dark":0.25,"heat":0.25}});
        let profile = builder::build(&input.to_string()).unwrap();
        let skill = &profile.skills[0];
        let expected_cd = if node == "비열한 칼날" { 40000.0 * 0.98 * 0.64 } else { 40000.0 * 0.98 };
        assert!((skill.cooldown_ms as f64 - expected_cd).abs() <= 1.0);
        assert_eq!(skill.cast_times_ms[0], (2067.0 / 1.08) as u32);
        assert!((skill.hits[0].damage_increase - (1.06 * 1.05 * if node == "잔혹한 칼날" { 1.60 } else { 1.0 } - 1.0)).abs() < 1e-12, "{node}: {}", skill.hits[0].damage_increase);
        let mut state = ReaperState::from_reference(100.0, profile.reaper_reference.as_ref().unwrap());
        state.hit(id, true);
        assert!((state.dark - if node == "비열한 칼날" { 0.025 } else if node == "잔혹한 칼날" { 0.50 } else { 0.25 }).abs() < 1e-12);
        state.chaos_until = 15000;
        if node == "암살자의 발자취" {
            assert!((state.boost(id, false).damage - 1.28).abs() < 1e-12);
            assert!((state.boost(id, true).crit_rate - state.boost(id, false).crit_rate - 0.20).abs() < 1e-12);
        }
        if node == "그림자 맹수" {
            state.chaos_until = 0; state.dark = 1.0; state.cast(PERSONA);
            assert_eq!(state.boost(id, false).crit_damage, 0.15);
            assert_eq!(state.boost(id, false).crit_rate, 0.30);
        }
        input["character"]["arkPassive"]["classNodes"].as_array_mut().unwrap().push(json!({"name":if node == "비열한 칼날" { "잔혹한 칼날" } else { "비열한 칼날" },"level":1}));
        assert!(builder::build(&input.to_string()).unwrap_err().contains("배타"));
    }
}

#[test]
fn reaper_identity_model_overflow_miss_heat_bonus_expire_and_reenter() {
    use crate::engine::state::class::reaper::*;
    let mut input = configured(true);
    input["reaperIdentityModel"] = identity_model();
    let profile = builder::build(&input.to_string()).unwrap();
    let mut state = ReaperState::from_reference(100.0, profile.reaper_reference.as_ref().unwrap());
    assert!(state.can_cast(SHADOW_DOT));
    assert!(!state.can_cast(FATAL_STEP));
    state.dark = 0.97;
    state.hit(SHADOW_DOT, false);
    assert_eq!(state.dark, 0.97);
    state.hit(SHADOW_DOT, true);
    assert_eq!((state.dark, state.heat), (1.0, 0.0)); // Overflow is discarded.
    state.hit(SHADOW_DOT, true);
    assert_eq!(state.heat, 0.375); // Legacy Hunger Lv3, explicitly applied once.
    state.hit(SHADOW_DOT, true);
    state.hit(SHADOW_DOT, true);
    assert_eq!(state.chaos_until, 15000);
    assert!(state.can_cast(FATAL_STEP));
    state.advance(15000);
    assert_eq!((state.dark, state.heat, state.chaos_until), (1.0, 0.0, 0));
    assert!(state.can_cast(RAGE_SPEAR)); // Full-cycle model keeps acting after expiry.
    for _ in 0..3 { state.hit(SHADOW_DOT, true); }
    assert_eq!(state.chaos_until, 30000);
}

#[test]
fn reaper_identity_model_natural_persona_exit_and_repeated_simulation() {
    use crate::engine::state::class::reaper::*;
    let mut input = configured(false);
    input["reaperIdentityModel"] = identity_model();
    input["reaperIdentityModel"]["darkPerSecond"] = json!(0.5);
    input["reaperIdentityModel"]["personaDarkLossPerSecond"] = json!(1);
    input["reaperIdentityModel"]["heatLossPerSecond"] = json!(0.1);
    let profile = builder::build(&input.to_string()).unwrap();
    let mut state = ReaperState::from_reference(100.0, profile.reaper_reference.as_ref().unwrap());
    state.advance(2000);
    assert_eq!(state.dark, 1.0);
    state.cast(PERSONA);
    assert_eq!(state.next_expiration(), Some(2100));
    state.hit(SHADOW_DOT, true); // No gains during Persona.
    assert_eq!(state.dark, 1.0);
    state.advance(3000);
    assert!(!state.persona && state.dark == 0.0);
    state.heat = 0.5;
    state.advance(4000);
    assert!((state.heat - 0.4).abs() < 1e-12);
    let (_, data) = engine::trace_one(&profile, 42, 300000);
    assert!(data.class_events.iter().any(|event| event.persona));
    assert_eq!(serde_json::to_value(&data).unwrap(), serde_json::to_value(engine::trace_one(&profile, 42, 300000).1).unwrap());
    input["reaperIdentityModel"]["skillGains"]["99999"] = json!({"dark":1,"heat":1});
    assert!(builder::build(&input.to_string()).is_err());
    input["reaperIdentityModel"]["skillGains"].as_object_mut().unwrap().remove("99999");
    input["reaperIdentityModel"]["heatLossPerSecond"] = json!(-0.1);
    assert!(builder::build(&input.to_string()).is_err());
}

#[test]
fn reaper_configured_equipment_level_gem_specialization_and_reset() {
    let mut input = configured(false);
    let initial = builder::build(&input.to_string()).unwrap();
    assert_eq!(initial.stat.base_attack_power, 0.0); // The normal snapshot computes equipment AP.
    assert_eq!(initial.stat.base_main_stat, 300000.0);
    assert_eq!(initial.stat.sum(crate::profile::StatField::WeaponAttackPower), 150000.0);
    assert_eq!(initial.stat.specialization, 0.0);
    assert_eq!(initial.stat.sum(crate::profile::StatField::AttackSpeed), 0.0);
    let old_rage = initial.skills.iter().find(|s| s.name == "레이지 스피어").unwrap();
    input["character"]["skills"][2]["level"] = json!(14);
    input["skillData"][2]["hits"][0]["fixedDamage"] = json!(vec![931.0; 14]);
    input["skillData"][2]["hits"][0]["damageRatio"] = json!([15.525,15.525,15.525,15.525,15.525,15.525,15.525,15.525,15.525,15.525,15.525,15.525,15.525,31.05]);
    input["collectionCombatStats"] = json!({"specialization":1800});
    input["character"]["gems"] = json!([
        {"type":"겁화","level":10,"skillId":26810,"skillName":"레이지 스피어","skillEffect":"피해 증가"},
        {"type":"작열","level":10,"skillId":26810,"skillName":"레이지 스피어","skillEffect":"재사용 대기시간 감소"}]);
    let built = builder::build(&input.to_string()).unwrap();
    let rage = built.skills.iter().find(|s| s.name == "레이지 스피어").unwrap();
    assert!(rage.hits[0].skill_modifier > old_rage.hits[0].skill_modifier);
    assert!((rage.hits[0].damage_increase - ((1.0 + 1800.0 / 699.0 * 0.31) * 1.44 * 1.05 - 1.0)).abs() < 1e-9);
    assert!(rage.cooldown_ms < old_rage.cooldown_ms);
    let (result, events) = engine::trace_one(&built, 42, 300_000);
    assert!(result.total_damage > 0.0);
    assert!(events.class_events.iter().any(|event| event.persona));
    let again = engine::trace_one(&built, 42, 300_000);
    assert_eq!(serde_json::to_value(&events).unwrap(), serde_json::to_value(again.1).unwrap());
    assert_eq!(built.reaper_reference.unwrap().metadata["input_mode"], "configured_build");
    assert!(trace(&configured(true), 300_000).0.total_damage > 0.0);
}

#[test]
fn reaper_configured_rejects_unmapped_inputs() {
    for (path, value) in [
        (vec!["character", "arkGrids"], json!({})),
        (vec!["character", "arkPassive"], json!({"classNodes":[{"name":"미지원 도약","level":3}]})),
        (vec!["reaperReference"], json!({"scenario":"dark_shadow_dot_legacy"})),
        (vec!["enableAwakeningPotion"], json!(true)),
    ] {
        let mut input = configured(false);
        let mut target = &mut input;
        for field in path { target = &mut target[field]; }
        *target = value;
        assert!(builder::build(&input.to_string()).is_err());
    }
    let mut input = configured(false);
    input["character"]["skills"][0]["rune"] = json!({"name":"풍요", "grade":"전설", "effects":[]});
    assert!(builder::build(&input.to_string()).unwrap_err().contains("질풍"));
    let mut input = configured(false);
    input["character"]["skills"][0]["tripods"] = json!([{"name":"미지원 선택"}]);
    assert!(builder::build(&input.to_string()).is_err());
    let mut input = configured(false);
    input["character"]["equipments"][0]["itemLevel"] = json!(9999);
    assert!(builder::build(&input.to_string()).unwrap_err().contains("장비 데이터 없음"));
}

#[test]
fn reaper_selected_passive_replacement_prerequisites_and_untrusted_values() {
    use crate::engine::state::class::reaper::*;
    let mut input = configured(false);
    input["character"]["arkPassive"] = json!({"classNodes": [
        {"name":"달의 소리","level":3,"values":[99999]}, {"name":"유령 무희","level":3},
        {"name":"그림자 밟기","level":1}, {"name":"곡예사","level":5}, {"name":"잠행","level":5}]});
    let profile = builder::build(&input.to_string()).unwrap();
    let mut state = ReaperState::from_reference(100.0, profile.reaper_reference.as_ref().unwrap());
    assert_eq!(state.boost(RAGE_SPEAR, true).damage, 0.50);
    state.dark = 1.0;
    state.cast(PERSONA);
    let boost = state.boost(RAGE_SPEAR, true);
    assert!((boost.damage - 2.38 * 1.07).abs() < 1e-12);
    assert_eq!(boost.crit_rate, 0.10);
    assert_eq!(boost.crit_damage, 0.15);
    state.cast(RAGE_SPEAR);
    assert_eq!(state.weakness, 0); // No 급소 확보 selection, no counter.
    input["character"]["arkPassive"]["classNodes"][0]["level"] = json!(2);
    let error = builder::build(&input.to_string()).unwrap_err();
    assert!(error.contains("선행") || error.contains("이전 티어"));
    input["character"]["arkPassive"]["classNodes"] = json!([{"name":"달의 소리","level":1}, {"name":"피냄새","level":1}]);
    assert!(builder::build(&input.to_string()).is_err());
    input["character"]["arkPassive"]["classNodes"] = json!([{"name":"달의 소리","level":4}]);
    assert!(builder::build(&input.to_string()).is_err());
}

#[test]
fn reaper_handwork_hit_group_miss_cap_expiry_and_chaos_selection() {
    use crate::engine::state::class::reaper::*;
    let mut input = configured(true);
    input["character"]["arkPassive"] = json!({"classNodes":[
        {"name":"피냄새","level":3}, {"name":"굶주림","level":3}, {"name":"갈증","level":1},
        {"name":"암살자의 손놀림","level":5}]});
    let profile = builder::build(&input.to_string()).unwrap();
    let mut state = ReaperState::from_reference(100.0, profile.reaper_reference.as_ref().unwrap());
    assert!(!state.can_cast(FATAL_STEP));
    assert!((state.boost(RAGE_SPEAR, true).attack_power - 1.14).abs() < 1e-12);
    assert_eq!(state.boost(RAGE_SPEAR, true).damage, 1.04);
    state.cast(RAGE_SPEAR);
    assert_eq!(state.chaos_until, 15000); // No 혼돈 강화 selection.
    state.hit(NIGHTMARE, true);
    state.hit(RAGE_SPEAR, false);
    assert_eq!(state.boost(RAGE_SPEAR, true).attack_speed, 0.10);
    for _ in 0..7 { state.hit(RAGE_SPEAR, true); }
    assert!((state.boost(RAGE_SPEAR, true).attack_speed - 0.15).abs() < 1e-12);
    assert_eq!(state.boost(26900, false).damage, 1.04); // Damage group includes Awakening.
    state.advance(6000);
    assert_eq!(state.boost(RAGE_SPEAR, true).attack_speed, 0.10);
}

#[test]
fn reaper_second_rush_consumes_persona_and_chaos_once() {
    use crate::engine::state::class::reaper::*;
    let mut moon = ReaperState::new(0.0, crate::profile::reaper::ReaperScenario::DarkShadowDotLegacy);
    moon.dark = 1.0;
    moon.cast(PERSONA);
    assert_eq!(moon.boost(SILENT_SMASHER, true).damage, 2.69);
    moon.cast(SILENT_SMASHER);
    assert!(!moon.persona);
    assert_eq!(moon.weakness, 1);
    assert_eq!(moon.dark, 0.0);
    moon.hit(SILENT_SMASHER, true);
    assert_eq!(moon.weakness, 1);
    let mut thirst = ReaperState::new(0.0, crate::profile::reaper::ReaperScenario::ThirstChaosSustain);
    thirst.cast(SILENT_SMASHER);
    assert_eq!(thirst.chaos_until, 13_000);
}

#[test]
fn reaper_added_daggers_refresh_only_on_hit_and_all_rush_ids_consume_once() {
    use crate::engine::state::class::reaper::*;
    use crate::profile::reaper::ReaperScenario;
    for id in [26050, 26060, 26080, 26110, 26120, 26140] {
        let mut state = ReaperState::new(0.0, ReaperScenario::ThirstChaosSustain);
        state.advance(5_000);
        state.cast(id);
        assert_eq!(state.chaos_until, 15_000);
        state.hit(id, false);
        assert_eq!(state.chaos_until, 15_000);
        state.hit(id, true);
        assert_eq!(state.chaos_until, 20_000);
    }
    for id in [26800, RAGE_SPEAR, SILENT_SMASHER, 26830, 26970] {
        let mut moon = ReaperState::new(0.0, ReaperScenario::DarkShadowDotLegacy);
        moon.dark = 1.0;
        moon.cast(PERSONA);
        assert_eq!(moon.boost(id, true).damage, 2.69);
        moon.cast(id);
        assert_eq!((moon.persona, moon.weakness), (false, 1));
        moon.hit(id, true);
        assert_eq!(moon.weakness, 1);
        let mut thirst = ReaperState::new(0.0, ReaperScenario::ThirstChaosSustain);
        thirst.cast(id);
        assert_eq!(thirst.chaos_until, 13_000);
    }
}

#[test]
fn reaper_awakening_excludes_rush_damage_boost_and_ultimate_cost_is_separate() {
    use crate::engine::state::class::reaper::*;
    use crate::engine::state::class::ClassState;
    let mut state = ReaperState::new(0.0, crate::profile::reaper::ReaperScenario::ThirstChaosSustain);
    state.cast(FATAL_STEP);
    assert_eq!(state.boost(26900, false).damage, 1.0);
    assert_eq!(state.boost(26910, false).damage, 1.0);
    assert_eq!(state.boost(26950, false).damage, 1.0);
    assert_eq!(state.boost(26970, true).damage, 1.06 * 1.60);
    state.set_resource("UltimatePoint", 10000.0);
    state.set_resource("UltimatePoint", state.get_resource("UltimatePoint") - 10000.0);
    assert_eq!(state.get_resource("UltimatePoint"), 0.0);
    assert_eq!((state.dark, state.heat), (1.0, 1.0));
}

#[test]
fn reaper_nightmare_return_requires_a_landed_live_mark_and_consumes_once() {
    use crate::engine::state::class::reaper::*;
    let mut state = ReaperState::new(0.0, crate::profile::reaper::ReaperScenario::DarkShadowDotLegacy);
    assert!(!state.can_cast(NIGHTMARE_RETURN));
    state.hit(NIGHTMARE, false);
    assert!(!state.can_cast(NIGHTMARE_RETURN));
    state.hit(NIGHTMARE, true);
    assert!(state.can_cast(NIGHTMARE_RETURN));
    state.cast(NIGHTMARE_RETURN);
    assert!(!state.can_cast(NIGHTMARE_RETURN));
    state.hit(NIGHTMARE, true);
    state.advance(7000);
    assert!(!state.can_cast(NIGHTMARE_RETURN));
}

#[test]
fn reaper_special_and_return_skills_cannot_equip_a_separate_direct_gem() {
    for (id,name) in [(26100,"나이트메어 (재입력)"),(26960,"쉐도우 나이프"),(26970,"피니쉬 스텝"),(26980,"페이탈 스텝"),(26900,"월식 : 카덴차"),(26910,"일식 : 레퀴엠"),(26940,"카덴차 델 루나"),(26950,"레퀴엠 델 솔")] {
        for kind in ["겁화","작열","멸화","홍염"] {
            let mut input=configured(true);
            let mut data=input["skillData"][0].clone();data["name"]=json!(name);data["gameSkillId"]=json!(id);
            input["skillData"]=json!([data]);
            input["character"]["skills"]=json!([{"name":name,"level":1,"tripods":[]}]);
            input["character"]["gems"]=json!([{"type":kind,"level":10,"skillId":id,"skillName":name,"skillEffect":"피해 증가"}]);
            let error=builder::build(&input.to_string()).unwrap_err();
            assert!(error.contains("보석 대상"),"{name}/{kind}: {error}");
        }
    }
}

#[test]
fn reaper_synthetic_gain_routes_by_pre_hit_fullness_and_discard_overflow_rp04_rp05() {
    let mut input = request(false);
    input["character"]["skills"] = json!([{"name":"쉐도우 닷", "level":1, "tripods":[]}]);
    input["aplConfig"] = json!("actions:\n - skill: 쉐도우 닷\n");
    input["reaperReference"]["gaugeTest"] = json!({"initialDark":0.97, "hitGains":{
        "dot:1":{"dark":0.06,"heat":0.06}, "dot:2":{"dark":0.06,"heat":0.06}}});
    let (_, data) = trace(&input, 1000);
    let hits: Vec<_> = data.class_events.iter().filter(|event| event.event == "hit").collect();
    assert_eq!((hits[0].dark, hits[0].heat), (1.0, 0.0));
    assert_eq!((hits[1].dark, hits[1].heat), (1.0, 0.06));
    assert_eq!(data.damage_events[0].resource_gains[0].resource, "ReaperDark");
    assert_eq!(data.damage_events[1].resource_gains[0].resource, "ReaperHeat");
    assert_eq!(data.damage_events[1].resource_gains[0].amount, 0.06);
    let built = builder::build(&input.to_string()).unwrap();
    assert_eq!(built.reaper_reference.unwrap().metadata["synthetic_gauge_test"]["initialDark"], 0.97);
    input["reaperReference"]["gaugeTest"]["hitGains"]["dot:2"]["heat"] = json!(-0.1);
    assert!(builder::build(&input.to_string()).is_err());
}

#[test]
fn reaper_reference_requires_opt_in_and_rejects_unmapped_choices() {
    let mut input = request(false);
    input.as_object_mut().unwrap().remove("reaperReference");
    assert!(builder::build(&input.to_string()).unwrap_err().contains("reaperReference"));
    let mut input = request(false);
    input["reaperReference"]["scenario"] = json!("unknown");
    assert!(builder::build(&input.to_string()).is_err());
    let mut input = request(false);
    input["character"]["skills"][0]["level"] = json!(13);
    assert!(builder::build(&input.to_string()).unwrap_err().contains("1레벨"));
    let mut input = request(false);
    input["skillData"][0]["gameSkillId"] = json!(19030);
    assert!(builder::build(&input.to_string()).unwrap_err().contains("지원 범위 밖"));
    let mut input = request(false);
    input["reaperReference"]["missedHitIds"] = json!(["missing"]);
    assert!(builder::build(&input.to_string()).unwrap_err().contains("없는 미스"));
    let mut input = request(false);
    input["character"]["className"] = json!("아르카나");
    assert!(builder::build(&input.to_string()).unwrap_err().contains("리퍼 요청"));
    let mut input = request(false);
    input["enablePetBuff"] = json!(true);
    assert!(builder::build(&input.to_string()).is_err());
    let mut input = request(false);
    input["skillData"][0]["hits"][1]["hitId"] = json!("dot:1");
    assert!(builder::build(&input.to_string()).unwrap_err().contains("중복"));
}

#[test]
fn reaper_moon_fills_after_nine_casts_and_repeats_independently() {
    let input = request(false);
    let profiles = builder::build(&input.to_string()).unwrap();
    let (result, data) = engine::trace_one(&profiles, 42, 300_000);
    assert!(result.total_damage > 0.0);
    let dot = data.damage_events.iter().find(|hit| hit.hit_id == "dot:1").unwrap();
    // shadow-dot.md's independently recorded raw macro: AP*24150/10000+(131+160)/2.
    assert!((dot.formula.base_term - 24_295.5).abs() < 1e-9);
    let persona = data.class_events.iter().find(|event| event.event == "cast" && event.game_skill_id == 26040).unwrap();
    assert_eq!(persona.dark, 1.0);
    assert_eq!(data.casts.iter().filter(|cast| cast.skill_name == "쉐도우 닷" && cast.time < persona.time).count(), 9);
    assert!(data.class_events.iter().any(|event| event.blade));
    let rage: Vec<_> = data.damage_events.iter().filter(|hit| hit.skill_name == "레이지 스피어").collect();
    assert!(rage.len() >= 4);
    assert!((rage[0].formula.damage_increase_multiplier - 2.69).abs() < 1e-12);
    assert!((rage[3].formula.damage_increase_multiplier - 2.69 * 2.25).abs() < 1e-12);
    assert_eq!(profiles.reaper_reference.as_ref().unwrap().metadata["status"], "provisional");
    let repeated = engine::trace_one(&profiles, 42, 300_000);
    assert_eq!(serde_json::to_value(&result).unwrap(), serde_json::to_value(&repeated.0).unwrap());
    assert_eq!(serde_json::to_value(&data).unwrap(), serde_json::to_value(&repeated.1).unwrap());
    let payload = crate::trace_payload(&profiles, 42, 300_000);
    let restored: Value = serde_json::from_str(&payload.to_string()).unwrap();
    assert_eq!(restored["builtProfile"]["reaper_reference"]["metadata"]["status"], "provisional");
    assert!(!restored["classEvents"].as_array().unwrap().is_empty());
}

#[test]
fn reaper_partial_miss_has_no_damage_or_gain_and_does_not_fill_in_nine_casts() {
    let mut input = request(false);
    input["reaperReference"]["missedHitIds"] = json!(["dot:2"]);
    let (_, data) = trace(&input, 60_000);
    assert!(!data.casts.iter().any(|cast| cast.skill_name == "페르소나"));
    assert!(!data.damage_events.iter().any(|hit| hit.hit_id == "dot:2"));
    assert!(data.class_events.iter().any(|event| event.event == "miss" && event.hit_id == "dot:2"));
    let first = data.damage_events.iter().find(|hit| hit.hit_id == "dot:1").unwrap();
    assert!((first.resource_gains[0].amount - 1.0 / 18.0).abs() < 1e-12);
}

#[test]
fn reaper_reserved_multi_hits_keep_one_cast_boost_and_front_hits_do_not_get_blade_damage() {
    let mut input = request(false);
    let mut second = input["skillData"][2]["hits"][0].clone();
    second["hitId"] = json!("rage:delayed");
    second["fixedDelayMs"] = json!(2000);
    input["skillData"][2]["hits"].as_array_mut().unwrap().push(second);
    let (_, data) = trace(&input, 300_000);
    for pair in data.damage_events.iter().filter(|hit| hit.skill_name == "레이지 스피어").collect::<Vec<_>>().chunks(2) {
        assert_eq!(pair.len(), 2);
        assert_eq!(pair[0].cast_id, pair[1].cast_id);
        assert_eq!(pair[0].formula.damage_increase_multiplier, pair[1].formula.damage_increase_multiplier);
    }
    input["skillData"][2]["properties"]["attackType"] = json!("");
    let (_, data) = trace(&input, 300_000);
    assert!(data.class_events.iter().any(|event| event.blade));
    assert!(data.damage_events.iter().filter(|hit| hit.skill_name == "레이지 스피어")
        .all(|hit| (hit.formula.damage_increase_multiplier - 2.69).abs() < 1e-12));
    input["reaperReference"]["missedHitIds"] = json!(["rage:1", "rage:delayed"]);
    let (_, data) = trace(&input, 300_000);
    assert!(!data.damage_events.iter().any(|hit| hit.skill_name == "레이지 스피어"));
    let uses: Vec<_> = data.class_events.iter().filter(|event| event.event == "cast" && event.game_skill_id == 26810).collect();
    assert!(uses[2].blade);
    assert!(!uses[3].blade && uses[3].weakness == 0);
}

#[test]
fn reaper_thirst_fatal_self_boost_and_expired_cast_snapshot_survive() {
    let (_, data) = trace(&request(true), 60_000);
    let fatal = data.damage_events.iter().find(|hit| hit.hit_id == "fatal:1").unwrap();
    assert!((fatal.formula.damage_increase_multiplier - 1.06 * 1.60).abs() < 1e-12);
    assert!((fatal.formula.attack_power - 13_800.0).abs() < 1e-9);
    assert!(data.casts.iter().any(|cast| cast.active_buffs.iter().any(|id| id == "reaper:fatal")));
    let mut input = request(true);
    input["character"]["skills"] = json!([{"name":"레이지 스피어", "level":1, "tripods":[]}]);
    input["aplConfig"] = json!("actions:\n - skill: 레이지 스피어\n   conditions:\n    - {type: time_remaining, operator: '<=', value: 6}\n");
    let (_, data) = trace(&input, 20_000);
    assert_eq!(data.casts.len(), 1);
    assert!(data.class_events.iter().any(|event| event.event == "cast" && event.chaos_remaining == 0.0));
    assert_eq!(data.damage_events.len(), 1);
    assert!((data.damage_events[0].formula.attack_power - 13_800.0).abs() < 1e-9);
    assert!((data.damage_events[0].formula.damage_increase_multiplier - 1.06).abs() < 1e-12);
    input["aplConfig"] = json!("actions:\n - skill: 레이지 스피어\n   conditions:\n    - {type: time_remaining, operator: '<=', value: 5}\n");
    let (_, data) = trace(&input, 20_000);
    assert!(data.casts.is_empty()); // Expire first at t=15, before a cast at the same time.
    assert!(data.class_events.iter().any(|event| event.event == "expire" && event.time == 15.0));
}

#[test]
fn reaper_shadow_miss_refreshes_chaos_but_dagger_miss_does_not() {
    for (name, misses, refreshed) in [
        ("디스토션", vec!["distortion:1", "distortion:2"], true),
        ("쉐도우 닷", vec!["dot:1", "dot:2"], false),
    ] {
        let mut input = request(true);
        input["character"]["skills"] = json!([{"name":name, "level":1, "tripods":[]}]);
        input["reaperReference"]["missedHitIds"] = json!(misses);
        input["aplConfig"] = json!(format!("actions:\n - skill: {name}\n   conditions:\n    - {{type: time_remaining, operator: '<=', value: 6}}\n"));
        let (_, data) = trace(&input, 20_000);
        assert!(data.damage_events.is_empty());
        let cast = data.class_events.iter().find(|event| event.event == "cast").unwrap();
        assert_eq!(cast.time, 14.0);
        assert_eq!(cast.chaos_remaining > 1.0, refreshed);
        assert_eq!(data.class_events.iter().any(|event| event.event == "expire" && event.time == 15.0), !refreshed);
    }
}

#[test]
fn reaper_target_conditions_are_explicit_static_predicates_and_skill_scoped() {
    for (id,name,choice,ids,immune,bonus) in [
        (26070,"쉐도우 닷","죽음의 일격",vec![260701,260702],true,1.2),
        (26510,"디스토션","약육강식",vec![265108],false,2.64),
        (26550,"콜 오브 나이프","약육강식",vec![265420,265421,265422],false,0.6),
        (26830,"댄싱 오브 퓨리","약육강식",vec![268300,268301],false,0.948),
    ] {
        let mut input=corrosion_input(id,name,&ids);
        input["character"]["skills"][0]["tripods"]=json!([]);
        // None of these existing target fields imply the two new predicates.
        input["targetHpPercent"]=json!(30);input["assumedTargetStaggered"]=json!(true);
        input["targetMonsterType"]=json!("악마");
        let mut other=input["skillData"][0].clone();other["name"]=json!("레이지 스피어");other["gameSkillId"]=json!(26810);
        for (i,hit) in other["hits"].as_array_mut().unwrap().iter_mut().enumerate() {hit["hitId"]=json!(format!("other:{i}"));}
        input["skillData"].as_array_mut().unwrap().push(other);
        input["character"]["skills"].as_array_mut().unwrap().push(json!({"name":"레이지 스피어","level":10,"tripods":[]}));
        let base=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"]=json!([{"name":choice,"level":1,"values":[999999]}]);
        for seed in [false,true] {for immune_flag in [false,true] {
            input["assumedTargetSeedOrLower"]=json!(seed);input["assumedTargetAbnormalMoveImmune"]=json!(immune_flag);
            let selected=builder::build(&input.to_string()).unwrap();
            let expected=1.+if if immune {immune_flag} else {seed} {bonus} else {0.};
            for (new,old) in selected.skills.iter().zip(&base.skills) {
                assert_eq!(new.cooldown_ms,old.cooldown_ms);assert_eq!(new.cast_times_ms,old.cast_times_ms);
                assert_eq!(new.hits.len(),old.hits.len());
                for (hit,previous) in new.hits.iter().zip(&old.hits) {
                    assert_eq!((hit.effect_id,hit.action_delay_ms,hit.base_damage,hit.skill_modifier),(previous.effect_id,previous.action_delay_ms,previous.base_damage,previous.skill_modifier));
                    let multiplier=if new.name==name {expected} else {1.};
                    assert!(((1.+hit.damage_increase)/(1.+previous.damage_increase)-multiplier).abs()<1e-12);
                }
            }
            let meta=&selected.reaper_reference.as_ref().unwrap().metadata["target_condition_assumptions"];
            assert_eq!(meta["seed_or_lower"],json!(seed));assert_eq!(meta["abnormal_move_immune"],json!(immune_flag));
        }}
        input["character"]["skills"][0]["tripods"][0]["level"]=json!(5);
        assert!(builder::build(&input.to_string()).is_err());
    }
}

#[test]
fn reaper_immune_damage_choice_composes_poison_without_inheriting_into_ticks_or_burst() {
    for poison in ["독 : 부식","독 : 출혈"] {
        let mut input=corrosion_input(26070,"쉐도우 닷",&[260701,260702]);
        input["character"]["skills"][0]["tripods"]=json!([{"name":poison,"level":1},{"name":"죽음의 일격","level":1,"values":[999999]}]);
        let base=builder::build(&input.to_string()).unwrap();
        let (_,old)=engine::trace_one(&base,42,10000);
        input["assumedTargetAbnormalMoveImmune"]=json!(true);
        let selected=builder::build(&input.to_string()).unwrap();
        let (_,new)=engine::trace_one(&selected,42,10000);
        let poison_events=|events:&crate::constants::TraceData| events.damage_events.iter().filter(|h|h.hit_id.starts_with("reaper:")).map(|h|serde_json::to_value(h).unwrap()).collect::<Vec<_>>();
        assert!(!poison_events(&new).is_empty());assert_eq!(poison_events(&new),poison_events(&old));
        if poison=="독 : 출혈" {assert!(new.damage_events.iter().any(|h|h.hit_id=="reaper:bleeding:burst"));}
        let direct:Vec<_>=new.damage_events.iter().filter(|h|!h.hit_id.starts_with("reaper:")).collect();
        let previous:Vec<_>=old.damage_events.iter().filter(|h|!h.hit_id.starts_with("reaper:")).collect();
        assert!(!direct.is_empty());
        for (a,b) in direct.iter().zip(previous) {assert!((a.formula.skill_damage_multiplier/b.formula.skill_damage_multiplier-2.2).abs()<1e-12);}
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        assert_eq!(serde_json::to_value(engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,10000).1.damage_events).unwrap(),serde_json::to_value(new.damage_events).unwrap());
    }
}

#[test]
fn reaper_black_storm_swaps_only_repeated_hits_and_composes_existing_selectors() {
    let ids=std::iter::once(266000).chain(std::iter::repeat_n(266001,10)).chain(std::iter::once(266002)).collect::<Vec<_>>();
    for other in ["", "그림자 충전", "꿰뚫는 일격", "치명적인 그림자"] {
        let mut input=corrosion_input(26600,"쉐도우 스톰",&ids);
        input["skillData"][0]["cooldown"]=json!(20);input["character"]["skills"][0]["tripods"]=json!([]);
        input["aplConfig"]=json!("actions:
 - skill: 쉐도우 스톰
");
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{"26600":{"dark":0.01,"heat":0.01}}});
        if !other.is_empty() {input["character"]["skills"][0]["tripods"]=json!([{"name":other,"level":1}]);}
        let base=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"검은 폭풍","level":1,"values":[999999]}));
        let selected=builder::build(&input.to_string()).unwrap();
        let skill=&selected.skills[0];assert_eq!(skill.hits.len(),12);
        assert_eq!(skill.hits.iter().filter(|h|h.effect_id==266070).count(),10);
        assert_eq!(skill.cast_times_ms,base.skills[0].cast_times_ms);assert_eq!(skill.cooldown_ms,base.skills[0].cooldown_ms);
        for (new,old) in skill.hits.iter().zip(&base.skills[0].hits) {
            assert_eq!(new.effect_id,if old.effect_id==266001 {266070} else {old.effect_id});
            assert_eq!((&new.hit_id,new.action_delay_ms,new.base_damage,new.skill_modifier,new.defense_ignore,new.crit_chance_bonus),(&old.hit_id,old.action_delay_ms,old.base_damage,old.skill_modifier,old.defense_ignore,old.crit_chance_bonus));
            assert!(((1.+new.damage_increase)/(1.+old.damage_increase)-1.95).abs()<1e-12);
        }
        let (_,old)=engine::trace_one(&base,42,10000);let (_,new)=engine::trace_one(&selected,42,10000);
        assert!(!new.damage_events.is_empty());assert_eq!(new.damage_events.len(),old.damage_events.len());
        for (a,b) in new.damage_events.iter().zip(&old.damage_events) {
            assert_eq!((&a.hit_id,a.time),(&b.hit_id,b.time));
            assert_eq!(serde_json::to_value(&a.resource_gains).unwrap(),serde_json::to_value(&b.resource_gains).unwrap());
            assert!((a.formula.skill_damage_multiplier/b.formula.skill_damage_multiplier-1.95).abs()<1e-12);
        }
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        assert_eq!(serde_json::to_value(engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,10000).1.damage_events).unwrap(),serde_json::to_value(new.damage_events).unwrap());
        let mut missed=selected.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=missed.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();
        assert!(engine::trace_one(&missed,42,10000).1.damage_events.is_empty());
    }
}

#[test]
fn reaper_wide_mist_preserves_callbacks_explosion_scalars_and_composes_spreading_mist() {
    for route in ["", "거짓 안개", "안개 폭발"] {for append in [false,true] {
        let mut input=corrosion_input(26560,"블랙 미스트",&[265600;5]);input["skillData"][0]["cooldown"]=json!(20);
        input["character"]["skills"][0]["tripods"]=json!([]);
        if !route.is_empty() {input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":route,"level":1}));}
        if append {input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"퍼지는 안개","level":1}));}
        input["aplConfig"]=json!("actions:
 - skill: 블랙 미스트
");
        let base=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"넓은 안개","level":1,"values":[999999]}));
        let selected=builder::build(&input.to_string()).unwrap();
        let pairs=std::collections::HashMap::from([(265600,265630),(265675,265676),(265682,265686),(265683,265687),(265684,265688)]);
        assert_eq!(selected.skills[0].hits.len(),base.skills[0].hits.len());
        assert_eq!(selected.skills[0].cast_times_ms,base.skills[0].cast_times_ms);
        for (new,old) in selected.skills[0].hits.iter().zip(&base.skills[0].hits) {
            assert_eq!(new.effect_id,*pairs.get(&old.effect_id).unwrap_or(&old.effect_id));
            assert_eq!((&new.hit_id,new.action_delay_ms,new.fixed_delay_ms,new.base_damage,new.skill_modifier,new.damage_increase),(&old.hit_id,old.action_delay_ms,old.fixed_delay_ms,old.base_damage,old.skill_modifier,old.damage_increase));
        }
        for fast in [false,true] {
            let mut previous=base.clone();let mut profile=selected.clone();
            if fast {previous.stat.attack_speed.insert("test".into(),50.);profile.stat.attack_speed.insert("test".into(),50.);}
            let (a,old)=engine::trace_one(&previous,42,10000);let (b,new)=engine::trace_one(&profile,42,10000);
            assert_eq!(a.total_damage,b.total_damage);
            assert_eq!(serde_json::to_value(old.damage_events).unwrap(),serde_json::to_value(new.damage_events).unwrap());
        }
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        let (_,new)=engine::trace_one(&selected,42,10000);
        assert_eq!(serde_json::to_value(new.damage_events).unwrap(),serde_json::to_value(engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,10000).1.damage_events).unwrap());
        let mut missed=selected.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=missed.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();
        assert!(engine::trace_one(&missed,42,10000).1.damage_events.is_empty());
    }}
}

#[test]
fn reaper_call_callbacks_extend_middle_hits_and_compose_crit_gain_with_charges() {
    use crate::engine::state::class::reaper::ReaperState;
    for (name,extended) in [("지속되는 부름",true),("치명적인 그림자",false)] {for charge in [false,true] {
        let mut input=corrosion_input(26550,"콜 오브 나이프",&[265420,265421,265421,265421,265422]);
        input["skillData"][0]["cooldown"]=json!(20);
        input["character"]["skills"][0]["tripods"]=json!([{"name":name,"level":1,"values":[999999]}]);
        if charge {input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"절망의 부름","level":1}));}
        input["aplConfig"]=json!("actions:\n - skill: 콜 오브 나이프\n");
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{"26550":{"dark":0.01,"heat":0.01}}});
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];
        let count=if extended {7} else {5};assert_eq!(skill.hits.len(),count);
        assert_eq!(skill.cast_times_ms,vec![1267]);assert_eq!(skill.max_stacks,if charge {2} else {0});
        for (i,h) in skill.hits.iter().enumerate() {
            assert_eq!(h.hit_id,format!("26550:0:{i}"));assert_eq!(h.action_delay_ms,339);assert_eq!(h.fixed_delay_ms,(i*350) as u32);
            assert_eq!(h.effect_id,if extended {if i==0 {265435} else if i==6 {265437} else {265436}} else {if i==0 {265420} else if i==4 {265422} else {265421}});
            assert_eq!(h.crit_chance_bonus,if extended {0.} else {0.3});
        }
        let mut state=ReaperState::from_reference(100.,profile.reaper_reference.as_ref().unwrap());
        state.hit(26550,false);assert_eq!(state.dark,0.);state.hit(26550,true);
        assert!((state.dark-if extended {0.01} else {0.013}).abs()<1e-12);
        for fast in [false,true] {
            let mut current=profile.clone();if fast {current.stat.attack_speed.insert("test".into(),50.);}
            let (_,execution)=engine::trace_one(&current,42,10000);
            let first=&execution.casts[0];let hits:Vec<_>=execution.damage_events.iter().filter(|h|h.cast_id==Some(first.cast_id)).collect();
            assert_eq!(hits.len(),count);
            for pair in hits.windows(2) {assert!((pair[1].time-pair[0].time-0.35).abs()<1e-12);}
        }
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        assert_eq!(serde_json::to_value(engine::trace_one(&profile,42,10000).1.damage_events).unwrap(),serde_json::to_value(engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,10000).1.damage_events).unwrap());
        let mut missed=profile.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();
        assert!(engine::trace_one(&missed,42,10000).1.damage_events.is_empty());
        input["reaperIdentityModel"]["skillGains"]=json!({});input["reaperIdentityModel"]["darkPerSecond"]=json!(0.2);
        let empty=builder::build(&input.to_string()).unwrap();let mut state=ReaperState::from_reference(100.,empty.reaper_reference.as_ref().unwrap());
        state.hit(26550,true);assert_eq!(state.dark,0.);state.advance(1000);assert_eq!(state.dark,0.2);
    }}
}

#[test]
fn reaper_trap_range_changes_only_effect_id_and_composes_charge_or_cooldown() {
    for other in ["", "연속 공격", "그림자 활성"] {
        let mut input=corrosion_input(26570,"쉐도우 트랩",&[265700]);input["skillData"][0]["cooldown"]=json!(20);
        input["skillData"][0]["castTimesMs"]=json!([500]);input["skillData"][0]["hits"][0]["actionDelayMs"]=json!(30);
        input["skillData"][0]["hits"][0]["fixedDelayMs"]=json!(400);
        input["character"]["skills"][0]["tripods"]=json!([]);
        if !other.is_empty() {input["character"]["skills"][0]["tripods"]=json!([{"name":other,"level":1}]);}
        let baseline=builder::build(&input.to_string()).unwrap();
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"덫 강화","level":1,"values":[999999]}));
        let selected=builder::build(&input.to_string()).unwrap();
        let old=&baseline.skills[0];let new=&selected.skills[0];
        assert_eq!(new.hits.len(),1);assert_eq!(new.hits[0].effect_id,265730);
        let mut expected=old.hits[0].clone();expected.effect_id=265730;
        assert_eq!(serde_json::to_value(&new.hits[0]).unwrap(),serde_json::to_value(expected).unwrap());
        assert_eq!((&new.cast_times_ms,new.cooldown_ms,new.max_stacks,new.charge_recovery_ms),(&old.cast_times_ms,old.cooldown_ms,old.max_stacks,old.charge_recovery_ms));
        for fast in [false,true] {
            let mut a=baseline.clone();let mut b=selected.clone();
            if fast {a.stat.attack_speed.insert("test".into(),50.);b.stat.attack_speed.insert("test".into(),50.);}
            let (ra,ta)=engine::trace_one(&a,42,10000);let (rb,tb)=engine::trace_one(&b,42,10000);
            assert_eq!(ra.total_damage,rb.total_damage);assert_eq!(serde_json::to_value(ta.damage_events).unwrap(),serde_json::to_value(tb.damage_events).unwrap());
        }
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        assert_eq!(serde_json::to_value(engine::trace_one(&selected,42,10000).1.damage_events).unwrap(),serde_json::to_value(engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,10000).1.damage_events).unwrap());
        let mut missed=selected.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=vec![new.hits[0].hit_id.clone()];
        assert!(engine::trace_one(&missed,42,10000).1.damage_events.is_empty());
    }
}

#[test]
fn reaper_trap_snare_composes_range_and_only_boosts_extra_callback() {
    for other in ["", "연속 공격", "그림자 활성"] {for range in [false,true] {
        let mut input=corrosion_input(26570,"쉐도우 트랩",&[265700]);input["skillData"][0]["cooldown"]=json!(20);
        input["character"]["skills"][0]["tripods"]=json!([{"name":"그림자 올가미","level":1,"values":[999999]}]);
        if !other.is_empty() {input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":other,"level":1}));}
        let plain=builder::build(&input.to_string()).unwrap();
        if range {input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"덫 강화","level":1}));}
        input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{"26570":{"dark":0.01,"heat":0.01}}});
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];
        assert_eq!(skill.hits.len(),2);assert_eq!(skill.cast_times_ms,vec![500]);
        assert_eq!((skill.hits[0].effect_id,skill.hits[1].effect_id),if range {(265765,265764)} else {(265762,265761)});
        assert_eq!((&skill.hits[0].hit_id,&skill.hits[1].hit_id),(&"26570:0:notify:265762".to_string(),&"26570:0:0".to_string()));
        assert_eq!((skill.hits[0].action_delay_ms,skill.hits[0].fixed_delay_ms,skill.hits[1].fixed_delay_ms),(30,200,400));
        assert!(((1.+skill.hits[0].damage_increase)/(1.+skill.hits[1].damage_increase)-2.25).abs()<1e-12);
        assert_eq!((skill.hits[0].skill_modifier,skill.hits[1].skill_modifier),(0.8112,4.056));
        for (a,b) in skill.hits.iter().zip(&plain.skills[0].hits) {assert_eq!((a.base_damage,a.skill_modifier,a.damage_increase),(b.base_damage,b.skill_modifier,b.damage_increase));}
        for fast in [false,true] {
            let mut current=profile.clone();if fast {current.stat.attack_speed.insert("test".into(),50.);}
            let (_,execution)=engine::trace_one(&current,42,10000);let first=&execution.casts[0];
            let hits:Vec<_>=execution.damage_events.iter().filter(|h|h.cast_id==Some(first.cast_id)).collect();
            assert_eq!(hits.len(),2);assert!((hits[1].time-hits[0].time-0.2).abs()<1e-12);
            for h in hits {assert!(h.resource_gains.iter().any(|g|g.resource=="ReaperDark" && (g.amount-0.01).abs()<1e-12));}
        }
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        assert_eq!(serde_json::to_value(engine::trace_one(&profile,42,10000).1.damage_events).unwrap(),serde_json::to_value(engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,10000).1.damage_events).unwrap());
        let mut missed=profile.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();
        assert!(engine::trace_one(&missed,42,10000).1.damage_events.is_empty());
        input["character"]["skills"][0]["tripods"]=json!([{"name":"그림자 올가미","level":1},{"name":"그림자 늪","level":1}]);
        assert!(builder::build(&input.to_string()).unwrap_err().contains("2단계"));
    }}
}

#[test]
fn reaper_graffiti_snare_preserves_main_and_composes_range_element_and_chaos() {
    for other in ["", "급습 강화", "혼돈 강화"] {for range in [false,true] {
        let mut input=corrosion_input(26800,"라스트 그래피티",&[268000]);
        input["character"]["skills"][0]["tripods"]=json!([{"name":"그림자 올가미","level":1,"values":[999999]}]);
        if !other.is_empty() {input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":other,"level":1}));}
        if range {input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().push(json!({"name":"범위 증가","level":1}));}
        let profile=builder::build(&input.to_string()).unwrap();let skill=&profile.skills[0];
        assert_eq!(skill.cast_times_ms,vec![1700]);assert_eq!(skill.hits.len(),2);
        assert_eq!((skill.hits[0].effect_id,skill.hits[1].effect_id),if range {(268082,268010)} else {(268081,268000)});
        assert_eq!((skill.hits[0].action_delay_ms,skill.hits[1].action_delay_ms),(181,933));
        assert_eq!((skill.hits[0].skill_modifier,skill.hits[1].skill_modifier),(6.1192,20.397));
        assert!(((1.+skill.hits[0].damage_increase)/(1.+skill.hits[1].damage_increase)-2.).abs()<1e-12);
        let mut gaps=Vec::new();
        for fast in [false,true] {
            let mut current=profile.clone();if fast {current.stat.attack_speed.insert("test".into(),50.);}
            let (_,execution)=engine::trace_one(&current,42,10000);let first=&execution.casts[0];
            let hits:Vec<_>=execution.damage_events.iter().filter(|h|h.cast_id==Some(first.cast_id)).collect();
            assert_eq!(hits.len(),2);assert!(hits.iter().all(|h|h.resource_gains.is_empty()));
            gaps.push(hits[1].time-hits[0].time);
        }
        assert!(gaps[1]>0. && gaps[1]<gaps[0]);
        input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();
        assert_eq!(serde_json::to_value(engine::trace_one(&profile,42,10000).1.damage_events).unwrap(),serde_json::to_value(engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,10000).1.damage_events).unwrap());
        let mut missed=profile.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=skill.hits.iter().map(|h|h.hit_id.clone()).collect();
        assert!(engine::trace_one(&missed,42,10000).1.damage_events.is_empty());
        input["character"]["skills"][0]["tripods"]=json!([{"name":"그림자 올가미","level":1},{"name":"유언","level":1}]);
        assert!(builder::build(&input.to_string()).is_err());
    }}
}


#[test]
fn reaper_swift_chaos_sun_accumulates_all_grades_and_thresholds_with_order_star() {
    for thirst in [false,true] { for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {
        for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&p|tier>0||p<=15) {
            let mut input=configured(thirst);let baseline=builder::build(&input.to_string()).unwrap();
            let mut swift=grid_slot("재빠른 공격",grade,points);swift["coreAlignment"]=json!("혼돈");swift["coreSymbol"]=json!("해");
            for gem in swift["gems"].as_array_mut().unwrap(){gem["name"]=json!("침식");}
            input["character"]["arkGrids"]=json!({"reaperSlots":[swift.clone()]});let selected=builder::build(&input.to_string()).unwrap();
            let speed=if points>=10 {1.} else {0.}+if points>=17&&tier>=2 {if tier==2 {2.} else {3.}} else {0.};
            let crit=if points>=14&&tier>=1 {1.4} else {0.}+if points>=17&&tier>=2 {if tier==2 {2.8} else {5.6}} else {0.}+if tier>=2 {points.saturating_sub(17) as f64*0.45} else {0.};
            assert!((selected.stat.sum(StatField::AttackSpeed)-baseline.stat.sum(StatField::AttackSpeed)-speed/100.).abs()<1e-12);
            assert!((selected.stat.sum(StatField::CritDmg)-baseline.stat.sum(StatField::CritDmg)-crit).abs()<1e-12);
            assert_eq!(serde_json::to_value(&selected.skills).unwrap(),serde_json::to_value(&baseline.skills).unwrap());
            let order=grid_slot(if thirst {"급습 악몽"} else {"할루시네이션"},"ancient",20);input["character"]["arkGrids"]=json!({"reaperSlots":[order.clone(),swift.clone()]});let combined=builder::build(&input.to_string()).unwrap();
            input["character"]["arkGrids"]=json!({"reaperSlots":[swift,order]});let reversed=builder::build(&input.to_string()).unwrap();assert_eq!(serde_json::to_value(&combined.stat).unwrap(),serde_json::to_value(&reversed.stat).unwrap());assert_eq!(serde_json::to_value(&combined.skills).unwrap(),serde_json::to_value(&reversed.skills).unwrap());
        }
    }}
}

#[test]
fn reaper_swift_core_changes_real_cast_speed_and_only_critical_damage() {
    for thirst in [false,true] {
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);input["reaperBuild"]=json!(if thirst {"thirst_chaos_sustain"} else {"dark_shadow_dot_legacy"});
        input["character"]["skills"][0]["tripods"]=json!([]);input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");let baseline=builder::build(&input.to_string()).unwrap();
        let mut swift=grid_slot("재빠른 공격","ancient",20);swift["coreAlignment"]=json!("혼돈");swift["coreSymbol"]=json!("해");for gem in swift["gems"].as_array_mut().unwrap(){gem["name"]=json!("침식");}
        input["character"]["arkGrids"]=json!({"reaperSlots":[swift]});let selected=builder::build(&input.to_string()).unwrap();
        let fast=engine::trace_one(&selected,42,10000).1;let slow=engine::trace_one(&baseline,42,10000).1;assert!(fast.damage_events[0].time<slow.damage_events[0].time);
        for critical in [false,true] {
            let mut p=selected.clone();p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),if critical {100.} else {-100.});let mut old=p.clone();old.stat.crit_dmg=baseline.stat.crit_dmg.clone();
            let hits=engine::trace_one(&p,42,10000).1.damage_events;let old_hits=engine::trace_one(&old,42,10000).1.damage_events;assert!(!hits.is_empty());
            for (hit,plain) in hits.iter().zip(&old_hits) {assert_eq!(hit.is_crit,critical);assert_eq!(hit.time,plain.time);let ratio=if critical {hit.formula.crit_multiplier.unwrap()/plain.formula.crit_multiplier.unwrap()} else {1.};assert!((hit.result_damage/plain.result_damage-ratio).abs()<1e-12);if critical {assert!((hit.formula.crit_multiplier.unwrap()-plain.formula.crit_multiplier.unwrap()-0.0835).abs()<1e-12);}}
        }
    }
}


#[test]
fn reaper_flamboyant_core_replaces_ability_totals_across_grades_and_thresholds() {
    for thirst in [false,true] {for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {
        for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&p|tier>0||p<=15) {
            let mut input=configured(thirst);let baseline=builder::build(&input.to_string()).unwrap();
            let mut core=grid_slot("현란한 공격",grade,points);core["coreAlignment"]=json!("혼돈");core["coreSymbol"]=json!("해");for gem in core["gems"].as_array_mut().unwrap(){gem["name"]=json!("침식");}
            input["character"]["arkGrids"]=json!({"reaperSlots":[core]});let p=builder::build(&input.to_string()).unwrap();
            let upgraded=tier>=2&&points>=17;
            let crit=if points<10 {0.} else if upgraded {if tier==2 {0.011} else {0.0165}} else {0.0055};
            let damage=(if tier<1||points<14 {0.} else if upgraded {if tier==2 {0.015} else {0.02}} else {0.005})+if tier>=2 {points.saturating_sub(17) as f64*0.0016} else {0.};
            assert!((p.stat.on_crit_damage_bonus-baseline.stat.on_crit_damage_bonus-crit).abs()<1e-12);
            assert!((p.stat.sum(StatField::TargetDamageIncrease)-baseline.stat.sum(StatField::TargetDamageIncrease)-damage).abs()<1e-12);
            assert_eq!(p.stat.sum(StatField::CritDmg),baseline.stat.sum(StatField::CritDmg));assert_eq!(serde_json::to_value(&p.skills).unwrap(),serde_json::to_value(&baseline.skills).unwrap());
        }
    }}
}

#[test]
fn reaper_flamboyant_core_uses_generic_damage_and_critical_hit_multiplier_with_other_slots() {
    for thirst in [false,true] {
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);input["reaperBuild"]=json!(if thirst {"thirst_chaos_sustain"} else {"dark_shadow_dot_legacy"});input["character"]["skills"][0]["tripods"]=json!([]);input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");let baseline=builder::build(&input.to_string()).unwrap();
        let mut core=grid_slot("현란한 공격","ancient",20);core["coreAlignment"]=json!("혼돈");core["coreSymbol"]=json!("해");for gem in core["gems"].as_array_mut().unwrap(){gem["name"]=json!("침식");}
        input["character"]["arkGrids"]=json!({"reaperSlots":[core.clone()]});let selected=builder::build(&input.to_string()).unwrap();
        for critical in [false,true] {let mut p=selected.clone();let mut old=baseline.clone();for profile in [&mut p,&mut old]{profile.stat.crit_rate.clear();profile.stat.crit_rate.insert("test".into(),if critical {100.} else {-100.});}
            let hits=engine::trace_one(&p,42,10000).1.damage_events;let previous=engine::trace_one(&old,42,10000).1.damage_events;assert!(!hits.is_empty());assert_eq!(hits.len(),previous.len());
            for (h,b) in hits.iter().zip(&previous) {assert_eq!(h.time,b.time);assert_eq!(h.is_crit,critical);assert_eq!(h.formula.crit_multiplier,b.formula.crit_multiplier);let generic=(1.+baseline.stat.sum(StatField::TargetDamageIncrease)+0.0248)/(1.+baseline.stat.sum(StatField::TargetDamageIncrease));let ratio=generic*if critical {1.0165} else {1.};assert!((h.result_damage/b.result_damage-ratio).abs()<1e-12);assert_eq!(h.formula.on_crit_damage_multiplier,if critical {Some(1.0165)} else {None});}
        }
        let order=grid_slot(if thirst {"급습 악몽"} else {"할루시네이션"},"ancient",20);let mut star=grid_slot("속도","ancient",20);star["coreAlignment"]=json!("혼돈");for gem in star["gems"].as_array_mut().unwrap(){gem["name"]=json!("침식");}
        input["character"]["arkGrids"]=json!({"reaperSlots":[order.clone(),core.clone(),star.clone()]});let combined=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[star,core.clone(),order]});let reversed=builder::build(&input.to_string()).unwrap();assert_eq!(serde_json::to_value(combined.stat).unwrap(),serde_json::to_value(reversed.stat).unwrap());assert_eq!(serde_json::to_value(combined.skills).unwrap(),serde_json::to_value(reversed.skills).unwrap());
        let mut swift=core.clone();swift["name"]=json!("재빠른 공격");input["character"]["arkGrids"]=json!({"reaperSlots":[core,swift]});assert!(builder::build(&input.to_string()).unwrap_err().contains("문양"));
    }
}


#[test]
fn reaper_burning_core_checks_all_ability_levels_boss_gating_and_one_replaced_dot() {
    use crate::profile::TriggerEffect;
    for thirst in [false,true] {for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&p|tier>0||p<=15) {for seed in [false,true] {
        let mut input=configured(thirst);input["assumedTargetSeedOrLower"]=json!(seed);let baseline=builder::build(&input.to_string()).unwrap();
        let mut core=grid_slot("불타는 일격",grade,points);core["coreAlignment"]=json!("혼돈");core["coreSymbol"]=json!("달");for gem in core["gems"].as_array_mut().unwrap(){gem["name"]=json!("침식");}input["character"]["arkGrids"]=json!({"reaperSlots":[core]});let p=builder::build(&input.to_string()).unwrap();
        let upgraded=tier>=2&&points>=17;let bonus=if seed {0.} else {(if tier<1||points<14 {0.} else if upgraded {if tier==2 {0.015} else {0.025}} else {0.005})+if tier>=2 {points.saturating_sub(17) as f64*0.0016} else {0.}};
        assert!((p.stat.sum(StatField::TargetDamageIncrease)-baseline.stat.sum(StatField::TargetDamageIncrease)-bonus).abs()<1e-12);
        let burns:Vec<_>=p.global_triggers.iter().filter(|t|matches!(&t.effect,TriggerEffect::ScheduleDot{dot_id,..} if dot_id=="608211000")).collect();assert_eq!(burns.len(),usize::from(points>=10&&!seed));
        if let Some(t)=burns.first(){if let TriggerEffect::ScheduleDot{hit,first_tick_ms,tick_interval_ms,tick_count,refresh,independent,..}=&t.effect {assert_eq!((*first_tick_ms,*tick_interval_ms,*tick_count,*refresh,*independent),(500,1000,6,true,true));assert_eq!((hit.base_damage,hit.skill_modifier),if upgraded {if tier==2 {(351.,1.725)} else {(438.5,2.1562)}} else {(175.5,0.8625)});}}
        assert_eq!(serde_json::to_value(&p.skills).unwrap(),serde_json::to_value(&baseline.skills).unwrap());
    }}}}
}

fn burning_input(thirst:bool) -> Value {
    let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);input["reaperBuild"]=json!(if thirst {"thirst_chaos_sustain"} else {"dark_shadow_dot_legacy"});input["character"]["skills"][0]["tripods"]=json!([]);input["skillData"][0]["cooldown"]=json!(300);input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");
    let mut core=grid_slot("불타는 일격","ancient",20);core["coreAlignment"]=json!("혼돈");core["coreSymbol"]=json!("달");for gem in core["gems"].as_array_mut().unwrap(){gem["name"]=json!("침식");}input["character"]["arkGrids"]=json!({"reaperSlots":[core]});input
}

#[test]
fn reaper_burning_dot_keeps_six_ticks_without_source_skill_effects_gauge_or_destruction() {
    use crate::profile::TriggerEffect;
    for thirst in [false,true] {for critical in [false,true] {
        let mut p=builder::build(&burning_input(thirst).to_string()).unwrap();p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),if critical {100.} else {-100.});let trace=engine::trace_one(&p,42,8000).1;let direct=trace.damage_events.iter().find(|h|h.hit_id!="burning_strike_tick").unwrap();let burns:Vec<_>=trace.damage_events.iter().filter(|h|h.hit_id=="burning_strike_tick").collect();assert_eq!(burns.len(),6);
        for (i,h) in burns.iter().enumerate(){assert!((h.time-direct.time-0.5-i as f64).abs()<1e-12);assert_eq!(h.damage_source,"dot:608211000");assert_eq!(h.is_crit,critical);assert_eq!((h.formula.base_damage,h.formula.skill_modifier),(438.5,2.1562));assert!(h.resource_gains.is_empty());assert_eq!(h.destruction_level,0);}
        let mut changed=p.clone();changed.skills[0].evolution_damage_bonuses.push(("owner-only".into(),999.));for hit in &mut changed.skills[0].hits{hit.damage_increase+=999.;hit.crit_damage_bonus+=999.;hit.defense_ignore=1.;}let changed_trace=engine::trace_one(&changed,42,8000).1;assert_ne!(changed_trace.damage_events[0].result_damage,direct.result_damage);assert_eq!(serde_json::to_value(&burns).unwrap(),serde_json::to_value(changed_trace.damage_events.iter().filter(|h|h.hit_id=="burning_strike_tick").collect::<Vec<_>>()).unwrap());
        let index=changed.global_triggers.iter().position(|t|matches!(&t.effect,TriggerEffect::ScheduleDot{dot_id,..} if dot_id=="608211000")).unwrap();let mut old=serde_json::to_value(&changed.global_triggers[index].effect).unwrap();old["ScheduleDot"].as_object_mut().unwrap().remove("independent");changed.global_triggers[index].effect=serde_json::from_value(old).unwrap();assert!(serde_json::to_value(&changed.global_triggers[index].effect).unwrap()["ScheduleDot"].get("independent").is_none());let legacy=engine::trace_one(&changed,42,8000).1;assert!(legacy.damage_events.iter().find(|h|h.hit_id=="burning_strike_tick").unwrap().result_damage>burns[0].result_damage*100.);
        let mut missed=p.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=p.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();assert!(engine::trace_one(&missed,42,8000).1.damage_events.is_empty());
    }}
}

#[test]
fn reaper_burning_refreshes_one_shared_timer_and_resets_after_iterations() {
    use crate::profile::TriggerEffect;
    let mut input=burning_input(true);input["skillData"][0]["cooldown"]=json!(1);let mut p=builder::build(&input.to_string()).unwrap();p.skills[0].max_uses=2;let trace=engine::trace_one(&p,42,8000).1;let direct:Vec<_>=trace.damage_events.iter().filter(|h|h.hit_id!="burning_strike_tick").collect();assert_eq!(direct.len(),2);let burns:Vec<_>=trace.damage_events.iter().filter(|h|h.hit_id=="burning_strike_tick").collect();assert_eq!(burns.len(),7);for (i,h) in burns[1..].iter().enumerate(){assert!((h.time-direct[1].time-0.5-i as f64).abs()<1e-12);}
    let result=engine::simulate(&p,3,8000,42,None);assert_eq!(result.skill_stats[&0].hit_count,27);for _ in 0..3 {assert_eq!(engine::trace_one(&p,42,8000).0.skill_stats[&0].hit_count,9);}
    let mut mixed=burning_input(true);let burn=mixed["character"]["arkGrids"]["reaperSlots"][0].clone();let mut order=grid_slot("급습 악몽","ancient",20);let mut sun=grid_slot("현란한 공격","ancient",20);sun["coreAlignment"]=json!("혼돈");sun["coreSymbol"]=json!("해");let mut star=grid_slot("속도","ancient",20);star["coreAlignment"]=json!("혼돈");let mut burn=burn;
    for slot in [&mut burn,&mut sun,&mut star]{for gem in slot["gems"].as_array_mut().unwrap(){gem["name"]=json!("침식");}}
    for slot in [&mut burn,&mut order]{for gem in slot["gems"].as_array_mut().unwrap(){gem["effects"]=json!([{"name":"공격력","level":5},{"name":"추가 피해","level":5}]);}}
    mixed["character"]["arkGrids"]=json!({"reaperSlots":[burn.clone(),order.clone(),sun.clone(),star.clone()]});let joined=builder::build(&mixed.to_string()).unwrap();assert!((joined.stat.sum(StatField::TargetDamageIncrease)-0.0546).abs()<1e-12);assert!((joined.stat.on_crit_damage_bonus-0.0165).abs()<1e-12);assert!((joined.stat.sum(StatField::AttackPowerMul)-p.stat.sum(StatField::AttackPowerMul)-0.0146).abs()<1e-12);assert!((joined.stat.sum(StatField::AdditionalDamage)-p.stat.sum(StatField::AdditionalDamage)-0.0323).abs()<1e-12);
    mixed["character"]["arkGrids"]=json!({"reaperSlots":[star,sun,order,burn]});let reversed=builder::build(&mixed.to_string()).unwrap();assert_eq!(serde_json::to_value(&joined.stat).unwrap(),serde_json::to_value(&reversed.stat).unwrap());assert_eq!(serde_json::to_value(&joined.skills).unwrap(),serde_json::to_value(&reversed.skills).unwrap());assert_eq!(serde_json::to_value(&joined.global_triggers).unwrap(),serde_json::to_value(&reversed.global_triggers).unwrap());
    input["assumedTargetSeedOrLower"]=json!(true);let off=builder::build(&input.to_string()).unwrap();assert!(off.global_triggers.iter().all(|t|!matches!(&t.effect,TriggerEffect::ScheduleDot{dot_id,..} if dot_id=="608211000")));assert!(engine::trace_one(&off,42,8000).1.damage_events.iter().all(|h|h.hit_id!="burning_strike_tick"));
}

fn flowing_slot(grade:&str,points:u32) -> Value {
    let mut slot=grid_slot("흐르는 마나",grade,points);slot["coreAlignment"]=json!("혼돈");slot["coreSymbol"]=json!("해");for gem in slot["gems"].as_array_mut().unwrap(){gem["name"]=json!("침식");}slot
}

#[test]
fn reaper_flowing_core_accumulates_all_thresholds_into_max_mp_and_one_cooldown_factor() {
    use crate::engine::state::class::{ClassState,reaper::ReaperState};
    for thirst in [false,true] {for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&p|tier>0||p<=15) {
        let mut input=configured(thirst);let old=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[flowing_slot(grade,points)]});let p=builder::build(&input.to_string()).unwrap();
        let mana=if points>=10 {80.} else {0.}+if tier>=2&&points>=17 {if tier==2 {160.} else {240.}} else {0.};
        let percent=if tier>=1&&points>=14 {0.4} else {0.}+if tier>=2&&points>=17 {if tier==2 {0.8} else {1.6}} else {0.}+if tier>=2 {points.saturating_sub(17) as f64*0.13} else {0.};
        assert_eq!(p.stat.max_mana,old.stat.max_mana+mana);assert_eq!(p.max_mp,old.max_mp+mana);
        let state=ReaperState::from_reference(p.max_mp,p.reaper_reference.as_ref().unwrap());assert_eq!(state.get_resource("Mp"),p.max_mp);assert_eq!(state.get_resource_max("Mp"),p.max_mp);
        for (skill,base) in p.skills.iter().zip(&old.skills) {let id=skill.tags.iter().find_map(|tag|if let crate::profile::SkillTag::SkillId(id)=tag {Some(*id)} else {None}).unwrap();let factor=if id==26040 {1.} else {1.-percent/100.};assert!((skill.cooldown_ms as f64-base.cooldown_ms as f64*factor).abs()<2.);assert!(!skill.resource_costs.contains_key("Mp"));assert_eq!(serde_json::to_value(&skill.hits).unwrap(),serde_json::to_value(&base.hits).unwrap());}
    }}}
}

#[test]
fn reaper_flowing_core_uses_original_group_for_all_27_skill_ids_including_identity_and_t() {
    let data:Value=serde_json::from_str(include_str!("../skill-data/reaper/backend/simulation-data.json")).unwrap();
    for raw in data["skills"].as_array().unwrap() {let id=raw["gameSkillId"].as_u64().unwrap();let name=raw["name"].as_str().unwrap();let mut input=corrosion_input(id as u32,name,&[]);input["reaperBuild"]=json!(if id==26980 {"thirst_chaos_sustain"} else {"dark_shadow_dot_legacy"});input["skillData"][0]["cooldown"]=json!(raw["cooldownMs"].as_f64().unwrap()/1000.);input["skillData"][0]["skillSlot"]=json!(match raw["skillSlot"].as_str().unwrap() {"각성 스킬"=>"Awakening","초각성기"=>"SuperAwakening","초각성 스킬"=>"HyperAwakeningTechniques",_=>"Normal"});input["character"]["skills"]=json!([{"name":name,"level":if [26040,26980].contains(&id) {1} else {12},"tripods":[]}]);input["aplConfig"]=json!(format!("actions:\n - skill: '{name}'\n"));let old=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[flowing_slot("ancient",20)]});let p=builder::build(&input.to_string()).unwrap();
        let affected=![26040,26900,26910,26940,26950].contains(&id);let factor=if affected {0.9761} else {1.};assert!((p.skills[0].cooldown_ms as f64-old.skills[0].cooldown_ms as f64*factor).abs()<2.,"{name}");if !affected {assert_eq!(p.skills[0].cooldown_ms,old.skills[0].cooldown_ms);}assert_eq!(serde_json::to_value(&p.skills[0].hits).unwrap(),serde_json::to_value(&old.skills[0].hits).unwrap());
    }
}

#[test]
fn reaper_flowing_core_changes_recasts_and_charge_recovery_with_other_slots_without_mana_costs() {
    for thirst in [false,true] {
        let mut input=corrosion_input(26050,"스피닝 대거",&[260500,260501]);input["reaperBuild"]=json!(if thirst {"thirst_chaos_sustain"} else {"dark_shadow_dot_legacy"});input["skillData"][0]["cooldown"]=json!(12);input["character"]["skills"][0]["tripods"]=json!([]);input["aplConfig"]=json!("actions:\n - skill: 스피닝 대거\n");let old=builder::build(&input.to_string()).unwrap();
        let core=flowing_slot("ancient",20);input["character"]["arkGrids"]=json!({"reaperSlots":[core.clone()]});let p=builder::build(&input.to_string()).unwrap();let plain=engine::trace_one(&old,42,30000).1;let faster=engine::trace_one(&p,42,30000).1;assert!(faster.casts[1].time<plain.casts[1].time);assert_eq!(faster.damage_events[0].result_damage,plain.damage_events[0].result_damage);
        let mut without_mp=p.clone();without_mp.stat.max_mana=old.stat.max_mana;without_mp.max_mp=old.max_mp;assert_eq!(serde_json::to_value(engine::trace_one(&p,42,30000)).unwrap(),serde_json::to_value(engine::trace_one(&without_mp,42,30000)).unwrap());
        input["character"]["skills"][0]["tripods"]=json!([{"name":"회전력 강화","level":1},{"name":"끝없는 춤","level":1}]);input["character"]["arkGrids"]=Value::Null;let charged=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[core.clone()]});let selected=builder::build(&input.to_string()).unwrap();assert_eq!(selected.skills[0].charge_recovery_ms,(charged.skills[0].charge_recovery_ms as f64*0.9761) as u32);assert_eq!(selected.skills[0].max_stacks,2);
        let order=grid_slot(if thirst {"급습 악몽"} else {"할루시네이션"},"ancient",20);let mut burn=flowing_slot("ancient",20);burn["name"]=json!("불타는 일격");burn["coreSymbol"]=json!("달");input["character"]["arkGrids"]=json!({"reaperSlots":[core.clone(),order.clone(),burn.clone()]});let combined=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[burn,order,core]});let reversed=builder::build(&input.to_string()).unwrap();assert_eq!(serde_json::to_value(combined.stat).unwrap(),serde_json::to_value(reversed.stat).unwrap());assert_eq!(serde_json::to_value(combined.skills).unwrap(),serde_json::to_value(reversed.skills).unwrap());
    }
}

fn destiny_slot(name:&str,grade:&str,points:u32)->Value {
    let mut core=grid_slot(name,grade,points);core["coreSymbol"]=json!(if name=="페르소나" {"달"} else {"해"});core
}

fn destiny_input()->Value {
    let mut input=corrosion_input(26810,"레이지 스피어",&[268100,268100,268100]);input["reaperBuild"]=json!("dark_shadow_dot_legacy");input["character"]["skills"][0]["tripods"]=json!([]);input["skillData"][0]["cooldown"]=json!(3);
    let mut persona=input["skillData"][0].clone();persona["gameSkillId"]=json!(26040);persona["name"]=json!("페르소나");persona["hits"]=json!([]);persona["cooldown"]=json!(0);persona["castTimesMs"]=json!([100]);
    input["skillData"].as_array_mut().unwrap().push(persona);input["character"]["skills"].as_array_mut().unwrap().push(json!({"name":"페르소나","level":1,"tripods":[]}));
    input["reaperIdentityModel"]=json!({"initialDark":1,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"heatLossPerSecond":0,"personaDarkLossPerSecond":0,"skillGains":{}});input["aplConfig"]=json!("actions:\n - skill: 페르소나\n - skill: 레이지 스피어\n");input
}

#[test]
fn reaper_destiny_cores_cover_all_grades_thresholds_static_damage_and_root_requirements() {
    for name in ["달의 향기","페르소나"] {for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate(){for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&p|tier>0||p<=15) {
        let mut input=destiny_input();let old=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[destiny_slot(name,grade,points)]});let p=builder::build(&input.to_string()).unwrap();
        let factor=(if points>=10 {1.016} else {1.})*(if tier>=2 {(1.0017_f64).powi(points.saturating_sub(17) as i32)} else {1.})*(if name=="달의 향기"&&tier>=1&&points>=14 {1.03} else {1.});
        for (h,b) in p.skills[0].hits.iter().zip(&old.skills[0].hits){assert!(((1.+h.damage_increase)/(1.+b.damage_increase)-factor).abs()<1e-12);}
        let condition=tier>=2&&points>=17&&name=="페르소나";assert_eq!(p.skills[0].cast_resource_damage_bonuses.len(),usize::from(condition));if condition {assert_eq!(p.skills[0].cast_resource_damage_bonuses[0],("ReaperPersona".into(),if tier==2 {0.035} else {0.043}));}
        let edge=p.skills[0].cast_buff_damage_bonuses.first();assert_eq!(edge.is_some(),tier>=2&&points>=17&&name=="달의 향기");
        let mut wrong=input.clone();wrong["reaperBuild"]=json!("thirst_chaos_sustain");wrong["character"]["skills"].as_array_mut().unwrap().retain(|s|s["name"]!="페르소나");assert!(builder::build(&wrong.to_string()).is_err());
    }}}
}

#[test]
fn reaper_destiny_pair_produces_expires_and_consumes_one_rush_even_on_miss() {
    for (grade,points,bonus) in [("hero",10,0.),("legend",14,0.),("relic",20,0.035),("ancient",20,0.043)] {
        let mut input=destiny_input();input["character"]["arkGrids"]=json!({"reaperSlots":[destiny_slot("달의 향기",grade,points),destiny_slot("페르소나",grade,points)]});let mut p=builder::build(&input.to_string()).unwrap();p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),-100.);
        let (_,trace)=engine::trace_one(&p,42,13000);let rush=trace.casts.iter().filter(|c|c.skill_name=="레이지 스피어").collect::<Vec<_>>();assert!(rush.len()>=4);
        let has_source=grade!="hero";let has_edge=bonus>0.;let first_hits=trace.damage_events.iter().filter(|h|h.cast_id==Some(rush[0].cast_id)).collect::<Vec<_>>();assert_eq!(first_hits.len(),3);
        for hit in first_hits {let factor=hit.formula.factors.iter().find(|f|f.name=="damageIncrease").unwrap();assert_eq!(factor.sources.iter().filter(|s|s.source=="cast:resource:ReaperPersona").count(),usize::from(has_edge));assert_eq!(factor.sources.iter().filter(|s|s.source.starts_with("cast:buff:")).count(),usize::from(has_edge));if has_edge {assert!((factor.sources.iter().filter(|s|s.source.starts_with("cast:")).map(|s|s.value).sum::<f64>()-2.*bonus).abs()<1e-12);}}
        let later=trace.damage_events.iter().filter(|h|h.cast_id!=Some(rush[0].cast_id));for hit in later {assert!(hit.formula.factors.iter().flat_map(|f|&f.sources).all(|s|!s.source.starts_with("cast:")));}
        assert!(trace.damage_events.iter().any(|h|h.time>10.));for hit in &trace.damage_events {let active=hit.formula.factors.iter().flat_map(|f|&f.sources).any(|s|s.source=="buff:3260100");assert_eq!(active,has_source&&hit.time<10.,"{grade} time={} sources={:?}",hit.time,hit.formula.factors.iter().flat_map(|f|&f.sources).map(|s|&s.source).collect::<Vec<_>>());}
        if has_edge {p.reaper_reference.as_mut().unwrap().missed_hit_ids=p.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();let missing=engine::trace_one(&p,42,13000).1;assert!(missing.damage_events.is_empty());assert!(missing.casts.iter().skip(2).all(|c|!c.active_buffs.contains(&if grade=="relic" {"3260200".into()} else {"3260201".into()})));}
    }
}

#[test]
fn reaper_destiny_pair_ignores_external_skill_groups_and_keeps_non_rush_and_slot_order() {
    let mut input=destiny_input();let mut normal=input["skillData"][0].clone();normal["gameSkillId"]=json!(26070);normal["name"]=json!("쉐도우 닷");normal["properties"]["skillGroupIds"]=json!([2260200]);normal["hits"]=json!([]);input["skillData"].as_array_mut().unwrap().push(normal);input["character"]["skills"].as_array_mut().unwrap().push(json!({"name":"쉐도우 닷","level":12,"tripods":[]}));
    let a=destiny_slot("달의 향기","ancient",20);let b=destiny_slot("페르소나","ancient",20);input["character"]["arkGrids"]=json!({"reaperSlots":[a.clone(),b.clone()]});let p=builder::build(&input.to_string()).unwrap();assert!(p.skills.iter().find(|s|s.name=="쉐도우 닷").unwrap().cast_buff_damage_bonuses.is_empty());input["character"]["arkGrids"]=json!({"reaperSlots":[b,a]});let reversed=builder::build(&input.to_string()).unwrap();assert_eq!(serde_json::to_value(&p.skills).unwrap(),serde_json::to_value(&reversed.skills).unwrap());assert_eq!(serde_json::to_value(&p.global_triggers).unwrap(),serde_json::to_value(&reversed.global_triggers).unwrap());assert_eq!(serde_json::to_value(engine::trace_one(&p,42,13000)).unwrap(),serde_json::to_value(engine::trace_one(&reversed,42,13000)).unwrap());
}

fn fatal_destiny_input() -> Value {
    let mut input=corrosion_input(26810,"레이지 스피어",&[268100,268100,268100]);
    input["character"]["skills"][0]["tripods"]=json!([]);input["skillData"][0]["cooldown"]=json!(1);
    for (id,name,cooldown) in [(26980,"페이탈 스텝",40),(26600,"쉐도우 스톰",5)] {
        let mut data=input["skillData"][0].clone();data["gameSkillId"]=json!(id);data["name"]=json!(name);data["cooldown"]=json!(cooldown);data["hits"]=json!([]);
        input["skillData"].as_array_mut().unwrap().push(data);input["character"]["skills"].as_array_mut().unwrap().push(json!({"name":name,"level":1,"tripods":[]}));
    }
    input["reaperIdentityModel"]=json!({"initialDark":1,"initialHeat":1,"initialChaosSeconds":15,"darkPerSecond":0,"heatLossPerSecond":0,"personaDarkLossPerSecond":0,"skillGains":{}});
    input["aplConfig"]=json!("actions:\n - skill: 페이탈 스텝\n - skill: 쉐도우 스톰\n - skill: 레이지 스피어\n");input
}

#[test]
fn reaper_remaining_hero_cores_apply_whole_damage_and_cooldown_contracts_at_ten_points() {
    for name in ["달의 낙하","갈증의 악몽","피의 갈증"] {for points in [9,10,14,15] {
        let moon=name=="달의 낙하";let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);
        if moon {input["reaperBuild"]=json!("dark_shadow_dot_legacy");}
        input["character"]["skills"][0]["tripods"]=json!([]);input["skillData"][0]["cooldown"]=json!(20);
        for (id,label) in [(26800,"라스트 그래피티"),(26820,"사일런트 스매셔"),(26830,"댄싱 오브 퓨리"),(26970,"피니쉬 스텝"),(26600,"쉐도우 스톰"),(26050,"스피닝 대거")] {
            let mut skill=input["skillData"][0].clone();skill["gameSkillId"]=json!(id);skill["name"]=json!(label);skill["hits"][0]["effectId"]=json!(id*10);skill["hits"][0]["hitId"]=json!(format!("hero:{id}"));
            input["skillData"].as_array_mut().unwrap().push(skill);input["character"]["skills"].as_array_mut().unwrap().push(json!({"name":label,"level":10,"tripods":[]}));
        }
        let base=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[destiny_slot(name,"hero",points)]});let selected=builder::build(&input.to_string()).unwrap();
        for (s,old) in selected.skills.iter().zip(&base.skills) {
            let id=input["skillData"].as_array().unwrap().iter().find(|d|d["name"]==s.name).unwrap()["gameSkillId"].as_u64().unwrap() as u32;
            let rush=[26800,26810,26820,26830,26970].contains(&id);
            let factor=if points<10 {1.} else if name=="갈증의 악몽" {1.015} else if name=="피의 갈증" {if rush {1.3}else{1.}} else if [26810,26830].contains(&id) {0.5} else if [26800,26820].contains(&id) {1.08} else {1.};
            for (h,b) in s.hits.iter().zip(&old.hits) {assert!(((1.+h.damage_increase)/(1.+b.damage_increase)-factor).abs()<1e-12);assert_eq!(h.action_delay_ms,b.action_delay_ms);assert_eq!(h.fixed_delay_ms,b.fixed_delay_ms);}
            assert_eq!(s.cooldown_ms,if name=="피의 갈증"&&points>=10&&rush {(old.cooldown_ms as f64*1.5) as u32}else{old.cooldown_ms});assert_eq!(s.cast_times_ms,old.cast_times_ms);assert_eq!(s.charge_recovery_ms,old.charge_recovery_ms);
        }
    }}
}

#[test]
fn reaper_remaining_hero_cores_reject_wrong_root_and_unimplemented_upper_grades() {
    for name in ["달의 낙하","갈증의 악몽","피의 갈증"] {for grade in ["hero","legend","relic","ancient"] {for points in [9,10,14,15] {
        let mut wrong=corrosion_input(26810,"레이지 스피어",&[268100]);wrong["character"]["skills"][0]["tripods"]=json!([]);
        if name!="달의 낙하" {wrong["reaperBuild"]=json!("dark_shadow_dot_legacy");}
        wrong["character"]["arkGrids"]=json!({"reaperSlots":[destiny_slot(name,grade,points)]});assert_eq!(builder::build(&wrong.to_string()).is_err(),points>=10);
    }}}
}

#[test]
fn reaper_moonfall_upper_grades_combine_flat_cooldown_buff_and_persona_tail() {
    for grade in ["legend","relic","ancient"] {for points in [9,10,13,14,16,17,18,19,20] {
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);
        input["reaperBuild"]=json!("dark_shadow_dot_legacy");
        input["character"]["skills"][0]["tripods"]=json!([]);
        for (id,name) in [(26800,"라스트 그래피티"),(26820,"사일런트 스매셔"),(26600,"쉐도우 스톰")] {
            let mut skill=input["skillData"][0].clone();skill["gameSkillId"]=json!(id);skill["name"]=json!(name);
            skill["hits"][0]["hitId"]=json!(format!("moonfall:{id}"));
            input["skillData"].as_array_mut().unwrap().push(skill);
            input["character"]["skills"].as_array_mut().unwrap().push(json!({"name":name,"level":10,"tripods":[]}));
        }
        let base=builder::build(&input.to_string()).unwrap();
        input["character"]["arkGrids"]=json!({"reaperSlots":[destiny_slot("달의 낙하",grade,points),silent_slot("legend",14)]});
        let p=builder::build(&input.to_string()).unwrap();
        let triggers=serde_json::to_string(&p.global_triggers).unwrap();
        assert_eq!(triggers.contains("ReduceCooldownsFlat"),points>=14);
        if points>=14 {assert!(triggers.contains("amount_ms\":4000"));}
        assert_eq!(triggers.contains(if grade=="ancient" {"3260800"} else {"3260700"}),grade!="legend"&&points>=17);
        for (skill,old) in p.skills.iter().zip(&base.skills) {
            assert_eq!(skill.cooldown_ms,old.cooldown_ms);
            assert_eq!(skill.cast_times_ms,old.cast_times_ms);
            let tail=if grade!="legend" {points.saturating_sub(17) as usize} else {0};
            assert_eq!(skill.cast_resource_damage_bonuses.iter().filter(|(resource,value)|resource=="ReaperPersona"&&(*value-0.0017).abs()<1e-12).count(),tail);
            let group=skill.name=="라스트 그래피티"||skill.name=="사일런트 스매셔";
            assert_eq!(skill.hits.iter().flat_map(|h|&h.triggers).any(|t|matches!(&t.effect,crate::profile::TriggerEffect::DamageBonus(value) if (*value-if grade=="ancient" {0.12}else{0.10}).abs()<1e-12)),group&&grade!="legend"&&points>=17);
        }
    }}
}

#[test]
fn reaper_moonfall_destiny_trace_reduces_group_cooldowns_and_expires_buff() {
    let mut input=silent_destiny_input();
    input["aplConfig"]=json!("actions:\n - skill: 페르소나\n - skill: 라스트 그래피티\n - skill: 사일런트 스매셔\n");
    input["character"]["arkGrids"]=json!({"reaperSlots":[silent_slot("legend",14)]});
    let base=builder::build(&input.to_string()).unwrap();
    input["character"]["arkGrids"]=json!({"reaperSlots":[silent_slot("legend",14),destiny_slot("달의 낙하","ancient",20)]});
    let selected=builder::build(&input.to_string()).unwrap();
    let plain=engine::trace_one(&base,42,30000).1;
    let boosted=engine::trace_one(&selected,42,30000).1;
    let graffiti=boosted.casts.iter().filter(|cast|cast.skill_name=="라스트 그래피티").map(|cast|cast.time).collect::<Vec<_>>();
    let plain_graffiti=plain.casts.iter().filter(|cast|cast.skill_name=="라스트 그래피티").map(|cast|cast.time).collect::<Vec<_>>();
    assert!(graffiti.len()>=2);
    assert!(boosted.casts.iter().filter(|cast|cast.skill_name=="사일런트 스매셔").count()>=2);
    assert!(graffiti[1]<plain_graffiti[1]);
    assert!(boosted.casts.iter().any(|cast|cast.active_buffs.contains(&"3260800".to_string())));
    assert!(boosted.damage_events.iter().any(|hit|hit.formula.factors.iter().flat_map(|factor|&factor.sources).any(|source|source.source=="runtime_damage_bonus"&&source.value>0.0)));
    assert!(boosted.casts.iter().any(|cast|cast.time>10.&&!cast.active_buffs.contains(&"3260800".to_string())));
}

#[test]
fn reaper_thirst_nightmare_upper_grades_keep_both_distinct_destiny_buffs() {
    for grade in ["legend","relic","ancient"] {for points in [9,10,13,14,16,17,18,19,20] {
        let mut input=fatal_destiny_input();
        input["character"]["arkGrids"]=json!({"reaperSlots":[last_spear_slot("legend",14)]});
        let base=builder::build(&input.to_string()).unwrap();
        input["character"]["arkGrids"]=json!({"reaperSlots":[destiny_slot("갈증의 악몽",grade,points),last_spear_slot("legend",14)]});
        let p=builder::build(&input.to_string()).unwrap();
        let triggers=serde_json::to_string(&p.global_triggers).unwrap();
        assert_eq!(triggers.contains("3261600"),points>=14);
        assert_eq!(triggers.contains("3261610"),grade!="legend"&&points>=17);
        let tail=if grade=="legend" {0}else{points.saturating_sub(17)};
        let expected=(if points>=10 {1.015}else{1.0})*(if grade=="relic"&&points>=17 {1.065}else if grade=="ancient"&&points>=17 {1.085}else{1.0})*1.003_f64.powi(tail as i32);
        let rage=&p.skills[0];
        assert!(((1.0+rage.hits[0].damage_increase)/(1.0+base.skills[0].hits[0].damage_increase)-expected).abs()<1e-9,"{grade} {points} actual={} expected={expected}",(1.0+rage.hits[0].damage_increase)/(1.0+base.skills[0].hits[0].damage_increase));
        if points>=14 {
            let trace=engine::trace_one(&p,42,13000).1;
            let hit=trace.damage_events.iter().find(|hit|hit.cast_id==trace.casts.iter().find(|cast|cast.skill_name=="레이지 스피어").map(|cast|cast.cast_id)).unwrap();
            let sources=hit.formula.factors.iter().flat_map(|factor|&factor.sources).collect::<Vec<_>>();
            assert_eq!(sources.iter().filter(|source|source.source=="buff:3261600").count(),1);
            assert_eq!(sources.iter().filter(|source|source.source=="buff:3261610").count(),usize::from(grade!="legend"&&points>=17));
        }
    }}
}

#[test]
fn reaper_blood_thirst_upper_grades_keep_cooldown_burst_explosion_and_cast_refresh() {
    for grade in ["legend","relic","ancient"] {for points in [9,10,13,14,16,17,18,19,20] {
        let mut input=fatal_destiny_input();
        input["character"]["arkGrids"]=json!({"reaperSlots":[destiny_slot("피의 갈증",grade,points),last_spear_slot("legend",14)]});
        let p=builder::build(&input.to_string()).unwrap();
        let triggers=serde_json::to_string(&p.global_triggers).unwrap();
        assert_eq!(triggers.contains("ReduceCooldowns"),points>=14);
        assert_eq!(triggers.contains("reaper:destiny:blood_explosion"),grade!="legend"&&points>=17);
        assert_eq!(p.skills[0].cooldown_ms,if points>=10 {1500}else{1000});
        if points>=14 {
            let trace=engine::trace_one(&p,42,13000).1;
            assert_eq!(trace.damage_events.iter().any(|hit|hit.hit_id=="reaper:destiny:blood_explosion"),grade!="legend"&&points>=17);
        }
    }}
    let mut dagger=corrosion_input(26050,"스피닝 대거",&[260500]);
    dagger["character"]["skills"][0]["tripods"]=json!([]);
    dagger["reaperIdentityModel"]=json!({"initialDark":1,"initialHeat":1,"initialChaosSeconds":5,"darkPerSecond":0,"heatLossPerSecond":0,"personaDarkLossPerSecond":0,"skillGains":{}});
    dagger["aplConfig"]=json!("actions:\n - skill: 스피닝 대거\n");
    dagger["character"]["arkGrids"]=json!({"reaperSlots":[destiny_slot("피의 갈증","legend",14)]});
    let mut p=builder::build(&dagger.to_string()).unwrap();
    assert!(p.skills[0].timed_effects.iter().any(|effect|matches!(effect.effect,crate::profile::TriggerEffect::RefreshReaperChaos)));
    p.reaper_reference.as_mut().unwrap().missed_hit_ids=p.skills[0].hits.iter().map(|hit|hit.hit_id.clone()).collect();
    let trace=engine::trace_one(&p,42,6000).1;
    assert!(trace.class_events.iter().any(|event|event.time>0.&&event.time<1.&&event.chaos_remaining>14.),"{:?}",trace.class_events);
}

#[test]
fn reaper_blood_thirst_feature116_scales_only_overbleed_damage_axes() {
    let mut input=corrosion_input(26070,"쉐도우 닷",&[260701,260702]);
    input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 출혈","level":1}]);
    input["aplConfig"]=json!("actions:\n - skill: 쉐도우 닷\n   conditions:\n    - {type: time_remaining, operator: '>=', value: 2}\n");
    input["character"]["arkGrids"]=json!({"reaperSlots":[destiny_slot("피의 갈증","hero",10)]});
    let baseline=engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,8000).1;
    let original=baseline.damage_events.iter().find(|hit|hit.hit_id=="reaper:bleeding:burst").unwrap();
    for (grade,points,expected) in [("legend",14,1.75),("relic",17,1.75*1.3),("ancient",17,1.75*1.45)] {
        input["character"]["arkGrids"]=json!({"reaperSlots":[destiny_slot("피의 갈증",grade,points)]});
        let trace=engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,8000).1;
        let burst=trace.damage_events.iter().find(|hit|hit.hit_id=="reaper:bleeding:burst").unwrap();
        assert!((burst.formula.base_damage/original.formula.base_damage-expected).abs()<1e-12);
        assert!((burst.formula.skill_modifier/original.formula.skill_modifier-expected).abs()<1e-12);
        let tick=trace.damage_events.iter().find(|hit|hit.hit_id=="reaper:bleeding:tick").unwrap();
        let before=baseline.damage_events.iter().find(|hit|hit.hit_id=="reaper:bleeding:tick").unwrap();
        assert_eq!(tick.formula.base_damage,before.formula.base_damage);
        assert_eq!(tick.formula.skill_modifier,before.formula.skill_modifier);
    }
}

fn trace_hero_slot(name:&str,grade:&str,points:u32)->Value {
    let mut slot=destiny_slot(name,grade,points);slot["coreAlignment"]=json!("혼돈");slot["coreSymbol"]=json!("달");
    for gem in slot["gems"].as_array_mut().unwrap(){gem["name"]=json!("침식");}slot
}

#[test]
fn reaper_absorption_replaces_heal_stage_and_accumulates_damage_at_all_thresholds() {
    for grade in ["hero","legend","relic","ancient"] {for points in [9,10,13,14,16,17,18,19,20] {
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);input["character"]["skills"][0]["tripods"]=json!([]);
        let baseline=builder::build(&input.to_string()).unwrap();let slot=trace_hero_slot("흡수의 일격",grade,points);
        input["character"]["arkGrids"]=json!({"reaperSlots":[slot]});let built=builder::build(&input.to_string());
        if grade=="hero"&&points>15 {assert!(built.unwrap_err().contains("예산 초과"));continue;}
        let p=built.unwrap();
        let heals:Vec<_>=p.global_triggers.iter().filter_map(|t|match &t.effect {crate::profile::TriggerEffect::GainResource{resource,amount} if resource=="Hp"=>Some((*amount,t.cooldown_ms)),_=>None}).collect();
        let upper=["relic","ancient"].contains(&grade);let amount=if points>=17&&upper {if grade=="ancient" {4000.0}else{3000.0}}else{2000.0};
        assert_eq!(heals,if points>=10 {vec![(amount,Some(20000))]}else{vec![]});
        let damage=if points<14||grade=="hero" {0.0}else{0.005+if upper&&points>=17 {if grade=="ancient" {0.02}else{0.01}}else{0.0}+if upper {points.saturating_sub(17) as f64*0.0016}else{0.0}};
        assert!((p.stat.sum(StatField::TargetDamageIncrease)-baseline.stat.sum(StatField::TargetDamageIncrease)-damage).abs()<1e-12);
        assert_eq!(serde_json::to_value(&p.skills).unwrap(),serde_json::to_value(&baseline.skills).unwrap());
        assert_eq!(p.skills[0].cooldown_ms,baseline.skills[0].cooldown_ms);assert_eq!(p.skills[0].cast_times_ms,baseline.skills[0].cast_times_ms);
    }}
}

#[test]
fn reaper_absorption_rejects_negative_max_hp_and_accepts_zero_before_execution() {
    let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);input["character"]["skills"][0]["tripods"]=json!([]);
    input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");
    input["character"]["arkGrids"]=json!({"reaperSlots":[trace_hero_slot("흡수의 일격","hero",10)]});
    input["character"]["stats"]["maxhp"]=json!(-1.0);
    assert!(builder::build(&input.to_string()).unwrap_err().contains("최대 생명력"));
    input["character"]["stats"]["maxhp"]=json!(0.0);
    let p=builder::build(&input.to_string()).unwrap();assert_eq!(p.max_hp,0.0);assert!(engine::trace_one(&p,42,1000).0.total_damage>0.0);
}

#[test]
fn reaper_faith_hero_threshold_and_upper_contract_do_not_change_damage_or_timing() {
    for grade in ["hero","legend","relic","ancient"] {for points in [9,10,14,15] {
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100,268100]);input["character"]["skills"][0]["tripods"]=json!([]);
        let base=builder::build(&input.to_string()).unwrap();let mut slot=trace_hero_slot("신념의 강화",grade,points);slot["coreSymbol"]=json!("해");
        input["character"]["arkGrids"]=json!({"reaperSlots":[slot]});let selected=builder::build(&input.to_string());
        if grade!="hero" {assert!(selected.unwrap_err().contains("미지원 아크그리드"));continue;}
        let selected=selected.unwrap();assert_eq!(serde_json::to_value(&selected.skills).unwrap(),serde_json::to_value(&base.skills).unwrap());
        let meta=&selected.reaper_reference.as_ref().unwrap().metadata;
        assert_eq!(meta["identity_grid_gain"].as_f64(),if points>=10 {Some(0.006)}else{None});
        if points>=10 {assert!(meta["identity_grid_policy"].as_str().unwrap().contains("게임 미교정"));}
    }}
}

#[test]
fn reaper_trace_thresholds_keep_existing_skill_profiles() {
    for name in ["강철의 흔적","치명적인 흔적"] {for grade in ["hero","legend","relic","ancient"] {for points in [9,10,14,15] {
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100,268100]);input["character"]["skills"][0]["tripods"]=json!([]);
        let base=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[trace_hero_slot(name,grade,points)]});let selected=builder::build(&input.to_string());
        let selected=selected.unwrap();let mut expected=base.skills.clone();if name=="치명적인 흔적" && grade!="hero" && points>=14 {for hit in &mut expected[0].hits {hit.stagger*=1.02;}}assert_eq!(serde_json::to_value(&selected.skills).unwrap(),serde_json::to_value(&expected).unwrap());
        assert_eq!(selected.global_triggers.len()-base.global_triggers.len(),usize::from(points>=10));
    }}}
}

#[test]
fn reaper_steel_trace_uses_one_strongest_stage_and_preserves_self_stats() {
    for grade in ["legend","relic","ancient"] {for points in [9,10,13,14,16,17,18,19,20] {
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100,268100]);
        input["character"]["skills"][0]["tripods"]=json!([]);
        let base=builder::build(&input.to_string()).unwrap();
        input["character"]["arkGrids"]=json!({"reaperSlots":[trace_hero_slot("강철의 흔적",grade,points)]});
        let profile=builder::build(&input.to_string()).unwrap();
        assert_eq!(serde_json::to_value(&profile.stat).unwrap(),serde_json::to_value(&base.stat).unwrap());
        let buffs:Vec<_>=profile.global_triggers.iter().filter_map(|trigger|match &trigger.effect {crate::profile::TriggerEffect::ApplyTargetBuff(buff) if buff.id.ends_with(":target_debuff")=>Some(buff),_=>None}).collect();
        assert_eq!(buffs.len(),usize::from(points>=10));
        if points>=10 {let expected=if points>=17 && grade=="ancient" {0.01} else if points>=17 && grade=="relic" {0.006} else {0.002};assert_eq!(buffs[0].effects[&StatField::TargetDefenseReduction],expected);}
        assert!(profile.reaper_reference.as_ref().unwrap().metadata["steel_trace_grid_policy"].as_str().unwrap().contains("보류"));
        input["character"]["arkGrids"]["reaperSlots"][0]["gems"].as_array_mut().unwrap().reverse();
        let reversed=builder::build(&input.to_string()).unwrap();
        assert_eq!(serde_json::to_value(engine::trace_one(&profile,42,10000)).unwrap(),serde_json::to_value(engine::trace_one(&reversed,42,10000)).unwrap());
    }}
}

#[test]
fn reaper_critical_trace_replaces_stage_and_multiplies_raw_points_once() {
    use crate::profile::TriggerEffect;
    for grade in ["hero","legend","relic","ancient"] {for points in [9,10,13,14,16,17,18,19,20] {for tripod in [false,true] {
        if grade=="hero" && points>15 {continue;}
        let mut input=corrosion_input(26800,"라스트 그래피티",&[268000]);
        input["character"]["skills"][0]["tripods"]=if tripod {json!([{"name":"무력화 강화","level":1,"values":[999]}])} else {json!([])};
        input["aplConfig"]=json!("actions:\n - skill: 라스트 그래피티\n");
        let base=builder::build(&input.to_string()).unwrap();
        input["character"]["arkGrids"]=json!({"reaperSlots":[trace_hero_slot("치명적인 흔적",grade,points)]});
        let mut profile=builder::build(&input.to_string()).unwrap();
        let ratio=if grade=="hero" || points<14 {0.} else {0.02+if matches!(grade,"relic"|"ancient") && points>=17 {if grade=="ancient" {0.06} else {0.04}} else {0.}+if matches!(grade,"relic"|"ancient") {points.saturating_sub(17) as f64*0.006} else {0.}};
        for (a,b) in profile.skills[0].hits.iter().zip(&base.skills[0].hits) {assert!((a.stagger-b.stagger*(1.+ratio)).abs()<1e-9);}
        let buffs:Vec<_>=profile.global_triggers.iter().filter_map(|trigger|match &trigger.effect {TriggerEffect::ApplyTargetBuff(buff) if buff.id.ends_with(":target_debuff")=>Some(buff),_=>None}).collect();
        assert_eq!(buffs.len(),usize::from(points>=10));
        if points>=10 {let expected=if grade=="ancient" && points>=17 {1.5} else if grade=="relic" && points>=17 {0.9} else {0.3};assert_eq!(buffs[0].effects[&StatField::CritDmg],expected);assert_eq!(buffs[0].duration_ms,6000);}
        let (result,trace)=engine::trace_one(&profile,42,10000);assert!((result.skill_stats[&0].raw_stagger_points-trace.damage_events.iter().map(|hit|hit.raw_stagger_points).sum::<f64>()).abs()<1e-9);
        input["character"]["arkGrids"]["reaperSlots"][0]["gems"].as_array_mut().unwrap().reverse();
        let reversed=builder::build(&input.to_string()).unwrap();assert_eq!(serde_json::to_value(engine::trace_one(&reversed,42,10000)).unwrap(),serde_json::to_value((&result,&trace)).unwrap());
        profile.reaper_reference.as_mut().unwrap().missed_hit_ids=profile.skills[0].hits.iter().map(|hit|hit.hit_id.clone()).collect();
        let (result,trace)=engine::trace_one(&profile,42,10000);assert!(result.skill_stats.values().all(|s|s.raw_stagger_points==0.));assert!(trace.casts.iter().all(|cast|cast.target_debuff_details_after_hit.is_empty()));
    }}}
}

#[test]
fn reaper_trace_heroes_apply_after_hit_refresh_once_expire_and_ignore_misses() {
    for (name,grade,points) in [("강철의 흔적","hero",10),("강철의 흔적","legend",14),("강철의 흔적","relic",17),("강철의 흔적","ancient",20),("치명적인 흔적","hero",10)] {for crit in [false,true] {for cooldown in [3,7] {for snapshot in [false,true] {
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100,268100]);input["character"]["skills"][0]["tripods"]=json!([]);input["skillData"][0]["cooldown"]=json!(cooldown);input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");input["targetDefense"]=json!(10000);
        input["character"]["arkGrids"]=json!({"reaperSlots":[trace_hero_slot(name,grade,points)]});let mut selected=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=Value::Null;let mut base=builder::build(&input.to_string()).unwrap();
        for p in [&mut selected,&mut base] {p.stat.swiftness=0.;p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),if crit {100.}else{-100.});for h in &mut p.skills[0].hits{h.use_snapshot=snapshot;}}
        let id=selected.global_triggers.iter().find_map(|t|match &t.effect {crate::profile::TriggerEffect::ApplyTargetBuff(b) if b.id.ends_with(":target_debuff")=>Some(b.id.clone()),_=>None}).unwrap();
        let trace=engine::trace_one(&selected,42,16000).1;let old=engine::trace_one(&base,42,16000).1;
        assert_eq!(trace.damage_events.len(),old.damage_events.len());assert_eq!(trace.damage_events[0].result_damage,old.damage_events[0].result_damage);
        assert!(trace.casts[0].target_debuff_details_after_hit.iter().any(|b|b.id==id&&b.stacks==1));
        assert!(trace.casts.iter().all(|c|c.target_debuff_details_after_hit.iter().filter(|b|b.id==id).count()<=1));
        if cooldown==3 {for cast in trace.casts.iter().skip(1) {if let Some(h)=trace.damage_events.iter().find(|h|h.cast_id==Some(cast.cast_id)) {let b=old.damage_events.iter().find(|b|b.hit_id==h.hit_id&&b.time==h.time).unwrap();if name=="강철의 흔적"||crit {assert!(h.result_damage>b.result_damage);}else{assert_eq!(h.result_damage,b.result_damage);}}}}
        else {for cast in &trace.casts {let h=trace.damage_events.iter().find(|h|h.cast_id==Some(cast.cast_id));if let Some(h)=h {let b=old.damage_events.iter().find(|b|b.hit_id==h.hit_id&&b.time==h.time).unwrap();assert_eq!(h.result_damage,b.result_damage);}}}
        selected.reaper_reference.as_mut().unwrap().missed_hit_ids=selected.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();let miss=engine::trace_one(&selected,42,16000).1;assert!(miss.damage_events.is_empty());assert!(miss.casts.iter().all(|c|c.target_debuff_details_after_hit.iter().all(|b|b.id!=id)));
    }}}}
}

fn last_spear_slot(grade:&str,points:u32)->Value {
    let mut slot=destiny_slot("라스트 스피어",grade,points);slot["coreSymbol"]=json!("달");slot
}

#[test]
fn reaper_last_spear_all_grades_cover_tooltip_damage_and_root_contract() {
    for grade in ["hero","legend"] {for points in [9,10,13,14,15,16,17,20].into_iter().filter(|p|grade!="hero"||*p<=15) {
        let mut input=fatal_destiny_input();let base=builder::build(&input.to_string()).unwrap();
        input["character"]["arkGrids"]=json!({"reaperSlots":[last_spear_slot(grade,points)]});let selected=builder::build(&input.to_string()).unwrap();
        for (s,b) in selected.skills.iter().zip(&base.skills) {
            assert_eq!(s.cooldown_ms,b.cooldown_ms);assert_eq!(s.charge_recovery_ms,b.charge_recovery_ms);
            assert_eq!(s.cast_times_ms,b.cast_times_ms);
            for (h,old) in s.hits.iter().zip(&b.hits) {assert_eq!(h.action_delay_ms,old.action_delay_ms);assert_eq!(h.fixed_delay_ms,old.fixed_delay_ms);assert!(((1.+h.damage_increase)/(1.+old.damage_increase)-if points>=10 {1.015}else{1.}).abs()<1e-12);}
        }
        let mut wrong=destiny_input();wrong["character"]["arkGrids"]=json!({"reaperSlots":[last_spear_slot(grade,points)]});assert_eq!(builder::build(&wrong.to_string()).is_err(),points>=10);
    }}
    for (grade,boost) in [("relic",1.07),("ancient",1.085)] {for points in [9,10,14,16,17,18,19,20] {
        let mut input=fatal_destiny_input();let base=builder::build(&input.to_string()).unwrap();
        input["character"]["arkGrids"]=json!({"reaperSlots":[last_spear_slot(grade,points)]});let selected=builder::build(&input.to_string()).unwrap();
        for (s,b) in selected.skills.iter().zip(&base.skills) {
            assert_eq!(s.cooldown_ms,b.cooldown_ms);assert_eq!(s.charge_recovery_ms,b.charge_recovery_ms);
            let speed=s.name=="페이탈 스텝"&&points>=17;
            assert_eq!(s.cast_times_ms,b.cast_times_ms.iter().map(|v|if speed {(*v as f64/1.25).round() as u32}else{*v}).collect::<Vec<_>>());
            let expected=(if points>=10 {1.015} else {1.})*(if s.name=="레이지 스피어"&&points>=17 {boost} else {1.})*(if s.name=="레이지 스피어" {1.003_f64.powi(points.saturating_sub(17) as i32)} else {1.});
            for (h,old) in s.hits.iter().zip(&b.hits) {assert!(((1.+h.damage_increase)/(1.+old.damage_increase)-expected).abs()<1e-12,"{grade}/{points}: {}",h.hit_id);}
        }
        let mut wrong=destiny_input();wrong["character"]["arkGrids"]=json!({"reaperSlots":[last_spear_slot(grade,points)]});assert_eq!(builder::build(&wrong.to_string()).is_err(),points>=10);
    }}
}

#[test]
fn reaper_last_spear_produces_once_per_rage_cast_after_consume_and_restores_slot_order() {
    for (grade,points,active) in [("hero",14,false),("legend",13,false),("legend",14,true)] {
        let mut input=fatal_destiny_input();input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");
        let a=destiny_slot("치명적인 발걸음","ancient",20);let b=last_spear_slot(grade,points);
        input["character"]["arkGrids"]=json!({"reaperSlots":[a.clone(),b.clone()]});let p=builder::build(&input.to_string()).unwrap();let trace=engine::trace_one(&p,42,7000).1;
        let casts=&trace.casts;assert!(casts.len()>=3);
        for (index,cast) in casts.iter().enumerate() {
            assert_eq!(cast.active_buffs.contains(&"3262100".into()),active);
            let hits=trace.damage_events.iter().filter(|h|h.cast_id==Some(cast.cast_id)).collect::<Vec<_>>();if index+1<casts.len(){assert_eq!(hits.len(),3);}else{assert!(hits.len()<=3);}
            for hit in hits {assert_eq!(hit.formula.factors.iter().flat_map(|f|&f.sources).any(|s|s.source=="cast:buff:3262100"),active&&index>0);}
        }
        // Multi-hit and misses are not additional casts; only Rage produces this destiny.
        let mut miss=p.clone();miss.reaper_reference.as_mut().unwrap().missed_hit_ids=miss.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();let missing=engine::trace_one(&miss,42,7000).1;
        assert!(missing.damage_events.is_empty());assert_eq!(missing.casts.iter().map(|c|(&c.active_buffs,c.time)).collect::<Vec<_>>(),casts.iter().map(|c|(&c.active_buffs,c.time)).collect::<Vec<_>>());
        input["character"]["arkGrids"]=json!({"reaperSlots":[b,a]});let reversed=builder::build(&input.to_string()).unwrap();
        assert_eq!(serde_json::to_value(&p.skills).unwrap(),serde_json::to_value(&reversed.skills).unwrap());
        assert_eq!(serde_json::to_value(engine::trace_one(&p,42,7000).1.casts).unwrap(),serde_json::to_value(engine::trace_one(&reversed,42,7000).1.casts).unwrap());
        input["aplConfig"]=json!("actions:\n - skill: 쉐도우 스톰\n");let other=engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,7000).1;
        assert!(other.casts.iter().all(|c|!c.active_buffs.contains(&"3262100".into())));
    }
}

#[test]
fn reaper_fatal_destiny_thresholds_keep_grade_damage_and_rush_stage_timing_scoped() {
    for name in ["치명적인 발걸음","치명적 악몽"] {for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&p|tier>0||p<=15) {
        let mut input=fatal_destiny_input();input["skillData"][0]["hits"][1]["fixedDelayMs"]=json!(500);let old=builder::build(&input.to_string()).unwrap();
        let mut core=destiny_slot(name,grade,points);core["coreSymbol"]=json!(if name=="치명적 악몽" {"달"} else {"해"});input["character"]["arkGrids"]=json!({"reaperSlots":[core]});let p=builder::build(&input.to_string()).unwrap();
        let static_bonus=if tier>=2&&points>=17 {if name=="치명적인 발걸음" {if tier==2 {0.04} else {0.055}} else if tier==2 {0.03} else {0.04}} else {0.};
        let factor=(if points>=10 {1.015} else {1.})*(1.+static_bonus)*(if tier>=2 {(1.003_f64).powi(points.saturating_sub(17) as i32)} else {1.});
        for (hit,base) in p.skills[0].hits.iter().zip(&old.skills[0].hits) {assert!(((1.+hit.damage_increase)/(1.+base.damage_increase)-factor).abs()<1e-12);assert_eq!(hit.fixed_delay_ms,base.fixed_delay_ms);}
        let speed=name=="치명적 악몽"&&tier>=2&&points>=17;assert_eq!(p.skills[0].cast_times_ms[0],if speed {(old.skills[0].cast_times_ms[0] as f64/1.1).round() as u32} else {old.skills[0].cast_times_ms[0]});
        for (hit,base) in p.skills[0].hits.iter().zip(&old.skills[0].hits) {assert_eq!(hit.action_delay_ms,if speed {(base.action_delay_ms as f64/1.1).round() as u32} else {base.action_delay_ms});}
        for (skill,base) in p.skills.iter().zip(&old.skills).skip(1) {assert_eq!(skill.cast_times_ms,base.cast_times_ms);assert_eq!(skill.cooldown_ms,base.cooldown_ms);}
        let mut wrong=input.clone();wrong["reaperBuild"]=json!("dark_shadow_dot_legacy");wrong["character"]["skills"].as_array_mut().unwrap().retain(|s|s["name"]!="페이탈 스텝");assert!(builder::build(&wrong.to_string()).is_err());
    }}}
}

#[test]
fn reaper_fatal_destiny_consumes_four_casts_not_hits_and_misses_without_normal_consumption() {
    let mut input=fatal_destiny_input();let mut sun=destiny_slot("치명적인 발걸음","ancient",20);let mut moon=destiny_slot("치명적 악몽","ancient",20);moon["coreSymbol"]=json!("달");sun["coreSymbol"]=json!("해");input["character"]["arkGrids"]=json!({"reaperSlots":[sun,moon]});
    let mut p=builder::build(&input.to_string()).unwrap();p.skills[1].max_uses=1;p.skills[2].max_uses=1;
    let trace=engine::trace_one(&p,42,9000).1;let rush=trace.casts.iter().filter(|c|c.skill_name=="레이지 스피어").collect::<Vec<_>>();assert!(rush.len()>=5);
    for (index,cast) in rush.iter().enumerate() {let hits=trace.damage_events.iter().filter(|h|h.cast_id==Some(cast.cast_id)).collect::<Vec<_>>();assert_eq!(hits.len(),3);for hit in hits {assert_eq!(hit.formula.factors.iter().flat_map(|f|&f.sources).any(|s|s.source=="cast:buff:3262100"),index<4);}}
    assert!(trace.casts.iter().find(|c|c.skill_name=="쉐도우 스톰").unwrap().active_buff_details.iter().any(|b|b.id=="3262100"&&b.stacks==4));
    p.reaper_reference.as_mut().unwrap().missed_hit_ids=p.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();let missing=engine::trace_one(&p,42,9000).1;assert!(missing.damage_events.is_empty());let fifth=missing.casts.iter().filter(|c|c.skill_name=="레이지 스피어").nth(4).unwrap();assert!(!fifth.active_buffs.contains(&"3262100".into()));
}

#[test]
fn reaper_fatal_destiny_reduces_its_new_cooldown_and_refreshes_four_uses_in_slot_order() {
    let mut input=fatal_destiny_input();input["aplConfig"]=json!("actions:\n - skill: 페이탈 스텝\n - skill: 쉐도우 스톰\n");
    let mut sun=destiny_slot("치명적인 발걸음","ancient",20);let mut moon=destiny_slot("치명적 악몽","ancient",20);moon["coreSymbol"]=json!("달");sun["coreSymbol"]=json!("해");input["character"]["arkGrids"]=json!({"reaperSlots":[sun.clone(),moon.clone()]});let p=builder::build(&input.to_string()).unwrap();let trace=engine::trace_one(&p,42,80000).1;
    let fatal=trace.casts.iter().filter(|c|c.skill_name=="페이탈 스텝").collect::<Vec<_>>();assert!(fatal.len()>=3);assert!((fatal[1].time-fatal[0].time-p.skills[1].cooldown_ms as f64*0.88/1000.).abs()<0.051,"{fatal:?}");assert!(fatal.iter().all(|c|c.active_buff_details.iter().any(|b|b.id=="3262100"&&b.stacks==4)));
    input["character"]["arkGrids"]=json!({"reaperSlots":[moon,sun]});let reversed=builder::build(&input.to_string()).unwrap();assert_eq!(serde_json::to_value(&p.skills).unwrap(),serde_json::to_value(&reversed.skills).unwrap());assert_eq!(serde_json::to_value(engine::trace_one(&p,42,80000)).unwrap(),serde_json::to_value(engine::trace_one(&reversed,42,80000)).unwrap());
}


fn graffiti_destiny_input() -> Value {
    let mut input=corrosion_input(26800,"라스트 그래피티",&[268000,268000,268000]);
    input["reaperBuild"]=json!("dark_shadow_dot_legacy");input["character"]["skills"][0]["tripods"]=json!([]);input["skillData"][0]["cooldown"]=json!(3);
    for (id,name) in [(26810,"레이지 스피어"),(26600,"쉐도우 스톰"),(26110,"샤벨 스팅거")] {
        let mut data=input["skillData"][0].clone();data["gameSkillId"]=json!(id);data["name"]=json!(name);data["hits"]=json!([]);data["cooldown"]=json!(5);
        input["skillData"].as_array_mut().unwrap().push(data);input["character"]["skills"].as_array_mut().unwrap().push(json!({"name":name,"level":10,"tripods":[]}));
    }
    input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"heatLossPerSecond":0,"personaDarkLossPerSecond":0,"skillGains":{}});
    input["aplConfig"]=json!("actions:\n - skill: 라스트 그래피티\n");input
}

fn graffiti_pair(grade:&str,points:u32) -> Value {
    let sun=destiny_slot("두 개의 달",grade,points);let mut moon=destiny_slot("더블 코어",grade,points);moon["coreSymbol"]=json!("달");json!({"reaperSlots":[sun,moon]})
}

#[test]
fn reaper_graffiti_destiny_grades_keep_direct_damage_crit_back_group_and_stage_speed() {
    for name in ["두 개의 달","더블 코어"] {for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&p|tier>0||p<=15) {
        let mut input=graffiti_destiny_input();input["skillData"][0]["hits"][1]["fixedDelayMs"]=json!(500);let old=builder::build(&input.to_string()).unwrap();
        let mut core=destiny_slot(name,grade,points);core["coreSymbol"]=json!(if name=="더블 코어" {"달"} else {"해"});input["character"]["arkGrids"]=json!({"reaperSlots":[core]});let p=builder::build(&input.to_string()).unwrap();
        let factor=(if points>=10 {1.016} else {1.})*(if tier>=2 {(1.0017_f64).powi(points.saturating_sub(17) as i32)} else {1.})*(if name=="더블 코어"&&tier>=2&&points>=17 {if tier==2 {1.20} else {1.24}} else {1.});
        for (h,b) in p.skills[0].hits.iter().zip(&old.skills[0].hits) {assert!(((1.+h.damage_increase)/(1.+b.damage_increase)-factor).abs()<1e-12);assert_eq!(h.fixed_delay_ms,b.fixed_delay_ms);}
        let speed=name=="더블 코어"&&tier>=2&&points>=17;
        for (index,(skill,base)) in p.skills.iter().zip(&old.skills).enumerate() {let expected=if speed&&index<3 {(base.cast_times_ms[0] as f64/1.1).round() as u32} else {base.cast_times_ms[0]};assert_eq!(skill.cast_times_ms[0],expected);assert_eq!(skill.cooldown_ms,base.cooldown_ms);}
        assert_eq!(p.skills[0].cast_buff_back_attack_id.is_some(),name=="두 개의 달"&&tier>=1&&points>=14);assert!(p.skills.iter().skip(1).all(|s|s.cast_buff_back_attack_id.is_none()));
        let mut wrong=input.clone();wrong["reaperBuild"]=json!("thirst_chaos_sustain");assert_eq!(builder::build(&wrong.to_string()).is_err(),points>=10);
    }}}
}

#[test]
fn reaper_graffiti_destiny_forced_back_reuses_normal_build_and_freezes_all_live_hits() {
    for choices in [vec![],vec!["급습 강화"],vec!["그림자 올가미","범위 증가","급습 강화"]] {
        let mut input=graffiti_destiny_input();input["character"]["arkGrids"]=graffiti_pair("ancient",20);input["skillData"][0]["properties"]["attackType"]=json!("");
        input["character"]["skills"][0]["tripods"]=json!(choices.iter().map(|name|json!({"name":name,"level":1})).collect::<Vec<_>>());
        input["character"]["engravings"]=json!([
            {"name":"test-back","grade":"기본","level":1,"simEffects":{"grades":{"기본":{"levels":[[{"kind":"stat","field":"damage_increase","tag":"AttackBack","value":25}]]}}}},
            {"name":"test-nondirectional","grade":"기본","level":1,"simEffects":{"grades":{"기본":{"levels":[[{"kind":"stat","field":"damage_increase","tag":"NonDirectional","value":16}]]}}}}
        ]);
        let p=builder::build(&input.to_string()).unwrap();let variant=p.skills[0].back_attack_variant.as_ref().unwrap();assert!(variant.tags.contains(&crate::profile::SkillTag::AttackBack));assert!(!variant.tags.contains(&crate::profile::SkillTag::NonDirectional));assert!(variant.back_attack_variant.is_none());assert!(((1.+variant.hits[0].damage_increase)/(1.+p.skills[0].hits[0].damage_increase)-1.05*1.25/1.16).abs()<1e-12);
        input["skillData"][0]["properties"]["attackType"]=json!("백 어택");let back=builder::build(&input.to_string()).unwrap();assert_eq!(serde_json::to_value(&variant.hits).unwrap(),serde_json::to_value(&back.skills[0].hits).unwrap());
        let a=engine::trace_one(&p,42,12000).1;let b=engine::trace_one(&back,42,12000).1;assert!(a.casts.len()>=3);
        for cast in a.casts.iter().filter(|c|c.time<10.) {let hits=a.damage_events.iter().filter(|h|h.cast_id==Some(cast.cast_id)).collect::<Vec<_>>();assert!(!hits.is_empty());for hit in hits {let enhanced=cast.cast_id>1;assert_eq!(hit.formula.factors.iter().flat_map(|f|&f.sources).any(|s|s.source=="cast:buff:3261100"),enhanced);if enhanced {let other=b.damage_events.iter().find(|h|h.cast_id==hit.cast_id&&h.hit_index==hit.hit_index).unwrap();assert_eq!(hit.result_damage,other.result_damage);assert_eq!(hit.formula.skill_damage_multiplier,other.formula.skill_damage_multiplier);}}}
        let mut missed=p.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=missed.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();let trace=engine::trace_one(&missed,42,12000).1;assert!(trace.damage_events.is_empty());assert!(trace.casts.iter().all(|c|c.active_buff_details.iter().any(|b|b.id=="3261100"&&b.stacks==1)));
    }
}

#[test]
fn reaper_graffiti_destiny_one_use_expiry_non_graffiti_and_reversed_slots() {
    let mut input=graffiti_destiny_input();input["character"]["arkGrids"]=graffiti_pair("ancient",20);let mut p=builder::build(&input.to_string()).unwrap();
    // A single non-rush producer isolates one-use consumption from the pair's automatic refresh.
    let source=p.global_triggers.iter_mut().find(|t|matches!(&t.effect,crate::profile::TriggerEffect::Sequence(_))).unwrap();source.filter=crate::profile::TriggerFilter::SkillCastId(2);
    p.skills[2].max_uses=1;p.apl_actions.insert(0,p.apl_actions[0].clone());p.apl_actions[0].skill_id=2;
    let a=engine::trace_one(&p,42,30000).1;let casts=a.casts.iter().filter(|c|c.skill_name=="라스트 그래피티").collect::<Vec<_>>();assert!(casts.len()>=4);
    for (i,cast) in casts.iter().enumerate() {for h in a.damage_events.iter().filter(|h|h.cast_id==Some(cast.cast_id)) {
        assert_eq!(h.formula.factors.iter().flat_map(|f|&f.sources).any(|s|s.source=="cast:buff:3261100"),i==0);
        let static_damage=p.skills[0].hits[h.hit_index].damage_increase;
        assert!((h.formula.skill_damage_multiplier-(1.+static_damage+if h.time<20. {0.063} else {0.})).abs()<1e-12);
    }}
    assert!(a.damage_events.iter().any(|h|h.time>20.));assert!(a.casts[0].active_buffs.contains(&"3261100".into()));
    input["character"]["arkGrids"]["reaperSlots"].as_array_mut().unwrap().reverse();let reversed=builder::build(&input.to_string()).unwrap();let normal=builder::build(&graffiti_destiny_input_with_pair().to_string()).unwrap();assert_eq!(serde_json::to_value(&normal.skills).unwrap(),serde_json::to_value(&reversed.skills).unwrap());assert_eq!(serde_json::to_value(engine::trace_one(&normal,42,30000)).unwrap(),serde_json::to_value(engine::trace_one(&reversed,42,30000)).unwrap());
}

fn graffiti_destiny_input_with_pair() -> Value {let mut input=graffiti_destiny_input();input["character"]["arkGrids"]=graffiti_pair("ancient",20);input}

#[test]
fn reaper_graffiti_destiny_stage_group_checks_all_public_ids_and_fixed_notify_delays() {
    let data:Value=serde_json::from_str(include_str!("../skill-data/reaper/backend/simulation-data.json")).unwrap();
    let source:Value=serde_json::from_str(include_str!("../skill-data/reaper/backend/research/graffiti-destiny-source-audit.json")).unwrap();
    for raw in data["skills"].as_array().unwrap() {
        let id=raw["gameSkillId"].as_u64().unwrap() as u32;let name=raw["name"].as_str().unwrap();
        let mut input=corrosion_input(id,name,&[]);input["reaperBuild"]=json!(if id==26980 {"thirst_chaos_sustain"} else {"dark_shadow_dot_legacy"});
        input["character"]["skills"]=json!([{"name":name,"level":if [26040,26980].contains(&id) {1} else {12},"tripods":[]}]);
        input["aplConfig"]=json!(format!("actions:\n - skill: '{name}'\n"));
        input["skillData"][0]["skillSlot"]=json!(match raw["skillSlot"].as_str().unwrap() {"각성 스킬"=>"Awakening","초각성기"=>"SuperAwakening","초각성 스킬"=>"HyperAwakeningTechniques",_=>"Normal"});
        if id==26510 {input["character"]["skills"][0]["tripods"]=json!([{"name":"순풍","level":1}]);}
        let old=builder::build(&input.to_string()).unwrap();
        // Fatal belongs to the other root and is excluded; raw group membership is checked below.
        if id==26980 {assert!(!source["sourceCandidates"]["stageSpeedSkillIds"].as_array().unwrap().contains(&json!(id)));continue;}
        let mut core=destiny_slot("더블 코어","ancient",20);core["coreSymbol"]=json!("달");input["character"]["arkGrids"]=json!({"reaperSlots":[core]});let p=builder::build(&input.to_string()).unwrap();
        let affected=source["sourceCandidates"]["stageSpeedSkillIds"].as_array().unwrap().contains(&json!(id));
        let scale=|ms:u32|if affected {(ms as f64/1.1).round() as u32} else {ms};
        assert_eq!(p.skills[0].cast_times_ms,old.skills[0].cast_times_ms.iter().copied().map(scale).collect::<Vec<_>>(),"{name}");
        for (h,b) in p.skills[0].hits.iter().zip(&old.skills[0].hits) {assert_eq!(h.action_delay_ms,scale(b.action_delay_ms));assert_eq!(h.fixed_delay_ms,b.fixed_delay_ms);}
        for (t,b) in p.skills[0].timed_effects.iter().zip(&old.skills[0].timed_effects) {assert_eq!(t.action_delay_ms,scale(b.action_delay_ms));}
        assert_eq!(p.skills[0].cooldown_ms,old.skills[0].cooldown_ms);assert_eq!(p.skills[0].resource_costs,old.skills[0].resource_costs);
    }
}


fn silent_destiny_input() -> Value {
    let mut input=corrosion_input(26820,"사일런트 스매셔",&[268200]);
    input["reaperBuild"]=json!("dark_shadow_dot_legacy");input["character"]["skills"][0]["tripods"]=json!([]);input["skillData"][0]["cooldown"]=json!(10);
    let persona=destiny_input();input["skillData"].as_array_mut().unwrap().push(persona["skillData"][1].clone());input["character"]["skills"].as_array_mut().unwrap().push(persona["character"]["skills"][1].clone());
    for (id,name,effects) in [(26800,"라스트 그래피티",vec![268000,268000,268000]),(26810,"레이지 스피어",vec![268100,268100,268100]),(26600,"쉐도우 스톰",vec![266000]),(26070,"쉐도우 닷",vec![260700])] {
        let mut added=corrosion_input(id,name,&effects);added["character"]["skills"][0]["tripods"]=json!([]);added["skillData"][0]["cooldown"]=json!(10);
        input["skillData"].as_array_mut().unwrap().push(added["skillData"][0].clone());input["character"]["skills"].as_array_mut().unwrap().push(added["character"]["skills"][0].clone());
    }
    input["reaperIdentityModel"]=persona["reaperIdentityModel"].clone();input["aplConfig"]=json!("actions:\n - skill: 페르소나\n - skill: 사일런트 스매셔\n");input
}

fn silent_slot(grade:&str,points:u32) -> Value {let mut slot=destiny_slot("사일런트",grade,points);slot["coreSymbol"]=json!("달");slot}

#[test]
fn reaper_silent_destiny_all_grades_keep_cooldown_persona_damage_and_direct_scope() {
    for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&p|tier>0||p<=15) {
        let mut input=silent_destiny_input();let old=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[silent_slot(grade,points)]});let p=builder::build(&input.to_string()).unwrap();
        for (skill,base) in p.skills.iter().zip(&old.skills) {
            assert_eq!(skill.cooldown_ms,base.cooldown_ms-if skill.name=="사일런트 스매셔"&&points>=10 {6000} else {0});
            let mut expected=vec![];if tier>=2&&points>=17&&skill.name=="사일런트 스매셔" {expected.push(("ReaperPersona".into(),if tier==2 {0.10} else {0.13}));}
            if tier>=2 {for _ in 0..points.saturating_sub(17) {expected.push(("ReaperPersona".into(),0.0017));}}
            assert_eq!(skill.cast_resource_damage_bonuses,expected);
            assert_eq!(serde_json::to_value(&skill.hits).unwrap(),serde_json::to_value(&base.hits).unwrap());assert_eq!(skill.cast_times_ms,base.cast_times_ms);
        }
        if points>=10 {input["reaperBuild"]=json!("thirst_chaos_sustain");assert!(builder::build(&input.to_string()).unwrap_err().contains("요구 깨달음"));}
    }}
}

#[test]
fn reaper_silent_destiny_qualifies_before_persona_consumption_and_grants_after_cast_even_on_miss() {
    for first in ["사일런트 스매셔","라스트 그래피티","레이지 스피어","쉐도우 스톰"] {for persona in [false,true] {for miss in [false,true] {
        let mut input=silent_destiny_input();input["character"]["arkGrids"]=json!({"reaperSlots":[destiny_slot("달의 향기","ancient",20),silent_slot("ancient",20)]});
        input["reaperIdentityModel"]["initialDark"]=json!(if persona {1.} else {0.});input["aplConfig"]=json!(format!("actions:\n{} - skill: {first}\n",if persona {" - skill: 페르소나\n"} else {""}));
        let mut p=builder::build(&input.to_string()).unwrap();if miss {p.reaper_reference.as_mut().unwrap().missed_hit_ids=p.skills.iter().flat_map(|s|s.hits.iter().map(|h|h.hit_id.clone())).collect();}
        let trace=engine::trace_one(&p,42,18000).1;let casts:Vec<_>=trace.casts.iter().filter(|c|c.skill_name==first).collect();assert!(casts.len()>=2);
        let eligible=persona&&matches!(first,"사일런트 스매셔"|"라스트 그래피티");assert_eq!(casts[0].active_buff_details.iter().any(|b|b.id=="3260201"&&b.stacks==1),eligible);
        assert!(casts.iter().all(|c|c.active_buff_details.iter().all(|b|!b.id.starts_with("reaper_destiny_persona_cast_"))));
        if eligible {assert!(casts.iter().skip(1).all(|c|c.active_buff_details.iter().all(|b|b.id!="3260201")));}
        if !miss {for hit in trace.damage_events.iter().filter(|h|h.cast_id==Some(casts[0].cast_id)) {assert!(hit.formula.factors.iter().flat_map(|f|&f.sources).all(|s|s.source!="cast:buff:3260200"));}}
    }}}
}

#[test]
fn reaper_silent_destiny_slot_order_and_untrusted_groups_cannot_expand_persona_trigger() {
    let mut input=silent_destiny_input();input["skillData"][4]["properties"]["skillGroupIds"]=json!([3260600]);
    let sun=destiny_slot("두 개의 달","ancient",20);let moon=silent_slot("ancient",20);input["character"]["arkGrids"]=json!({"reaperSlots":[sun.clone(),moon.clone()]});
    input["aplConfig"]=json!("actions:\n - skill: 페르소나\n - skill: 쉐도우 스톰\n");let p=builder::build(&input.to_string()).unwrap();let trace=engine::trace_one(&p,42,18000).1;
    assert!(trace.casts.iter().all(|c|c.active_buff_details.iter().all(|b|b.id!="3261100")));input["character"]["arkGrids"]=json!({"reaperSlots":[moon,sun]});let reversed=builder::build(&input.to_string()).unwrap();assert_eq!(serde_json::to_value(engine::trace_one(&p,42,18000)).unwrap(),serde_json::to_value(engine::trace_one(&reversed,42,18000)).unwrap());
}

fn assassin_input() -> Value {
    let mut input=silent_destiny_input();
    let finish=corrosion_input(26970,"피니쉬 스텝",&[269700,269700,269700]);
    input["skillData"].as_array_mut().unwrap().push(finish["skillData"][0].clone());
    let mut chosen=finish["character"]["skills"][0].clone();chosen["tripods"]=json!([]);
    input["character"]["skills"].as_array_mut().unwrap().push(chosen);
    input
}

fn assassin_slot(grade:&str,points:u32) -> Value {
    let mut slot=destiny_slot("암살자의 그림자",grade,points);slot["coreSymbol"]=json!("별");slot
}

#[test]
fn reaper_assassin_all_grades_thresholds_and_choices_keep_complete_direct_scope() {
    for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&p|tier>0||p<=15) {for shadow in [false,true] {
        let mut input=assassin_input();if shadow {input["character"]["skills"][0]["tripods"]=json!([{"name":"그림자 잔상","level":1}]);}
        let base=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[assassin_slot(grade,points)]});let p=builder::build(&input.to_string()).unwrap();
        for (skill,old) in p.skills.iter().zip(&base.skills) {
            let finish=skill.name=="피니쉬 스텝"&&points>=10;
            assert_eq!(skill.cast_resource_crit_rate_bonuses,if finish {vec![("ReaperPersona".into(),-0.1)]} else {vec![]});
            let mut resource=if finish {vec![("ReaperPersona".into(),0.1)]} else {vec![]};
            if tier>=2 {for _ in 0..points.saturating_sub(17) {resource.push(("ReaperPersona".into(),0.0017));}}
            assert_eq!(skill.cast_resource_damage_bonuses,resource);
            let protected=tier>=1&&points>=14&&shadow&&skill.name=="사일런트 스매셔";
            assert_eq!(skill.timed_effects.iter().any(|t|matches!(&t.effect,crate::profile::TriggerEffect::ApplyBuff(b) if b.id=="3266601")),protected);
            let bonus=if protected {0.1} else if tier>=2&&points>=17&&skill.name=="라스트 그래피티" {if tier==2 {0.1} else {0.13}} else {0.};
            for (h,b) in skill.hits.iter().zip(&old.hits) {assert!(((1.+h.damage_increase)/(1.+b.damage_increase)-(1.+bonus)).abs()<1e-12);}
            assert_eq!(skill.cast_times_ms,old.cast_times_ms);assert_eq!(skill.cooldown_ms,old.cooldown_ms);
        }
        if points>=10 {input["reaperBuild"]=json!("thirst_chaos_sustain");assert!(builder::build(&input.to_string()).is_err());}
    }}}
}

#[test]
fn reaper_assassin_persona_crit_penalty_freezes_all_hits_and_respects_floor_and_cap() {
    for persona in [false,true] {for base_crit in [0.,50.,150.] {
        let mut input=assassin_input();input["character"]["arkGrids"]=json!({"reaperSlots":[assassin_slot("ancient",20)]});
        input["reaperIdentityModel"]["initialDark"]=json!(if persona {1.} else {0.});input["reaperIdentityModel"]["skillGains"]=json!({});
        input["aplConfig"]=json!(format!("actions:\n{} - skill: 피니쉬 스텝\n",if persona {" - skill: 페르소나\n"} else {""}));
        let mut p=builder::build(&input.to_string()).unwrap();p.stat.crit=0.;p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),base_crit);p.stat.crit_rate_cap=0.8;p.stat.bake();
        let trace=engine::trace_one(&p,42,15000).1;let first=trace.casts.iter().find(|c|c.skill_name=="피니쉬 스텝").unwrap();
        let hits:Vec<_>=trace.damage_events.iter().filter(|h|h.cast_id==Some(first.cast_id)).collect();assert_eq!(hits.len(),3);
        for hit in hits {
            let crit=hit.formula.factors.iter().find(|f|f.name=="critRate").unwrap();
            assert_eq!(crit.sources.iter().any(|s|s.source=="cast:resource:ReaperPersona"&&s.value==-0.1),persona);
            let expected=(base_crit/100.-if persona {0.1} else {0.}).clamp(0.,0.8);
            assert!((crit.value-expected).abs()<1e-12,"{crit:?}");
        }
        for hit in trace.damage_events.iter().filter(|h|h.cast_id!=Some(first.cast_id)) {assert!(hit.formula.factors.iter().flat_map(|f|&f.sources).all(|s|s.source!="cast:resource:ReaperPersona"));}
    }}
}

#[test]
fn reaper_assassin_shadow_protection_expires_at_cast_end_without_damage_or_hit_dependency() {
    for first in [None,Some("가벼운 동작"),Some("넓은 공격")] {for ground in [false,true] {for fast in [false,true] {for miss in [false,true] {
        let mut input=assassin_input();let mut names=vec!["그림자 잔상"];if let Some(name)=first {names.push(name);}if ground {names.push("지면 강타");}
        input["character"]["skills"][0]["tripods"]=json!(names.iter().map(|name|json!({"name":name,"level":1})).collect::<Vec<_>>());input["character"]["arkGrids"]=json!({"reaperSlots":[assassin_slot("ancient",20)]});input["aplConfig"]=json!("actions:\n - skill: 사일런트 스매셔\n");
        let mut p=builder::build(&input.to_string()).unwrap();p.skills[0].cooldown_ms=300000;p.stat.attack_speed.clear();p.stat.swiftness=0.;p.stat.attack_move_speed_percent=0.;p.stat.bake();if fast {p.stat.attack_speed.insert("test".into(),40.);}
        if miss {p.reaper_reference.as_mut().unwrap().missed_hit_ids=p.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();}
        let mut base=p.clone();base.skills[0].timed_effects.retain(|t|!matches!(&t.effect,crate::profile::TriggerEffect::ApplyBuff(b) if b.id=="3266601"));
        let e=engine::trace_one(&p,42,5000).1;let b=engine::trace_one(&base,42,5000).1;assert_eq!(serde_json::to_value(&e.damage_events).unwrap(),serde_json::to_value(&b.damage_events).unwrap());assert_eq!(serde_json::to_value(&e.class_events).unwrap(),serde_json::to_value(&b.class_events).unwrap());
        assert!(e.casts[0].active_buffs.iter().any(|id|id=="3266601"));
        let end=if fast {(p.skills[0].cast_times_ms[0] as f64/1.4) as u64} else {p.skills[0].cast_times_ms[0] as u64};
        let mut observer=p.skills[0].clone();observer.skill_id=99;observer.name="보호 만료 관측".into();observer.timed_effects.clear();p.skills.push(observer);let mut action=p.apl_actions[0].clone();action.skill_id=99;p.apl_actions.push(action);
        let e=engine::trace_one(&p,42,end as u32+100).1;assert_eq!(e.casts[1].time,end as f64/1000.);assert!(!e.casts[1].active_buffs.iter().any(|id|id=="3266601"));
    }}}}
}

fn approaching_slot(grade:&str,points:u32) -> Value {
    let mut slot=destiny_slot("다가오는 죽음",grade,points);slot["coreSymbol"]=json!("별");slot
}

#[test]
fn reaper_approaching_death_grades_keep_dagger_scope_and_two_start_protection_boundaries() {
    for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&p|tier>0||p<=15) {
        let mut input=corrosion_input(26110,"샤벨 스팅거",&[261100,261100,261101,261101]);input["character"]["skills"][0]["tripods"]=json!([]);
        for (id,name,ids) in [(26050,"스피닝 대거",vec![260500,260501]),(26810,"레이지 스피어",vec![268100])] {
            let other=corrosion_input(id,name,&ids);input["skillData"].as_array_mut().unwrap().push(other["skillData"][0].clone());let mut chosen=other["character"]["skills"][0].clone();chosen["tripods"]=json!([]);input["character"]["skills"].as_array_mut().unwrap().push(chosen);
        }
        let base=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[approaching_slot(grade,points)]});let p=builder::build(&input.to_string()).unwrap();
        let global=if tier>=2&&points>=17 {(if tier==2 {1.01} else {1.02})*(1.0016_f64).powi(points.saturating_sub(17) as i32)} else {1.};
        for (index,(skill,old)) in p.skills.iter().zip(&base.skills).enumerate() {
            let dagger=index<2;let factor=global*if dagger&&points>=10 {1.1} else {1.};
            for (h,b) in skill.hits.iter().zip(&old.hits) {assert!(((1.+h.damage_increase)/(1.+b.damage_increase)-factor).abs()<1e-12);}
            let protected=if index==0 {points>=10} else {index==1&&tier>=1&&points>=14};
            assert_eq!(skill.timed_effects.iter().any(|t|matches!(&t.effect,crate::profile::TriggerEffect::ApplyBuff(b) if b.id=="3268510")),protected);
            assert_eq!(skill.cast_times_ms,old.cast_times_ms);assert_eq!(skill.cooldown_ms,old.cooldown_ms);
        }
        if points>=10 {input["reaperBuild"]=json!("dark_shadow_dot_legacy");assert!(builder::build(&input.to_string()).is_err());}
    }}
}

#[test]
fn reaper_approaching_death_scales_burst_only_and_general_damage_reaches_independent_ticks() {
    for (id,name,ids) in [(26050,"스피닝 대거",vec![260500,260501]),(26060,"스피릿 캐치",vec![260600,260612]),(26070,"쉐도우 닷",vec![260701,260702]),(26080,"이블리스토",vec![260800,260801,260802,260803]),(26110,"샤벨 스팅거",vec![261100,261100,261101,261101]),(26120,"데스 사이드",vec![261200,261201]),(26140,"팬텀 댄서",vec![261400,261401,261402,261403])] {
        let mut input=corrosion_input(id,name,&ids);input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 출혈","level":1}]);input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n   conditions:\n    - {{type: time_remaining, operator: '>=' , value: 2}}\n"));
        let base=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[approaching_slot("ancient",20)]});let p=builder::build(&input.to_string()).unwrap();let e=engine::trace_one(&p,42,8000).1;let b=engine::trace_one(&base,42,8000).1;
        assert_eq!(e.damage_events.len(),b.damage_events.len());let global=1.02*(1.0016_f64).powi(3);let mut burst=false;let mut tick=false;
        for (hit,old) in e.damage_events.iter().zip(&b.damage_events) {
            assert_eq!((hit.time,hit.hit_id.as_str(),hit.is_crit),(old.time,old.hit_id.as_str(),old.is_crit));assert_eq!(serde_json::to_value(&hit.resource_gains).unwrap(),serde_json::to_value(&old.resource_gains).unwrap());
            let independent=hit.hit_id.starts_with("reaper:");let expected=global*if independent {1.} else {1.1};assert!((hit.formula.skill_damage_multiplier/old.formula.skill_damage_multiplier-expected).abs()<1e-12);
            let scale=if hit.hit_id=="reaper:bleeding:burst" {burst=true;1.2*1.15} else {if hit.hit_id=="reaper:bleeding:tick" {tick=true;}1.};
            assert!((hit.formula.base_damage/old.formula.base_damage-scale).abs()<1e-12);assert!((hit.formula.skill_modifier/old.formula.skill_modifier-scale).abs()<1e-12);
        }
        assert!(burst&&tick,"{name}");input["character"]["skills"][0]["tripods"].as_array_mut().unwrap().reverse();assert_eq!(serde_json::to_value(engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,8000)).unwrap(),serde_json::to_value(engine::trace_one(&p,42,8000)).unwrap());
    }
}

#[test]
fn reaper_approaching_death_start_protection_is_miss_independent_and_expires_before_next_skill() {
    for (id,name,ids) in [(26110,"샤벨 스팅거",vec![261100,261100,261101,261101]),(26050,"스피닝 대거",vec![260500,260501])] {for fast in [false,true] {for miss in [false,true] {
        let mut input=corrosion_input(id,name,&ids);input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 출혈","level":1}]);input["character"]["arkGrids"]=json!({"reaperSlots":[approaching_slot("ancient",20)]});input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{}});input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n"));let mut p=builder::build(&input.to_string()).unwrap();p.skills[0].cooldown_ms=300000;p.stat.attack_speed.clear();p.stat.attack_move_speed_percent=0.;p.stat.swiftness=0.;p.stat.bake();if fast {p.stat.attack_speed.insert("test".into(),40.);}if miss {p.reaper_reference.as_mut().unwrap().missed_hit_ids=p.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();}
        let mut base=p.clone();base.skills[0].timed_effects.retain(|t|!matches!(&t.effect,crate::profile::TriggerEffect::ApplyBuff(b) if b.id=="3268510"));let a=engine::trace_one(&p,42,1000).1;let b=engine::trace_one(&base,42,1000).1;assert_eq!(serde_json::to_value(a.damage_events).unwrap(),serde_json::to_value(b.damage_events).unwrap());assert!(a.casts[0].active_buffs.iter().any(|id|id=="3268510"));
        let end=if fast {(p.skills[0].cast_times_ms[0] as f64/1.4) as u32} else {p.skills[0].cast_times_ms[0]};let mut observer=p.skills[0].clone();observer.skill_id=99;observer.name="경직 보호 만료 관측".into();observer.timed_effects.clear();p.skills.push(observer);let mut action=p.apl_actions[0].clone();action.skill_id=99;p.apl_actions.push(action);let e=engine::trace_one(&p,42,end+100).1;assert_eq!(e.casts[1].time,end as f64/1000.);assert!(!e.casts[1].active_buffs.iter().any(|id|id=="3268510"));
    }}}
}

#[test]
fn reaper_general_core_damage_all_grades_reaches_poison_without_inheriting_source_bonuses() {
    for core in ["치명적인 발걸음","치명적 악몽","할루시네이션"] {for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {for poison in [None,Some("독 : 부식"),Some("독 : 출혈"),Some("독 : 신경")] {
        let mut input=corrosion_input(26070,"쉐도우 닷",&[260701,260702]);input["reaperBuild"]=json!(if core=="할루시네이션" {"dark_shadow_dot_legacy"} else {"thirst_chaos_sustain"});input["assumedTargetAbnormalMoveImmune"]=json!(true);input["assumedTargetNerveVulnerable"]=json!(true);
        let mut choices=vec![json!({"name":"죽음의 일격","level":1})];if let Some(name)=poison {choices.push(json!({"name":name,"level":1}));}input["character"]["skills"][0]["tripods"]=json!(choices);input["aplConfig"]=json!("actions:\n - skill: 쉐도우 닷\n   conditions:\n    - {type: time_remaining, operator: '>=', value: 2}\n");
        let old=builder::build(&input.to_string()).unwrap();let points=if tier==0 {15} else {20};let mut slot=destiny_slot(core,grade,points);slot["coreSymbol"]=json!(if core=="치명적인 발걸음" {"해"} else if core=="치명적 악몽" {"달"} else {"별"});input["character"]["arkGrids"]=json!({"reaperSlots":[slot]});let p=builder::build(&input.to_string()).unwrap();
        let factor=if core=="할루시네이션" {if tier==2 {1.01} else if tier==3 {1.015} else {1.}} else {1.015*if core=="치명적 악몽"&&tier>=2 {if tier==2 {1.03} else {1.04}} else {1.}};
        let a=engine::trace_one(&p,42,8000).1;let b=engine::trace_one(&old,42,8000).1;assert_eq!(a.damage_events.len(),b.damage_events.len());let mut independent=false;
        for (hit,old) in a.damage_events.iter().zip(&b.damage_events) {
            assert_eq!((hit.time,hit.hit_id.as_str(),hit.is_crit),(old.time,old.hit_id.as_str(),old.is_crit));
            assert!((hit.formula.skill_damage_multiplier/old.formula.skill_damage_multiplier-factor).abs()<1e-12,"{core}/{grade}/{poison:?}/{}: actual {}, expected {factor}",hit.hit_id,hit.formula.skill_damage_multiplier/old.formula.skill_damage_multiplier);
            assert_eq!(hit.formula.base_damage,old.formula.base_damage);assert_eq!(hit.formula.skill_modifier,old.formula.skill_modifier);
            if hit.hit_id.starts_with("reaper:") {independent=true;assert!((hit.formula.skill_damage_multiplier-factor).abs()<1e-12);}
        }
        assert_eq!(independent,matches!(poison,Some("독 : 부식"|"독 : 출혈")));
    }}}
}

fn bleeding_poison_slot(grade:&str,points:u32) -> Value {
    let mut slot=destiny_slot("출혈독",grade,points);slot["coreSymbol"]=json!("달");slot
}

#[test]
fn reaper_bleeding_poison_grades_keep_stage_global_burst_and_cast_counter_boundaries() {
    for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&p|tier>0||p<=15) {
        let mut input=corrosion_input(26070,"쉐도우 닷",&[260701,260702]);input["character"]["skills"][0]["tripods"]=json!([]);
        let other=corrosion_input(26810,"레이지 스피어",&[268100]);input["skillData"].as_array_mut().unwrap().push(other["skillData"][0].clone());input["character"]["skills"].as_array_mut().unwrap().push(json!({"name":"레이지 스피어","level":10,"tripods":[]}));input["skillData"][1]["properties"]["skillGroupIds"]=json!([3262600]);
        let base=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=json!({"reaperSlots":[bleeding_poison_slot(grade,points)]});let p=builder::build(&input.to_string()).unwrap();
        let factor=if points>=10 {1.015} else {1.}*if tier>=2&&points>=17 {1.01*(1.0016_f64).powi(points.saturating_sub(17) as i32)} else {1.};
        for (index,(skill,old)) in p.skills.iter().zip(&base.skills).enumerate() {
            assert_eq!(skill.cooldown_ms,old.cooldown_ms);assert_eq!(skill.hits.len(),old.hits.len());
            let speed=if points>=10&&index==0 {1.15} else {1.};assert_eq!(skill.cast_times_ms,old.cast_times_ms.iter().map(|&t|(t as f64/speed).round() as u32).collect::<Vec<_>>());
            for (hit,before) in skill.hits.iter().zip(&old.hits) {assert!(((1.+hit.damage_increase)/(1.+before.damage_increase)-factor).abs()<1e-12);assert_eq!(hit.action_delay_ms,(before.action_delay_ms as f64/speed).round() as u32);}
        }
        let count=p.global_triggers.iter().filter(|t|matches!(t.condition,crate::profile::TriggerCondition::BuffStacksAtLeast{..})).count();assert_eq!(count,usize::from(tier>=1&&points>=14));
        if points>=10 {input["reaperBuild"]=json!("dark_shadow_dot_legacy");assert!(builder::build(&input.to_string()).is_err());}
    }}
}

#[test]
fn reaper_bleeding_poison_all_daggers_scale_only_burst_axes_with_identical_stage_timing() {
    for (id,name,ids) in [(26050,"스피닝 대거",vec![260500,260501]),(26060,"스피릿 캐치",vec![260600,260612]),(26070,"쉐도우 닷",vec![260701,260702]),(26080,"이블리스토",vec![260800,260801,260802,260803]),(26110,"샤벨 스팅거",vec![261100,261100,261101,261101]),(26120,"데스 사이드",vec![261200,261201]),(26140,"팬텀 댄서",vec![261400,261401,261402,261403])] {
        let mut input=corrosion_input(id,name,&ids);input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 출혈","level":1}]);input["aplConfig"]=json!(format!("actions:\n - skill: {name}\n   conditions:\n    - {{type: time_remaining, operator: '>=' , value: 2}}\n"));input["character"]["arkGrids"]=json!({"reaperSlots":[bleeding_poison_slot("hero",10)]});let base=engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,8000).1;
        for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {
            input["character"]["arkGrids"]=json!({"reaperSlots":[bleeding_poison_slot(grade,if tier==0 {15} else {20})]});let p=builder::build(&input.to_string()).unwrap();let trace=engine::trace_one(&p,42,8000).1;
            assert_eq!(trace.damage_events.len(),base.damage_events.len());let global=if tier>=2 {1.01*(1.0016_f64).powi(3)} else {1.};let mut burst=false;let mut tick=false;
            for (hit,before) in trace.damage_events.iter().zip(&base.damage_events) {
                assert_eq!((hit.time,hit.hit_id.as_str(),hit.is_crit),(before.time,before.hit_id.as_str(),before.is_crit));assert_eq!(serde_json::to_value(&hit.resource_gains).unwrap(),serde_json::to_value(&before.resource_gains).unwrap());
                let scale=if hit.hit_id=="reaper:bleeding:burst" {burst=true; if tier==2 {1.4} else if tier==3 {1.55} else {1.}} else {if hit.hit_id=="reaper:bleeding:tick" {tick=true;}1.};
                assert!((hit.formula.base_damage/before.formula.base_damage-scale).abs()<1e-12);assert!((hit.formula.skill_modifier/before.formula.skill_modifier-scale).abs()<1e-12);assert!((hit.formula.skill_damage_multiplier/before.formula.skill_damage_multiplier-global).abs()<1e-12);
            }
            assert!(burst&&tick,"{name}/{grade}");
        }
    }
}

#[test]
fn reaper_bleeding_poison_counts_five_casts_across_misses_and_preserves_seven_stack_capacity() {
    use crate::profile::{TriggerEffect,TriggerCondition,TriggerFilter,GlobalTrigger,BuffSpec,StackType};
    let mut input=corrosion_input(26070,"쉐도우 닷",&[260701,260702]);input["character"]["skills"][0]["tripods"]=json!([{"name":"독 : 출혈","level":1}]);input["aplConfig"]=json!("actions:\n - skill: 쉐도우 닷\n");input["character"]["arkGrids"]=json!({"reaperSlots":[destiny_slot("치명적인 발걸음","ancient",20),bleeding_poison_slot("ancient",20)]});let mut p=builder::build(&input.to_string()).unwrap();p.skills[0].max_uses=11;
    for miss in [false,true] {let mut current=p.clone();if miss {current.reaper_reference.as_mut().unwrap().missed_hit_ids=current.skills[0].hits.iter().map(|h|h.hit_id.clone()).collect();}let t=engine::trace_one(&current,42,5000).1;assert_eq!(t.casts.len(),11);
        for (i,cast) in t.casts.iter().enumerate() {let stacks=cast.active_buff_details.iter().find(|b|b.id=="3265600").map_or(0,|b|b.stacks);assert_eq!(stacks,((i+1)%5) as u32);assert_eq!(cast.active_buffs.contains(&"3262100".into()),i>=4);}
        if miss {assert!(t.damage_events.is_empty());} else {assert!(t.damage_events.iter().any(|h|h.hit_id=="reaper:bleeding:burst"));}
    }
    let mut overflow=p.clone();overflow.global_triggers.insert(0,GlobalTrigger {filter:TriggerFilter::BeforeSkillCastId(0),condition:TriggerCondition::Always,effect:TriggerEffect::ApplyBuffStacks {spec:BuffSpec {id:"3265600".into(),effects:Default::default(),max_stacks:7,stack_type:StackType::Refresh,duration_ms:u32::MAX},stacks:6},cooldown_ms:Some(u32::MAX)});let t=engine::trace_one(&overflow,42,5000).1;assert_eq!(t.casts[0].active_buff_details.iter().find(|b|b.id=="3265600").unwrap().stacks,2);assert!(t.casts[0].active_buffs.contains(&"3262100".into()));
    input["character"]["arkGrids"]["reaperSlots"].as_array_mut().unwrap().reverse();let mut reversed=builder::build(&input.to_string()).unwrap();reversed.skills[0].max_uses=11;assert_eq!(serde_json::to_value(engine::trace_one(&p,42,5000)).unwrap(),serde_json::to_value(engine::trace_one(&reversed,42,5000)).unwrap());
    let mut isolated=input.clone();isolated["character"]["arkGrids"]=json!({"reaperSlots":[bleeding_poison_slot("ancient",20)]});let mut standalone=builder::build(&isolated.to_string()).unwrap();standalone.skills[0].max_uses=6;let t=engine::trace_one(&standalone,42,5000).1;assert!(!t.casts[4].active_buffs.contains(&"3265600".into()));assert_eq!(t.casts[5].active_buff_details.iter().find(|b|b.id=="3265600").unwrap().stacks,1);
}

fn smashing_slot(grade:&str,points:u32) -> Value {
    let mut slot=flowing_slot(grade,points);slot["name"]=json!("부수는 일격");slot["coreSymbol"]=json!("달");slot
}

fn smashing_input(grade:&str,points:u32) -> Value {
    let mut input=corrosion_input(26070,"쉐도우 닷",&[260701,260702]);input["character"]["skills"][0]["tripods"]=json!([]);input["character"]["arkGrids"]=json!({"reaperSlots":[smashing_slot(grade,points)]});input["aplConfig"]=json!("actions:\n - skill: 쉐도우 닷\n");input["reaperIdentityModel"]=json!({"initialDark":0,"initialHeat":0,"initialChaosSeconds":0,"darkPerSecond":0,"personaDarkLossPerSecond":0,"heatLossPerSecond":0,"skillGains":{}});input
}

#[test]
fn reaper_smashing_core_all_grades_replace_attack_stage_and_accumulate_crit() {
    use crate::profile::TriggerEffect;
    for (tier,grade) in ["hero","legend","relic","ancient"].into_iter().enumerate() {for points in [9,10,13,14,16,17,18,19,20].into_iter().filter(|&p|tier>0||p<=15) {for seed in [false,true] {
        let mut input=smashing_input(grade,points);input["assumedTargetSeedOrLower"]=json!(seed);let p=builder::build(&input.to_string()).unwrap();input["character"]["arkGrids"]=Value::Null;let base=builder::build(&input.to_string()).unwrap();
        let crit=if tier>=1&&points>=14 {0.65} else {0.}+if tier>=2&&points>=17 {if tier==2 {1.3} else {2.6}} else {0.}+if tier>=2 {points.saturating_sub(17) as f64*0.21} else {0.};assert!((p.stat.sum(StatField::CritRate)-base.stat.sum(StatField::CritRate)-crit).abs()<1e-12);
        let procs:Vec<_>=p.global_triggers.iter().filter(|t|matches!(&t.effect,TriggerEffect::ScheduleDot{dot_id,..} if dot_id=="608231000")).collect();assert_eq!(procs.len(),usize::from(points>=10&&!seed));
        if let Some(proc)=procs.first() {assert_eq!(proc.cooldown_ms,Some(if tier>=2&&points>=17 {if tier==2 {20000} else {15000}} else {30000}));if let TriggerEffect::ScheduleDot{hit,first_tick_ms,tick_count,tick_interval_ms,independent,..}=&proc.effect {assert_eq!((*first_tick_ms,*tick_count,*tick_interval_ms,*independent),(0,1,0,true));assert_eq!((hit.base_damage,hit.skill_modifier,hit.destruction),(736.5,6.813,1));}}
        assert_eq!(serde_json::to_value(&p.skills).unwrap(),serde_json::to_value(&base.skills).unwrap());
    }}}
}

#[test]
fn reaper_smashing_core_exact_shared_cooldown_and_potential_destruction_histogram() {
    use crate::profile::TriggerEffect;
    for (grade,cd,count) in [("hero",30000,3),("relic",20000,4),("ancient",15000,5)] {
        let mut p=builder::build(&smashing_input(grade,if grade=="hero" {15} else {20}).to_string()).unwrap();p.stat.swiftness=0.;p.stat.attack_speed.clear();p.stat.attack_move_speed_percent=0.;p.stat.bake();p.skills[0].cast_times_ms=vec![1000];for hit in &mut p.skills[0].hits {hit.action_delay_ms=0;}
        let (result,trace)=engine::trace_one(&p,42,61001);let procs:Vec<_>=trace.damage_events.iter().filter(|h|h.hit_id=="smashing_strike").collect();assert_eq!(procs.len(),count);for (i,hit) in procs.iter().enumerate() {assert_eq!((hit.time,hit.destruction_level),(i as f64*cd as f64/1000.,1));assert!(hit.resource_gains.is_empty());assert_eq!((hit.formula.base_damage,hit.formula.skill_modifier),(736.5,6.813));}
        assert_eq!(result.skill_stats[&0].destruction_hits,std::collections::BTreeMap::from([(1,count as u32)]));assert_eq!(engine::simulate(&p,3,61001,42,None).skill_stats[&0].destruction_hits,std::collections::BTreeMap::from([(1,3*count as u32)]));
        let mut plain=p.clone();for t in &mut plain.global_triggers {if let TriggerEffect::ScheduleDot{dot_id,hit,..}=&mut t.effect {if dot_id=="608231000" {hit.destruction=0;}}}let (old,other)=engine::trace_one(&plain,42,61001);assert_eq!(result.total_damage,old.total_damage);assert_eq!(result.skill_stats[&0].hit_count,old.skill_stats[&0].hit_count);assert!(old.skill_stats[&0].destruction_hits.is_empty());
        for (a,b) in trace.damage_events.iter().zip(&other.damage_events) {let mut a=serde_json::to_value(a).unwrap();let mut b=serde_json::to_value(b).unwrap();a.as_object_mut().unwrap().remove("destructionLevel");b.as_object_mut().unwrap().remove("destructionLevel");assert_eq!(a,b);}
    }
}

#[test]
fn reaper_smashing_core_independent_packet_excludes_source_bonuses_and_missed_hits() {
    let mut input=smashing_input("ancient",20);let other=corrosion_input(26810,"레이지 스피어",&[268100]);input["skillData"].as_array_mut().unwrap().push(other["skillData"][0].clone());input["character"]["skills"].as_array_mut().unwrap().push(json!({"name":"레이지 스피어","level":10,"tripods":[]}));input["aplConfig"]=json!("actions:\n - skill: 쉐도우 닷\n - skill: 레이지 스피어\n");let mut p=builder::build(&input.to_string()).unwrap();p.skills[0].cooldown_ms=1500;p.skills[1].cooldown_ms=1000;p.stat.crit_rate.clear();p.stat.crit_rate.insert("test".into(),-100.);
    let trace=engine::trace_one(&p,42,46000).1;let procs:Vec<_>=trace.damage_events.iter().filter(|h|h.hit_id=="smashing_strike").collect();assert_eq!(procs.len(),4);for pair in procs.windows(2) {let delta=((pair[1].time-pair[0].time)*1000.).round();assert!(delta>=15000.&&delta<16500.,"proc delta={delta}ms");}
    let mut changed=p.clone();for skill in &mut changed.skills {skill.evolution_damage_bonuses.push(("owner-only".into(),999.));for hit in &mut skill.hits {hit.damage_increase+=999.;hit.crit_damage_bonus+=999.;hit.defense_ignore=1.;}}let changed=engine::trace_one(&changed,42,46000).1;assert_eq!(serde_json::to_value(procs).unwrap(),serde_json::to_value(changed.damage_events.iter().filter(|h|h.hit_id=="smashing_strike").collect::<Vec<_>>()).unwrap());
    let mut missed=p.clone();missed.reaper_reference.as_mut().unwrap().missed_hit_ids=missed.skills.iter().flat_map(|s|s.hits.iter().map(|h|h.hit_id.clone())).collect();let (r,t)=engine::trace_one(&missed,42,46000);assert!(t.damage_events.is_empty());assert!(r.skill_stats.values().all(|s|s.destruction_hits.is_empty()));
}

#[test]
fn reaper_incoming_damage_order_limits_reset_and_no_outgoing_rng_change() {
    let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);input["character"]["skills"][0]["tripods"]=json!([]);
    input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");
    input["character"]["stats"]["maxhp"]=json!(1000.);
    let plain=builder::build(&input.to_string()).unwrap(); let baseline=engine::trace_one(&plain,42,1000);assert!(!baseline.1.damage_events.is_empty());
    input["reaperIncomingDamage"]=json!([{"timeMs":500,"damage":900.},{"timeMs":0,"damage":100.},{"timeMs":500,"damage":50.},{"timeMs":1000,"damage":0.},{"timeMs":1001,"damage":500.}]);
    let selected=builder::build(&input.to_string()).unwrap();let (result,trace)=engine::trace_one(&selected,42,1000);
    assert_eq!(serde_json::to_value(result).unwrap(),serde_json::to_value(baseline.0).unwrap());
    assert_eq!(serde_json::to_value(&trace.damage_events).unwrap(),serde_json::to_value(baseline.1.damage_events).unwrap());
    assert_eq!(trace.incoming_damage_events.iter().map(|e|(e.time,e.hp_before,e.hp_after,e.applied_damage)).collect::<Vec<_>>(),vec![(0.,1000.,900.,100.),(0.5,900.,0.,900.),(0.5,0.,0.,0.),(1.,0.,0.,0.)]);
    for _ in 0..100 {assert_eq!(serde_json::to_value(engine::trace_one(&selected,42,1000).1.incoming_damage_events).unwrap(),serde_json::to_value(&trace.incoming_damage_events).unwrap());}
    assert!(engine::trace_one(&plain,42,1000).1.incoming_damage_events.is_empty());
    for bad in [json!([{"timeMs":0,"damage":-1}]),json!([{"timeMs":-1,"damage":1}]),json!([{"timeMs":0.5,"damage":1}]),json!([{"timeMs":0,"damage":1,"unknown":1}]),json!(vec![json!({"timeMs":0,"damage":1});1025])] {
        input["reaperIncomingDamage"]=bad; assert!(builder::build(&input.to_string()).is_err());
    }
    input["reaperIncomingDamage"]=json!([{"timeMs":0,"damage":1}]);input["character"]["stats"]["maxhp"]=json!(0.);assert!(builder::build(&input.to_string()).unwrap_err().contains("피격 검사"));
}

#[test]
fn reaper_incoming_damage_keeps_default_cast_polling_and_heal_caps() {
    let mut input=configured(true);input["character"]["stats"]["maxhp"]=json!(10000.);
    let plain=builder::build(&input.to_string()).unwrap();let old=engine::trace_one(&plain,42,300000);
    input["reaperIncomingDamage"]=json!([{"timeMs":0,"damage":2500.},{"timeMs":500,"damage":12000.}]);
    let profile=builder::build(&input.to_string()).unwrap();let now=engine::trace_one(&profile,42,300000);
    assert_eq!(serde_json::to_value(old.0).unwrap(),serde_json::to_value(now.0).unwrap());
    assert_eq!(old.1.damage_events.iter().map(|e|(e.time,e.result_damage,e.is_crit)).collect::<Vec<_>>(),now.1.damage_events.iter().map(|e|(e.time,e.result_damage,e.is_crit)).collect::<Vec<_>>());
    let mut heal=corrosion_input(26810,"레이지 스피어",&[268100]);heal["character"]["skills"][0]["tripods"]=json!([]);
    heal["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");
    heal["character"]["stats"]["maxhp"]=json!(5000.);
    heal["character"]["arkGrids"]=json!({"reaperSlots":[trace_hero_slot("흡수의 일격","hero",10)]});
    heal["reaperIncomingDamage"]=json!([{"timeMs":0,"damage":1000.},{"timeMs":11,"damage":2100.}]);
    let events=engine::trace_one(&builder::build(&heal.to_string()).unwrap(),42,1000).1.incoming_damage_events;
    assert_eq!((events[0].hp_after,events[1].hp_before,events[1].hp_after),(4000.,5000.,2900.));
}

#[test]
fn reaper_stable_attack_whole_grades_replace_reduction_and_keep_outgoing_scope() {
    for grade in ["hero","legend","relic","ancient"] {for points in [9,10,13,14,15,16,17,18,19,20] {
        if grade=="hero" && points>15 {continue;}
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);input["character"]["skills"][0]["tripods"]=json!([]);
        input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");input["character"]["stats"]["maxhp"]=json!(10000.);
        input["reaperIncomingDamage"]=json!([{"timeMs":0,"damage":1000.},{"timeMs":0,"damage":0.},{"timeMs":50,"damage":20000.}]);
        let base=builder::build(&input.to_string()).unwrap();let old=engine::trace_one(&base,42,1000);
        let mut slot=trace_hero_slot("안정적인 공격",grade,points);slot["coreSymbol"]=json!("해");
        input["character"]["arkGrids"]=json!({"reaperSlots":[slot]});let selected=builder::build(&input.to_string()).unwrap();
        let upper=["relic","ancient"].contains(&grade);
        let rate=if points<10 {0.}else if upper&&points>=17 {if grade=="ancient" {0.025}else{0.015}}else{0.005};
        let bonus=if grade=="hero" || points<14 {0.}else{0.007+if upper&&points>=17 {if grade=="ancient" {0.028}else{0.014}}else{0.}+if upper{points.saturating_sub(17) as f64*0.0023}else{0.}};
        assert!((selected.stat.sum(StatField::AdditionalDamage)-base.stat.sum(StatField::AdditionalDamage)-bonus).abs()<1e-12);
        let (_,trace)=engine::trace_one(&selected,42,1000);
        assert!((trace.incoming_damage_events[0].applied_damage-1000.*(1.-rate)).abs()<1e-9);
        assert_eq!(trace.incoming_damage_events[1].applied_damage,0.);assert_eq!(trace.incoming_damage_events[2].hp_after,0.);
        assert_eq!(trace.damage_events.iter().map(|e|e.time).collect::<Vec<_>>(),old.1.damage_events.iter().map(|e|e.time).collect::<Vec<_>>());
        for (event,previous) in trace.damage_events.iter().zip(&old.1.damage_events) {
            let expected=(1.+selected.stat.sum(StatField::AdditionalDamage))/(1.+base.stat.sum(StatField::AdditionalDamage));
            assert!((event.result_damage/previous.result_damage-expected).abs()<1e-10);
            assert_eq!(event.is_crit,previous.is_crit);
        }
        let mut reversed=input.clone();reversed["character"]["arkGrids"]["reaperSlots"][0]["gems"].as_array_mut().unwrap().reverse();
        assert_eq!(serde_json::to_value(engine::trace_one(&builder::build(&reversed.to_string()).unwrap(),42,1000).1.incoming_damage_events).unwrap(),serde_json::to_value(trace.incoming_damage_events).unwrap());
    }}
}

#[test]
fn reaper_life_all_grades_require_separate_baseline_and_share_hp_heal_cap() {
    for grade in ["hero","legend","relic","ancient"] {for points in [9,10,13,14,16,17,18,19,20] {
        if grade=="hero" && points>15 {continue;}
        let mut input=corrosion_input(26810,"레이지 스피어",&[268100]);
        input["character"]["skills"][0]["tripods"]=json!([]);input["aplConfig"]=json!("actions:\n - skill: 레이지 스피어\n");
        input["character"]["stats"]["maxhp"]=json!(20000.);
        let base=builder::build(&input.to_string()).unwrap();
        let mut slot=trace_hero_slot("생명",grade,points);slot["coreSymbol"]=json!("별");
        input["character"]["arkGrids"]=json!({"reaperSlots":[slot]});
        let damage_only=builder::build(&input.to_string()).unwrap();
        assert_eq!(damage_only.max_hp,base.max_hp);
        assert_eq!(damage_only.stat.base_vitality,base.stat.base_vitality);
        assert_eq!(damage_only.stat.max_hp,base.stat.max_hp);
        assert_eq!(engine::trace_one(&base,42,1000).0.total_damage,
            engine::trace_one(&damage_only,42,1000).0.total_damage);
        input["reaperHpBaseline"]=json!(5000.);
        let upper=["relic","ancient"].contains(&grade);
        let con=if points<10 {0.}else{420.+if upper&&points>=17 {if grade=="ancient" {1260.}else{840.}}else{0.}+if upper {points.saturating_sub(17) as f64*140.}else{0.}};
        let flat=if grade=="hero"||points<14 {0.}else{900.+if upper&&points>=17 {if grade=="ancient" {2700.}else{1800.}}else{0.}};
        let selected=builder::build(&input.to_string()).unwrap();let hp=5000.+con*2.+flat;
        assert_eq!(selected.max_hp,hp);
        assert_eq!(selected.stat.base_vitality-base.stat.base_vitality,con);assert_eq!(selected.stat.max_hp-base.stat.max_hp,flat);
        let old=engine::trace_one(&base,42,1000);let same=engine::trace_one(&selected,42,1000);
        assert_eq!(old.0.total_damage,same.0.total_damage);assert_eq!(old.1.damage_events.len(),same.1.damage_events.len());assert!(!same.1.damage_events.is_empty());
        input["character"]["arkGrids"]["reaperSlots"].as_array_mut().unwrap().push(trace_hero_slot("흡수의 일격","hero",10));
        input["reaperIncomingDamage"]=json!([{"timeMs":0,"damage":1000.},{"timeMs":11,"damage":2100.}]);
        let traced=engine::trace_one(&builder::build(&input.to_string()).unwrap(),42,1000).1;
        assert_eq!(traced.incoming_damage_events[0].hp_before,hp);assert_eq!(traced.incoming_damage_events[0].hp_after,hp-1000.);
        assert_eq!(traced.incoming_damage_events[1].hp_before,hp);assert_eq!(traced.incoming_damage_events[1].hp_after,hp-2100.);
        for invalid in [json!(0),json!(-1),json!(true),json!("5000")] {let mut bad=input.clone();bad["reaperHpBaseline"]=invalid;assert!(builder::build(&bad.to_string()).is_err());}
    }}
}
