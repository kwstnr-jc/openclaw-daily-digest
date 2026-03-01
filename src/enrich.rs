use crate::types::Enrichment;
use crate::util::{call_openclaw, extract_json, which_exists};

pub fn enrich(
    task_content: &str,
    openclaw_cmd: &str,
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

    let mut args = vec![
        "agent".to_string(),
        "--agent".to_string(),
        "main".to_string(),
        "--timeout".to_string(),
        "120".to_string(),
    ];

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
