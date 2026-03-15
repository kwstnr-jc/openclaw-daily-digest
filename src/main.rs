mod api;
mod classify;
mod discord;
mod enrich;
#[allow(
    dead_code,
    clippy::too_many_arguments,
    clippy::collapsible_if,
    clippy::collapsible_str_replace
)]
mod execute;
#[allow(dead_code)]
mod git;
mod report;
mod types;
mod util;

use chrono::Local;
use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};

use crate::api::{ApiConfig, map_task_type};
use crate::classify::{classify_action_type, classify_project};
use crate::discord::{format_discord_message, post_to_discord};
use crate::enrich::enrich;
use crate::report::build_report;
use crate::types::ItemResult;
use crate::util::{
    atomic_write, find_first_inbox_item, move_file, read_first_n_lines, rotate_logs,
};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "openclaw-daily-digest")]
enum Cli {
    /// Process inbox items
    Run {
        /// Inbox directory (source items)
        #[arg(long, env = "DIGEST_INBOX_DIR")]
        inbox: String,

        /// Outbox directory (digest reports)
        #[arg(long, env = "DIGEST_OUTBOX_DIR")]
        outbox: String,

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
            inbox,
            outbox,
            dry_run,
            max_items,
            no_discord,
        } => {
            let exit_code = run(&inbox, &outbox, dry_run, max_items, no_discord);
            std::process::exit(exit_code);
        }
    }
}

