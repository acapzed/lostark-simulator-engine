use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaperScenario {
    DarkShadowDotLegacy,
    ThirstChaosSustain,
}

/// Opt-in reference runs only; not complete Reaper support.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaperReference {
    pub scenario: ReaperScenario,
    #[serde(default)]
    pub missed_hit_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gauge_test: Option<ReaperGaugeTest>,
    #[serde(default, skip_deserializing)]
    pub metadata: serde_json::Value,
}

/// Compiled profiles retain builder-owned metadata. Input ReaperReference continues
/// to reject caller-supplied metadata through its separate Deserialize contract.
pub fn deserialize_compiled_reference<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ReaperReference>, D::Error> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Compiled {
        scenario: ReaperScenario,
        #[serde(default)]
        missed_hit_ids: Vec<String>,
        #[serde(default)]
        gauge_test: Option<ReaperGaugeTest>,
        metadata: serde_json::Value,
    }
    Ok(Option::<Compiled>::deserialize(deserializer)?.map(|r| ReaperReference {
        scenario: r.scenario, missed_hit_ids: r.missed_hit_ids,
        gauge_test: r.gauge_test, metadata: r.metadata,
    }))
}

/// Synthetic boundary inputs; never interpreted as measured game calibration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaperGaugeTest {
    pub initial_dark: f64,
    pub hit_gains: std::collections::HashMap<String, ReaperGaugeGain>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaperGaugeGain { pub dark: f64, pub heat: f64 }

/// Explicit calibration inputs, not measured or inferred game values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaperIdentityModel {
    pub initial_dark: f64,
    pub initial_heat: f64,
    pub initial_chaos_seconds: f64,
    pub dark_per_second: f64,
    pub persona_dark_loss_per_second: f64,
    pub heat_loss_per_second: f64,
    pub skill_gains: std::collections::HashMap<u32, ReaperGaugeGain>,
}

/// Explicit damage after defense, before core reduction; not an enemy AI model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaperIncomingDamage { pub time_ms: u32, pub damage: f64 }
