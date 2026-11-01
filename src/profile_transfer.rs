//! Internal, same-engine Worker transport. Not a persistent user save format.
use crate::profile::{CharacterProfiles, StatField};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    version: u32,
    stat_sums: [f64; 18],
    profile: T,
}

pub fn encode(profile: &CharacterProfiles) -> Result<String, String> {
    use StatField::*;
    let fields = [WeaponAttackPower, CritRate, CritDmg, OnCritDamageTaken,
        AttackPowerMul, AdditionalDamage, EvolutionDamage, DamageIncrease,
        TargetDamageIncrease, AttackSpeed, MovementSpeed, MovementSpeedMax,
        TargetMovementSpeedReduction, DodgeRate, CooldownReduction,
        TargetDefenseReduction, TypeDamage, ElementalDamage];
    let mut sums = [0.0; 18];
    for field in fields { sums[field as usize] = profile.stat.sum(field); }
    serde_json::to_string(&Envelope { version: 1, stat_sums: sums, profile }).map_err(|e| e.to_string())
}

pub fn decode(json: &str) -> Result<CharacterProfiles, String> {
    let mut payload: Envelope<CharacterProfiles> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if payload.version != 1 {
        return Err("지원하지 않는 워커 프로필 형식입니다.".into());
    }
    if payload.stat_sums.iter().any(|v| !v.is_finite()) { return Err("프로필 능력치 합산값이 올바르지 않습니다.".into()); }
    payload.profile.stat.compiled_sums = Some(payload.stat_sums);
    Ok(payload.profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_profile_copies_preserve_values_and_seeded_execution() {
        let profile = crate::builder::build_raid(include_str!("../fixtures/arcana-representative-request.json")).unwrap();
        let serialized = encode(&profile).unwrap();
        let first = decode(&serialized).unwrap();
        assert_eq!(serde_json::to_value(&profile).unwrap(), serde_json::to_value(&first).unwrap());
        for seed in [1, 42, 1968666852] {
            let expected = crate::engine::trace_one(&first, seed, 180000);
            let original = crate::engine::trace_one(&profile, seed, 180000);
            assert_eq!(serde_json::to_value(&original.0).unwrap(), serde_json::to_value(&expected.0).unwrap());
            for _ in 0..3 {
                let copy = decode(&serialized).unwrap();
                let actual = crate::engine::trace_one(&copy, seed, 180000);
                assert_eq!(serde_json::to_value(&expected.0).unwrap(), serde_json::to_value(&actual.0).unwrap());
                assert_eq!(serde_json::to_value(&expected.1).unwrap(), serde_json::to_value(&actual.1).unwrap());
            }
        }
        assert!(decode(&serialized.replace("\"version\":1", "\"version\":2")).is_err());
        assert!(decode("{}").is_err());
    }

    #[test]
    fn transfer_preserves_original_bucket_sum_and_mutation_invalidates_it() {
        let mut profile = crate::builder::build_raid(include_str!("../fixtures/arcana-representative-request.json")).unwrap();
        for (source, value) in [("a", 1e16), ("b", -1e16), ("c", 1.0), ("d", 0.1)] {
            profile.stat.insert(StatField::EvolutionDamage, source, value);
        }
        let sum = profile.stat.sum(StatField::EvolutionDamage);
        let mut restored = decode(&encode(&profile).unwrap()).unwrap();
        assert_eq!(restored.stat.sum(StatField::EvolutionDamage).to_bits(), sum.to_bits());
        restored.stat.field_mut(StatField::EvolutionDamage).clear();
        assert_eq!(restored.stat.sum(StatField::EvolutionDamage), 0.0);
        let value = serde_json::to_value(&restored).unwrap();
        assert!(value["stat"].get("compiled_sums").is_none());
        assert!(serde_json::from_str::<crate::profile::reaper::ReaperReference>(
            r#"{"scenario":"dark_shadow_dot_legacy","metadata":{}}"#).is_err());
    }
}
