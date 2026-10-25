use regex::Regex;
use std::path::{Component, Path};
use std::sync::LazyLock;

/// Result of a sanitization check
#[derive(Debug, PartialEq, Eq)]
pub enum SanitizationResult {
    Clean(String),
    SecretDetected { reason: String, pattern: String },
}

/// One secret-scan hit: the rule that fired and the 1-based line it fired on.
///
/// It never holds the matched value, so a refusal built from it cannot leak the secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule: String,
    pub line: usize,
}

impl Finding {
    /// Refusal text naming the rule and the line, never the matched value.
    pub fn reason(&self) -> String {
        format!(
            "Forbidden secret pattern matched: {} at line {}",
            self.rule, self.line
        )
    }
}

/// The UDE named secret rules, in reporting order (first hit wins).
///
/// Compiled once per process as ASCII-mode byte regexes: `(?i)`, `\b` and every
/// class are ASCII-only, matching Go's and gitleaks' semantics, so a non-ASCII byte
/// next to a token is a word boundary rather than part of the word.
static NAMED_RULES: LazyLock<Vec<(&'static str, regex::bytes::Regex)>> = LazyLock::new(|| {
    [
        // Private SSH, TLS, or PKCS keys (including PKCS8 headers)
        (
            "Private Key",
            r"-----BEGIN (?:[A-Z0-9_-]+ )?PRIVATE KEY-----",
        ),
        // GitHub personal access tokens (classic ghp/gho/ghu/ghs/ghr and fine-grained github_pat)
        (
            "GitHub Personal Access Token",
            r"\b(?:gh[pousr]_[A-Za-z0-9_]{36,255}|github_pat_[A-Za-z0-9_]{50,255})\b",
        ),
        // GitLab personal access tokens
        (
            "GitLab Personal Access Token",
            r"\bglpat-[A-Za-z0-9_\-]{20,}\b",
        ),
        // HTTP Bearer tokens
        ("Bearer Token", r"(?i)\bBearer\s+[A-Za-z0-9_\-\.]{20,}\b"),
        // Generic high-entropy API keys / secrets (quoted and unquoted assignments, including JSON syntax)
        (
            "Generic API Key",
            r#"(?i)["']?(?:api[_-]?key|secret[_-]?key|access[_-]?token|auth[_-]?token)["']?\s*[:=]\s*['"]?[A-Za-z0-9_\-\.]{20,}['"]?"#,
        ),
        // AWS Access Key ID
        (
            "AWS Access Key",
            r"\b(A3T[A-Z0-9]|AKIA|AGPA|AIDA|AROA|AIPA|ANPA|ANVA|ASIA)[A-Z0-9]{16}\b",
        ),
    ]
    .into_iter()
    .map(|(name, pattern)| {
        let regex = regex::bytes::RegexBuilder::new(pattern)
            .unicode(false)
            .build()
            .expect("valid named secret pattern");
        (name, regex)
    })
    .collect()
});

/// 1-based line of byte `offset`: one plus the LF bytes before it.
fn line_at(bytes: &[u8], offset: usize) -> usize {
    1 + bytes[..offset].iter().filter(|&&b| b == b'\n').count()
}

