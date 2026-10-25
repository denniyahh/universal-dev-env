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
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
            });
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
        let signers_status = if !allowed_signers.exists() {
            CheckStatus::Critical(format!("Missing {}", allowed_signers.display()))
        } else {
            match std::fs::read_to_string(&allowed_signers) {
                Ok(content) => {
                    let has_valid_key = content.lines().any(|line| {
                        let trimmed = line.trim();
                        !trimmed.starts_with('#')
                            && !trimmed.is_empty()
                            && (trimmed.contains("ssh-ed25519")
                                || trimmed.contains("ssh-rsa")
                                || trimmed.contains("ecdsa-sha2-"))
                    });
                    if has_valid_key {
                        CheckStatus::Pass
                    } else {
                        CheckStatus::Critical(format!(
                            "Allowed signers file '{}' contains no authorized key fingerprints",
                            allowed_signers.display()
                        ))
                    }
                }
                Err(e) => CheckStatus::Critical(format!(
                    "Failed to read {}: {e}",
                    allowed_signers.display()
                )),
            }
        };
        reports.push(AuditReport {
            category: "Git & SSH Security".to_string(),
            name: "Allowed SSH Signers file".to_string(),
            status: signers_status,
        });

        // 3. Global git config GPG format
        let mut gpg_format_cmd = std::process::Command::new("git");
        gpg_format_cmd.args(["config", "--global", "--get", "gpg.format"]);
        let gpg_format = crate::process::run_bounded(
            gpg_format_cmd,
            "git config gpg.format",
            crate::process::LOCAL_TIMEOUT,
        )
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

        reports.push(AuditReport {
            category: "Git & SSH Security".to_string(),
            name: "Global Git SSH Signing Format".to_string(),
            status: if gpg_format == "ssh" {
                CheckStatus::Pass
            } else {
                CheckStatus::Critical(
                    "Global git gpg.format is not configured to 'ssh'".to_string(),
                )
            },
        });

        // 4. Global git commit signing
        let mut gpg_sign_cmd = std::process::Command::new("git");
        gpg_sign_cmd.args(["config", "--global", "--get", "commit.gpgsign"]);
        let gpg_sign = crate::process::run_bounded(
            gpg_sign_cmd,
            "git config commit.gpgsign",
            crate::process::LOCAL_TIMEOUT,
        )
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

        reports.push(AuditReport {
            category: "Git & SSH Security".to_string(),
            name: "Global Git Commit Signing".to_string(),
            status: if gpg_sign == "true" {
                CheckStatus::Pass
            } else {
                CheckStatus::Critical(
                    "Global git commit.gpgsign is not enabled ('true')".to_string(),
                )
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
                status: CheckStatus::Fail(format!(
                    "{} is not a git repository",
                    repo_path.display()
                )),
            });
            return reports;
        }

        // 1. Check tracked .gitconfig include
        let gitconfig = repo_path.join(".gitconfig");
        let gitconfig_status = if !gitconfig.exists() {
            CheckStatus::Warning("Missing root .gitconfig template".to_string())
        } else if std::fs::metadata(&gitconfig).map(|m| m.len()).unwrap_or(0) == 0 {
            CheckStatus::Warning("Root .gitconfig template is empty (0 bytes)".to_string())
        } else {
            // Verify repository-local .git/config includes the root .gitconfig
            let mut include_cmd = std::process::Command::new("git");
            include_cmd.current_dir(repo_path).args([
                "config",
                "--local",
                "--get-all",
                "include.path",
            ]);
            let include_out = crate::process::run_bounded(
                include_cmd,
                "git config include.path",
                crate::process::LOCAL_TIMEOUT,
            )
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
            let has_include = include_out
                .lines()
                .any(|l| l.trim() == "../.gitconfig" || l.trim() == ".gitconfig");
            if has_include {
                CheckStatus::Pass
            } else {
                CheckStatus::Warning(
                    "Repository .git/config does not include root .gitconfig (missing 'include.path = ../.gitconfig')".to_string(),
                )
            }
        };
        reports.push(AuditReport {
            category: "Repository Baseline".to_string(),
            name: "Tracked .gitconfig Include".to_string(),
            status: gitconfig_status,
        });

        // 2. Check core.hooksPath configuration
        let mut hooks_path_cmd = std::process::Command::new("git");
        hooks_path_cmd
            .current_dir(repo_path)
            .args(["config", "--get", "core.hooksPath"]);
        let hooks_path_out = crate::process::run_bounded(
            hooks_path_cmd,
            "git config core.hooksPath",
            crate::process::LOCAL_TIMEOUT,
        )
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
        reports.push(AuditReport {
            category: "Git Hooks".to_string(),
            name: "Repository core.hooksPath".to_string(),
            status: if hooks_path_out == "scripts/hooks" {
                CheckStatus::Pass
            } else if hooks_path_out.is_empty() {
                CheckStatus::Warning("Repository core.hooksPath is unset (should be 'scripts/hooks')".to_string())
            } else {
                CheckStatus::Warning(format!(
                    "Repository core.hooksPath is set to '{hooks_path_out}', expected 'scripts/hooks'"
                ))
            },
        });

        // 3. Check hooks suite & index execution modes
        let hooks_dir = repo_path.join("scripts/hooks");
        for hook in &["commit-msg", "pre-commit", "pre-push"] {
            let hook_file = hooks_dir.join(hook);
            let status = if !hook_file.exists() {
                CheckStatus::Warning(format!("Missing hook: scripts/hooks/{hook}"))
            } else if std::fs::metadata(&hook_file).map(|m| m.len()).unwrap_or(0) == 0 {
                CheckStatus::Warning(format!("Hook 'scripts/hooks/{hook}' is empty (0 bytes)"))
            } else {
                // Verify tracked executable mode 100755 in git index
                let mut ls_stage_cmd = std::process::Command::new("git");
                ls_stage_cmd.current_dir(repo_path).args([
                    "ls-files",
                    "--stage",
                    &format!("scripts/hooks/{hook}"),
                ]);
                let ls_stage = crate::process::run_bounded(
                    ls_stage_cmd,
                    "git ls-files --stage",
                    crate::process::LOCAL_TIMEOUT,
                )
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();
                let first_word = ls_stage.split_whitespace().next().unwrap_or("");
                if first_word == "100755" {
                    CheckStatus::Pass
                } else if first_word == "100644" {
                    CheckStatus::Warning(format!(
                        "Hook 'scripts/hooks/{hook}' is tracked with non-executable mode 100644 in git index (run 'git update-index --chmod=+x scripts/hooks/{hook}')"
                    ))
                } else if ls_stage.is_empty() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let perms = std::fs::metadata(&hook_file)
                            .map(|m| m.permissions().mode())
                            .unwrap_or(0);
                        if perms & 0o111 != 0 {
                            CheckStatus::Pass
                        } else {
                            CheckStatus::Warning(format!(
                                "Hook 'scripts/hooks/{hook}' is untracked and non-executable on disk"
                            ))
                        }
                    }
                    #[cfg(not(unix))]
                    CheckStatus::Pass
                } else {
                    CheckStatus::Warning(format!(
                        "Hook 'scripts/hooks/{hook}' has unrecognized git index mode '{first_word}'"
                    ))
                }
            };
            reports.push(AuditReport {
                category: "Git Hooks".to_string(),
                name: format!("Hook: {hook}"),
                status,
            });
        }

        // 4. Per-machine sync state must not be committed
        reports.push(AuditReport {
            category: "Repository Baseline".to_string(),
            name: format!("Per-machine sync state ({SYNC_STATE_PATH})"),
            status: sync_state_tracking_status(repo_path),
        });

        reports
    }
}

