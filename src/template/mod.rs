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
    pub fn detect_for_project(
        project_dir: &std::path::Path,
        project_name: &str,
        default_project_type: &str,
    ) -> Self {
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

        let effective_project_type = if default_project_type != "generic"
            && !default_project_type.is_empty()
        {
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

/// The single render rule (SEC-04) shared by init, sync scan, sync pull and push's
/// render-back check: a template is rendered when its path has the `.jinja` extension
/// or its text contains `{{`. Anything else, and any template that is not UTF-8, is
/// used byte for byte.
pub fn should_render(path: &std::path::Path, text: &str) -> bool {
    path.extension().is_some_and(|ext| ext == "jinja") || text.contains("{{")
}

/// A template error reduced to minijinja's fixed error category and the line, so it can
/// be shown for a file whose content must not be echoed. It never holds the engine's
/// detail text or any template source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderFailure {
    /// The fixed description of minijinja's `ErrorKind` (for example `syntax error`).
    pub category: String,
    /// The 1-based line the engine reported, when it reported one.
    pub line: Option<usize>,
}

impl RenderFailure {
    /// `{category} at line {n}`, or `{category} at an unknown line`.
    pub fn describe(&self) -> String {
        String::new()
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

    /// Renders like [`Self::render_str`], but reduces any error to a [`RenderFailure`]
    /// (fixed category and line), never the engine's detail text, which quotes
    /// identifiers and tokens from the template.
    pub fn render_str_checked(
        &self,
        _template_str: &str,
        _ctx: &TemplateContext,
    ) -> Result<String, RenderFailure> {
        Err(RenderFailure {
            category: String::new(),
            line: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_render() {
        use std::path::Path;
        assert!(should_render(Path::new("x.jinja"), "plain text"));
        assert!(should_render(
            Path::new("gitconfig.repo"),
            "path = {{ user.home }}/.gitconfig.local"
        ));
        assert!(!should_render(
            Path::new("commit-msg"),
            "if [ \"${#FIRST_LINE}\" -gt 72 ]; then"
        ));
        // Block tags alone do not trigger rendering, matching sync scan and pull.
        assert!(!should_render(
            Path::new("gitconfig.repo"),
            "{% if true %}x{% endif %}"
        ));
    }

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

    fn alice_ctx() -> TemplateContext {
        TemplateContext {
            user: UserContext {
                home: "/home/alice".to_string(),
                xdg_config: "/home/alice/.config".to_string(),
                xdg_data: "/home/alice/.local/share".to_string(),
            },
            user_home: "/home/alice".to_string(),
            xdg_config: "/home/alice/.config".to_string(),
            xdg_data: "/home/alice/.local/share".to_string(),
            git_user_name: "Alice".to_string(),
            git_email: "alice@example.com".to_string(),
            project_name: "sample-proj".to_string(),
            project_type: "generic".to_string(),
            maintainer_ssh_pubkey: None,
            agent_ssh_pubkey: None,
            extra: HashMap::new(),
        }
    }

    #[test]
    fn test_render_str_checked_reports_category_and_line_only() {
        let renderer = TemplateRenderer::new();
        let ctx = alice_ctx();
        let sentinel = "ude_sentinel_label";

        let unknown_filter = format!("[core]\n\tx = {{{{ 1 | {sentinel} }}}}\n");
        let failure = renderer
            .render_str_checked(&unknown_filter, &ctx)
            .expect_err("unknown filter must fail");
        assert_eq!(
            failure,
            RenderFailure {
                category: "unknown filter".to_string(),
                line: Some(2),
            }
        );
        assert!(!format!("{failure:?}").contains(sentinel), "{failure:?}");
        assert!(
            !failure.describe().contains(sentinel),
            "{}",
            failure.describe()
        );
        assert_eq!(failure.describe(), "unknown filter at line 2");

        // The engine reports an unterminated comment where it reached the end of the text.
        let open_comment = "[core]\n{# unterminated\n{{ user.home }}\n";
        assert_eq!(
            renderer.render_str_checked(open_comment, &ctx),
            Err(RenderFailure {
                category: "syntax error".to_string(),
                line: Some(4),
            })
        );

        let undefined = "[core]\n\tt = ${{ secrets.TOKEN }}\n";
        assert_eq!(
            renderer.render_str_checked(undefined, &ctx),
            Err(RenderFailure {
                category: "undefined value".to_string(),
                line: Some(2),
            })
        );

        let no_line = RenderFailure {
            category: "syntax error".to_string(),
            line: None,
        };
        assert_eq!(no_line.describe(), "syntax error at an unknown line");

        // Negative control: the unchecked renderer's error does quote the identifier,
        // which is the leak render_str_checked exists to avoid.
        let leaked = renderer
            .render_str(&unknown_filter, &ctx)
            .expect_err("unknown filter must fail");
        assert!(leaked.contains(sentinel), "{leaked}");

        // Positive control: a valid template renders exactly as render_str renders it.
        let valid = "{{ user.home }}/x\n";
        assert_eq!(
            renderer.render_str_checked(valid, &ctx),
            Ok(renderer.render_str(valid, &ctx).unwrap())
        );
        assert_eq!(
            renderer.render_str_checked(valid, &ctx).unwrap(),
            "/home/alice/x\n"
        );
    }
}
