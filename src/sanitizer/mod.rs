use regex::Regex;
use std::path::{Component, Path};

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
            // Private SSH, TLS, or PKCS keys (including PKCS8 headers)
            (
                "Private Key",
                Regex::new(r"-----BEGIN (?:[A-Z0-9_-]+ )?PRIVATE KEY-----").unwrap(),
            ),
            // GitHub personal access tokens (classic ghp/gho/ghu/ghs/ghr and fine-grained github_pat)
            (
                "GitHub Personal Access Token",
                Regex::new(r"\b(?:gh[pousr]_[A-Za-z0-9_]{36,255}|github_pat_[A-Za-z0-9_]{50,255})\b").unwrap(),
            ),
            // GitLab personal access tokens
            (
                "GitLab Personal Access Token",
                Regex::new(r"\bglpat-[A-Za-z0-9_\-]{20,}\b").unwrap(),
            ),
            // HTTP Bearer tokens
            (
                "Bearer Token",
                Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9_\-\.]{20,}\b").unwrap(),
            ),
            // Generic high-entropy API keys / secrets (quoted and unquoted assignments, including JSON syntax)
            (
                "Generic API Key",
                Regex::new(r#"(?i)["']?(?:api[_-]?key|secret[_-]?key|access[_-]?token|auth[_-]?token)["']?\s*[:=]\s*['"]?[A-Za-z0-9_\-\.]{20,}['"]?"#).unwrap(),
            ),
            // AWS Access Key ID
            (
                "AWS Access Key",
                Regex::new(r"\b(A3T[A-Z0-9]|AKIA|AGPA|AIDA|AROA|AIPA|ANPA|ANVA|ASIA)[A-Z0-9]{16}\b").unwrap(),
            ),
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

    /// Parameterizes hardcoded user home and config paths into portable tokens.
    /// Strictly asserts on leading and trailing path boundaries so neighboring paths
    /// (e.g. /home/user2 or /backup/home/user) are never corrupted.
    pub fn parameterize_paths(&self, content: &str, user_home: &str, xdg_config: &str) -> String {
        let mut result = content.to_string();

        if !xdg_config.is_empty() && xdg_config != user_home {
            let pattern = format!(r#"(^|[^A-Za-z0-9_.\-]){}([/\\"'\s:]|$)"#, regex::escape(xdg_config));
            if let Ok(re) = Regex::new(&pattern) {
                loop {
                    let next = re.replace_all(&result, "${1}{{ user.xdg_config }}$2").to_string();
                    if next == result {
                        break;
                    }
                    result = next;
                }
            }
        }
        if !user_home.is_empty() {
            let pattern = format!(r#"(^|[^A-Za-z0-9_.\-]){}([/\\"'\s:]|$)"#, regex::escape(user_home));
            if let Ok(re) = Regex::new(&pattern) {
                loop {
                    let next = re.replace_all(&result, "${1}{{ user.home }}$2").to_string();
                    if next == result {
                        break;
                    }
                    result = next;
                }
            }
        }
        result
    }

    /// Asserts whether a path is safe to publish or belongs to the local-only contribution boundary.
    /// When `allow_agent_rules` is true, canonical agent rule templates (e.g. `.agents/AGENTS.md`)
    /// are permitted for central template updates.
    pub fn is_local_only_path(&self, path: &Path, allow_agent_rules: bool) -> bool {
        let mut normalized_parts: Vec<String> = Vec::new();
        for component in path.components() {
            match component {
                Component::Normal(c) => normalized_parts.push(c.to_string_lossy().to_string()),
                Component::CurDir => {}
                Component::ParentDir => {
                    normalized_parts.pop();
                }
                _ => {}
            }
        }

        if normalized_parts.is_empty() {
            return false;
        }

        let first = &normalized_parts[0];
        let full_rel = normalized_parts.join("/");

        if first == ".planning"
            || first == "scratch"
            || first == ".local"
            || full_rel == "mise.local.toml"
            || full_rel.ends_with(".local.md")
            || full_rel.ends_with(".local.toml")
            || full_rel.ends_with(".local")
        {
            return true;
        }

        if first == ".agents" {
            if allow_agent_rules && full_rel == ".agents/AGENTS.md" {
                return false;
            }
            return true;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_detection_github_token() {
        let sanitizer = Sanitizer::new();
        // Dynamically assembled fixtures so host git pre-commit hooks (gitleaks) don't trigger on test source
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
    fn test_secret_detection_fine_grained_pat() {
        let sanitizer = Sanitizer::new();
        let pfx = "github_pat_";
        let part1 = "11AABBCCDDEEFF0011223344";
        let part2 = "5566778899AABBCCDDEEFF00";
        let part3 = "112233445566778899AABBCCDDEEFF00112233";
        let dirty = format!("token = '{pfx}{part1}{part2}{part3}'");
        match sanitizer.check_secrets(&dirty) {
            SanitizationResult::SecretDetected { pattern, .. } => {
                assert_eq!(pattern, "GitHub Personal Access Token");
            }
            SanitizationResult::Clean(_) => panic!("Expected fine-grained GitHub PAT detection!"),
        }
    }

    #[test]
    fn test_secret_detection_gitlab_pat() {
        let sanitizer = Sanitizer::new();
        let pfx = "glpat-";
        let p1 = "0123456789";
        let p2 = "abcdefghij";
        let dirty = format!("GITLAB_TOKEN={pfx}{p1}{p2}");
        match sanitizer.check_secrets(&dirty) {
            SanitizationResult::SecretDetected { pattern, .. } => {
                assert_eq!(pattern, "GitLab Personal Access Token");
            }
            SanitizationResult::Clean(_) => panic!("Expected GitLab PAT detection!"),
        }
    }

    #[test]
    fn test_secret_detection_pkcs8_private_key() {
        let sanitizer = Sanitizer::new();
        let b1 = "-----BEGIN";
        let b2 = "PRIVATE KEY-----";
        let e1 = "-----END";
        let e2 = "PRIVATE KEY-----";
        let dirty = format!("{b1} {b2}\nMIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC6...\n{e1} {e2}");
        match sanitizer.check_secrets(&dirty) {
            SanitizationResult::SecretDetected { pattern, .. } => {
                assert_eq!(pattern, "Private Key");
            }
            SanitizationResult::Clean(_) => panic!("Expected PKCS#8 private key detection!"),
        }
    }

    #[test]
    fn test_secret_detection_bearer_and_unquoted_key() {
        let sanitizer = Sanitizer::new();
        let b_hdr = "Bearer";
        let b_jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0";
        let bearer_dirty = format!("Authorization: {b_hdr} {b_jwt}");
        match sanitizer.check_secrets(&bearer_dirty) {
            SanitizationResult::SecretDetected { pattern, .. } => {
                assert_eq!(pattern, "Bearer Token");
            }
            SanitizationResult::Clean(_) => panic!("Expected Bearer token detection!"),
        }

        let k_name = "api_key";
        let k_val_p1 = "sk-proj-";
        let k_val_p2 = "1234567890abcdef1234";
        let unquoted_dirty = format!("{k_name}={k_val_p1}{k_val_p2}");
        match sanitizer.check_secrets(&unquoted_dirty) {
            SanitizationResult::SecretDetected { pattern, .. } => {
                assert_eq!(pattern, "Generic API Key");
            }
            SanitizationResult::Clean(_) => panic!("Expected unquoted API key detection!"),
        }
    }

    #[test]
    fn test_secret_detection_negative_control() {
        let sanitizer = Sanitizer::new();
        let benign = "export APP_ENV=production\nexport PORT=8080\n# Public documentation link: https://api.github.com";
        match sanitizer.check_secrets(benign) {
            SanitizationResult::Clean(content) => {
                assert_eq!(content, benign);
            }
            SanitizationResult::SecretDetected { reason, .. } => {
                panic!("False positive on benign content: {reason}");
            }
        }
    }

    #[test]
    fn test_secret_detection_json_key() {
        let sanitizer = Sanitizer::new();
        let k = "api_key";
        let v1 = "abcdefghijkl";
        let v2 = "mnopqrstuvwx";
        let json_dirty = format!(r#"{{"{k}":"{v1}{v2}"}}"#);
        match sanitizer.check_secrets(&json_dirty) {
            SanitizationResult::SecretDetected { pattern, .. } => {
                assert_eq!(pattern, "Generic API Key");
            }
            SanitizationResult::Clean(_) => panic!("Expected JSON secret detection!"),
        }
    }

    #[test]
    fn test_path_parameterization_boundary_safety() {
        let sanitizer = Sanitizer::new();
        // Negative control: Neighboring path /home/alice2 must NOT be corrupted when home is /home/alice
        let raw = "User home is /home/alice and neighboring user is /home/alice2/data";
        let parameterized = sanitizer.parameterize_paths(raw, "/home/alice", "/home/alice/.config");
        assert_eq!(
            parameterized,
            "User home is {{ user.home }} and neighboring user is /home/alice2/data"
        );

        // Negative control: Preceding path /backup/home/alice must NOT be corrupted when home is /home/alice
        let backup_raw = "Archive is at /backup/home/alice/data.tar";
        let backup_param = sanitizer.parameterize_paths(backup_raw, "/home/alice", "/home/alice/.config");
        assert_eq!(
            backup_param,
            "Archive is at /backup/home/alice/data.tar"
        );

        let config_raw = "Config in /home/alice/.config/app/conf.toml";
        let config_param = sanitizer.parameterize_paths(config_raw, "/home/alice", "/home/alice/.config");
        assert_eq!(
            config_param,
            "Config in {{ user.xdg_config }}/app/conf.toml"
        );
    }

    #[test]
    fn test_path_parameterization_repeated_adjacent_paths() {
        let sanitizer = Sanitizer::new();
        // Space separated adjacent occurrences
        let space_separated = "/home/alice /home/alice";
        let param_space = sanitizer.parameterize_paths(space_separated, "/home/alice", "/home/alice/.config");
        assert_eq!(param_space, "{{ user.home }} {{ user.home }}");

        // Colon separated adjacent occurrences (e.g. in PATH or env vars)
        let colon_separated = "/home/alice:/home/alice";
        let param_colon = sanitizer.parameterize_paths(colon_separated, "/home/alice", "/home/alice/.config");
        assert_eq!(param_colon, "{{ user.home }}:{{ user.home }}");
    }

    #[test]
    fn test_local_only_boundary() {
        let sanitizer = Sanitizer::new();
        // By default without agent rules allowed:
        assert!(sanitizer.is_local_only_path(Path::new(".agents/AGENTS.md"), false));
        // With agent rules allowed for canonical template sync:
        assert!(!sanitizer.is_local_only_path(Path::new(".agents/AGENTS.md"), true));

        // Scratch and planning are always blocked
        assert!(sanitizer.is_local_only_path(Path::new("scratch/ci-mac.sh"), true));
        assert!(sanitizer.is_local_only_path(Path::new(".planning/STATE.md"), true));
        assert!(sanitizer.is_local_only_path(Path::new("CLAUDE.local.md"), true));

        // Path traversal attempts are blocked
        assert!(sanitizer.is_local_only_path(Path::new("subdir/../scratch/leak.sh"), false));

        // Generic repo files are publishable
        assert!(!sanitizer.is_local_only_path(Path::new("src/main.rs"), false));
        assert!(!sanitizer.is_local_only_path(Path::new("Cargo.toml"), false));
    }
}
