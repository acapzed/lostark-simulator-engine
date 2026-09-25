use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AplAction {
    pub skill_id: u32,
    pub conditions: Vec<AplCondition>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CardAplAction {
    pub card_id: u32,
    pub card_name: String,
    pub conditions: Vec<AplCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AplCondition {
    CooldownReady,
    SkillCooldownReady(u32),
    SkillCooldownRemaining {
        skill_id: u32,
        operator: CompareOperator,
        value_ms: u64,
    },
    Resource {
        resource: String,
        operator: CompareOperator,
        value: f64,
    },
    CardHeld(u32),
    CardCount {
        card_id: u32,
        operator: CompareOperator,
        value: f64,
    },
    NextSkill(u32),
    BuffActive(String),
    BuffInactive(String),
    BuffRemaining {
        buff_id: String,
        operator: CompareOperator,
        value_ms: u64,
    },
    TargetDebuffStacks {
        debuff_id: String,
        operator: CompareOperator,
        value: f64,
    },
    ComboPhase {
        skill_id: u32,
        operator: CompareOperator,
        value: f64,
    },
    ChainPhase {
        skill_id: u32,
        operator: CompareOperator,
        value: f64,
    },
    TimeRemaining {
        operator: CompareOperator,
        value_ms: u64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CompareOperator {
    Lt,
    Gt,
    Lte,
    Gte,
    Eq,
    Ne,
}

impl CompareOperator {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "<" => Some(Self::Lt),
            ">" => Some(Self::Gt),
            "<=" => Some(Self::Lte),
            ">=" => Some(Self::Gte),
            "==" => Some(Self::Eq),
            "!=" => Some(Self::Ne),
            _ => None,
        }
    }

    pub fn compare(self, actual: f64, expected: f64) -> bool {
        match self {
            Self::Lt => actual < expected,
            Self::Gt => actual > expected,
            Self::Lte => actual <= expected,
            Self::Gte => actual >= expected,
            Self::Eq => (actual - expected).abs() < f64::EPSILON,
            Self::Ne => (actual - expected).abs() >= f64::EPSILON,
        }
    }
}
