use std::fs;
use std::path::Path;

use crate::git::{
    call_openclaw_in_dir, create_pull_request, find_repo_dir, git_default_branch,
    git_untracked_files, run_git,
};
use crate::policy::{select_model, PolicyConfig};
use crate::util::{call_openclaw, which_exists};

/// Returns (status, json, output_file, pr_url).
pub fn execute_handler(
    action_type: &str,
    task_content: &str,
    outbox: &Path,
    timestamp: &str,
    stem: &str,
    openclaw_cmd: &str,
    policy: &Option<PolicyConfig>,
    projects_dir: &Path,
    project_name: Option<&str>,
    project_kind: &str,
    enrichment_rendered: &str,
) -> (String, serde_json::Value, Option<String>, Option<String>) {
    match action_type {
        "research" => {
            let (s, j, f) = execute_research(task_content, outbox, timestamp, stem, openclaw_cmd, policy);
            (s, j, f, None)
        }
        "question" => {
            let (s, j, f) = execute_question(task_content, outbox, timestamp, stem, openclaw_cmd, policy);
            (s, j, f, None)
        }
        "repo-change" => execute_repo_change(
            task_content, outbox, timestamp, stem, openclaw_cmd, policy,
            projects_dir, project_name, project_kind, enrichment_rendered,
        ),
        "ops" => {
            let (s, j, f) = execute_ops(task_content, outbox, timestamp, stem, openclaw_cmd, policy);
            (s, j, f, None)
        }
        _ => {
            let json = serde_json::json!({"handler":"note","status":"none","reason":"No execution required for notes"});
            ("none".to_string(), json, None, None)
        }
    }
}

fn execute_research(
    task_content: &str, outbox: &Path, timestamp: &str, stem: &str,
    openclaw_cmd: &str, policy: &Option<PolicyConfig>,
) -> (String, serde_json::Value, Option<String>) {
    let exec_file = outbox.join(format!("{}-{}.research.md", timestamp, stem));
    println!("Executing research handler...");
    if !which_exists(openclaw_cmd) {
        let json = serde_json::json!({"handler":"research","status":"skipped","reason":"OpenClaw not available"});
        return ("skipped".to_string(), json, None);
    }
    let model = select_model(policy, "research", task_content);
    let mut args = vec![
        "agent".to_string(), "--agent".to_string(), "main".to_string(),
        "--timeout".to_string(), "120".to_string(),
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
         Task:\n{}", task_content
    );
    args.push("--message".to_string());
    args.push(prompt);
    if let Some(output) = call_openclaw(openclaw_cmd, &args) {
        if !output.is_empty() {
            let _ = fs::write(&exec_file, &output);
            let fname = exec_file.file_name().unwrap().to_string_lossy().to_string();
            println!("Research report written: {}", exec_file.display());
            let json = serde_json::json!({"handler":"research","status":"completed","output_file":fname});
            return ("completed".to_string(), json, Some(fname));
        }
    }
    let json = serde_json::json!({"handler":"research","status":"failed","reason":"OpenClaw returned empty response"});
    ("failed".to_string(), json, None)
}

fn execute_question(
    task_content: &str, outbox: &Path, timestamp: &str, stem: &str,
    openclaw_cmd: &str, policy: &Option<PolicyConfig>,
) -> (String, serde_json::Value, Option<String>) {
    let exec_file = outbox.join(format!("{}-{}.research.md", timestamp, stem));
    println!("Executing question handler...");
    if !which_exists(openclaw_cmd) {
        let json = serde_json::json!({"handler":"question","status":"skipped","reason":"OpenClaw not available"});
        return ("skipped".to_string(), json, None);
    }
    let model = select_model(policy, "question", task_content);
    let mut args = vec![
        "agent".to_string(), "--agent".to_string(), "main".to_string(),
        "--timeout".to_string(), "120".to_string(),
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
         Question:\n{}", task_content
    );
    args.push("--message".to_string());
    args.push(prompt);
    if let Some(output) = call_openclaw(openclaw_cmd, &args) {
        if !output.is_empty() {
            let _ = fs::write(&exec_file, &output);
            let fname = exec_file.file_name().unwrap().to_string_lossy().to_string();
            println!("Answer report written: {}", exec_file.display());
            let json = serde_json::json!({"handler":"question","status":"completed","output_file":fname});
            return ("completed".to_string(), json, Some(fname));
        }
    }
    let json = serde_json::json!({"handler":"question","status":"failed","reason":"OpenClaw returned empty response"});
    ("failed".to_string(), json, None)
}

