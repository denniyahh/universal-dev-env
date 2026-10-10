use serde::Serialize;
use std::path::{Path, PathBuf};
use which::which;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "message")]
pub enum CheckStatus {
    Pass,
    Warning(String),
    Fail(String),
    Critical(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditReport {
    pub category: String,
    pub name: String,
    pub status: CheckStatus,
}

impl AuditReport {
    pub fn is_fail(&self) -> bool {
        matches!(self.status, CheckStatus::Fail(_) | CheckStatus::Critical(_))
    }

    pub fn is_critical(&self) -> bool {
        matches!(self.status, CheckStatus::Critical(_))
    }

    pub fn is_warning(&self) -> bool {
        matches!(self.status, CheckStatus::Warning(_))
    }
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
                CheckStatus::Critical(format!("Missing {}", allowed_signers.display()))
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
                CheckStatus::Critical("Global git gpg.format is not configured to 'ssh'".to_string())
            },
        });

        // 4. Global git commit signing
        let gpg_sign = std::process::Command::new("git")
            .args(["config", "--global", "--get", "commit.gpgsign"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        reports.push(AuditReport {
            category: "Git & SSH Security".to_string(),
            name: "Global Git Commit Signing".to_string(),
            status: if gpg_sign == "true" {
                CheckStatus::Pass
            } else {
                CheckStatus::Critical("Global git commit.gpgsign is not enabled ('true')".to_string())
            },
        });

        // 5. Canonical agent rules mesh (~/.config/agents/AGENTS.md)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_audit_critical_classification() {
        let report = AuditReport {
            category: "Git & SSH Security".to_string(),
            name: "Allowed SSH Signers file".to_string(),
            status: CheckStatus::Critical("Missing allowed_signers".to_string()),
        };
        assert!(report.is_fail());
        assert!(report.is_critical());
        assert!(!report.is_warning());
    }

    #[test]
    fn test_audit_host_missing_allowed_signers_is_critical() {
        let temp = tempdir().unwrap();
        let auditor = Auditor {
            user_home: temp.path().to_path_buf(),
            xdg_config: temp.path().join(".config"),
        };
        let reports = auditor.audit_host();
        let signers_report = reports
            .iter()
            .find(|r| r.name == "Allowed SSH Signers file")
            .expect("must check allowed signers");

        assert!(
            signers_report.is_critical(),
            "Missing allowed_signers must be classified as Critical (exit code 2)"
        );
    }
}
