use chrono::{Local, NaiveDate};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::types::Envelope;

pub fn call_openclaw(cmd: &str, args: &[String]) -> Option<String> {
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

pub fn extract_json(raw: &str) -> Option<serde_json::Value> {
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

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), std::io::Error> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".tmp-{}", std::process::id()));
    let mut file = fs::File::create(&tmp)?;
    file.write_all(data)?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn write_envelope(
    path: &Path,
    envelope: &Envelope,
) -> Result<(), std::io::Error> {
    let json = serde_json::to_string_pretty(envelope)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    atomic_write(path, json.as_bytes())
}

pub fn append_log(
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

pub fn rotate_logs(logs_dir: &Path, retention_days: u64) {
    let cutoff = Local::now().date_naive() - chrono::Duration::days(retention_days as i64);
    let entries = match fs::read_dir(logs_dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        let date = match NaiveDate::parse_from_str(stem, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => continue,
        };
        if date < cutoff {
            println!("Rotating old log: {}", path.display());
            if let Err(e) = fs::remove_file(&path) {
                eprintln!("Failed to remove old log {}: {}", path.display(), e);
            }
        }
    }
}

pub fn move_file(from: &Path, to: &Path) {
    if let Err(e) = fs::rename(from, to) {
        eprintln!(
            "Failed to move {} -> {}: {}",
            from.display(),
            to.display(),
            e
        );
    }
}

pub fn which_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn read_first_n_lines(path: &Path, n: usize) -> Result<String, std::io::Error> {
    let content = fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().take(n).collect();
    Ok(lines.join("\n"))
}

pub fn find_first_inbox_item(inbox: &Path) -> Option<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(inbox)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "md"))
        .collect();
    entries.sort();
    entries.into_iter().next()
}
