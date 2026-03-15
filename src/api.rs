use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub api_url: String,
    pub api_key: String,
}

#[derive(Serialize)]
struct CreateProjectRequest {
    name: String,
    description: String,
}

#[derive(Deserialize, Debug)]
pub struct ProjectResponse {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
struct CreateTaskRequest {
    title: String,
    description: String,
    task_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct TaskResponse {
    pub id: String,
}

impl ApiConfig {
    /// Load API config from env vars. Returns None if API_URL is not set.
    pub fn from_env() -> Option<Self> {
        let api_url = std::env::var("API_URL").ok()?;
        if api_url.is_empty() {
            return None;
        }
        let api_key = std::env::var("API_KEY").unwrap_or_default();
        Some(ApiConfig { api_url, api_key })
    }

    /// Ensure a project exists. Creates it if it doesn't exist.
    /// Handles 409 (conflict/duplicate) gracefully by fetching existing.
    pub fn ensure_project(&self, name: &str) -> Result<ProjectResponse, String> {
        let client = Client::new();
        let url = format!("{}/api/projects", self.api_url);

        let resp = client
            .post(&url)
            .header("X-API-Key", &self.api_key)
            .json(&CreateProjectRequest {
                name: name.to_string(),
                description: format!("Auto-created by daily-digest for project: {}", name),
            })
            .send()
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        let status = resp.status();
        if status.is_success() {
            let project: ProjectResponse = resp
                .json()
                .map_err(|e| format!("Failed to parse project response: {}", e))?;
            println!("Created project '{}' with ID {}", name, project.id);
            return Ok(project);
        }

        if status.as_u16() == 409 {
            // Project already exists, fetch it
            println!("Project '{}' already exists, fetching...", name);
            return self.find_project(name);
        }

        Err(format!(
            "Failed to create project '{}': {} {}",
            name,
            status,
            resp.text().unwrap_or_default()
        ))
    }

    /// Find a project by name (searches the list endpoint).
    fn find_project(&self, name: &str) -> Result<ProjectResponse, String> {
        let client = Client::new();
        let url = format!("{}/api/projects", self.api_url);

        let resp = client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Failed to list projects: {}", resp.status()));
        }

        let projects: Vec<ProjectResponse> = resp
            .json()
            .map_err(|e| format!("Failed to parse projects list: {}", e))?;

        projects
            .into_iter()
            .find(|p| p.name == name)
            .ok_or_else(|| format!("Project '{}' not found after 409", name))
    }

    /// Push a task to the API.
    pub fn push_task(
        &self,
        title: &str,
        description: &str,
        task_type: &str,
        project_id: Option<&str>,
    ) -> Result<TaskResponse, String> {
        let client = Client::new();
        let url = format!("{}/api/tasks", self.api_url);

        let resp = client
            .post(&url)
            .header("X-API-Key", &self.api_key)
            .json(&CreateTaskRequest {
                title: title.to_string(),
                description: description.to_string(),
                task_type: task_type.to_string(),
                project_id: project_id.map(|s| s.to_string()),
            })
            .send()
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if resp.status().is_success() {
            let task: TaskResponse = resp
                .json()
                .map_err(|e| format!("Failed to parse task response: {}", e))?;
            println!("Pushed task '{}' with ID {}", title, task.id);
            Ok(task)
        } else {
            Err(format!(
                "Failed to push task: {} {}",
                resp.status(),
                resp.text().unwrap_or_default()
            ))
        }
    }
}

/// Map action_type to task_type for the API.
pub fn map_task_type(action_type: &str) -> &str {
    match action_type {
        "research" | "question" => "research",
        "repo-change" | "ops" | "note" => "dev",
        _ => "dev",
    }
}
