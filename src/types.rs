use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Enrichment {
    pub planned_actions: Vec<String>,
    pub clarifying_questions: Vec<String>,
    pub next_step: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProjectClassification {
    pub project: ProjectInfo,
    pub confidence: f64,
    pub rationale: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProjectInfo {
    pub kind: String,
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActionTypeClassification {
    pub action_type: String,
    pub confidence: f64,
    pub rationale: String,
    pub suggested_repo: Option<String>,
}

#[allow(dead_code)]
#[derive(Serialize, Clone, Debug)]
pub struct Envelope {
    pub version: String,
    pub timestamp: String,
    pub source_file: String,
    pub task_text: String,
    pub classification: serde_json::Value,
    pub action_type: serde_json::Value,
    pub planning: serde_json::Value,
    pub enrichment: serde_json::Value,
    pub execution: serde_json::Value,
    pub status: String,
}

/// Result of processing a single inbox item.
#[derive(Debug)]
pub struct ItemResult {
    pub source_file: String,
    pub project_name: Option<String>,
    pub action_type: String,
    pub exec_status: String,
    pub enriched: bool,
    pub failed: bool,
    pub pr_url: Option<String>,
}