fn execute_repo_change(
    task_content: &str, outbox: &Path, timestamp: &str, stem: &str,
    openclaw_cmd: &str, policy: &Option<PolicyConfig>,
    projects_dir: &Path, project_name: Option<&str>,
    project_kind: &str, enrichment_rendered: &str,
) -> (String, serde_json::Value, Option<String>, Option<String>) {
    println!("Executing repo-change handler...");

    // 1. Determine target repo
    let repo_dir = match find_repo_dir(projects_dir, project_name, project_kind) {
        Some(d) => d,
        None => {
            let reason = "Cannot determine target repo";
            println!("{}", reason);
            let json = serde_json::json!({"handler":"repo-change","status":"skipped","reason":reason});
            return ("skipped".to_string(), json, None, None);
        }
    };
    println!("Target repo: {}", repo_dir.display());

    if !which_exists(openclaw_cmd) {
        let json = serde_json::json!({"handler":"repo-change","status":"skipped","reason":"OpenClaw not available"});
        return ("skipped".to_string(), json, None, None);
    }

    // 2. Create feature branch
    let slug: String = stem.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_lowercase();
    let short_ts = &timestamp.replace('-', "").replace('_', "");
    let short_ts = if short_ts.len() >= 8 { &short_ts[4..8] } else { short_ts.as_str() };
    let branch_name = format!("agent/{}-{}", slug, short_ts);

    let default_branch = git_default_branch(&repo_dir);
    println!("Default branch: {}, creating: {}", default_branch, branch_name);

    let _ = run_git(&repo_dir, &["fetch", "origin"]);
    if run_git(&repo_dir, &["checkout", &default_branch]).is_err() {
        let json = serde_json::json!({"handler":"repo-change","status":"failed","reason":"Cannot checkout default branch"});
        return ("failed".to_string(), json, None, None);
    }
    let _ = run_git(&repo_dir, &["pull", "--ff-only"]);
    if run_git(&repo_dir, &["checkout", "-b", &branch_name]).is_err() {
        let json = serde_json::json!({"handler":"repo-change","status":"failed","reason":"Cannot create feature branch"});
        return ("failed".to_string(), json, None, None);
    }

    // 3. Execute code change via OpenClaw
    let model = select_model(policy, "execution", task_content);
    let mut args = vec![
        "agent".to_string(), "--agent".to_string(), "default".to_string(),
        "--timeout".to_string(), "300".to_string(),
    ];
    if let Some(ref m) = model {
        args.push("--model".to_string());
        args.push(m.clone());
    }
    args.push("--message".to_string());
    args.push(format!(
        "You are working in the repository at {}. Execute the following task.\n\n\
         Task:\n{}\n\nPlanned actions:\n{}",
        repo_dir.display(), task_content, enrichment_rendered
    ));

    println!("Calling OpenClaw for code change...");
    let openclaw_output = call_openclaw_in_dir(openclaw_cmd, &args, &repo_dir);

    if openclaw_output.is_none() {
        let _ = run_git(&repo_dir, &["checkout", &default_branch]);
        let _ = run_git(&repo_dir, &["branch", "-D", &branch_name]);
        let json = serde_json::json!({"handler":"repo-change","status":"failed","reason":"OpenClaw execution failed"});
        return ("failed".to_string(), json, None, None);
    }

    // 4. Check for changes, commit and push
    let has_changes = run_git(&repo_dir, &["diff", "--quiet"]).is_err()
        || run_git(&repo_dir, &["diff", "--cached", "--quiet"]).is_err()
        || !git_untracked_files(&repo_dir).is_empty();

    if !has_changes {
        println!("No changes after OpenClaw execution — no-op.");
        let _ = run_git(&repo_dir, &["checkout", &default_branch]);
        let _ = run_git(&repo_dir, &["branch", "-D", &branch_name]);
        let json = serde_json::json!({"handler":"repo-change","status":"no-op","reason":"No changes produced"});
        return ("no-op".to_string(), json, None, None);
    }

    let _ = run_git(&repo_dir, &["add", "-A"]);
    let first_line = task_content.lines().find(|l| !l.trim().is_empty()).unwrap_or("agent task");
    let commit_msg = format!(
        "feat: {}\n\nAutonomously executed by openclaw-daily-digest.\nSource: {}",
        first_line.trim().trim_start_matches("Project:").trim().to_lowercase(), stem
    );
    if run_git(&repo_dir, &["commit", "-m", &commit_msg]).is_err() {
        let _ = run_git(&repo_dir, &["checkout", &default_branch]);
        let _ = run_git(&repo_dir, &["branch", "-D", &branch_name]);
        let json = serde_json::json!({"handler":"repo-change","status":"failed","reason":"Git commit failed"});
        return ("failed".to_string(), json, None, None);
    }

    if run_git(&repo_dir, &["push", "-u", "origin", &branch_name]).is_err() {
        eprintln!("Git push failed, branch {} exists locally", branch_name);
        let json = serde_json::json!({"handler":"repo-change","status":"failed","reason":"Git push failed","branch":branch_name});
        return ("failed".to_string(), json, None, None);
    }
    println!("Pushed branch: {}", branch_name);

    // 5. Open PR via gh
    let pr_title = first_line.trim().trim_start_matches("Project:").trim();
    let pr_title = if pr_title.len() > 70 { &pr_title[..70] } else { pr_title };
    let pr_body = format!(
        "## Task\n\n{}\n\n## Planned Actions\n\n{}\n\n---\n\n_Auto-generated by openclaw-daily-digest agent._",
        task_content, enrichment_rendered
    );
    let pr_url = create_pull_request(&repo_dir, pr_title, &pr_body, &default_branch);

    // Write execution log
    let exec_file = outbox.join(format!("{}-{}.repo-change.md", timestamp, stem));
    let exec_log = format!(
        "# Repo-Change Execution\n\n- **Repo:** {}\n- **Branch:** {}\n- **PR:** {}\n- **Status:** {}\n\n\
         ## OpenClaw Output\n\n```\n{}\n```\n",
        repo_dir.display(), branch_name,
        pr_url.as_deref().unwrap_or("(none)"),
        if pr_url.is_some() { "completed" } else { "pushed" },
        openclaw_output.as_deref().unwrap_or("(no output)"),
    );
    let _ = fs::write(&exec_file, &exec_log);
    let fname = exec_file.file_name().unwrap().to_string_lossy().to_string();

    let status = if pr_url.is_some() { "completed" } else { "pushed" };
    let json = serde_json::json!({
        "handler": "repo-change", "status": status,
        "branch": branch_name, "pr_url": pr_url, "output_file": fname,
    });

    let _ = run_git(&repo_dir, &["checkout", &default_branch]);
    (status.to_string(), json, Some(fname), pr_url)
}