/// The named-rule pass: the first rule in [`NAMED_RULES`] that matches, with the
/// line of its match start.
fn first_named_rule(bytes: &[u8]) -> Option<Finding> {
    NAMED_RULES.iter().find_map(|(name, regex)| {
        regex.find(bytes).map(|m| Finding {
            rule: (*name).to_string(),
            line: line_at(bytes, m.start()),
        })
    })
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

    /// The single secret-scan entry point for every outbound check.
    ///
    /// `bytes` are scanned raw, with no decoding. The raw view is always scanned
    /// first, unchanged. When it holds a NUL byte, a second view with every NUL byte
    /// removed is scanned too: that view is the text of UTF-16LE, UTF-16BE (with or
    /// without a BOM) and UTF-32 ASCII content, keeps every LF byte (so its line
    /// numbers equal the raw view's) and can only add findings, never remove one.
    /// Within a view the first hit wins, in detector order.
    ///
    /// `path` is the publish-relative path, used by path-scoped rules.
    pub fn scan_bytes(&self, bytes: &[u8], path: &Path) -> Option<Finding> {
        if let Some(finding) = self.scan_view(bytes, path) {
            return Some(finding);
        }
        if !bytes.contains(&0) {
            return None;
        }
        let stripped: Vec<u8> = bytes.iter().copied().filter(|&b| b != 0).collect();
        self.scan_view(&stripped, path)
    }

    /// One detector pass over one view of the bytes. The named rules run first.
    fn scan_view(&self, bytes: &[u8], _path: &Path) -> Option<Finding> {
        first_named_rule(bytes)
    }

    /// Parameterizes hardcoded user home and config paths into portable tokens.
    /// Strictly asserts on leading and trailing path boundaries so neighboring paths
    /// (e.g. /home/user2 or /backup/home/user) are never corrupted.
    pub fn parameterize_paths(&self, content: &str, user_home: &str, xdg_config: &str) -> String {
        let mut result = content.to_string();

        if !xdg_config.is_empty() && xdg_config != user_home {
            let pattern = format!(
                r#"(^|[^A-Za-z0-9_.\-]){}([/\\"'\s:]|$)"#,
                regex::escape(xdg_config)
            );
            if let Ok(re) = Regex::new(&pattern) {
                loop {
                    let next = re
                        .replace_all(&result, "${1}{{ user.xdg_config }}$2")
                        .to_string();
                    if next == result {
                        break;
                    }
                    result = next;
                }
            }
        }
        if !user_home.is_empty() {
            let pattern = format!(
                r#"(^|[^A-Za-z0-9_.\-]){}([/\\"'\s:]|$)"#,
                regex::escape(user_home)
            );
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
        let dirty = format!(
            "{b1} {b2}\nMIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC6...\n{e1} {e2}"
        );
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
        let backup_param =
            sanitizer.parameterize_paths(backup_raw, "/home/alice", "/home/alice/.config");
        assert_eq!(backup_param, "Archive is at /backup/home/alice/data.tar");

        let config_raw = "Config in /home/alice/.config/app/conf.toml";
        let config_param =
            sanitizer.parameterize_paths(config_raw, "/home/alice", "/home/alice/.config");
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
        let param_space =
            sanitizer.parameterize_paths(space_separated, "/home/alice", "/home/alice/.config");
        assert_eq!(param_space, "{{ user.home }} {{ user.home }}");

        // Colon separated adjacent occurrences (e.g. in PATH or env vars)
        let colon_separated = "/home/alice:/home/alice";
        let param_colon =
            sanitizer.parameterize_paths(colon_separated, "/home/alice", "/home/alice/.config");
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

    /// Encodes `text` as UTF-16, optionally preceded by `bom`, with each unit
    /// serialized by `unit_bytes` (little- or big-endian).
    fn utf16_bytes(text: &str, bom: &[u8], unit_bytes: fn(u16) -> [u8; 2]) -> Vec<u8> {
        let mut out = bom.to_vec();
        for unit in text.encode_utf16() {
            out.extend_from_slice(&unit_bytes(unit));
        }
        out
    }

    #[test]
    fn test_scan_bytes_non_utf8() {
        let sanitizer = Sanitizer::new();
        let path = Path::new("scripts/hooks/pre-push");
        // 0xFF never occurs in UTF-8, so these bytes are invalid UTF-8 by definition
        // (rustc's invalid_from_utf8 lint rejects asserting it on a literal).
        let invalid: &[u8] = &[0xFF, 0xFE, 0xFD, 0x80];

        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        let mut dirty = invalid.to_vec();
        dirty.push(b'\n');
        dirty.extend_from_slice(format!("{prefix}{body}").as_bytes());

        assert_eq!(
            sanitizer.scan_bytes(&dirty, path),
            Some(Finding {
                rule: "GitHub Personal Access Token".to_string(),
                line: 2,
            })
        );
        // Negative control: the invalid bytes alone hold no token.
        assert_eq!(sanitizer.scan_bytes(invalid, path), None);
    }

    #[test]
    fn test_scan_bytes_reports_rule_and_line_without_value() {
        let sanitizer = Sanitizer::new();
        let path = Path::new("AGENTS.md");
        let prefix = "glpat-";
        let body = "0123456789abcdefghij";
        let dirty = format!("first line\nsecond line\nGITLAB_TOKEN={prefix}{body}\n");

        let finding = sanitizer
            .scan_bytes(dirty.as_bytes(), path)
            .expect("GitLab token on line 3 must be found");
        assert_eq!(
            finding,
            Finding {
                rule: "GitLab Personal Access Token".to_string(),
                line: 3,
            }
        );
        let reason = finding.reason();
        assert!(reason.contains("GitLab Personal Access Token"), "{reason}");
        assert!(reason.contains("at line 3"), "{reason}");
        assert!(!reason.contains(body), "reason leaked the token body");

        // Negative control: prose with no run of 20 or more candidate characters.
        let benign = "just some plain words\nanother short line\nand a third one\n";
        assert_eq!(sanitizer.scan_bytes(benign.as_bytes(), path), None);
    }

    #[test]
    fn test_scan_bytes_utf16() {
        let sanitizer = Sanitizer::new();
        let path = Path::new("AGENTS.md");
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        let text = format!("x\nexport TOKEN={prefix}{body}\n");
        let expected = Some(Finding {
            rule: "GitHub Personal Access Token".to_string(),
            line: 2,
        });

        let le_bom = utf16_bytes(&text, &[0xFF, 0xFE], u16::to_le_bytes);
        let be_bom = utf16_bytes(&text, &[0xFE, 0xFF], u16::to_be_bytes);
        let le_plain = utf16_bytes(&text, &[], u16::to_le_bytes);
        assert_eq!(
            sanitizer.scan_bytes(&le_bom, path),
            expected,
            "UTF-16LE + BOM"
        );
        assert_eq!(
            sanitizer.scan_bytes(&be_bom, path),
            expected,
            "UTF-16BE + BOM"
        );
        assert_eq!(
            sanitizer.scan_bytes(&le_plain, path),
            expected,
            "UTF-16LE, no BOM"
        );

        // Negative control 1 (the escape the NUL-stripped view closes): the named
        // rules alone see nothing in the raw, NUL-interleaved bytes.
        assert_eq!(first_named_rule(&le_bom), None);

        // Negative control 2: benign UTF-16 text scans clean.
        let benign = utf16_bytes("just some plain words\n", &[0xFF, 0xFE], u16::to_le_bytes);
        assert_eq!(sanitizer.scan_bytes(&benign, path), None);
    }
}
