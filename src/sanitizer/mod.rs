pub mod entropy;
pub mod gitleaks;

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

/// Whether a path match may start here. `prev` is the char directly before the
/// match and `before_prev` the one before that (`None` at the start of the text).
///
/// The start of the text is a boundary. A `-` is one only directly after `:`, the
/// shell default `${VAR:-...}`, so `/backup-/home/alice` and `x-/home/alice` are
/// not. Any other char is a boundary unless it is alphanumeric (Unicode), `_`
/// or `.`.
fn is_leading_boundary(prev: Option<char>, before_prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some('-') => before_prev == Some(':'),
        Some(c) => !(c.is_alphanumeric() || c == '_' || c == '.'),
    }
}

/// Whether a path match may end here; `rest` is the text after the match.
///
/// The end of the text is a boundary. A `.` is one only when the text ends after
/// it or the next char is not alphanumeric, `_`, `-`, `.` or `/` (a
/// sentence-final `.`, not `.bak`, `..bak` or `./x`, which name other
/// directories). Any other char is a boundary unless it is alphanumeric
/// (Unicode), `_` or `-`.
fn is_trailing_boundary(rest: &str) -> bool {
    let mut chars = rest.chars();
    match chars.next() {
        None => true,
        Some('.') => chars
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/')),
        Some(c) => !(c.is_alphanumeric() || c == '_' || c == '-'),
    }
}

/// Whether `path` names a real directory to parameterize: absolute, with at least
/// one normal component.
///
/// This rejects the empty string, `.` (the fallback `src/main.rs` and
/// `TemplateContext::detect_for_project` use when no home is found), a relative
/// path, and `/` or `//` (`HOME=/` in a container), any of which would rewrite
/// text that is not the pusher's home.
fn is_parameterizable_root(path: &str) -> bool {
    let path = Path::new(path);
    path.is_absolute()
        && path
            .components()
            .any(|component| matches!(component, Component::Normal(_)))
}

/// Removes trailing path separators (`std::path::is_separator`) from a home or
/// XDG config root; returns `path` unchanged when nothing would remain.
///
/// dirs-sys 0.5.0 returns `$HOME` as given and directories 6.0.0 takes
/// `XDG_CONFIG_HOME` as given, so a trailing separator reaches every consumer.
/// A needle ending in `/` never meets `replace_bounded`'s trailing boundary
/// (`/home/alice/x` has a word char after it, `(/home/alice)` lacks the `/`), so
/// an untrimmed root parameterizes nothing (review CR-01).
///
/// A root made only of separators (`/`, `//`) is returned as is, so
/// `is_parameterizable_root` still rejects it instead of seeing the empty string.
/// `push_updates`, `Sanitizer::parameterize_paths` and
/// `TemplateContext::detect_for_project` share this helper, so rendering and
/// parameterization use one spelling of each root. Only trailing separators are
/// normalized: no `.` components, no repeated inner separators, no symlink
/// resolution.
pub fn normalize_root(path: &str) -> &str {
    match path.trim_end_matches(std::path::is_separator) {
        "" => path,
        trimmed => trimmed,
    }
}

/// Replaces each occurrence of `needle` in `content` whose leading and trailing
/// boundaries both hold with `placeholder`, in one left-to-right pass over
/// `str::match_indices`. Everything else is copied through unchanged.
///
/// Boundaries are read from `content`, never from the output, so a replacement
/// cannot create a boundary for the next occurrence.
fn replace_bounded(content: &str, needle: &str, placeholder: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut copied = 0;
    for (start, _) in content.match_indices(needle) {
        let end = start + needle.len();
        let mut before = content[..start].chars().rev();
        let prev = before.next();
        let before_prev = before.next();
        if is_leading_boundary(prev, before_prev) && is_trailing_boundary(&content[end..]) {
            out.push_str(&content[copied..start]);
            out.push_str(placeholder);
            copied = end;
        }
    }
    out.push_str(&content[copied..]);
    out
}

/// Secret scanning, home-path parameterization and the local-only path boundary.
///
/// Holds no state: the secret rules are process-wide statics compiled once.
pub struct Sanitizer;

impl Default for Sanitizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Sanitizer {
    pub fn new() -> Self {
        Self
    }