fn run(
    inbox_dir: &str,
    outbox_dir: &str,
    dry_run: bool,
    max_items: usize,
    no_discord: bool,
) -> i32 {
    let inbox = PathBuf::from(inbox_dir);
    let outbox = PathBuf::from(outbox_dir);
    let logs = outbox.join("Logs");
    let processed = inbox.join("Processed");
    let failed = inbox.join("Failed");

    // Ensure directories exist
    for dir in [&outbox, &logs, &processed, &failed] {
        fs::create_dir_all(dir).ok();
    }

    let retention_days: u64 = std::env::var("DIGEST_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    rotate_logs(&logs, retention_days);

    let llm_cmd = std::env::var("LLM_CMD").unwrap_or_else(|_| "claude".to_string());

    let api_config = ApiConfig::from_env();
    if api_config.is_none() {
        println!("API_URL not set — tasks will not be pushed to the API.");
    }

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
            &processed,
            &failed,
            &llm_cmd,
            api_config.as_ref(),
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
    if find_first_inbox_item(&inbox).is_some() {
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
    processed: &Path,
    failed: &Path,
    llm_cmd: &str,
    api_config: Option<&ApiConfig>,
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
    let report_path = outbox.join(format!("{}-{}-digest.md", timestamp, stem));

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

    // Use a temporary empty dir for project classification (no local projects dir)
    let tmp_projects = std::env::temp_dir().join("digest-empty-projects");
    fs::create_dir_all(&tmp_projects).ok();

    // --- Project Classification (Level 1) ---
    let (project_kind, project_name, classification_method, _classification_json) =
        classify_project(&task_content, &tmp_projects, llm_cmd);
    println!(
        "Project routing: kind={} name={} method={}",
        project_kind,
        project_name.as_deref().unwrap_or("<none>"),
        classification_method
    );

    // --- Action Type Classification (Level 2) ---
    let (action_type, action_type_method, _action_type_json) =
        classify_action_type(&task_content, llm_cmd);
    println!("Action type: {} method={}", action_type, action_type_method);

    // --- LLM Enrichment ---
    let (enriched, enrichment_rendered, enrichment_json) = enrich(&task_content, llm_cmd);

    // --- Push to API ---
    let mut api_pushed = false;
    if let Some(config) = api_config {
        // Ensure project exists if one was classified
        let project_id = if let Some(ref name) = project_name {
            match config.ensure_project(name) {
                Ok(project) => {
                    println!("Project '{}' ready (ID: {})", name, project.id);
                    Some(project.id)
                }
                Err(e) => {
                    eprintln!("Failed to ensure project '{}': {}", name, e);
                    None
                }
            }
        } else {
            None
        };

        // Create task
        let title = task_content
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or(stem)
            .trim()
            .to_string();
        let task_type = map_task_type(&action_type);

        match config.push_task(&title, &task_content, task_type, project_id.as_deref()) {
            Ok(task) => {
                println!("Task pushed to API (ID: {})", task.id);
                api_pushed = true;
            }
            Err(e) => {
                eprintln!("Failed to push task to API: {}", e);
            }
        }
    }

    // --- Build report ---
    let report_content = build_report(
        &task_content,
        stem,
        &project_kind,
        project_name.as_deref(),
        &classification_method,
        &action_type,
        &action_type_method,
        &enrichment_rendered,
        enriched,
        &enrichment_json,
        api_pushed,
    );

    // Write report atomically
    if let Err(e) = atomic_write(&report_path, report_content.as_bytes()) {
        eprintln!("Cannot write report: {}", e);
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

    if !dry_run {
        move_file(inbox_file, &processed.join(&original_name));
    }

    println!("Digest written: {}", report_path.display());
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
        exec_status: if api_pushed {
            "pushed".to_string()
        } else {
            "filed".to_string()
        },
        enriched,
        failed: false,
        pr_url: None,
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
        inbox: PathBuf,
        outbox: PathBuf,
    }

    impl TestVault {
        fn new() -> Self {
            let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
            let root =
                std::env::temp_dir().join(format!("digest-test-{}-{}", std::process::id(), id));
            let _ = fs::remove_dir_all(&root);
            let inbox = root.join("Inbox");
            let outbox = root.join("Outbox");
            fs::create_dir_all(inbox.join("Processed")).unwrap();
            fs::create_dir_all(inbox.join("Failed")).unwrap();
            fs::create_dir_all(&outbox).unwrap();
            TestVault { inbox, outbox }
        }

        fn place_inbox(&self, name: &str, content: &str) {
            fs::write(self.inbox.join(name), content).unwrap();
        }
    }

    impl Drop for TestVault {
        fn drop(&mut self) {
            // Clean up parent dir (root)
            if let Some(root) = self.inbox.parent() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if self.outbox.exists() {
                        let _ =
                            fs::set_permissions(&self.outbox, fs::Permissions::from_mode(0o755));
                    }
                }
                let _ = fs::remove_dir_all(root);
            }
        }
    }

    fn mock_path() -> PathBuf {
        let manifest = env!("CARGO_MANIFEST_DIR");
        PathBuf::from(manifest).join("tests/helpers/mock-openclaw.sh")
    }

    fn run_with_vault(vault: &TestVault, dry_run: bool) -> i32 {
        // SAFETY: tests run with --test-threads=1 so no concurrent env mutation
        unsafe {
            std::env::set_var("LLM_CMD", mock_path().to_str().unwrap());
            std::env::remove_var("API_URL");
            std::env::remove_var("API_KEY");
            std::env::remove_var("MOCK_OPENCLAW_FAIL");
            std::env::remove_var("MOCK_OPENCLAW_INVALID");
            std::env::remove_var("MOCK_OPENCLAW_LOG");
        }
        run(
            vault.inbox.to_str().unwrap(),
            vault.outbox.to_str().unwrap(),
            dry_run,
            10,
            true,
        )
    }

    #[test]
    fn test_empty_inbox() {
        let vault = TestVault::new();
        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);
        assert_eq!(
            fs::read_dir(&vault.outbox)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .count(),
            0
        );
    }

    #[test]
    fn test_happy_path_enriched() {
        let vault = TestVault::new();
        vault.place_inbox(
            "test-task.md",
            "Project: some-project\n\nFix the envelope writer.",
        );

        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);

        // Item moved to Processed
        assert!(!vault.inbox.join("test-task.md").exists());
        assert!(vault.inbox.join("Processed/test-task.md").exists());

        // Outbox has report
        let outbox_files: Vec<_> = fs::read_dir(&vault.outbox)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && e.path().to_string_lossy().contains("digest"))
            .collect();
        assert!(!outbox_files.is_empty());

        // Report contains enrichment
        let report = fs::read_to_string(outbox_files[0].path()).unwrap();
        assert!(report.contains("## Planned Actions"));
        assert!(report.contains("Mock action 1"));
    }

    #[test]
    fn test_llm_failure_unenriched() {
        let vault = TestVault::new();
        // SAFETY: tests run with --test-threads=1 so no concurrent env mutation
        unsafe {
            std::env::set_var("LLM_CMD", mock_path().to_str().unwrap());
            std::env::set_var("MOCK_OPENCLAW_FAIL", "1");
            std::env::remove_var("API_URL");
        }
        vault.place_inbox("fail-task.md", "Project: some-project\n\nFix something.");

        let code = run(
            vault.inbox.to_str().unwrap(),
            vault.outbox.to_str().unwrap(),
            false,
            10,
            true,
        );
        assert_eq!(code, 0);
        assert!(vault.inbox.join("Processed/fail-task.md").exists());

        // Report should exist but with fallback enrichment
        let outbox_files: Vec<_> = fs::read_dir(&vault.outbox)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && e.path().to_string_lossy().contains("digest"))
            .collect();
        assert!(!outbox_files.is_empty());
        let report = fs::read_to_string(outbox_files[0].path()).unwrap();
        assert!(report.contains("manual review required"));
    }

    #[test]
    fn test_dry_run_no_move() {
        let vault = TestVault::new();
        vault.place_inbox("dry-task.md", "Just a note about groceries.");

        let code = run_with_vault(&vault, true);
        assert_eq!(code, 0);
        assert!(vault.inbox.join("dry-task.md").exists());
        assert!(!vault.inbox.join("Processed/dry-task.md").exists());
    }

    #[test]
    fn test_io_failure_moves_to_failed() {
        let vault = TestVault::new();
        vault.place_inbox("io-task.md", "Some task.");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&vault.outbox, fs::Permissions::from_mode(0o000)).unwrap();
        }

        let code = run_with_vault(&vault, false);
        assert_ne!(code, 0);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&vault.outbox, fs::Permissions::from_mode(0o755)).unwrap();
        }

        assert!(vault.inbox.join("Failed/io-task.md").exists());
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
        assert!(vault.inbox.join("Processed/a-task.md").exists());
        assert!(vault.inbox.join("Processed/b-task.md").exists());
        assert!(vault.inbox.join("Processed/c-task.md").exists());

        // No items left in Inbox
        let remaining: Vec<_> = fs::read_dir(&vault.inbox)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
            .collect();
        assert_eq!(remaining.len(), 0);

        // Outbox should have 3 digest reports
        let outbox_count = fs::read_dir(&vault.outbox)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && e.path().to_string_lossy().contains("digest"))
            .count();
        assert_eq!(outbox_count, 3);
    }

    #[test]
    fn test_max_items_limit() {
        let vault = TestVault::new();
        vault.place_inbox("a-task.md", "Task A.");
        vault.place_inbox("b-task.md", "Task B.");
        vault.place_inbox("c-task.md", "Task C.");

        // SAFETY: tests run with --test-threads=1
        unsafe {
            std::env::set_var("LLM_CMD", mock_path().to_str().unwrap());
            std::env::remove_var("API_URL");
            std::env::remove_var("MOCK_OPENCLAW_FAIL");
            std::env::remove_var("MOCK_OPENCLAW_INVALID");
        }

        let code = run(
            vault.inbox.to_str().unwrap(),
            vault.outbox.to_str().unwrap(),
            false,
            2,
            true,
        );
        assert_eq!(code, 0);

        // Only 2 processed, 1 remaining
        let processed_count = fs::read_dir(vault.inbox.join("Processed"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
            .count();
        assert_eq!(processed_count, 2);

        let remaining_count = fs::read_dir(&vault.inbox)
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
    fn test_llm_invalid_json_unenriched() {
        let vault = TestVault::new();
        unsafe {
            std::env::set_var("LLM_CMD", mock_path().to_str().unwrap());
            std::env::set_var("MOCK_OPENCLAW_INVALID", "1");
            std::env::remove_var("API_URL");
        }
        vault.place_inbox("invalid.md", "Some random note.");
        let code = run(
            vault.inbox.to_str().unwrap(),
            vault.outbox.to_str().unwrap(),
            false,
            10,
            true,
        );
        assert_eq!(code, 0);
        // Report should have fallback enrichment
        let outbox_files: Vec<_> = fs::read_dir(&vault.outbox)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && e.path().to_string_lossy().contains("digest"))
            .collect();
        assert!(!outbox_files.is_empty());
        let report = fs::read_to_string(outbox_files[0].path()).unwrap();
        assert!(report.contains("manual review required"));
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

    #[test]
    fn test_action_type_classification() {
        let vault = TestVault::new();
        vault.place_inbox("groceries.md", "Pick up milk, eggs, bread, coffee beans.");
        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);

        // Should be classified as "note" (no action keywords)
        let outbox_files: Vec<_> = fs::read_dir(&vault.outbox)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && e.path().to_string_lossy().contains("digest"))
            .collect();
        assert!(!outbox_files.is_empty());
        let report = fs::read_to_string(outbox_files[0].path()).unwrap();
        assert!(report.contains("**Type:** note"));
    }

    #[test]
    fn test_api_push_skipped_when_no_config() {
        let vault = TestVault::new();
        vault.place_inbox("api-test.md", "Research Rust async patterns.");
        let code = run_with_vault(&vault, false);
        assert_eq!(code, 0);

        let outbox_files: Vec<_> = fs::read_dir(&vault.outbox)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && e.path().to_string_lossy().contains("digest"))
            .collect();
        assert!(!outbox_files.is_empty());
        let report = fs::read_to_string(outbox_files[0].path()).unwrap();
        assert!(report.contains("Skipped (no API configured)"));
    }

    #[test]
    fn test_map_task_type() {
        assert_eq!(map_task_type("research"), "research");
        assert_eq!(map_task_type("question"), "research");
        assert_eq!(map_task_type("repo-change"), "dev");
        assert_eq!(map_task_type("ops"), "dev");
        assert_eq!(map_task_type("note"), "dev");
    }
}
