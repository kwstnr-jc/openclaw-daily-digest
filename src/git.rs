use std::path::{Path, PathBuf};
use std::process::Command;

pub fn find_repo_dir(
    projects_dir: &Path,
    project_name: Option<&str>,
    project_kind: &str,
) -> Option<PathBuf> {
    let name = project_name?;
    if project_kind != "existing" && project_kind != "new" {
        return None;
    }
    // Check $ROOT/Projects/<name>/
    let candidate = projects_dir.join(name);
    if candidate.join(".git").exists() {
        return Some(candidate);
    }
    // Check ~/work/<name>/
    if let Ok(home) = std::env::var("HOME") {
        let candidate = PathBuf::from(home).join("work").join(name);
        if candidate.join(".git").exists() {
            return Some(candidate);
        }
    }
    None
}

pub fn git_default_branch(repo_dir: &Path) -> String {
    if let Ok(output) = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
        .current_dir(repo_dir)
        .output()
        && output.status.success()
    {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return branch
            .strip_prefix("origin/")
            .unwrap_or(&branch)
            .to_string();
    }
    if run_git(repo_dir, &["rev-parse", "--verify", "main"]).is_ok() {
        return "main".to_string();
    }
    "master".to_string()
}

pub fn run_git(repo_dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_dir)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

pub fn git_untracked_files(repo_dir: &Path) -> Vec<String> {
    match Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(repo_dir)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect(),
        _ => vec![],
    }
}

pub fn call_openclaw_in_dir(cmd: &str, args: &[String], dir: &Path) -> Option<String> {
    let output = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if stdout.trim().is_empty() {
        return None;
    }
    Some(stdout)
}

pub fn create_pull_request(repo_dir: &Path, title: &str, body: &str, base: &str) -> Option<String> {
    let output = Command::new("gh")
        .args([
            "pr", "create", "--title", title, "--body", body, "--base", base,
        ])
        .current_dir(repo_dir)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let url = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if url.starts_with("http") {
                println!("PR created: {}", url);
                let _ = Command::new("gh")
                    .args(["pr", "edit", &url, "--add-label", "agent-generated"])
                    .current_dir(repo_dir)
                    .output();
                Some(url)
            } else {
                println!("PR created but URL not recognized: {}", url);
                Some(url)
            }
        }
        Ok(o) => {
            eprintln!(
                "gh pr create failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            None
        }
        Err(e) => {
            eprintln!("gh not available: {}", e);
            None
        }
    }
}
