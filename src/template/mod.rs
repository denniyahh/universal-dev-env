use directories::BaseDirs;
use minijinja::Environment;
use serde::Serialize;
use std::collections::HashMap;

/// Nested user path variables for structured template access (e.g. {{ user.home }})
#[derive(Debug, Clone, Serialize)]
pub struct UserContext {
    pub home: String,
    pub xdg_config: String,
    pub xdg_data: String,
}

/// System context parameters available in all UDE templates
#[derive(Debug, Clone, Serialize)]
pub struct TemplateContext {
    pub user: UserContext,
    pub user_home: String,
    pub xdg_config: String,
    pub xdg_data: String,
    pub git_user_name: String,
    pub git_email: String,
    pub project_name: String,
    pub project_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maintainer_ssh_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_ssh_pubkey: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, String>,
}

impl TemplateContext {
    /// Detects host defaults from system environment and git config for a specific project directory
    pub fn detect_for_project(project_dir: &std::path::Path, project_name: &str, default_project_type: &str) -> Self {
        let base_dirs = BaseDirs::new();
        let user_home = base_dirs
            .as_ref()
            .map(|b| b.home_dir().to_string_lossy().to_string())
            .unwrap_or_else(|| {
                std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| ".".to_string())
            });

        let xdg_config = base_dirs
            .as_ref()
            .map(|b| b.config_dir().to_string_lossy().to_string())
            .unwrap_or_else(|| format!("{}/.config", user_home));

        let xdg_data = base_dirs
            .as_ref()
            .map(|b| b.data_dir().to_string_lossy().to_string())
            .unwrap_or_else(|| format!("{}/.local/share", user_home));

        let mut name_cmd = std::process::Command::new("git");
        name_cmd
            .current_dir(project_dir)
            .args(["config", "--get", "user.name"]);
        let git_user_name = crate::process::run_bounded(
            name_cmd,
            "git config user.name",
            crate::process::LOCAL_TIMEOUT,
        )
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Developer".to_string());

        let mut email_cmd = std::process::Command::new("git");
        email_cmd
            .current_dir(project_dir)
            .args(["config", "--get", "user.email"]);
        let git_email = crate::process::run_bounded(
            email_cmd,
            "git config user.email",
            crate::process::LOCAL_TIMEOUT,
        )
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "developer@example.com".to_string());

        let user = UserContext {
            home: user_home.clone(),
            xdg_config: xdg_config.clone(),
            xdg_data: xdg_data.clone(),
        };

        let effective_project_type = if default_project_type != "generic" && !default_project_type.is_empty() {
            default_project_type.to_string()
        } else if let Ok(state_str) = std::fs::read_to_string(project_dir.join(".ude/state.json"))
            && let Ok(state_json) = serde_json::from_str::<serde_json::Value>(&state_str)
            && let Some(pt) = state_json.get("project_type").and_then(|v| v.as_str())
        {
            pt.to_string()
        } else if project_dir.join("Cargo.toml").exists() {
            "rust".to_string()
        } else if project_dir.join("package.json").exists() {
            "node".to_string()
        } else {
            default_project_type.to_string()
        };

        Self {
            user,
            user_home,
            xdg_config,
            xdg_data,
            git_user_name,
            git_email,
            project_name: project_name.to_string(),
            project_type: effective_project_type,
            maintainer_ssh_pubkey: None,
            agent_ssh_pubkey: None,
            extra: HashMap::new(),
        }
    }

    /// Detects host defaults from system environment and git config in the current working directory
    pub fn detect(project_name: &str, project_type: &str) -> Self {
        Self::detect_for_project(std::path::Path::new("."), project_name, project_type)
    }
}

pub struct TemplateRenderer {
    env: Environment<'static>,
}

impl Default for TemplateRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateRenderer {
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_keep_trailing_newline(true);
        Self { env }
    }

    /// Renders a template string given a context
    pub fn render_str(&self, template_str: &str, ctx: &TemplateContext) -> Result<String, String> {
        let mut env = self.env.clone();
        env.add_template("template", template_str)
            .map_err(|e| format!("Template compilation error: {e}"))?;
        let tmpl = env
            .get_template("template")
            .map_err(|e| format!("Template fetch error: {e}"))?;
        tmpl.render(ctx)
            .map_err(|e| format!("Template render error: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_rendering_basic() {
        let renderer = TemplateRenderer::new();
        let ctx = TemplateContext {
            user: UserContext {
                home: "/home/testuser".to_string(),
                xdg_config: "/home/testuser/.config".to_string(),
                xdg_data: "/home/testuser/.local/share".to_string(),
            },
            user_home: "/home/testuser".to_string(),
            xdg_config: "/home/testuser/.config".to_string(),
            xdg_data: "/home/testuser/.local/share".to_string(),
            git_user_name: "Test User".to_string(),
            git_email: "test@example.com".to_string(),
            project_name: "sample-proj".to_string(),
            project_type: "rust".to_string(),
            maintainer_ssh_pubkey: Some("AAAAC3NzaC1lZDI1NTE5AAAA...".to_string()),
            agent_ssh_pubkey: Some("AAAAC3NzaC1lZDI1NTE5AAAA_AGENT...".to_string()),
            extra: HashMap::new(),
        };

        let tmpl = "{{ git_email }} ssh-ed25519 {{ maintainer_ssh_pubkey }} maintainer\n{{ user.home }}/.config/git/allowed_signers\n{{ user.xdg_config }}";
        let rendered = renderer.render_str(tmpl, &ctx).expect("render");
        assert!(rendered.contains("test@example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAA..."));
        assert!(rendered.contains("/home/testuser/.config/git/allowed_signers"));
        assert!(rendered.contains("/home/testuser/.config"));
    }
}
