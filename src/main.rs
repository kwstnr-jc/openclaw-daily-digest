use chrono::Local;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "openclaw-daily-digest")]
enum Cli {
    /// Process inbox items
    Run {
        /// Override the vault root directory
        #[arg(long, env = "DIGEST_ROOT")]
        root: Option<String>,

        /// Do everything except move the inbox file
        #[arg(long)]
        dry_run: bool,

        /// Maximum items to process per run (0 = unlimited)
        #[arg(long, default_value = "10")]
        max_items: usize,
    },
}

// ---------------------------------------------------------------------------
// JSON schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Enrichment {
    planned_actions: Vec<String>,
    clarifying_questions: Vec<String>,
    next_step: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ProjectClassification {
    project: ProjectInfo,
    confidence: f64,
    rationale: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ProjectInfo {
    kind: String,
    name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ActionTypeClassification {
    action_type: String,
    confidence: f64,
    rationale: String,
    suggested_repo: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
struct Envelope {
    version: String,
    timestamp: String,
    source_file: String,
    task_text: String,
    classification: serde_json::Value,
    action_type: serde_json::Value,
    planning: serde_json::Value,
    enrichment: serde_json::Value,
    execution: serde_json::Value,
    status: String,
}

#[derive(Deserialize, Debug)]
struct PolicyConfig {
    models: std::collections::HashMap<String, ModelConfig>,
    routing: std::collections::HashMap<String, String>,
    overrides: PolicyOverrides,
}

#[derive(Deserialize, Debug)]
struct ModelConfig {
    name: String,
    #[allow(dead_code)]
    max_tokens: u32,
    #[allow(dead_code)]
    temperature: f64,
}

#[derive(Deserialize, Debug)]
struct PolicyOverrides {
    #[serde(default = "default_deep_tag")]
    deep_tag: String,
    #[serde(default = "default_expensive")]
    deep_tag_model: String,
}

fn default_deep_tag() -> String {
    "#deep".to_string()
}
fn default_expensive() -> String {
    "expensive".to_string()
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();
    match cli {
        Cli::Run {
            root,
            dry_run,
            max_items,
        } => {
            let exit_code = run(root, dry_run, max_items);
            std::process::exit(exit_code);
        }
    }
}

/// Result of processing a single inbox item.
#[derive(Debug)]
struct ItemResult {
    source_file: String,
    project_name: Option<String>,
    action_type: String,
    exec_status: String,
    enriched: bool,
    failed: bool,
}

fn run(root_override: Option<String>, dry_run: bool, max_items: usize) -> i32 {
    let root = PathBuf::from(
        root_override.unwrap_or_else(|| "/Users/Shared/agent-vault/Agent".to_string()),
    );
    let inbox = root.join("Inbox");
    let outbox = root.join("Outbox");
    let logs = root.join("Logs");
    let processed = inbox.join("Processed");
    let failed = inbox.join("Failed");
    let projects_dir = root.join("Projects");

    // Ensure directories exist
    for dir in [&outbox, &logs, &processed, &failed] {
        fs::create_dir_all(dir).ok();
    }

    let openclaw_cmd =
        std::env::var("OPENCLAW_CMD").unwrap_or_else(|_| "openclaw".to_string());

    // Load policy
    let policy = load_policy();

    // Process items in a loop
    let mut results: Vec<ItemResult> = Vec::new();
    let limit = if max_items == 0 { usize::MAX } else { max_items };

    loop {
        if results.len() >= limit {
            break;
        }

        let inbox_file = match find_first_inbox_item(&inbox) {
            Some(f) => f,
            None => break,
        };

        let item_num = results.len() + 1;
        println!("\n--- Item {} ---", item_num);

        let result = process_one_item(
            &inbox_file,
            &outbox,
            &logs,
            &processed,
            &failed,
            &projects_dir,
            &openclaw_cmd,
            &policy,
            dry_run,
        );
        results.push(result);
    }

    if results.is_empty() {
        println!("No inbox items.");
        return 0;
    }

    // Print summary
    let total = results.len();
    let enriched_count = results.iter().filter(|r| r.enriched).count();
    let unenriched_count = results.iter().filter(|r| !r.enriched && !r.failed).count();
    let failed_count = results.iter().filter(|r| r.failed).count();

    println!("\n--- Summary ---");
    println!(
        "Processed {} items ({} enriched, {} unenriched, {} failed).",
        total, enriched_count, unenriched_count, failed_count
    );

    // Check if more items remain
    if let Some(_) = find_first_inbox_item(&inbox) {
        println!("More items remaining. Run again to continue.");
    }

    if failed_count > 0 && failed_count == total {
        1
    } else {
        0
    }
}

fn process_one_item(
    inbox_file: &Path,
    outbox: &Path,
    logs: &Path,
    processed: &Path,
    failed: &Path,
    projects_dir: &Path,
    openclaw_cmd: &str,
    policy: &Option<PolicyConfig>,
    dry_run: bool,
) -> ItemResult {
    let original_name = inbox_file
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let stem = original_name.strip_suffix(".md").unwrap_or(&original_name);
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d_%H%M").to_string();
    let today = now.format("%Y-%m-%d").to_string();
    let report_path = outbox.join(format!("{}-{}-digest.md", timestamp, stem));
    let envelope_path = outbox.join(format!("{}-{}.envelope.json", timestamp, stem));

    // Read inbox content (first 200 lines)
    let task_content = match read_first_n_lines(inbox_file, 200) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cannot read inbox file: {}", e);
            move_file(inbox_file, &failed.join(&original_name));
            return ItemResult {
                source_file: original_name,
                project_name: None,
                action_type: "unknown".to_string(),
                exec_status: "failed".to_string(),
                enriched: false,
                failed: true,
            };
        }
    };

    // --- Project Classification (Level 1) ---
    let (project_kind, project_name, classification_method, classification_json) =
        classify_project(&task_content, projects_dir, openclaw_cmd, policy);
    println!(
        "Project routing: kind={} name={} method={}",
        project_kind,
        project_name.as_deref().unwrap_or("<none>"),
        classification_method
    );

    // Create new project directory if classified as "new"
    if project_kind == "new" {
        if let Some(ref name) = project_name {
            let new_proj = projects_dir.join(name);
            if !new_proj.exists() {
                fs::create_dir_all(new_proj.join("Inbox")).ok();
                let readme = format!(
                    "# {}\n\nCreated: {}\nSource: {}\n\n## Description\n\n(Auto-created by inbox orchestrator. Update this with project details.)\n",
                    name, today, original_name
                );
                fs::write(new_proj.join("README.md"), readme).ok();
                println!("Created new project: {}", new_proj.display());
            }
        }
    }

    // --- Action Type Classification (Level 2) ---
    let (action_type, action_type_method, action_type_json) =
        classify_action_type(&task_content, openclaw_cmd, policy);
    println!("Action type: {} method={}", action_type, action_type_method);

    // --- LLM Enrichment ---
    let (enriched, enrichment_rendered, enrichment_json) =
        enrich(&task_content, openclaw_cmd, policy);

    // --- Execution Handlers ---
    let (exec_result, exec_json, exec_file) = execute_handler(
        &action_type,
        &task_content,
        outbox,
        &timestamp,
        stem,
        openclaw_cmd,
        policy,
    );
    println!("Execution: {}", exec_result);

    // --- Build report ---
    let report_content = build_report(
        &task_content,
        &project_kind,
        project_name.as_deref(),
        &classification_method,
        &action_type,
        &action_type_method,
        &exec_result,
        exec_file.as_deref(),
        &enrichment_rendered,
        enriched,
        &enrichment_json,
    );

    // Write report atomically
    if let Err(e) = atomic_write(&report_path, report_content.as_bytes()) {
        eprintln!("Cannot write report: {}", e);
        let _ = append_log(logs, &today, &timestamp, &original_name, &report_path, "Failed/", "error");
        if !dry_run {
            move_file(inbox_file, &failed.join(&original_name));
        }
        return ItemResult {
            source_file: original_name,
            project_name: project_name.clone(),
            action_type,
            exec_status: "failed".to_string(),
            enriched: false,
            failed: true,
        };
    }

    let envelope_status = if enriched { "enriched" } else { "unenriched" };

    let envelope = Envelope {
        version: "1.0.0".to_string(),
        timestamp: timestamp.clone(),
        source_file: original_name.clone(),
        task_text: task_content.clone(),
        classification: classification_json,
        action_type: action_type_json,
        planning: serde_json::Value::Null,
        enrichment: enrichment_json,
        execution: exec_json,
        status: envelope_status.to_string(),
    };

    if let Err(e) = write_envelope(&envelope_path, &envelope) {
        eprintln!("Cannot write envelope: {}", e);
    }

    let _ = append_log(logs, &today, &timestamp, &original_name, &report_path, "Processed/", envelope_status);

    if !dry_run {
        move_file(inbox_file, &processed.join(&original_name));
    }

    println!("Digest written: {}", report_path.display());
    println!("Envelope written: {}", envelope_path.display());
    if dry_run {
        println!("Dry run — inbox item NOT moved.");
    } else {
        println!("Inbox item moved to: {}", processed.join(&original_name).display());
    }

    ItemResult {
        source_file: original_name,
        project_name,
        action_type,
        exec_status: exec_result,
        enriched,
        failed: false,
    }
}

// ---------------------------------------------------------------------------
// Policy loading
// ---------------------------------------------------------------------------

fn load_policy() -> Option<PolicyConfig> {
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

fn select_model(
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

// ---------------------------------------------------------------------------
// Inbox scanning
// ---------------------------------------------------------------------------

fn find_first_inbox_item(inbox: &Path) -> Option<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(inbox)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "md"))
        .collect();
    entries.sort();
    entries.into_iter().next()
}

