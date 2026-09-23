use regex::Regex;
use std::path::Path;

/// Result of a sanitization check
#[derive(Debug, PartialEq, Eq)]
pub enum SanitizationResult {
    Clean(String),
    SecretDetected { reason: String, pattern: String },
}

pub struct Sanitizer {
    secret_patterns: Vec<(&'static str, Regex)>,
}

impl Default for Sanitizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Sanitizer {
    pub fn new() -> Self {
        let patterns = vec![
            // Private SSH keys
            ("Private SSH Key", Regex::new(r"-----BEGIN [A-Z0-9_-]+ PRIVATE KEY-----").unwrap()),
            // GitHub personal access tokens
            ("GitHub Personal Access Token", Regex::new(r"gh[pousr]_[A-Za-z0-9_]{36,255}").unwrap()),
            // Generic high-entropy API keys
            ("Generic API Key", Regex::new(r#"(?i)(api[_-]?key|secret[_-]?key|bearer)\s*[:=]\s*['"][A-Za-z0-9_\-.]{20,}['"]"#).unwrap()),
            // AWS Access Key ID
            ("AWS Access Key", Regex::new(r"(A3T[A-Z0-9]|AKIA|AGPA|AIDA|AROA|AIPA|ANPA|ANVA|ASIA)[A-Z0-9]{16}").unwrap()),
        ];
        Self {
            secret_patterns: patterns,
        }
    }

    /// Checks content for secrets and credentials. Rejects if any match.
    pub fn check_secrets(&self, content: &str) -> SanitizationResult {
        for (name, regex) in &self.secret_patterns {
            if regex.is_match(content) {
                return SanitizationResult::SecretDetected {
                    reason: format!("Forbidden secret pattern matched: {name}"),
                    pattern: name.to_string(),
                };
            }
        }
        SanitizationResult::Clean(content.to_string())
    }

    /// Parameterizes hardcoded user home and config paths into portable tokens
    pub fn parameterize_paths(&self, content: &str, user_home: &str, xdg_config: &str) -> String {
        let mut result = content.to_string();

        if !xdg_config.is_empty() && xdg_config != user_home {
            result = result.replace(xdg_config, "{{ xdg_config }}");
        }
        if !user_home.is_empty() {
            result = result.replace(user_home, "{{ user_home }}");
        }
        result
    }

    /// Asserts whether a path is safe to publish or belongs to the local-only contribution boundary
    pub fn is_local_only_path(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        let path_norm = path_str.trim_start_matches("./");

        path_norm == ".agents"
            || path_norm.starts_with(".agents/")
            || path_norm == ".planning"
            || path_norm.starts_with(".planning/")
            || path_norm == "scratch"
            || path_norm.starts_with("scratch/")
            || path_norm == "mise.local.toml"
            || path_norm.ends_with(".local.md")
            || path_norm.ends_with(".local.toml")
            || path_norm.ends_with(".local")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_detection_github_token() {
        let sanitizer = Sanitizer::new();
        // Dynamically assembled fixture so git-commit secret scanners don't flag static unit test literals
        let token_prefix = "ghp_";
        let token_body = "111122223333444455556666777788889999";
        let dirty = format!("export GITHUB_TOKEN={token_prefix}{token_body}");
        match sanitizer.check_secrets(&dirty) {
            SanitizationResult::SecretDetected { pattern, .. } => {
                assert_eq!(pattern, "GitHub Personal Access Token");
            }
            SanitizationResult::Clean(_) => panic!("Expected secret detection!"),
        }
    }

    #[test]
    fn test_secret_detection_ssh_private_key() {
        let sanitizer = Sanitizer::new();
        let dirty = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAA...\n-----END OPENSSH PRIVATE KEY-----";
        match sanitizer.check_secrets(dirty) {
            SanitizationResult::SecretDetected { pattern, .. } => {
                assert_eq!(pattern, "Private SSH Key");
            }
            SanitizationResult::Clean(_) => panic!("Expected private key detection!"),
        }
    }

    #[test]
    fn test_path_parameterization() {
        let sanitizer = Sanitizer::new();
        let raw = "Config loaded from /home/denniyahh/.config/git/allowed_signers for /home/denniyahh";
        let parameterized = sanitizer.parameterize_paths(
            raw,
            "/home/denniyahh",
            "/home/denniyahh/.config",
        );
        assert_eq!(
            parameterized,
            "Config loaded from {{ xdg_config }}/git/allowed_signers for {{ user_home }}"
        );
    }

    #[test]
    fn test_local_only_boundary() {
        let sanitizer = Sanitizer::new();
        assert!(sanitizer.is_local_only_path(Path::new(".agents/AGENTS.md")));
        assert!(sanitizer.is_local_only_path(Path::new("scratch/ci-mac.sh")));
        assert!(sanitizer.is_local_only_path(Path::new("CLAUDE.local.md")));
        assert!(!sanitizer.is_local_only_path(Path::new("src/main.rs")));
        assert!(!sanitizer.is_local_only_path(Path::new("Cargo.toml")));
    }
}
