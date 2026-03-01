use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize, Debug)]
pub struct PolicyConfig {
    pub models: std::collections::HashMap<String, ModelConfig>,
    pub routing: std::collections::HashMap<String, String>,
    pub overrides: PolicyOverrides,
}

#[derive(Deserialize, Debug)]
pub struct ModelConfig {
    pub name: String,
    #[allow(dead_code)]
    pub max_tokens: u32,
    #[allow(dead_code)]
    pub temperature: f64,
}

#[derive(Deserialize, Debug)]
pub struct PolicyOverrides {
    #[serde(default = "default_deep_tag")]
    pub deep_tag: String,
    #[serde(default = "default_expensive")]
    pub deep_tag_model: String,
}

fn default_deep_tag() -> String {
    "#deep".to_string()
}
fn default_expensive() -> String {
    "expensive".to_string()
}

pub fn load_policy() -> Option<PolicyConfig> {
    let policy_path = std::env::var("DIGEST_POLICY")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let exe = std::env::current_exe().unwrap_or_default();
            let repo_root = exe
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent());
            match repo_root {
                Some(r) => r.join("config/policy.json"),
                None => PathBuf::from("config/policy.json"),
            }
        });

    if !policy_path.exists() {
        return None;
    }

    let data = fs::read_to_string(&policy_path).ok()?;
    let config: PolicyConfig = serde_json::from_str(&data).ok()?;
    println!("Policy loaded: {}", policy_path.display());
    Some(config)
}

pub fn select_model(
    policy: &Option<PolicyConfig>,
    step: &str,
    task_content: &str,
) -> Option<String> {
    let policy = policy.as_ref()?;
    let tier_name = policy
        .routing
        .get(step)
        .cloned()
        .unwrap_or_else(|| "mid".to_string());

    let effective_tier = if task_content.contains(&policy.overrides.deep_tag) {
        &policy.overrides.deep_tag_model
    } else {
        &tier_name
    };

    policy.models.get(effective_tier).map(|m| m.name.clone())
}
