#[allow(clippy::too_many_arguments)]
pub fn build_report(
    task_content: &str,
    source_stem: &str,
    project_kind: &str,
    project_name: Option<&str>,
    classification_method: &str,
    action_type: &str,
    action_type_method: &str,
    enrichment_rendered: &str,
    enriched: bool,
    enrichment_json: &serde_json::Value,
    api_pushed: bool,
) -> String {
    let mut report = String::new();

    // Frontmatter
    report.push_str("---\ntags:\n  - digest\n");
    if let Some(name) = project_name {
        report.push_str(&format!("  - {}\n", name));
    }
    report.push_str(&format!("source: \"[[{}]]\"\n", source_stem));
    report.push_str("---\n\n");

    // Wikilink to source item
    report.push_str(&format!("Source: [[{}]]\n\n", source_stem));

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

    report.push_str("## API\n\n");
    if api_pushed {
        report.push_str("- Task pushed to task-orchestrator API\n\n");
    } else {
        report.push_str("- Skipped (no API configured)\n\n");
    }

    report.push_str(enrichment_rendered);

    if enriched && !enrichment_json.is_null() {
        report.push_str(&format!(
            "\n## Enrichment (raw JSON)\n\n```json\n{}\n```\n",
            serde_json::to_string_pretty(enrichment_json).unwrap_or_default()
        ));
    }

    report
}