fn execute_ops(
    task_content: &str, outbox: &Path, timestamp: &str, stem: &str,
    openclaw_cmd: &str, policy: &Option<PolicyConfig>,
) -> (String, serde_json::Value, Option<String>) {
    println!("Executing ops handler...");

    if !which_exists(openclaw_cmd) {
        let json = serde_json::json!({"handler":"ops","status":"skipped","reason":"OpenClaw not available"});
        return ("skipped".to_string(), json, None);
    }

    // Safety check: scan task for dangerous patterns
    let lower = task_content.to_lowercase();
    let dangerous = ["rm -rf", "rm -r /", "ssh-keygen", "ssh_key",
        "credential", "passwd", ".ssh/authorized_keys", "sudoers"];
    for pattern in &dangerous {
        if lower.contains(pattern) {
            println!("Skipped: potentially unsafe ops task (matched: {})", pattern);
            let json = serde_json::json!({
                "handler": "ops", "status": "skipped",
                "reason": format!("Skipped: potentially unsafe (matched: {})", pattern),
            });
            return ("skipped".to_string(), json, None);
        }
    }

    let model = select_model(policy, "execution", task_content);
    let mut args = vec![
        "agent".to_string(), "--agent".to_string(), "default".to_string(),
        "--timeout".to_string(), "120".to_string(),
    ];
    if let Some(ref m) = model {
        args.push("--model".to_string());
        args.push(m.clone());
    }
    args.push("--message".to_string());
    args.push(format!(
        "Execute the following ops task. Only use safe commands \
         (brew install/upgrade, launchctl load/unload, mkdir, cp, ln, chmod). \
         NEVER use rm -rf, never delete data, never modify SSH keys or credentials.\n\n\
         Task:\n{}", task_content
    ));

    println!("Calling OpenClaw for ops task...");
    let output = call_openclaw(openclaw_cmd, &args);

    let exec_file = outbox.join(format!("{}-{}.ops-log.md", timestamp, stem));
    let exec_log = format!(
        "# Ops Execution Log\n\n## Task\n\n{}\n\n## Output\n\n```\n{}\n```\n\n## Status\n\n{}\n",
        task_content,
        output.as_deref().unwrap_or("(no output)"),
        if output.is_some() { "completed" } else { "failed" },
    );
    let _ = fs::write(&exec_file, &exec_log);
    let fname = exec_file.file_name().unwrap().to_string_lossy().to_string();

    let status = if output.is_some() { "completed" } else { "failed" };
    let json = serde_json::json!({"handler": "ops", "status": status, "output_file": fname});
    (status.to_string(), json, Some(fname))
}
