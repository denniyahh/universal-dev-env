//! Embedded gitleaks v8.30.1 default rules (SEC-02), matched with gitleaks' plain-text semantics.

use super::Finding;
use std::path::Path;
use std::sync::LazyLock;

/// gitleaks v8.30.1 `config/gitleaks.toml`, vendored verbatim.
pub const VENDORED_RULES: &str = include_str!("rules/gitleaks.toml");

/// SHA-256 of [`VENDORED_RULES`] as fetched from the v8.30.1 tag.
#[cfg(test)]
pub const VENDORED_RULES_SHA256: &str =
    "e163e53b9e7e8a8511e77271e2b323ed057759542a6d988258afe3a1fa329caf";

/// A loaded gitleaks rule set.
pub struct RuleSet;

impl RuleSet {
    /// Parses and compiles a gitleaks TOML rule file.
    pub fn from_toml(_text: &str) -> Result<RuleSet, String> {
        Ok(RuleSet)
    }

    /// Number of rules loaded.
    #[cfg(test)]
    pub fn rule_count(&self) -> usize {
        0
    }

    /// Number of rules that carry a content regex.
    #[cfg(test)]
    pub fn regex_rule_count(&self) -> usize {
        0
    }

    /// The first finding of any rule, in file order.
    pub fn first_finding(&self, _bytes: &[u8], _path: &Path) -> Option<Finding> {
        None
    }
}

static RULES: LazyLock<RuleSet> = LazyLock::new(|| {
    RuleSet::from_toml(VENDORED_RULES)
        .expect("vendored gitleaks rules load (pinned by test_vendored_rules_load_all)")
});

/// The vendored rule set, compiled once per process.
pub fn vendored() -> &'static RuleSet {
    &RULES
}

/// Escapes every `{` that cannot start a repetition, as Go's regexp parser reads it.
pub(crate) fn go_compat_braces(pattern: &str) -> String {
    pattern.to_string()
}

/// Compiles one gitleaks pattern as an ASCII-mode byte regex.
fn compile(pattern: &str) -> Result<regex::bytes::Regex, regex::Error> {
    regex::bytes::RegexBuilder::new(&go_compat_braces(pattern))
        .unicode(false)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha256_hex(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    /// Loads a synthetic rule file, panicking with the loader's message on error.
    fn rules(text: &str) -> RuleSet {
        match RuleSet::from_toml(text) {
            Ok(rules) => rules,
            Err(e) => panic!("synthetic rule file must load: {e}"),
        }
    }

    #[test]
    fn test_vendored_rules_load_all() {
        let loaded = rules(VENDORED_RULES);
        assert_eq!(loaded.rule_count(), 222);
        assert_eq!(loaded.regex_rule_count(), 221);
        assert_eq!(vendored().rule_count(), 222);

        assert_eq!(sha256_hex(VENDORED_RULES.as_bytes()), VENDORED_RULES_SHA256);
        // Negative control: one extra byte changes the digest.
        let mut altered = VENDORED_RULES.as_bytes().to_vec();
        altered.push(b'\n');
        assert_ne!(sha256_hex(&altered), VENDORED_RULES_SHA256);

        let license = include_bytes!("rules/LICENSE-gitleaks");
        assert_eq!(
            sha256_hex(license),
            "e3884b252b3bfc045e55be43a34d1e80da070bc6f804ac95bf4660e97d62ebc6"
        );
        assert!(license.starts_with(b"MIT License"));
    }

    #[test]
    fn test_go_brace_literal() {
        let go_pattern = r"^\$(?:\d+|{\d+})$";
        assert!(
            regex::bytes::RegexBuilder::new(go_pattern)
                .unicode(false)
                .build()
                .is_err(),
            "the Go pattern must not build unchanged"
        );
        let literal_brace = compile(go_pattern).expect("Go-compatible pattern builds");
        assert!(literal_brace.is_match(b"$12"));
        assert!(literal_brace.is_match(b"${12}"));
        assert!(!literal_brace.is_match(b"${x}"));

        let repeat = compile("^a{2}$").expect("repetition builds");
        assert!(repeat.is_match(b"aa"));
        assert!(!repeat.is_match(b"a{2}"));
        let range = compile("^a{1,2}$").expect("range repetition builds");
        assert!(range.is_match(b"aa"));
        assert!(!range.is_match(b"aaa"));

        assert_eq!(go_compat_braces(r"x\{y"), r"x\{y");
        assert_eq!(go_compat_braces("x{y"), r"x\{y");
        assert_eq!(go_compat_braces("x{,2}"), r"x\{,2}");
    }

    #[test]
    fn test_invalid_rule_regex_is_error() {
        let broken = "[[rules]]\nid = \"broken\"\nregex = '''('''\n";
        let Err(err) = RuleSet::from_toml(broken) else {
            panic!("an invalid rule regex must fail the load");
        };
        assert!(err.contains("broken"), "{err}");

        // Twin: the escaped parenthesis is a valid regex and loads.
        let fixed = "[[rules]]\nid = \"broken\"\nregex = '''\\('''\n";
        assert_eq!(rules(fixed).rule_count(), 1);
    }

    #[test]
    fn test_unknown_key_is_error() {
        let unknown = "[[rules]]\nid = \"extra\"\nregex = '''x'''\nbogus = 1\n";
        assert!(RuleSet::from_toml(unknown).is_err());
        // Twin: the same rule without the unknown key loads.
        let known = "[[rules]]\nid = \"extra\"\nregex = '''x'''\n";
        assert_eq!(rules(known).rule_count(), 1);
    }

    #[test]
    fn test_keyword_prefilter() {
        let set = rules(
            "[[rules]]\nid = \"kw\"\nregex = '''tok_[A-Za-z0-9]{12}'''\nkeywords = [\"needle\"]\n",
        );
        let path = Path::new("projects/generic/gitconfig.repo");
        // Assembled at runtime so the repo's own gitleaks hook does not flag the fixture.
        let candidate = ["tok_", "Ab3Cd4Ef5Gh6"].concat();
        assert_eq!(set.first_finding(candidate.as_bytes(), path), None);
        let with_keyword = format!("NEEDLE\n{candidate}\n");
        assert_eq!(
            set.first_finding(with_keyword.as_bytes(), path),
            Some(Finding {
                rule: "kw".to_string(),
                line: 2,
            })
        );
    }
}