fn read_first_n_lines(path: &Path, n: usize) -> Result<String, std::io::Error> {
    let content = fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().take(n).collect();
    Ok(lines.join("\n"))
}

// ---------------------------------------------------------------------------
// Project classification
// ---------------------------------------------------------------------------

fn classify_project(
    task_content: &str,
    projects_dir: &Path,
    openclaw_cmd: &str,
    policy: &Option<PolicyConfig>,
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
        let model = select_model(policy, "classification", task_content);
        let mut args = vec![
            "agent".to_string(),
            "--agent".to_string(),
            "main".to_string(),
            "--timeout".to_string(),
            "120".to_string(),
        ];
        if let Some(ref m) = model {
            args.push("--model".to_string());
            args.push(m.clone());
        }

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

// ---------------------------------------------------------------------------
// Action type classification
// ---------------------------------------------------------------------------

fn classify_action_type(
    task_content: &str,
    openclaw_cmd: &str,
    policy: &Option<PolicyConfig>,
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
        let model = select_model(policy, "action_type", task_content);
        let mut args = vec![
            "agent".to_string(),
            "--agent".to_string(),
            "main".to_string(),
            "--timeout".to_string(),
            "120".to_string(),
        ];
        if let Some(ref m) = model {
            args.push("--model".to_string());
            args.push(m.clone());
        }

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
                if let Ok(at) =
                    serde_json::from_value::<ActionTypeClassification>(parsed.clone())
                {
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

// ---------------------------------------------------------------------------
// Enrichment
// ---------------------------------------------------------------------------

fn enrich(
    task_content: &str,
    openclaw_cmd: &str,
    policy: &Option<PolicyConfig>,
) -> (bool, String, serde_json::Value) {
    let fallback = "## Planned Actions\n\
        - (LLM enrichment unavailable — manual review required)\n\n\
        ## Clarifying Questions\n\
        - None\n\n\
        ## Suggested Next Step\n\
        - Review inbox item manually and determine actions"
        .to_string();

    if !which_exists(openclaw_cmd) {
        println!("Enrichment unavailable or invalid, using fallback.");
        return (false, fallback, serde_json::Value::Null);
    }

    let model = select_model(policy, "enrichment", task_content);
    let mut args = vec![
        "agent".to_string(),
        "--agent".to_string(),
        "main".to_string(),
        "--timeout".to_string(),
        "120".to_string(),
    ];
    if let Some(ref m) = model {
        args.push("--model".to_string());
        args.push(m.clone());
    }

    let prompt = format!(
        "You are a strict JSON API. Given the task below, return ONLY a single JSON object. \
         No markdown fences, no prose, no explanation — just the raw JSON object.\n\n\
         Schema:\n\
         {{\"planned_actions\": [\"string\", ...], \"clarifying_questions\": [\"string\", ...], \
         \"next_step\": \"string\"}}\n\n\
         Rules:\n\
         - planned_actions: array of concrete action strings (at least one).\n\
         - clarifying_questions: array of question strings. Use [] if the task is clear.\n\
         - next_step: single string describing the immediate next action.\n\
         - Output MUST be valid JSON. Nothing else.\n\n\
         Task:\n{}",
        task_content
    );
    args.push("--message".to_string());
    args.push(prompt);

    println!("Calling OpenClaw for JSON enrichment...");
    if let Some(output) = call_openclaw(openclaw_cmd, &args) {
        if let Some(parsed) = extract_json(&output) {
            if let Ok(enrichment) =
                serde_json::from_value::<Enrichment>(parsed.clone())
            {
                let mut rendered = String::from("## Planned Actions\n");
                for action in &enrichment.planned_actions {
                    rendered.push_str(&format!("- {}\n", action));
                }
                rendered.push_str("\n## Clarifying Questions\n");
                if enrichment.clarifying_questions.is_empty() {
                    rendered.push_str("- None\n");
                } else {
                    for q in &enrichment.clarifying_questions {
                        rendered.push_str(&format!("- {}\n", q));
                    }
                }
                rendered.push_str(&format!(
                    "\n## Suggested Next Step\n- {}\n",
                    enrichment.next_step
                ));
                println!("Enrichment received and parsed as JSON.");
                return (true, rendered, parsed);
            }
        }
        println!("JSON parse failed. Using fallback.");
    }

    println!("Enrichment unavailable or invalid, using fallback.");
    (false, fallback, serde_json::Value::Null)
}

// ---------------------------------------------------------------------------
// Execution handlers
// ---------------------------------------------------------------------------

fn execute_handler(
    action_type: &str,
    task_content: &str,
    outbox: &Path,
    timestamp: &str,
    stem: &str,
    openclaw_cmd: &str,
    policy: &Option<PolicyConfig>,
) -> (String, serde_json::Value, Option<String>) {
    match action_type {
        "research" => {
            let exec_file =
                outbox.join(format!("{}-{}.research.md", timestamp, stem));
            println!("Executing research handler...");
            if which_exists(openclaw_cmd) {
                let model = select_model(policy, "research", task_content);
                let mut args = vec![
                    "agent".to_string(),
                    "--agent".to_string(),
                    "main".to_string(),
                    "--timeout".to_string(),
                    "120".to_string(),
                ];
                if let Some(ref m) = model {
                    args.push("--model".to_string());
                    args.push(m.clone());
                }
                let prompt = format!(
                    "You are a research assistant. Given the task below, produce a structured research report.\n\n\
                     Format your response as markdown with these exact sections:\n\
                     ## Summary\n(2-3 sentence overview)\n\n\
                     ## Findings\n(bulleted list of key findings)\n\n\
                     ## Sources\n(bulleted list — use placeholder URLs for now)\n\n\
                     ## Next Steps\n(bulleted list of recommended follow-up actions)\n\n\
                     Task:\n{}",
                    task_content
                );
                args.push("--message".to_string());
                args.push(prompt);

                if let Some(output) = call_openclaw(openclaw_cmd, &args) {
                    if !output.is_empty() {
                        let _ = fs::write(&exec_file, &output);
                        let fname = exec_file
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .to_string();
                        println!(
                            "Research report written: {}",
                            exec_file.display()
                        );
                        let json = serde_json::json!({"handler":"research","status":"completed","output_file":fname});
                        return ("completed".to_string(), json, Some(fname));
                    }
                }
                let json = serde_json::json!({"handler":"research","status":"failed","reason":"OpenClaw returned empty response"});
                ("failed".to_string(), json, None)
            } else {
                let json = serde_json::json!({"handler":"research","status":"skipped","reason":"OpenClaw not available"});
                ("skipped".to_string(), json, None)
            }
        }
        "question" => {
            let exec_file =
                outbox.join(format!("{}-{}.research.md", timestamp, stem));
            println!("Executing question handler...");
            if which_exists(openclaw_cmd) {
                let model = select_model(policy, "question", task_content);
                let mut args = vec![
                    "agent".to_string(),
                    "--agent".to_string(),
                    "main".to_string(),
                    "--timeout".to_string(),
                    "120".to_string(),
                ];
                if let Some(ref m) = model {
                    args.push("--model".to_string());
                    args.push(m.clone());
                }
                let prompt = format!(
                    "You are an expert assistant. Given the question below, produce a structured answer.\n\n\
                     Format your response as markdown with these exact sections:\n\
                     ## Answer\n(clear, direct answer to the question)\n\n\
                     ## Details\n(supporting explanation with bullet points)\n\n\
                     ## Sources\n(bulleted list — use placeholder URLs for now)\n\n\
                     ## Follow-up Questions\n(bulleted list of related questions worth exploring)\n\n\
                     Question:\n{}",
                    task_content
                );
                args.push("--message".to_string());
                args.push(prompt);

                if let Some(output) = call_openclaw(openclaw_cmd, &args) {
                    if !output.is_empty() {
                        let _ = fs::write(&exec_file, &output);
                        let fname = exec_file
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .to_string();
                        println!(
                            "Answer report written: {}",
                            exec_file.display()
                        );
                        let json = serde_json::json!({"handler":"question","status":"completed","output_file":fname});
                        return ("completed".to_string(), json, Some(fname));
                    }
                }
                let json = serde_json::json!({"handler":"question","status":"failed","reason":"OpenClaw returned empty response"});
                ("failed".to_string(), json, None)
            } else {
                let json = serde_json::json!({"handler":"question","status":"skipped","reason":"OpenClaw not available"});
                ("skipped".to_string(), json, None)
            }
        }
        "repo-change" => {
            println!("Execution blocked: repo-change requires approval");
            let json = serde_json::json!({"handler":"repo-change","status":"blocked","reason":"Execution blocked: requires approval"});
            ("blocked".to_string(), json, None)
        }
        "ops" => {
            println!("Execution blocked: ops requires approval");
            let json = serde_json::json!({"handler":"ops","status":"blocked","reason":"Execution blocked: requires approval"});
            ("blocked".to_string(), json, None)
        }
        _ => {
            let json = serde_json::json!({"handler":"note","status":"none","reason":"No execution required for notes"});
            ("none".to_string(), json, None)
        }
    }
}

// ---------------------------------------------------------------------------
// Report building
// ---------------------------------------------------------------------------

fn build_report(
    task_content: &str,
    project_kind: &str,
    project_name: Option<&str>,
    classification_method: &str,
    action_type: &str,
    action_type_method: &str,
    exec_result: &str,
    exec_file: Option<&str>,
    enrichment_rendered: &str,
    enriched: bool,
    enrichment_json: &serde_json::Value,
) -> String {
    let mut report = String::new();
    report.push_str(task_content);
    report.push_str("\n\n---\n\n");

    report.push_str("## Project Routing\n\n");
    report.push_str(&format!("- **Kind:** {}\n", project_kind));
    if let Some(name) = project_name {
        report.push_str(&format!("- **Project:** {}\n", name));
    }
    report.push_str(&format!("- **Method:** {}\n\n", classification_method));

    report.push_str("## Action Type\n\n");
    report.push_str(&format!("- **Type:** {}\n", action_type));
    report.push_str(&format!("- **Method:** {}\n\n", action_type_method));

    report.push_str("## Execution\n\n");
    report.push_str(&format!("- **Handler:** {}\n", action_type));
    report.push_str(&format!("- **Status:** {}\n", exec_result));
    if let Some(fname) = exec_file {
        report.push_str(&format!("- **Output:** {}\n", fname));
    }
    report.push('\n');

    report.push_str(enrichment_rendered);

    if enriched && !enrichment_json.is_null() {
        report.push_str(&format!(
            "\n## Enrichment (raw JSON)\n\n```json\n{}\n```\n",
            serde_json::to_string_pretty(enrichment_json).unwrap_or_default()
        ));
    }

    report
}

// ---------------------------------------------------------------------------
// IO helpers
// ---------------------------------------------------------------------------

fn call_openclaw(cmd: &str, args: &[String]) -> Option<String> {
    let output = Command::new(cmd).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if stdout.trim().is_empty() {
        return None;
    }
    Some(stdout)
}

fn extract_json(raw: &str) -> Option<serde_json::Value> {
    // Try parsing directly
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.trim()) {
        return Some(v);
    }
    // Strip markdown fences and try again
    let mut in_json = false;
    let mut json_lines = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if !in_json && trimmed.starts_with('{') {
            in_json = true;
        }
        if in_json {
            json_lines.push(line);
            if trimmed.ends_with('}') {
                let candidate = json_lines.join("\n");
                if let Ok(v) =
                    serde_json::from_str::<serde_json::Value>(&candidate)
                {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), std::io::Error> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".tmp-{}", std::process::id()));
    let mut file = fs::File::create(&tmp)?;
    file.write_all(data)?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn write_envelope(
    path: &Path,
    envelope: &Envelope,
) -> Result<(), std::io::Error> {
    let json = serde_json::to_string_pretty(envelope)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    atomic_write(path, json.as_bytes())
}

fn append_log(
    logs_dir: &Path,
    today: &str,
    timestamp: &str,
    source: &str,
    report: &Path,
    dest: &str,
    status: &str,
) -> Result<(), std::io::Error> {
    let logfile = logs_dir.join(format!("{}.md", today));
    let report_name =
        report.file_name().unwrap_or_default().to_string_lossy();
    let line = format!(
        "[{}] {} -> {} -> {} [{}]\n",
        timestamp, source, report_name, dest, status
    );
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logfile)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

fn move_file(from: &Path, to: &Path) {
    if let Err(e) = fs::rename(from, to) {
        eprintln!(
            "Failed to move {} -> {}: {}",
            from.display(),
            to.display(),
            e
        );
    }
}

fn which_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .is_ok_and(|o| o.status.success())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestVault {
        root: PathBuf,
    }

    impl TestVault {
        fn new() -> Self {
            let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
            let root = std::env::temp_dir().join(format!(
                "digest-test-{}-{}",
                std::process::id(),
                id
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("Inbox/Processed")).unwrap();
            fs::create_dir_all(root.join("Inbox/Failed")).unwrap();
            fs::create_dir_all(root.join("Outbox")).unwrap();
            fs::create_dir_all(root.join("Logs")).unwrap();
            fs::create_dir_all(root.join("Projects")).unwrap();
            TestVault { root }
        }

        fn place_inbox(&self, name: &str, content: &str) {
            fs::write(self.root.join("Inbox").join(name), content).unwrap();
        }

        fn create_project(&self, name: &str) {
            fs::create_dir_all(self.root.join("Projects").join(name))
                .unwrap();
        }
    }

    impl Drop for TestVault {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let outbox = self.root.join("Outbox");
                if outbox.exists() {
                    let _ = fs::set_permissions(
                        &outbox,
                        fs::Permissions::from_mode(0o755),
                    );
                }
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn mock_path() -> PathBuf {
        let manifest = env!("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest)
            .join("tests/helpers/mock-openclaw.sh")
    }

    fn run_with_vault(vault: &TestVault, dry_run: bool) -> i32 {
        // SAFETY: tests run with --test-threads=1 so no concurrent env mutation
        unsafe {
            std::env::set_var(
                "OPENCLAW_CMD",
                mock_path().to_str().unwrap(),
            );
            std::env::set_var(
                "DIGEST_POLICY",
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("config/policy.json")
                    .to_str()
                    .unwrap(),
            );
            std::env::remove_var("MOCK_OPENCLAW_FAIL");
            std::env::remove_var("MOCK_OPENCLAW_INVALID");
        }
        run(Some(vault.root.to_string_lossy().to_string()), dry_run, 10)
    }

    #[test]
    fn test_empty_inbox() {
        let vault = TestVault::new();
        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);
        assert_eq!(
            fs::read_dir(vault.root.join("Outbox")).unwrap().count(),
            0
        );
    }

    #[test]
    fn test_happy_path_enriched() {
        let vault = TestVault::new();
        vault.create_project("openclaw-daily-digest");
        vault.place_inbox(
            "test-task.md",
            "Project: openclaw-daily-digest\n\nFix the envelope writer.",
        );

        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);

        // Item moved to Processed
        assert!(!vault.root.join("Inbox/test-task.md").exists());
        assert!(vault.root.join("Inbox/Processed/test-task.md").exists());

        // Outbox has report and envelope
        let outbox_files: Vec<_> = fs::read_dir(vault.root.join("Outbox"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(outbox_files.len() >= 2);

        // Envelope is valid JSON with correct fields
        let envelope_file = outbox_files
            .iter()
            .find(|e| {
                e.path().extension().is_some_and(|ext| ext == "json")
            })
            .unwrap();
        let content =
            fs::read_to_string(envelope_file.path()).unwrap();
        let envelope: serde_json::Value =
            serde_json::from_str(&content).unwrap();
        assert_eq!(envelope["status"], "enriched");
        assert_eq!(
            envelope["classification"]["project"]["kind"],
            "existing"
        );

        // Log exists
        assert!(
            fs::read_dir(vault.root.join("Logs"))
                .unwrap()
                .count()
                > 0
        );
    }

    #[test]
    fn test_openclaw_failure_unenriched() {
        let vault = TestVault::new();
        // SAFETY: tests run with --test-threads=1 so no concurrent env mutation
        unsafe {
            std::env::set_var(
                "OPENCLAW_CMD",
                mock_path().to_str().unwrap(),
            );
            std::env::set_var("MOCK_OPENCLAW_FAIL", "1");
            std::env::set_var(
                "DIGEST_POLICY",
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("config/policy.json")
                    .to_str()
                    .unwrap(),
            );
        }
        vault.create_project("openclaw-daily-digest");
        vault.place_inbox(
            "fail-task.md",
            "Project: openclaw-daily-digest\n\nFix something.",
        );

        let code =
            run(Some(vault.root.to_string_lossy().to_string()), false, 10);
        assert_eq!(code, 0);
        assert!(
            vault.root.join("Inbox/Processed/fail-task.md").exists()
        );

        let envelope_file = fs::read_dir(vault.root.join("Outbox"))
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| {
                e.path().extension().is_some_and(|ext| ext == "json")
            })
            .unwrap();
        let content =
            fs::read_to_string(envelope_file.path()).unwrap();
        let envelope: serde_json::Value =
            serde_json::from_str(&content).unwrap();
        assert_eq!(envelope["status"], "unenriched");
    }

    #[test]
    fn test_dry_run_no_move() {
        let vault = TestVault::new();
        vault.place_inbox(
            "dry-task.md",
            "Just a note about groceries.",
        );

        let code = run_with_vault(&vault, true);
        assert_eq!(code, 0);
        assert!(vault.root.join("Inbox/dry-task.md").exists());
        assert!(
            !vault.root.join("Inbox/Processed/dry-task.md").exists()
        );
    }

    #[test]
    fn test_io_failure_moves_to_failed() {
        let vault = TestVault::new();
        vault.place_inbox("io-task.md", "Some task.");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                vault.root.join("Outbox"),
                fs::Permissions::from_mode(0o000),
            )
            .unwrap();
        }

        let code = run_with_vault(&vault, false);
        assert_ne!(code, 0);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                vault.root.join("Outbox"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }

        assert!(vault.root.join("Inbox/Failed/io-task.md").exists());
    }

    #[test]
    fn test_multiple_items_processed() {
        let vault = TestVault::new();
        vault.place_inbox("a-task.md", "Buy groceries.");
        vault.place_inbox("b-task.md", "Read a book.");
        vault.place_inbox("c-task.md", "Water the plants.");

        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);

        // All three should be in Processed
        assert!(vault.root.join("Inbox/Processed/a-task.md").exists());
        assert!(vault.root.join("Inbox/Processed/b-task.md").exists());
        assert!(vault.root.join("Inbox/Processed/c-task.md").exists());

        // No items left in Inbox
        let remaining: Vec<_> = fs::read_dir(vault.root.join("Inbox"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
            .collect();
        assert_eq!(remaining.len(), 0);

        // Outbox should have 3 reports + 3 envelopes = 6 files
        let outbox_count = fs::read_dir(vault.root.join("Outbox"))
            .unwrap()
            .count();
        assert!(outbox_count >= 6);
    }

    #[test]
    fn test_max_items_limit() {
        let vault = TestVault::new();
        vault.place_inbox("a-task.md", "Task A.");
        vault.place_inbox("b-task.md", "Task B.");
        vault.place_inbox("c-task.md", "Task C.");

        // SAFETY: tests run with --test-threads=1
        unsafe {
            std::env::set_var("OPENCLAW_CMD", mock_path().to_str().unwrap());
            std::env::set_var(
                "DIGEST_POLICY",
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("config/policy.json")
                    .to_str()
                    .unwrap(),
            );
            std::env::remove_var("MOCK_OPENCLAW_FAIL");
            std::env::remove_var("MOCK_OPENCLAW_INVALID");
        }

        let code = run(Some(vault.root.to_string_lossy().to_string()), false, 2);
        assert_eq!(code, 0);

        // Only 2 processed, 1 remaining
        let processed_count = fs::read_dir(vault.root.join("Inbox/Processed"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
            .count();
        assert_eq!(processed_count, 2);

        let remaining_count = fs::read_dir(vault.root.join("Inbox"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
            .count();
        assert_eq!(remaining_count, 1);
    }
}
