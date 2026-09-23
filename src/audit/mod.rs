use std::path::{Path, PathBuf};
use which::which;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warning(String),
    Fail(String),
}

#[derive(Debug, Clone)]
pub struct AuditReport {
    pub category: String,
    pub name: String,
    pub status: CheckStatus,
}

pub struct Auditor {
    #[allow(dead_code)]
    user_home: PathBuf,
    xdg_config: PathBuf,
}

impl Default for Auditor {
    fn default() -> Self {
        Self::new()
    }
}

impl Auditor {
    pub fn new() -> Self {
        let base_dirs = directories::BaseDirs::new();
        let user_home = base_dirs
            .as_ref()
            .map(|b| b.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string())));
        let xdg_config = base_dirs
            .as_ref()
            .map(|b| b.config_dir().to_path_buf())
            .unwrap_or_else(|| user_home.join(".config"));

        Self {
            user_home,
            xdg_config,
        }
    }

    /// Runs all host-level environment checks
    pub fn audit_host(&self) -> Vec<AuditReport> {
        let mut reports = Vec::new();

        // 1. Toolchain availability
        let required_tools = vec![
            ("git", true),
            ("cargo", false),
            ("zig", false),
            ("gitleaks", false),
            ("mise", false),
            ("podman", false),
        ];

        for (tool, required) in required_tools {
            let present = which(tool).is_ok();
            reports.push(AuditReport {
                category: "Host Toolchains".to_string(),
                name: format!("CLI tool: {tool}"),
                status: if present {
                    CheckStatus::Pass
                } else if required {
                    CheckStatus::Fail(format!("Required tool '{tool}' is missing on PATH"))
                } else {
                    CheckStatus::Warning(format!("Optional tool '{tool}' not found"))
                },
            });
        }

        // 2. SSH allowed_signers
        let allowed_signers = self.xdg_config.join("git/allowed_signers");
        reports.push(AuditReport {
            category: "Git & SSH Security".to_string(),
            name: "Allowed SSH Signers file".to_string(),
            status: if allowed_signers.exists() {
                CheckStatus::Pass
            } else {
                CheckStatus::Warning(format!("Missing {}", allowed_signers.display()))
            },
        });

        // 3. Global git config GPG format
        let gpg_format = std::process::Command::new("git")
            .args(["config", "--global", "--get", "gpg.format"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        reports.push(AuditReport {
            category: "Git & SSH Security".to_string(),
            name: "Global Git SSH Signing Format".to_string(),
            status: if gpg_format == "ssh" {
                CheckStatus::Pass
            } else {
                CheckStatus::Warning("Global git gpg.format is not configured to 'ssh'".to_string())
            },
        });

        // 4. Canonical agent rules mesh (~/.config/agents/AGENTS.md)
        let agents_md = self.xdg_config.join("agents/AGENTS.md");
        reports.push(AuditReport {
            category: "AI Agent Governance".to_string(),
            name: "Canonical AGENTS.md".to_string(),
            status: if agents_md.exists() {
                CheckStatus::Pass
            } else {
                CheckStatus::Warning(format!("Missing canonical {}", agents_md.display()))
            },
        });

        reports
    }

    /// Runs all project-level repository checks
    pub fn audit_repo(&self, repo_path: &Path) -> Vec<AuditReport> {
        let mut reports = Vec::new();

        if !repo_path.join(".git").exists() {
            reports.push(AuditReport {
                category: "Repository Baseline".to_string(),
                name: "Git Repository".to_string(),
                status: CheckStatus::Fail(format!("{} is not a git repository", repo_path.display())),
            });
            return reports;
        }

        // 1. Check tracked .gitconfig include
        let gitconfig = repo_path.join(".gitconfig");
        reports.push(AuditReport {
            category: "Repository Baseline".to_string(),
            name: "Tracked .gitconfig Include".to_string(),
            status: if gitconfig.exists() {
                CheckStatus::Pass
            } else {
                CheckStatus::Warning("Missing root .gitconfig template".to_string())
            },
        });

        // 2. Check hooks suite
        let hooks_dir = repo_path.join("scripts/hooks");
        for hook in &["commit-msg", "pre-commit", "pre-push"] {
            let hook_file = hooks_dir.join(hook);
            reports.push(AuditReport {
                category: "Git Hooks".to_string(),
                name: format!("Hook: {hook}"),
                status: if hook_file.exists() {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Warning(format!("Missing hook: scripts/hooks/{hook}"))
                },
            });
        }

        reports
    }
}