    /// Checks text for secrets and credentials; a thin wrapper over [`Self::scan_bytes`]
    /// for callers that hold a `&str` (the pre-API AGENTS.md scan).
    ///
    /// It passes an empty path, so path-scoped rules never fire through it.
    pub fn check_secrets(&self, content: &str) -> SanitizationResult {
        match self.scan_bytes(content.as_bytes(), Path::new("")) {
            None => SanitizationResult::Clean(content.to_string()),
            Some(finding) => SanitizationResult::SecretDetected {
                reason: finding.reason(),
                pattern: finding.rule,
            },
        }
    }

    /// The single secret-scan entry point for every outbound check.
    ///
    /// `bytes` are scanned raw, with no decoding. The raw view is always scanned
    /// first, unchanged. When it holds a NUL byte, a second view with every NUL byte
    /// removed is scanned too: that view is the text of UTF-16LE, UTF-16BE (with or
    /// without a BOM) and UTF-32 ASCII content, keeps every LF byte (so its line
    /// numbers equal the raw view's) and can only add findings, never remove one.
    /// Within a view the first hit wins, in detector order: the named rules, the
    /// vendored gitleaks rules, then R11.
    ///
    /// `path` is the publish-relative path, used by the gitleaks path-scoped rules.
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

    /// One detector pass over one view of the bytes, in order: the named rules (so
    /// a token they match keeps their rule name), then the vendored gitleaks rules
    /// (with `path` for their path-scoped rules), then the R11 entropy screen.
    fn scan_view(&self, bytes: &[u8], path: &Path) -> Option<Finding> {
        if let Some(finding) = first_named_rule(bytes) {
            return Some(finding);
        }
        if let Some(finding) = gitleaks::vendored().first_finding(bytes, path) {
            return Some(finding);
        }
        if let Some(line) = entropy::first_high_entropy_line(bytes) {
            return Some(Finding {
                rule: entropy::RULE_NAME.to_string(),
                line,
            });
        }
        None
    }