/// Project-relative path of UDE's per-machine sync ancestor state.
const SYNC_STATE_PATH: &str = ".ude/state.json";

/// Warns when [`SYNC_STATE_PATH`] is tracked by git although the project's ignore
/// rules ignore it: the committed copy makes init on another machine treat every
/// uncommitted managed file as deleted locally. The warning names the remedy.
fn sync_state_tracking_status(repo_path: &Path) -> CheckStatus {
    let tracked = git_exit_flag(
        repo_path,
        &["ls-files", "--error-unmatch", "--", SYNC_STATE_PATH],
        "git ls-files --error-unmatch",
    );
    // `--no-index`: without it check-ignore reports every tracked path as not ignored.
    let ignored = git_exit_flag(
        repo_path,
        &["check-ignore", "-q", "--no-index", "--", SYNC_STATE_PATH],
        "git check-ignore",
    );
    match (tracked, ignored) {
        (Ok(true), Ok(true)) => CheckStatus::Warning(format!(
            "{SYNC_STATE_PATH} is tracked by git but is per-machine sync state that .gitignore ignores; another machine's init would treat uncommitted managed files as deleted (run 'git rm --cached {SYNC_STATE_PATH}', then commit)"
        )),
        (Err(e), _) | (_, Err(e)) => CheckStatus::Warning(format!(
            "Could not check whether {SYNC_STATE_PATH} is tracked and ignored: {e}"
        )),
        _ => CheckStatus::Pass,
    }
}

/// Runs `git <args>` in `repo_path` through the bounded runner and maps exit 0 to
/// `true` and exit 1 to `false`. Any other outcome is an error naming `label`.
fn git_exit_flag(repo_path: &Path, args: &[&str], label: &str) -> Result<bool, String> {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(repo_path).args(args);
    let out = crate::process::run_bounded(cmd, label, crate::process::LOCAL_TIMEOUT)?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        code => Err(format!(
            "{label} exited with {code:?}: {}",
            crate::process::stderr_excerpt(&out)
        )),
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
