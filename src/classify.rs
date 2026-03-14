use std::fs;
use std::path::Path;

use crate::types::{ActionTypeClassification, ProjectClassification};
use crate::util::{call_openclaw, extract_json, which_exists};

pub fn classify_project(
    task_content: &str,
    projects_dir: &Path,
    openclaw_cmd: &str,
) -> (String, Option<String>, String, serde_json::Value) {
    // Rule 1: Explicit "Project: <name>" line
    for line in task_content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix("Project:")
            .or_else(|| trimmed.strip_prefix("project:"))
        {
            let name = rest.trim().to_string();
            if !name.is_empty() {
                let kind = if projects_dir.join(&name).is_dir() {
                    "existing"
                } else {
                    "new"
                };
                let json = serde_json::json!({
                    "project": { "kind": kind, "name": name },
                    "confidence": 1.0,
                    "rationale": "Matched via explicit-line"
                });
                return (
                    kind.to_string(),
                    Some(name),
                    "explicit-line".to_string(),
                    json,
                );
            }
        }
    }

    // Rule 2: #project/<name> tag
    for line in task_content.lines() {
        if let Some(pos) = line.find("#project/") {
            let rest = &line[pos + 9..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if !name.is_empty() {
                let kind = if projects_dir.join(&name).is_dir() {
                    "existing"
                } else {
                    "new"
                };
                let json = serde_json::json!({
                    "project": { "kind": kind, "name": name },
                    "confidence": 1.0,
                    "rationale": "Matched via tag"
                });
                return (kind.to_string(), Some(name), "tag".to_string(), json);
            }
        }
    }

    // Rule 3: Case-insensitive substring match
    let task_lower = task_content.to_lowercase();
    if let Ok(entries) = fs::read_dir(projects_dir) {
        let mut project_names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        project_names.sort();
        for name in project_names {
            if task_lower.contains(&name.to_lowercase()) {
                let json = serde_json::json!({
                    "project": { "kind": "existing", "name": name },
                    "confidence": 1.0,
                    "rationale": "Matched via folder-match"
                });
                return (
                    "existing".to_string(),
                    Some(name),
                    "folder-match".to_string(),
                    json,
                );
            }
        }
    }

    // Rule 4: AI classification
    if which_exists(openclaw_cmd) {
        let args_base = vec![
            "agent".to_string(),
            "--agent".to_string(),
            "main".to_string(),
            "--timeout".to_string(),
            "120".to_string(),
        ];

        let existing_projects = fs::read_dir(projects_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();

        let prompt = format!(
            "You are a strict JSON API. Classify the following task into a project.\n\n\
             Return ONLY a JSON object:\n\
             {{\"project\": {{\"kind\": \"existing\"|\"new\"|\"none\", \"name\": \"string or null\"}}, \
             \"confidence\": 0.0, \"rationale\": \"string\"}}\n\n\
             Existing projects: {}\n\n\
             Rules:\n\
             - kind=existing if the task clearly belongs to one of the existing projects.\n\
             - kind=new if the task requires a new project that doesn't exist yet. Provide a kebab-case name.\n\
             - kind=none if it's personal admin, a question, or doesn't warrant a project.\n\
             - Output MUST be valid JSON. Nothing else.\n\n\
             Task:\n{}",
            existing_projects, task_content
        );
        let mut args = args_base;
        args.push("--message".to_string());
        args.push(prompt);

        println!("Calling OpenClaw for project classification...");
        if let Some(output) = call_openclaw(openclaw_cmd, &args) {
            if let Some(parsed) = extract_json(&output) {
                if let Ok(classification) =
                    serde_json::from_value::<ProjectClassification>(parsed.clone())
                {
                    let kind = classification.project.kind.clone();
                    let name = classification.project.name.clone();
                    println!(
                        "AI classification: kind={} name={}",
                        kind,
                        name.as_deref().unwrap_or("")
                    );
                    return (kind, name, "ai".to_string(), parsed);
                }
            }
            println!("AI classification JSON invalid, skipping.");
        }
    }

    // Default
    let json = serde_json::json!({
        "project": { "kind": "none", "name": null },
        "confidence": 1.0,
        "rationale": "No project match (default)"
    });
    ("none".to_string(), None, "default".to_string(), json)
}

pub fn classify_action_type(
    task_content: &str,
    openclaw_cmd: &str,
) -> (String, String, serde_json::Value) {
    let task_lower = task_content.to_lowercase();

    // Deterministic keyword overrides
    let keywords_repo = ["fix", "implement", "add flag", "refactor", "pr"];
    let keywords_research = ["compare", "research", "find out", "summarize"];
    let keywords_ops = ["install", "brew", "launchctl", "tailscale"];

    if keywords_repo.iter().any(|k| contains_word(&task_lower, k)) {
        let json = action_type_json("repo-change", "keyword");
        return ("repo-change".to_string(), "keyword".to_string(), json);
    }
    if keywords_research
        .iter()
        .any(|k| contains_word(&task_lower, k))
    {
        let json = action_type_json("research", "keyword");
        return ("research".to_string(), "keyword".to_string(), json);
    }
    if keywords_ops.iter().any(|k| contains_word(&task_lower, k)) {
        let json = action_type_json("ops", "keyword");
        return ("ops".to_string(), "keyword".to_string(), json);
    }
    if task_content.lines().any(|l| l.trim().ends_with('?')) {
        let json = action_type_json("question", "keyword");
        return ("question".to_string(), "keyword".to_string(), json);
    }

    // AI fallback
    if which_exists(openclaw_cmd) {
        let mut args = vec![
            "agent".to_string(),
            "--agent".to_string(),
            "main".to_string(),
            "--timeout".to_string(),
            "120".to_string(),
        ];

        let prompt = format!(
            "You are a strict JSON API. Classify the action type for the following task.\n\n\
             Return ONLY a JSON object:\n\
             {{\"action_type\": \"repo-change\"|\"research\"|\"ops\"|\"question\"|\"note\", \
             \"confidence\": 0.0, \"rationale\": \"...\", \"suggested_repo\": \"string or null\"}}\n\n\
             Rules:\n\
             - repo-change: task requires code changes, PRs, or modifications to a repository\n\
             - research: task requires investigation, comparison, or summarization\n\
             - ops: task requires infrastructure, tooling, or system administration\n\
             - question: task is asking a question that needs an answer\n\
             - note: everything else (personal admin, reminders, etc.)\n\
             - Output MUST be valid JSON. Nothing else.\n\n\
             Task:\n{}",
            task_content
        );
        args.push("--message".to_string());
        args.push(prompt);

        println!("Calling OpenClaw for action type classification...");
        if let Some(output) = call_openclaw(openclaw_cmd, &args) {
            if let Some(parsed) = extract_json(&output) {
                if let Ok(at) = serde_json::from_value::<ActionTypeClassification>(parsed.clone()) {
                    let valid = ["repo-change", "research", "ops", "question", "note"];
                    if valid.contains(&at.action_type.as_str()) {
                        println!("AI action type: {}", at.action_type);
                        return (at.action_type, "ai".to_string(), parsed);
                    }
                }
            }
            println!("AI action type JSON invalid, defaulting to note.");
        }
    }

    let json = action_type_json("note", "default");
    ("note".to_string(), "default".to_string(), json)
}

fn action_type_json(action_type: &str, method: &str) -> serde_json::Value {
    serde_json::json!({
        "action_type": action_type,
        "confidence": 1.0,
        "rationale": format!("Matched via {}", method),
        "suggested_repo": null
    })
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack.contains(needle)
}
