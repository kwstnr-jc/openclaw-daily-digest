mod classify;
mod discord;
mod enrich;
mod execute;
mod git;
mod report;
mod types;
mod util;

use chrono::Local;
use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};

use crate::classify::{classify_action_type, classify_project};
use crate::discord::{format_discord_message, post_to_discord};
use crate::enrich::enrich;
use crate::execute::execute_handler;
use crate::report::build_report;
use crate::types::{Envelope, ItemResult};
use crate::util::{
    append_log, atomic_write, find_first_inbox_item, move_file, read_first_n_lines, rotate_logs,
    write_envelope,
};

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

        /// Skip posting to Discord
        #[arg(long)]
        no_discord: bool,
    },
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
            no_discord,
        } => {
            let exit_code = run(root, dry_run, max_items, no_discord);
            std::process::exit(exit_code);
        }
    }
}

fn run(root_override: Option<String>, dry_run: bool, max_items: usize, no_discord: bool) -> i32 {
    let root = PathBuf::from(
        root_override.unwrap_or_else(|| "/Users/Shared/agent-vault/Agent".to_string()),
    );
    let inbox = root.join("Inbox");
    let outbox = root.join("Outbox");
    let logs = root.join("Logs");
    let processed = inbox.join("Processed");
    let failed = inbox.join("Failed");
    let projects_dir = root.join("Projects");
    let envelope_dir = PathBuf::from(std::env::var("DIGEST_ENVELOPE_DIR").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/agent".to_string());
        format!("{}/data/digest-envelopes", home)
    }));

    // Ensure directories exist
    for dir in [&outbox, &logs, &processed, &failed, &envelope_dir] {
        fs::create_dir_all(dir).ok();
    }

    let retention_days: u64 = std::env::var("DIGEST_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    rotate_logs(&logs, retention_days);

    let openclaw_cmd = std::env::var("OPENCLAW_CMD").unwrap_or_else(|_| "openclaw".to_string());

    // Process items in a loop
    let mut results: Vec<ItemResult> = Vec::new();
    let limit = if max_items == 0 {
        usize::MAX
    } else {
        max_items
    };

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
            &envelope_dir,
            &logs,
            &processed,
            &failed,
            &projects_dir,
            &openclaw_cmd,
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

    // Post to Discord
    if !no_discord && !results.is_empty() {
        let message = format_discord_message(&results);
        match post_to_discord(&message) {
            Ok(()) => println!("Discord summary posted."),
            Err(e) => eprintln!("Discord post failed (non-fatal): {}", e),
        }
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
    envelope_dir: &Path,
    logs: &Path,
    processed: &Path,
    failed: &Path,
    projects_dir: &Path,
    openclaw_cmd: &str,
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
    let envelope_path = envelope_dir.join(format!("{}-{}.envelope.json", timestamp, stem));

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
                pr_url: None,
            };
        }
    };

    // --- Project Classification (Level 1) ---
    let (project_kind, project_name, classification_method, classification_json) =
        classify_project(&task_content, projects_dir, openclaw_cmd);
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
        classify_action_type(&task_content, openclaw_cmd);
    println!("Action type: {} method={}", action_type, action_type_method);

    // --- LLM Enrichment ---
    let (enriched, enrichment_rendered, enrichment_json) = enrich(&task_content, openclaw_cmd);

    // --- Execution Handlers ---
    let (exec_result, exec_json, exec_file, pr_url) = execute_handler(
        &action_type,
        &task_content,
        outbox,
        &timestamp,
        stem,
        openclaw_cmd,
        projects_dir,
        project_name.as_deref(),
        &project_kind,
        &enrichment_rendered,
    );
    println!("Execution: {}", exec_result);

    // --- Build report ---
    let report_content = build_report(
        &task_content,
        stem,
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
        let _ = append_log(
            logs,
            &today,
            &timestamp,
            &original_name,
            &report_path,
            "Failed/",
            "error",
        );
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
            pr_url: None,
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

    let _ = append_log(
        logs,
        &today,
        &timestamp,
        &original_name,
        &report_path,
        "Processed/",
        envelope_status,
    );

    if !dry_run {
        move_file(inbox_file, &processed.join(&original_name));
    }

    println!("Digest written: {}", report_path.display());
    println!("Envelope written: {}", envelope_path.display());
    if dry_run {
        println!("Dry run — inbox item NOT moved.");
    } else {
        println!(
            "Inbox item moved to: {}",
            processed.join(&original_name).display()
        );
    }

    ItemResult {
        source_file: original_name,
        project_name,
        action_type,
        exec_status: exec_result,
        enriched,
        failed: false,
        pr_url,
    }
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
            let root =
                std::env::temp_dir().join(format!("digest-test-{}-{}", std::process::id(), id));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("Inbox/Processed")).unwrap();
            fs::create_dir_all(root.join("Inbox/Failed")).unwrap();
            fs::create_dir_all(root.join("Outbox")).unwrap();
            fs::create_dir_all(root.join("Envelopes")).unwrap();
            fs::create_dir_all(root.join("Logs")).unwrap();
            fs::create_dir_all(root.join("Projects")).unwrap();
            TestVault { root }
        }

        fn place_inbox(&self, name: &str, content: &str) {
            fs::write(self.root.join("Inbox").join(name), content).unwrap();
        }

        fn create_project(&self, name: &str) {
            fs::create_dir_all(self.root.join("Projects").join(name)).unwrap();
        }
    }

    impl Drop for TestVault {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let outbox = self.root.join("Outbox");
                if outbox.exists() {
                    let _ = fs::set_permissions(&outbox, fs::Permissions::from_mode(0o755));
                }
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn mock_path() -> PathBuf {
        let manifest = env!("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest).join("tests/helpers/mock-openclaw.sh")
    }

    fn run_with_vault(vault: &TestVault, dry_run: bool) -> i32 {
        // SAFETY: tests run with --test-threads=1 so no concurrent env mutation
        unsafe {
            std::env::set_var("OPENCLAW_CMD", mock_path().to_str().unwrap());
            std::env::set_var(
                "DIGEST_ENVELOPE_DIR",
                vault.root.join("Envelopes").to_str().unwrap(),
            );
            std::env::remove_var("MOCK_OPENCLAW_FAIL");
            std::env::remove_var("MOCK_OPENCLAW_INVALID");
            std::env::remove_var("MOCK_OPENCLAW_LOG");
        }
        run(
            Some(vault.root.to_string_lossy().to_string()),
            dry_run,
            10,
            true,
        )
    }

    fn find_envelope(vault: &TestVault) -> serde_json::Value {
        let envelope_file = fs::read_dir(vault.root.join("Envelopes"))
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .expect("No envelope.json found in Envelopes/");
        serde_json::from_str(&fs::read_to_string(envelope_file.path()).unwrap()).unwrap()
    }

    #[test]
    fn test_empty_inbox() {
        let vault = TestVault::new();
        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);
        assert_eq!(fs::read_dir(vault.root.join("Outbox")).unwrap().count(), 0);
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

        // Outbox has report
        let outbox_files: Vec<_> = fs::read_dir(vault.root.join("Outbox"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!outbox_files.is_empty());

        // Envelope is in Envelopes dir with correct fields
        let envelope = find_envelope(&vault);
        assert_eq!(envelope["status"], "enriched");
        assert_eq!(envelope["classification"]["project"]["kind"], "existing");

        // Log exists
        assert!(fs::read_dir(vault.root.join("Logs")).unwrap().count() > 0);
    }

    #[test]
    fn test_openclaw_failure_unenriched() {
        let vault = TestVault::new();
        // SAFETY: tests run with --test-threads=1 so no concurrent env mutation
        unsafe {
            std::env::set_var("OPENCLAW_CMD", mock_path().to_str().unwrap());
            std::env::set_var("MOCK_OPENCLAW_FAIL", "1");
            std::env::set_var(
                "DIGEST_ENVELOPE_DIR",
                vault.root.join("Envelopes").to_str().unwrap(),
            );
        }
        vault.create_project("openclaw-daily-digest");
        vault.place_inbox(
            "fail-task.md",
            "Project: openclaw-daily-digest\n\nFix something.",
        );

        let code = run(
            Some(vault.root.to_string_lossy().to_string()),
            false,
            10,
            true,
        );
        assert_eq!(code, 0);
        assert!(vault.root.join("Inbox/Processed/fail-task.md").exists());

        let envelope = find_envelope(&vault);
        assert_eq!(envelope["status"], "unenriched");
    }

    #[test]
    fn test_dry_run_no_move() {
        let vault = TestVault::new();
        vault.place_inbox("dry-task.md", "Just a note about groceries.");

        let code = run_with_vault(&vault, true);
        assert_eq!(code, 0);
        assert!(vault.root.join("Inbox/dry-task.md").exists());
        assert!(!vault.root.join("Inbox/Processed/dry-task.md").exists());
    }

    #[test]
    fn test_io_failure_moves_to_failed() {
        let vault = TestVault::new();
        vault.place_inbox("io-task.md", "Some task.");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(vault.root.join("Outbox"), fs::Permissions::from_mode(0o000))
                .unwrap();
        }

        let code = run_with_vault(&vault, false);
        assert_ne!(code, 0);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(vault.root.join("Outbox"), fs::Permissions::from_mode(0o755))
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

        // Outbox should have 3 digest reports
        let outbox_count = fs::read_dir(vault.root.join("Outbox")).unwrap().count();
        assert_eq!(outbox_count, 3);

        // Envelopes dir should have 3 envelope files
        let envelope_count = fs::read_dir(vault.root.join("Envelopes")).unwrap().count();
        assert_eq!(envelope_count, 3);
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
                "DIGEST_ENVELOPE_DIR",
                vault.root.join("Envelopes").to_str().unwrap(),
            );
            std::env::remove_var("MOCK_OPENCLAW_FAIL");
            std::env::remove_var("MOCK_OPENCLAW_INVALID");
        }

        let code = run(
            Some(vault.root.to_string_lossy().to_string()),
            false,
            2,
            true,
        );
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

    #[test]
    fn test_discord_message_formatting() {
        let results = vec![
            ItemResult {
                source_file: "fix-bug.md".to_string(),
                project_name: Some("my-project".to_string()),
                action_type: "repo-change".to_string(),
                exec_status: "completed".to_string(),
                enriched: true,
                failed: false,
                pr_url: Some("https://github.com/org/repo/pull/42".to_string()),
            },
            ItemResult {
                source_file: "research-ai.md".to_string(),
                project_name: None,
                action_type: "research".to_string(),
                exec_status: "completed".to_string(),
                enriched: true,
                failed: false,
                pr_url: None,
            },
            ItemResult {
                source_file: "grocery-list.md".to_string(),
                project_name: None,
                action_type: "note".to_string(),
                exec_status: "none".to_string(),
                enriched: false,
                failed: false,
                pr_url: None,
            },
        ];

        let msg = format_discord_message(&results);

        // Header
        assert!(msg.contains("**Daily Digest** \u{2014}"));
        assert!(msg.contains("Processed 3 items:"));
        // Per-item lines use markdown dash, not bullet
        assert!(msg.contains("- `fix-bug.md` \u{2192} **my-project** (repo-change) \u{2014} PR opened: <https://github.com/org/repo/pull/42>"));
        assert!(msg.contains("- `research-ai.md` \u{2192} **none** (research) \u{2014} completed"));
        assert!(msg.contains("- `grocery-list.md` \u{2192} **none** (note) \u{2014} filed"));
        // Summary
        assert!(msg.contains("Enriched: 2 | Unenriched: 1 | Failed: 0"));
    }

    #[test]
    fn test_discord_post_graceful_failure_missing_token() {
        // SAFETY: tests run with --test-threads=1
        unsafe {
            std::env::set_var("DISCORD_TOKEN_FILE", "/nonexistent/token");
        }
        let result = post_to_discord("test message");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Cannot read token file"));
        unsafe {
            std::env::remove_var("DISCORD_TOKEN_FILE");
        }
    }

    #[test]
    fn test_repo_change_skipped_no_git_repo() {
        let vault = TestVault::new();
        // Create project dir without .git
        vault.create_project("my-project");
        vault.place_inbox(
            "fix-thing.md",
            "Project: my-project\n\nFix the broken thing.",
        );

        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);
        assert!(vault.root.join("Inbox/Processed/fix-thing.md").exists());

        let envelope = find_envelope(&vault);
        // repo-change should be skipped because no .git dir exists
        assert_eq!(envelope["execution"]["status"], "skipped");
        assert_eq!(envelope["execution"]["handler"], "repo-change");
    }

    #[test]
    fn test_ops_executed_with_output() {
        let vault = TestVault::new();
        vault.place_inbox("install-thing.md", "Install htop via brew.\nlaunchctl list");

        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);
        assert!(vault.root.join("Inbox/Processed/install-thing.md").exists());

        // Should have an ops-log file in outbox
        let ops_log: Vec<_> = fs::read_dir(vault.root.join("Outbox"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().to_string_lossy().contains(".ops-log.md"))
            .collect();
        assert_eq!(ops_log.len(), 1);

        let envelope = find_envelope(&vault);
        assert_eq!(envelope["execution"]["handler"], "ops");
        assert_eq!(envelope["execution"]["status"], "completed");
    }

    #[test]
    fn test_ops_skipped_dangerous_task() {
        let vault = TestVault::new();
        vault.place_inbox(
            "dangerous-task.md",
            "Install a cleanup tool. Then run rm -rf / to clean up disk space.",
        );

        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);
        assert!(
            vault
                .root
                .join("Inbox/Processed/dangerous-task.md")
                .exists()
        );

        let envelope = find_envelope(&vault);
        assert_eq!(envelope["execution"]["status"], "skipped");
    }

    #[test]
    fn test_new_project_creates_dir_and_readme() {
        let vault = TestVault::new();
        vault.place_inbox(
            "new-proj.md",
            "Project: home-automation\n\nResearch HomeKit on a Pi. Compare the options.",
        );
        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);
        let proj = vault.root.join("Projects/home-automation");
        assert!(proj.exists());
        assert!(proj.join("README.md").exists());
        assert!(proj.join("Inbox").exists());
        let envelope = find_envelope(&vault);
        assert_eq!(envelope["classification"]["project"]["kind"], "new");
        assert_eq!(
            envelope["classification"]["project"]["name"],
            "home-automation"
        );
    }

    #[test]
    fn test_research_handler_produces_output() {
        let vault = TestVault::new();
        vault.place_inbox(
            "research.md",
            "Compare HomeAssistant vs Homebridge for HomeKit.",
        );
        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);
        let research: Vec<_> = fs::read_dir(vault.root.join("Outbox"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().to_string_lossy().contains(".research.md"))
            .collect();
        assert_eq!(research.len(), 1);
        let content = fs::read_to_string(research[0].path()).unwrap();
        assert!(content.contains("## Summary"));
    }

    #[test]
    fn test_question_handler_produces_answer() {
        let vault = TestVault::new();
        vault.place_inbox("q.md", "What is the difference between launchd and cron?");
        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);
        let answers: Vec<_> = fs::read_dir(vault.root.join("Outbox"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().to_string_lossy().contains(".research.md"))
            .collect();
        assert_eq!(answers.len(), 1);
        let content = fs::read_to_string(answers[0].path()).unwrap();
        assert!(content.contains("## Answer"));
        let envelope = find_envelope(&vault);
        assert_eq!(envelope["action_type"]["action_type"], "question");
    }

    #[test]
    fn test_tag_routing_existing_project() {
        let vault = TestVault::new();
        vault.create_project("openclaw-daily-digest");
        vault.place_inbox(
            "tag.md",
            "#project/openclaw-daily-digest\n\nUpdate the README.",
        );
        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);
        let envelope = find_envelope(&vault);
        assert_eq!(envelope["classification"]["project"]["kind"], "existing");
        assert_eq!(
            envelope["classification"]["project"]["name"],
            "openclaw-daily-digest"
        );
    }

    #[test]
    fn test_unclassified_note_uses_ai_fallback() {
        let vault = TestVault::new();
        vault.place_inbox("groceries.md", "Pick up milk, eggs, bread, coffee beans.");
        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);
        let envelope = find_envelope(&vault);
        assert_eq!(envelope["action_type"]["action_type"], "note");
        assert_eq!(envelope["execution"]["status"], "none");
    }

    #[test]
    fn test_openclaw_invalid_json_unenriched() {
        let vault = TestVault::new();
        unsafe {
            std::env::set_var("OPENCLAW_CMD", mock_path().to_str().unwrap());
            std::env::set_var("MOCK_OPENCLAW_INVALID", "1");
            std::env::set_var(
                "DIGEST_ENVELOPE_DIR",
                vault.root.join("Envelopes").to_str().unwrap(),
            );
        }
        vault.place_inbox("invalid.md", "Some random note.");
        let code = run(
            Some(vault.root.to_string_lossy().to_string()),
            false,
            10,
            true,
        );
        assert_eq!(code, 0);
        let envelope = find_envelope(&vault);
        assert_eq!(envelope["status"], "unenriched");
    }

    #[test]
    fn test_log_entry_format() {
        let vault = TestVault::new();
        vault.create_project("openclaw-daily-digest");
        vault.place_inbox(
            "log-test.md",
            "Project: openclaw-daily-digest\n\nFix something.",
        );
        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);
        let logs: Vec<_> = fs::read_dir(vault.root.join("Logs"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!logs.is_empty());
        let content = fs::read_to_string(logs[0].path()).unwrap();
        assert!(content.contains("log-test.md"));
        assert!(content.contains("Processed/"));
        assert!(content.contains("enriched"));
    }

    #[test]
    fn test_log_rotation_deletes_old_logs() {
        use crate::util::rotate_logs;
        let dir = std::env::temp_dir().join(format!("digest-rotation-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let recent = (chrono::Local::now() - chrono::Duration::days(5))
            .format("%Y-%m-%d")
            .to_string();
        let old = (chrono::Local::now() - chrono::Duration::days(60))
            .format("%Y-%m-%d")
            .to_string();

        fs::write(dir.join(format!("{}.md", today)), "today").unwrap();
        fs::write(dir.join(format!("{}.md", recent)), "recent").unwrap();
        fs::write(dir.join(format!("{}.md", old)), "old").unwrap();
        // Non-.md file should be ignored
        fs::write(dir.join("digest-stderr.log"), "legacy").unwrap();

        rotate_logs(&dir, 30);

        assert!(dir.join(format!("{}.md", today)).exists());
        assert!(dir.join(format!("{}.md", recent)).exists());
        assert!(!dir.join(format!("{}.md", old)).exists());
        assert!(dir.join("digest-stderr.log").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
