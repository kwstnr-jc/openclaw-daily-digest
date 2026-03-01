Project: openclaw-daily-digest

Fix the envelope.json writer to escape newlines in task_text properly.
The current implementation breaks when the inbox item contains multi-line content.

Refactor the _write_envelope function to use jq for all JSON construction.