    /// Replaces the pusher's XDG config path with `{{ user.xdg_config }}` and home
    /// path with `{{ user.home }}`, the two placeholders push publishes (SEC-03).
    ///
    /// The XDG path goes first, and only when it is non-empty, differs from the
    /// home path and is a parameterizable root; then the home path, when it is one
    /// (`is_parameterizable_root`: an empty, `.`, relative, `/` or `//` path is
    /// never parameterized). An occurrence is replaced only when both
    /// boundaries hold (`is_leading_boundary`, `is_trailing_boundary`), so
    /// `(`, `)`, `,`, `;`, `}`, quotes, whitespace, `/`, `:`, `:-` and a
    /// sentence-final `.` delimit a path, while a word char, `.word`, `..`, `./` or
    /// a `-` that is not part of `:-` blocks it: `/home/alice2`, `/home/alice.bak`,
    /// `/home/alice./x` and `/backup-/home/alice` are never rewritten. Each pass
    /// is one left-to-right scan (`replace_bounded`) with no regex built from the
    /// path.
    ///
    /// Matching is exact substring equality on the UTF-8 text, with no Unicode
    /// normalization: an NFD spelling of an NFC home path is not matched. The
    /// boundary table is `tests::test_path_boundary_table`.
    pub fn parameterize_paths(&self, content: &str, user_home: &str, xdg_config: &str) -> String {
        let mut result = content.to_string();
        if !xdg_config.is_empty() && xdg_config != user_home && is_parameterizable_root(xdg_config)
        {
            result = replace_bounded(&result, xdg_config, "{{ user.xdg_config }}");
        }
        if is_parameterizable_root(user_home) {
            result = replace_bounded(&result, user_home, "{{ user.home }}");
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
        // JWT assembled at runtime from separately bound parts (header split too), so
        // no whole token or header occurs as a literal in this file.
        let jwt_header_p1 = "eyJhbGciOiJIUzI1NiIs";
        let jwt_header_p2 = "InR5cCI6IkpXVCJ9";
        let jwt_payload = "eyJzdWIiOiIxMjM0NTY3ODkwIn0";
        let jwt_signature = "dGVzdC1zaWduYXR1cmU";
        let b_jwt = format!("{jwt_header_p1}{jwt_header_p2}.{jwt_payload}.{jwt_signature}");
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

    /// The SEC-03 boundary table for home `/home/alice` (XDG `/home/alice/.config`):
    /// every row is checked and every failing row is named in the one report.
    #[test]
    fn test_path_boundary_table() {
        let sanitizer = Sanitizer::new();
        let home = "/home/alice";
        let xdg = "/home/alice/.config";
        let rows: &[(&str, &str)] = &[
            // Replaced: both boundaries hold.
            ("cd /home/alice now", "cd {{ user.home }} now"),
            ("(/home/alice)", "({{ user.home }})"),
            ("${H:-/home/alice}", "${H:-{{ user.home }}}"),
            (
                "${W:-/home/alice/Github/x}",
                "${W:-{{ user.home }}/Github/x}",
            ),
            ("see /home/alice.", "see {{ user.home }}."),
            ("see /home/alice. Then", "see {{ user.home }}. Then"),
            ("/home/alice. next", "{{ user.home }}. next"),
            ("/home/alice.)", "{{ user.home }}.)"),
            ("/home/alice.\n", "{{ user.home }}.\n"),
            ("/home/alice,", "{{ user.home }},"),
            ("a;/home/alice;b", "a;{{ user.home }};b"),
            ("/home/alice}", "{{ user.home }}}"),
            ("file:///home/alice/x", "file://{{ user.home }}/x"),
            ("\"/home/alice\"", "\"{{ user.home }}\""),
            ("/home/alice:/home/alice", "{{ user.home }}:{{ user.home }}"),
            // Untouched: a word char, `.word` or `-` neighbour blocks the match.
            ("/home/alice2", "/home/alice2"),
            ("/home/alice.bak", "/home/alice.bak"),
            ("/home/alice._x", "/home/alice._x"),
            ("/home/alice.-x", "/home/alice.-x"),
            // A `.` followed by `.` or `/` names another directory (`alice..bak`,
            // `alice.`), so it is not a sentence-final `.`.
            ("/home/alice..bak", "/home/alice..bak"),
            ("/home/alice./x", "/home/alice./x"),
            ("/home/alice.../y", "/home/alice.../y"),
            ("/home/alice-old", "/home/alice-old"),
            ("/home/alice_x", "/home/alice_x"),
            ("/backup/home/alice", "/backup/home/alice"),
            ("./home/alice", "./home/alice"),
            ("/backup-/home/alice/x", "/backup-/home/alice/x"),
            ("x-/home/alice", "x-/home/alice"),
            // The first occurrence is bounded; the second follows a word char.
            ("/home/alice/home/alice", "{{ user.home }}/home/alice"),
        ];
        let failures: Vec<String> = rows
            .iter()
            .filter_map(|(input, expected)| {
                let actual = sanitizer.parameterize_paths(input, home, xdg);
                (actual != *expected)
                    .then(|| format!("row {input:?}: expected {expected:?}, got {actual:?}"))
            })
            .collect();
        assert!(
            failures.is_empty(),
            "{} of {} boundary rows failed:\n{}",
            failures.len(),
            rows.len(),
            failures.join("\n")
        );
    }

    /// Trailing separators are trimmed from a root; a root made only of
    /// separators, and anything with no trailing separator, is returned as is.
    #[test]
    fn test_normalize_root() {
        #[allow(unused_mut)]
        let mut rows: Vec<(&str, &str)> = vec![
            ("/home/alice/", "/home/alice"),
            ("/home/alice//", "/home/alice"),
            ("/home/alice", "/home/alice"),
            // A root of only separators is kept, so is_parameterizable_root
            // still rejects it instead of seeing the empty string.
            ("/", "/"),
            ("//", "//"),
            ("///", "///"),
            (".", "."),
            ("", ""),
            // Still relative after trimming, so still rejected.
            ("home/alice/", "home/alice"),
        ];
        // On Unix a backslash is a file-name character, not a separator.
        #[cfg(unix)]
        rows.push(("/home/alice\\", "/home/alice\\"));
        let failures: Vec<String> = rows
            .iter()
            .filter_map(|(input, expected)| {
                let actual = normalize_root(input);
                (actual != *expected)
                    .then(|| format!("row {input:?}: expected {expected:?}, got {actual:?}"))
            })
            .collect();
        assert!(
            failures.is_empty(),
            "{} of {} normalize_root rows failed:\n{}",
            failures.len(),
            rows.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn test_degenerate_home_not_parameterized() {
        let sanitizer = Sanitizer::new();
        let xdg = "/home/alice/.config";
        let content = "/etc/x /home/alice/y ./z";
        // The `.` fallback when no home is found, HOME=/ in a container, an empty
        // or relative home: none of them is a path to parameterize.
        for home in ["/", "//", ".", "", "home/alice"] {
            assert_eq!(
                sanitizer.parameterize_paths(content, home, xdg),
                content,
                "home {home:?} rewrote the text"
            );
        }

        // A degenerate XDG path is ignored the same way; the home pass still runs.
        let with_relative = "/etc/x /home/alice/y ./z =home/alice/.config/q";
        let home_only = "/etc/x {{ user.home }}/y ./z =home/alice/.config/q";
        for xdg in ["/", "//", ".", "home/alice/.config"] {
            assert_eq!(
                sanitizer.parameterize_paths(with_relative, "/home/alice", xdg),
                home_only,
                "xdg {xdg:?} rewrote the text"
            );
        }

        // Positive control: a real home path is still parameterized.
        assert_eq!(
            sanitizer.parameterize_paths(content, "/home/alice", xdg),
            "/etc/x {{ user.home }}/y ./z"
        );
    }

    #[test]
    fn test_path_encoding_rows() {
        let sanitizer = Sanitizer::new();
        let home = "/home/alice";
        let xdg = "/home/alice/.config";
        // The boundary test reads the neighbouring code point with char::is_alphanumeric.
        assert_eq!(
            sanitizer.parameterize_paths("/home/alice\u{e9}", home, xdg),
            "/home/alice\u{e9}"
        );
        assert_eq!(
            sanitizer.parameterize_paths("/home/alice\u{2192}x", home, xdg),
            "{{ user.home }}\u{2192}x"
        );
        assert_eq!(
            sanitizer.parameterize_paths("\u{e9}/home/alice", home, xdg),
            "\u{e9}/home/alice"
        );
        assert_eq!(sanitizer.parameterize_paths("", home, xdg), "");

        // Exact code points, no Unicode normalization: an NFD spelling of an NFC
        // home path is not matched (documented behaviour).
        let nfc_home = "/home/jos\u{e9}";
        let nfc_xdg = "/home/jos\u{e9}/.config";
        let nfd_text = "/home/jose\u{301}/x";
        assert_eq!(
            sanitizer.parameterize_paths(nfd_text, nfc_home, nfc_xdg),
            nfd_text
        );
        // Control: the NFC spelling of the same path is replaced.
        assert_eq!(
            sanitizer.parameterize_paths("/home/jos\u{e9}/x", nfc_home, nfc_xdg),
            "{{ user.home }}/x"
        );
    }

    /// A template context whose home and XDG config paths are `home` and `xdg`,
    /// built by hand so no git or environment lookup runs.
    fn home_context(home: &str, xdg: &str) -> crate::template::TemplateContext {
        crate::template::TemplateContext {
            user: crate::template::UserContext {
                home: home.to_string(),
                xdg_config: xdg.to_string(),
                xdg_data: format!("{home}/.local/share"),
            },
            user_home: home.to_string(),
            xdg_config: xdg.to_string(),
            xdg_data: format!("{home}/.local/share"),
            git_user_name: "Alice".to_string(),
            git_email: "alice@example.com".to_string(),
            project_name: "sample-proj".to_string(),
            project_type: "generic".to_string(),
            maintainer_ssh_pubkey: None,
            agent_ssh_pubkey: None,
            extra: std::collections::HashMap::new(),
        }
    }

    /// One piece of a round-trip text: `kind` picks the shape, `word` and
    /// `delimiter` fill it.
    fn roundtrip_piece(kind: u8, word: &str, delimiter: char, home: &str, xdg: &str) -> String {
        match kind {
            0 => home.to_string(),
            1 => xdg.to_string(),
            2 => format!("{home}/{word}"),
            3 => format!("{xdg}/{word}"),
            4 => word.to_string(),
            5 => delimiter.to_string(),
            6 => "2".to_string(),
            _ => ".bak".to_string(),
        }
    }

    proptest::proptest! {
        #[test]
        fn prop_no_partial_substring_corruption(
            name in "[a-z][a-z0-9]{0,7}",
            word_suffix in proptest::prop_oneof![
                "[A-Za-z0-9_-][A-Za-z0-9_./-]{0,10}",
                r"\.[A-Za-z0-9][A-Za-z0-9_.-]{0,8}",
            ],
            delimiter_suffix in r#"[ )\],;:"'}]"#,
            word_prefix in r"[A-Za-z0-9_]{1,6}[-.]?",
            delimiter_prefix in r#"[a-z]{0,4}( |\(|=|"|:-)"#,
        ) {
            let sanitizer = Sanitizer::new();
            let home = format!("/home/{name}");
            let xdg = format!("{home}/.config");

            // Trailing side: a word char, `_`, `-` or `.word` after the home path
            // makes it a different path, which stays byte-identical.
            let glued_after = format!("{home}{word_suffix}");
            proptest::prop_assert_eq!(
                sanitizer.parameterize_paths(&glued_after, &home, &xdg),
                glued_after
            );
            // Companion: a delimiter after it is a boundary.
            let delimited_after = format!("{home}{delimiter_suffix}");
            let out = sanitizer.parameterize_paths(&delimited_after, &home, &xdg);
            proptest::prop_assert!(out.starts_with("{{ user.home }}"), "{:?} -> {:?}", delimited_after, out);

            // Leading side: a word char, `_`, `.`, or a `-` after one, before it.
            let glued_before = format!("{word_prefix}{home}/x");
            proptest::prop_assert_eq!(
                sanitizer.parameterize_paths(&glued_before, &home, &xdg),
                glued_before
            );
            // Companion: a space, `(`, `=`, `"` or `:-` before it is a boundary.
            let delimited_before = format!("{delimiter_prefix}{home}/x");
            let out = sanitizer.parameterize_paths(&delimited_before, &home, &xdg);
            proptest::prop_assert!(out.contains("{{ user.home }}"), "{:?} -> {:?}", delimited_before, out);
        }

        #[test]
        fn prop_parameterize_render_roundtrip(
            name in "[a-z][a-z0-9]{0,7}",
            pieces in proptest::collection::vec(
                (
                    0u8..8,
                    "[A-Za-z0-9_]{1,6}",
                    proptest::sample::select(" \t\n/\\\"':,;()[]<>-.".chars().collect::<Vec<char>>()),
                ),
                0..=12,
            ),
        ) {
            let sanitizer = Sanitizer::new();
            let renderer = crate::template::TemplateRenderer::new();
            let home = format!("/home/{name}");
            let xdg = format!("{home}/.config");
            let text: String = pieces
                .iter()
                .map(|(kind, word, delimiter)| roundtrip_piece(*kind, word, *delimiter, &home, &xdg))
                .collect();

            let published = sanitizer.parameterize_paths(&text, &home, &xdg);
            let rendered = renderer.render_str(&published, &home_context(&home, &xdg));
            proptest::prop_assert_eq!(rendered, Ok(text.clone()));

            // Cross-machine control: another home (one char longer, so never equal)
            // renders every placeholder differently.
            if published != text {
                let other_home = format!("{home}z");
                let other_xdg = format!("{other_home}/.config");
                let elsewhere = renderer
                    .render_str(&published, &home_context(&other_home, &other_xdg));
                proptest::prop_assert_ne!(elsewhere, Ok(text));
            }
        }
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

    #[test]
    fn test_check_secrets_ascii_word_boundary() {
        let sanitizer = Sanitizer::new();
        let prefix = "ghp_";
        let body = "111122223333444455556666777788889999";
        // A non-ASCII letter before the token is a word boundary (Go/gitleaks semantics).
        let after_non_ascii = format!("x=é{prefix}{body}");
        match sanitizer.check_secrets(&after_non_ascii) {
            SanitizationResult::SecretDetected { pattern, .. } => {
                assert_eq!(pattern, "GitHub Personal Access Token");
            }
            SanitizationResult::Clean(_) => panic!("Expected detection after a non-ASCII letter"),
        }

        // Negative control, named-rule layer only: an ASCII letter glued to the token
        // leaves no word boundary, so the named rule does not fire.
        let glued = format!("a{prefix}{body}");
        assert_eq!(first_named_rule(glued.as_bytes()), None);
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
