use chrono::Local;
use std::fs;

use crate::types::ItemResult;

pub fn format_discord_message(results: &[ItemResult]) -> String {
    let now = Local::now();
    let mut msg = format!(
        "**Daily Digest** \u{2014} {}\n\nProcessed {} items:\n",
        now.format("%Y-%m-%d %H:%M"),
        results.len()
    );

    for r in results {
        let project = r.project_name.as_deref().unwrap_or("none");
        let detail = match r.exec_status.as_str() {
            "completed" => "completed".to_string(),
            "failed" => "failed".to_string(),
            "none" => "filed".to_string(),
            "skipped" => "skipped".to_string(),
            "no-op" => "no changes".to_string(),
            "pushed" => "pushed (PR failed)".to_string(),
            other => other.to_string(),
        };
        let suffix = match &r.pr_url {
            Some(url) => format!("PR opened: <{}>", url),
            None => detail,
        };
        msg.push_str(&format!(
            "- `{}` \u{2192} **{}** ({}) \u{2014} {}\n",
            r.source_file, project, r.action_type, suffix
        ));
    }

    let enriched = results.iter().filter(|r| r.enriched).count();
    let unenriched = results.iter().filter(|r| !r.enriched && !r.failed).count();
    let failed = results.iter().filter(|r| r.failed).count();
    msg.push_str(&format!(
        "\nEnriched: {} | Unenriched: {} | Failed: {}",
        enriched, unenriched, failed
    ));

    // Discord limit is 2000 chars
    if msg.len() > 2000 {
        msg.truncate(1990);
        msg.push_str("\n...(truncated)");
    }

    msg
}

pub fn post_to_discord(message: &str) -> Result<(), String> {
    let token_file = std::env::var("DISCORD_TOKEN_FILE").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        format!("{}/.digest-bot-token", home)
    });

    let token = fs::read_to_string(&token_file)
        .map_err(|e| format!("Cannot read token file {}: {}", token_file, e))?
        .trim()
        .to_string();

    if token.is_empty() {
        return Err("Bot token is empty".to_string());
    }

    let channel_id =
        std::env::var("DISCORD_CHANNEL_ID").unwrap_or_else(|_| "1477340656350396668".to_string());

    let url = format!(
        "https://discord.com/api/v10/channels/{}/messages",
        channel_id
    );

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bot {}", token))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "content": message }))
        .send()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "Discord API returned {}: {}",
            resp.status(),
            resp.text().unwrap_or_default()
        ))
    }
}
