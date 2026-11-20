//! Embedded gitleaks v8.30.1 default rules (SEC-02), matched with gitleaks' plain-text semantics.
//!
//! The rule file is vendored verbatim and compiled once per process. Matching
//! follows gitleaks v8.30.1 `detect/detect.go` (`detectRule`) for plain text: no
//! encoded-segment decoding, no `gitleaks:allow` (D-03), and no global `paths`
//! allowlist, so no file is ever skipped whole.

use super::{Finding, line_at};
use regex::bytes::{Regex, RegexBuilder, RegexSet, RegexSetBuilder};
use serde::Deserialize;
use std::path::Path;
use std::sync::LazyLock;

/// gitleaks v8.30.1 `config/gitleaks.toml`, vendored verbatim.
pub const VENDORED_RULES: &str = include_str!("rules/gitleaks.toml");

/// SHA-256 of [`VENDORED_RULES`] as fetched from the v8.30.1 tag.
#[cfg(test)]
pub const VENDORED_RULES_SHA256: &str =
    "e163e53b9e7e8a8511e77271e2b323ed057759542a6d988258afe3a1fa329caf";

/// The rule file as written. Every key the vendored file uses is listed, and any
/// other key is an error, so a silently changed schema cannot load.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(rename = "title")]
    _title: Option<String>,
    #[serde(rename = "minVersion")]
    _min_version: Option<String>,
    allowlist: Option<RawAllowlist>,
    #[serde(default)]
    rules: Vec<RawRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    id: String,
    #[serde(rename = "description")]
    _description: Option<String>,
    regex: Option<String>,
    entropy: Option<f64>,
    #[serde(default)]
    keywords: Vec<String>,
    path: Option<String>,
    #[serde(rename = "secretGroup")]
    secret_group: Option<usize>,
    #[serde(default)]
    allowlists: Vec<RawAllowlist>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAllowlist {
    #[serde(rename = "description")]
    _description: Option<String>,
    condition: Option<String>,
    #[serde(rename = "regexTarget")]
    regex_target: Option<String>,
    #[serde(default)]
    regexes: Vec<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    stopwords: Vec<String>,
}

/// How an allowlist combines its checks (gitleaks `MatchCondition`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Condition {
    /// Any regex on the target, or any stopword in the secret.
    Or,
    /// Every defined check among paths, regexes and stopwords.
    And,
}

/// What an allowlist's regexes run on (gitleaks `RegexTarget`).
#[derive(Clone, Copy)]
enum Target {
    Secret,
    Match,
    Line,
}

struct Allowlist {
    condition: Condition,
    target: Target,
    regexes: Vec<Regex>,
    paths: Vec<Regex>,
    /// Lower-cased.
    stopwords: Vec<Vec<u8>>,
}

struct Rule {
    id: String,
    regex: Option<Regex>,
    path: Option<Regex>,
    /// Indices into [`RuleSet::keywords`]; empty means the rule always runs.
    keywords: Vec<usize>,
    /// Shannon-entropy floor; a secret at or below it is skipped. `None` = off.
    entropy: Option<f64>,
    /// Capture group holding the secret; `None` = first non-empty group.
    secret_group: Option<usize>,
    allowlists: Vec<Allowlist>,
}

/// A loaded gitleaks rule set: rules in file order plus the global allowlist.
pub struct RuleSet {
    rules: Vec<Rule>,
    global: Option<Allowlist>,
    /// Every distinct lower-cased keyword, one literal pattern each.
    keywords: RegexSet,
}

impl RuleSet {
    /// Parses and compiles a gitleaks TOML rule file.
    ///
    /// A parse error, an unknown key, an unknown `condition` or `regexTarget`, an
    /// empty allowlist or any pattern that fails to compile is an `Err` naming the
    /// rule (or `global allowlist`). Nothing is filtered away: a partial rule set
    /// never loads.
    pub fn from_toml(text: &str) -> Result<RuleSet, String> {
        let raw: RawConfig =
            toml::from_str(text).map_err(|e| format!("gitleaks rule file: {e}"))?;
        let global = raw
            .allowlist
            .as_ref()
            .map(|a| compile_allowlist(a, "global allowlist"))
            .transpose()?;

        let mut keyword_list: Vec<String> = Vec::new();
        let mut rules = Vec::with_capacity(raw.rules.len());
        for raw_rule in &raw.rules {
            rules.push(compile_rule(raw_rule, &mut keyword_list)?);
        }
        let keywords = RegexSetBuilder::new(keyword_list.iter().map(|k| regex::escape(k)))
            .unicode(false)
            .build()
            .map_err(|e| format!("gitleaks keywords: {e}"))?;
        Ok(RuleSet {
            rules,
            global,
            keywords,
        })
    }

    /// Number of rules loaded.
    #[cfg(test)]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Number of rules that carry a content regex.
    #[cfg(test)]
    pub fn regex_rule_count(&self) -> usize {
        self.rules.iter().filter(|r| r.regex.is_some()).count()
    }

    /// Every rule id, in file order.
    #[cfg(test)]
    pub(crate) fn rule_ids(&self) -> Vec<&str> {
        self.rules.iter().map(|r| r.id.as_str()).collect()
    }

    /// The first finding of any rule, in file order, with the 1-based line of the
    /// match start. `path` is the publish-relative path for path-scoped rules.
    pub fn first_finding(&self, bytes: &[u8], path: &Path) -> Option<Finding> {
        // detect.go: keywords are matched against the lower-cased fragment.
        let lowered = bytes.to_ascii_lowercase();
        let present = self.keywords.matches(&lowered);
        let path = path.as_os_str().as_encoded_bytes();
        self.rules.iter().find_map(|rule| {
            let runs =
                rule.keywords.is_empty() || rule.keywords.iter().any(|&k| present.matched(k));
            if !runs {
                return None;
            }
            rule.first_line(bytes, path, self.global.as_ref())
                .map(|line| Finding {
                    rule: rule.id.clone(),
                    line,
                })
        })
    }
}

impl Rule {
    /// The line of this rule's first reported match in `bytes`, if any, following
    /// detect.go `detectRule` step by step.
    fn first_line(&self, bytes: &[u8], path: &[u8], global: Option<&Allowlist>) -> Option<usize> {
        // detect.go `checkCommitOrPathAllowed(fragment, r.Allowlists)`: a rule
        // allowlist can skip this rule for the whole file by its path.
        if self.allowlists.iter().any(|a| a.skips_file(path)) {
            return None;
        }
        // detect.go `if r.Path != nil`: a path-only rule reports the file itself;
        // a rule with both a path and a regex runs only on a matching path.
        if let Some(path_regex) = &self.path {
            let path_matches = path_regex.is_match(path);
            if self.regex.is_none() {
                return path_matches.then_some(1);
            }
            if !path_matches {
                return None;
            }
        }
        let regex = self.regex.as_ref()?;
        regex.find_iter(bytes).find_map(|m| {
            // detect.go: `secret := strings.Trim(raw[m0:m1], "\n")`.
            let (start, end) = trim_lf(bytes, m.start(), m.end());
            let candidate = Candidate {
                secret: self.secret(regex, &bytes[start..end]),
                matched: &bytes[start..end],
                line: line_span(bytes, start, end),
            };
            // detect.go: `if r.Entropy != 0 && shannonEntropy(secret) <= r.Entropy`.
            if self
                .entropy
                .is_some_and(|floor| shannon_entropy(candidate.secret) <= floor)
            {
                return None;
            }
            // detect.go `checkFindingAllowed`: the global allowlist, then the rule's.
            let allowed = global.is_some_and(|a| a.allows(&candidate, path))
                || self.allowlists.iter().any(|a| a.allows(&candidate, path));
            (!allowed).then(|| line_at(bytes, start))
        })
    }

    /// detect.go: the regex re-run on the trimmed match; `secretGroup` when set,
    /// else the first non-empty capture group, else the trimmed match itself.
    fn secret<'a>(&self, regex: &Regex, matched: &'a [u8]) -> &'a [u8] {
        let Some(groups) = regex.captures(matched) else {
            return matched;
        };
        if groups.len() < 2 {
            return matched;
        }
        match self.secret_group {
            // A group that did not take part is Go's empty string.
            Some(group) => groups.get(group).map_or(&[][..], |g| g.as_bytes()),
            None => groups
                .iter()
                .skip(1)
                .flatten()
                .map(|g| g.as_bytes())
                .find(|g| !g.is_empty())
                .unwrap_or(matched),
        }
    }
}

/// One match as the allowlists see it (gitleaks `finding.Secret`, `.Match` and
/// `.Line`).
struct Candidate<'a> {
    secret: &'a [u8],
    /// The LF-trimmed match.
    matched: &'a [u8],
    /// The whole line(s) holding the match.
    line: &'a [u8],
}

impl Allowlist {
    /// detect.go `checkCommitOrPathAllowed`, path half: OR skips the file when a
    /// path regex matches; AND skips it only when it has no regexes and no
    /// stopwords (those are checked per finding) and its paths match. An empty
    /// path (the check_secrets wrapper) never skips. Only rule allowlists call
    /// this: the global `paths` never skip a whole file (assumption A4).
    fn skips_file(&self, path: &[u8]) -> bool {
        if self.condition == Condition::And
            && (!self.regexes.is_empty() || !self.stopwords.is_empty())
        {
            return false;
        }
        self.path_allowed(path)
    }

    /// detect.go `checkFindingAllowed` for one allowlist. OR: a regex matches the
    /// target or a stopword occurs in the lower-cased secret. AND: every defined
    /// check among paths, regexes and stopwords holds.
    fn allows(&self, candidate: &Candidate<'_>, path: &[u8]) -> bool {
        let target = match self.target {
            Target::Secret => candidate.secret,
            Target::Match => candidate.matched,
            Target::Line => candidate.line,
        };
        // gitleaks `RegexAllowed` and `ContainsStopWord` are false on empty input.
        let regex_allowed = !target.is_empty() && self.regexes.iter().any(|r| r.is_match(target));
        let lowered = candidate.secret.to_ascii_lowercase();
        let stopword = self.stopwords.iter().any(|word| contains(&lowered, word));
        match self.condition {
            Condition::Or => regex_allowed || stopword,
            Condition::And => {
                (self.paths.is_empty() || self.path_allowed(path))
                    && (self.regexes.is_empty() || regex_allowed)
                    && (self.stopwords.is_empty() || stopword)
            }
        }
    }

    /// gitleaks `PathAllowed`: false for an empty path or when no path regex matches.
    fn path_allowed(&self, path: &[u8]) -> bool {
        !path.is_empty() && self.paths.iter().any(|r| r.is_match(path))
    }
}

/// True when `needle` is a non-empty run inside `haystack` (an empty stopword
/// never allows anything).
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// The bytes from the start of the line holding `start` to the end of the line
/// holding `end` (gitleaks `finding.Line`), without the line terminators.
fn line_span(bytes: &[u8], start: usize, end: usize) -> &[u8] {
    let line_start = bytes[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let line_end = bytes[end..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |i| end + i);
    &bytes[line_start..line_end]
}

/// Narrows `start..end` past LF bytes at both ends (Go `strings.Trim(s, "\n")`).
fn trim_lf(bytes: &[u8], start: usize, end: usize) -> (usize, usize) {
    let span = &bytes[start..end];
    let leading = span.iter().take_while(|&&b| b == b'\n').count();
    let trailing = span[leading..]
        .iter()
        .rev()
        .take_while(|&&b| b == b'\n')
        .count();
    (start + leading, end - trailing)
}

/// Shannon entropy in bits, H = −Σ p_b·log2(p_b) over the distinct bytes b of
/// `secret`, p_b = count(b) / byte length: gitleaks' `shannonEntropy`
/// (detect/utils.go), which counts runes over the byte length and so equals this
/// for ASCII secrets.
fn shannon_entropy(secret: &[u8]) -> f64 {
    if secret.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for &byte in secret {
        counts[usize::from(byte)] += 1;
    }
    let len = secret.len() as f64;
    counts
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let p = count as f64 / len;
            -p * p.log2()
        })
        .sum()
}

fn compile_rule(raw: &RawRule, keyword_list: &mut Vec<String>) -> Result<Rule, String> {
    let id = &raw.id;
    let compile_field = |pattern: &Option<String>, kind: &str| {
        pattern
            .as_deref()
            .filter(|p| !p.is_empty())
            .map(|p| compile(p).map_err(|e| format!("gitleaks rule {id}: {kind}: {e}")))
            .transpose()
    };
    let regex = compile_field(&raw.regex, "regex")?;
    let path = compile_field(&raw.path, "path")?;
    if regex.is_none() && path.is_none() {
        return Err(format!("gitleaks rule {id}: neither regex nor path is set"));
    }
    let secret_group = raw.secret_group.filter(|&g| g > 0);
    if let (Some(group), Some(regex)) = (secret_group, &regex)
        && group >= regex.captures_len()
    {
        return Err(format!(
            "gitleaks rule {id}: secretGroup {group} exceeds the regex's capture groups"
        ));
    }
    let keywords = raw
        .keywords
        .iter()
        .map(|k| {
            let k = k.to_ascii_lowercase();
            match keyword_list.iter().position(|known| *known == k) {
                Some(index) => index,
                None => {
                    keyword_list.push(k);
                    keyword_list.len() - 1
                }
            }
        })
        .collect();
    let allowlists = raw
        .allowlists
        .iter()
        .map(|a| compile_allowlist(a, &format!("rule {id} allowlist")))
        .collect::<Result<_, _>>()?;
    Ok(Rule {
        id: id.clone(),
        regex,
        path,
        keywords,
        entropy: raw.entropy.filter(|&e| e != 0.0),
        secret_group,
        allowlists,
    })
}

/// gitleaks `parseAllowlist` + `Validate`: `condition` is AND / && or OR / || /
/// unset (any case), `regexTarget` is secret / match / line / unset, and at least
/// one check must be present.
fn compile_allowlist(raw: &RawAllowlist, owner: &str) -> Result<Allowlist, String> {
    let condition = match raw
        .condition
        .as_deref()
        .map(str::to_ascii_uppercase)
        .as_deref()
    {
        None | Some("") | Some("OR") | Some("||") => Condition::Or,
        Some("AND") | Some("&&") => Condition::And,
        Some(other) => return Err(format!("gitleaks {owner}: unknown condition {other:?}")),
    };
    let target = match raw.regex_target.as_deref() {
        None | Some("") | Some("secret") => Target::Secret,
        Some("match") => Target::Match,
        Some("line") => Target::Line,
        Some(other) => return Err(format!("gitleaks {owner}: unknown regexTarget {other:?}")),
    };
    if raw.regexes.is_empty() && raw.paths.is_empty() && raw.stopwords.is_empty() {
        return Err(format!("gitleaks {owner}: no regexes, paths or stopwords"));
    }
    let compile_all = |patterns: &[String], kind: &str| {
        patterns
            .iter()
            .map(|p| compile(p).map_err(|e| format!("gitleaks {owner}: {kind}: {e}")))
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(Allowlist {
        condition,
        target,
        regexes: compile_all(&raw.regexes, "regex")?,
        paths: compile_all(&raw.paths, "path")?,
        stopwords: raw
            .stopwords
            .iter()
            .map(|w| w.as_bytes().to_ascii_lowercase())
            .collect(),
    })
}

static RULES: LazyLock<RuleSet> = LazyLock::new(|| {
    RuleSet::from_toml(VENDORED_RULES)
        .expect("vendored gitleaks rules load (pinned by test_vendored_rules_load_all)")
});

/// The vendored rule set, compiled once per process.
pub fn vendored() -> &'static RuleSet {
    &RULES
}

/// Escapes every `{` that cannot start a repetition (`{n}`, `{n,}`, `{n,m}`), as
/// Go's regexp parser reads it: "If the repeat cannot be parsed, { is a literal."
/// Rust rejects such a brace, so it is rewritten to `\{`. Escapes are copied as is.
pub(crate) fn go_compat_braces(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.char_indices();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\\' => {
                out.push(ch);
                if let Some((_, escaped)) = chars.next() {
                    out.push(escaped);
                }
            }
            '{' if !starts_valid_repeat(&pattern[index + 1..]) => out.push_str("\\{"),
            _ => out.push(ch),
        }
    }
    out
}

/// True when `rest` (the text after a `{`) begins with `\d+}` or `\d+,\d*}`.
fn starts_valid_repeat(rest: &str) -> bool {
    let digit_run = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let min_len = digit_run(rest);
    if min_len == 0 {
        return false;
    }
    let after_min = &rest[min_len..];
    match after_min.strip_prefix(',') {
        Some(after_comma) => after_comma[digit_run(after_comma)..].starts_with('}'),
        None => after_min.starts_with('}'),
    }
}

/// Compiles one gitleaks pattern as an ASCII-mode byte regex: Go's `\w`, `\d`,
/// `\b` and `(?i)` are ASCII-only, and Unicode classes would also push three
/// vendored patterns past the regex size limit.
fn compile(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(&go_compat_braces(pattern))
        .unicode(false)
        .build()
}

/// The vendored rule ids the category table (`test_category_positives`) covers with
/// one positive and one benign twin each, in table order. Shared with the
/// uncovered-rule report so the two cannot drift apart.
#[cfg(test)]
const CATEGORY_RULE_IDS: [&str; 15] = [
    "private-key",
    "aws-access-token",
    "gcp-api-key",
    "github-pat",
    "gitlab-pat",
    "npm-access-token",
    "pypi-upload-token",
    "slack-bot-token",
    "stripe-access-token",
    "anthropic-api-key",
    "jwt",
    "generic-api-key",
    "datadog-access-token",
    "vault-batch-token",
    "curl-auth-user",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanitizer::Sanitizer;
    use crate::sanitizer::entropy::{
        ALLOW_MARKER, SeededChars, class_cycling_token, template_corpus_files,
    };
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

    /// The line of `set`'s first finding on `text` at `path`.
    fn line_of(set: &RuleSet, text: &str, path: &str) -> Option<usize> {
        set.first_finding(text.as_bytes(), Path::new(path))
            .map(|f| f.line)
    }

    const ANY_PATH: &str = "projects/generic/gitconfig.repo";

    #[test]
    fn test_secret_group_selection() {
        let regex = r"regex = '''(?:(foo)|bar)=([A-Za-z0-9]{16})'''";
        let first_group = rules(&format!(
            "[[rules]]\nid = \"grp\"\n{regex}\nentropy = 3.0\n"
        ));
        let distinct = "a1B2c3D4e5F6g7H8";
        // The secret is group 2 (group 1 did not take part): 16 equal letters have
        // entropy 0, 16 distinct characters have entropy 4.
        assert_eq!(
            line_of(&first_group, "bar=aaaaaaaaaaaaaaaa", ANY_PATH),
            None
        );
        assert_eq!(
            line_of(&first_group, &format!("bar={distinct}"), ANY_PATH),
            Some(1)
        );
        // With `foo` the first non-empty group is `foo` (entropy about 1.58).
        assert_eq!(
            line_of(&first_group, &format!("foo={distinct}"), ANY_PATH),
            None
        );

        let group = |n: usize| {
            rules(&format!(
                "[[rules]]\nid = \"grp\"\n{regex}\nentropy = 3.0\nsecretGroup = {n}\n"
            ))
        };
        assert_eq!(
            line_of(&group(1), &format!("foo={distinct}"), ANY_PATH),
            None
        );
        assert_eq!(
            line_of(&group(2), &format!("foo={distinct}"), ANY_PATH),
            Some(1)
        );
    }

    #[test]
    fn test_entropy_floor_is_strict() {
        let set = rules("[[rules]]\nid = \"floor\"\nregex = '''val=([a-z]+)'''\nentropy = 2.0\n");
        // `abcd` has entropy exactly 2.0, which does not exceed the floor.
        assert_eq!(line_of(&set, "val=abcd", ANY_PATH), None);
        // `abcde` has entropy log2(5), about 2.32. A distinct-count ratio (1.0)
        // would wrongly skip it.
        assert_eq!(line_of(&set, "val=abcde", ANY_PATH), Some(1));
    }

    #[test]
    fn test_global_allowlist() {
        let set = rules(concat!(
            "[allowlist]\n",
            "paths = ['''gitconfig\\.repo$''']\n",
            "regexes = ['''^SAFE''']\n",
            "stopwords = [\"example\"]\n",
            "[[rules]]\nid = \"tok\"\nregex = '''tok_([A-Za-z0-9]{12})'''\n",
        ));
        let prefix = "tok_";
        // Regex on the secret.
        assert_eq!(
            line_of(&set, &format!("{prefix}SAFEx7Kq2Wm9"), ANY_PATH),
            None
        );
        // Stopword, case-insensitive, in the secret.
        assert_eq!(
            line_of(&set, &format!("{prefix}q1ExAmPlEz9w"), ANY_PATH),
            None
        );
        // Neither: reported, although the global `paths` entry matches the path.
        assert_eq!(
            line_of(&set, &format!("{prefix}A1b2C3d4E5f6"), ANY_PATH),
            Some(1)
        );
    }

    /// A rule `pfx_\w*:([a-z0-9]{12})` (secret = group 1) whose one allowlist
    /// regex is `marker`, with `regexTarget` set to `target` (`None` = unset).
    fn marker_rule(target: Option<&str>) -> RuleSet {
        let target_line = target.map_or(String::new(), |t| format!("regexTarget = \"{t}\"\n"));
        rules(&format!(
            "[[rules]]\nid = \"m\"\nregex = '''pfx_\\w*:([a-z0-9]{{12}})'''\n\
             [[rules.allowlists]]\n{target_line}regexes = ['''marker''']\n"
        ))
    }

    #[test]
    fn test_rule_allowlist_targets() {
        let secret = marker_rule(None);
        let matched = marker_rule(Some("match"));
        let line = marker_rule(Some("line"));
        let verdicts = |text: &str| {
            [
                line_of(&secret, text, ANY_PATH),
                line_of(&matched, text, ANY_PATH),
                line_of(&line, text, ANY_PATH),
            ]
        };
        // No marker anywhere: every rule reports.
        assert_eq!(verdicts("pfx_ab:abc123def456"), [Some(1); 3]);
        // In the secret, which lies inside the match and the line too.
        assert_eq!(verdicts("pfx_ab:marker123456"), [None; 3]);
        // In the match, outside the secret group.
        assert_eq!(verdicts("pfx_marker:abc123def456"), [Some(1), None, None]);
        // Elsewhere on the line only.
        assert_eq!(
            verdicts("marker pfx_ab:abc123def456"),
            [Some(1), Some(1), None]
        );
    }

    #[test]
    fn test_rule_allowlist_condition_and() {
        let set = rules(concat!(
            "[[rules]]\nid = \"and\"\nregex = '''=\"([a-z0-9]{10})\"'''\n",
            "[[rules.allowlists]]\ncondition = \"AND\"\n",
            "paths = ['''\\.bb$''']\nregexTarget = \"line\"\nregexes = ['''^SRC''']\n",
        ));
        let src_line = "SRC_URI=\"abc123def4\"";
        assert_eq!(line_of(&set, src_line, "recipe.bb"), None);
        assert_eq!(line_of(&set, src_line, ANY_PATH), Some(1));
        assert_eq!(
            line_of(&set, "DST_URI=\"abc123def4\"", "recipe.bb"),
            Some(1)
        );
    }

    #[test]
    fn test_rule_allowlist_path_prefilter() {
        let set = rules(concat!(
            "[[rules]]\nid = \"pre\"\nregex = '''tok_[a-z0-9]{8}'''\n",
            "[[rules.allowlists]]\npaths = ['''README\\.md$''']\n",
        ));
        let text = "tok_ab12cd34";
        assert_eq!(line_of(&set, text, "docs/README.md"), None);
        assert_eq!(line_of(&set, text, "README.txt"), Some(1));
        // An empty path (the check_secrets wrapper) never skips a rule.
        assert_eq!(line_of(&set, text, ""), Some(1));
    }

    #[test]
    fn test_rule_path_gating() {
        let scoped =
            rules("[[rules]]\nid = \"tf\"\npath = '''\\.tf$'''\nregex = '''pw=[a-z0-9]{8}'''\n");
        assert_eq!(line_of(&scoped, "x\npw=ab12cd34", "main.tf"), Some(2));
        assert_eq!(line_of(&scoped, "x\npw=ab12cd34", "main.repo"), None);

        let path_only = rules("[[rules]]\nid = \"p12\"\npath = '''\\.p12$'''\n");
        assert_eq!(
            path_only.first_finding(b"any\ncontent", Path::new("certs/a.p12")),
            Some(Finding {
                rule: "p12".to_string(),
                line: 1,
            })
        );
        assert_eq!(line_of(&path_only, "any\ncontent", "certs/a.repo"), None);
    }

    const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
    const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const DIGITS: &str = "0123456789";

    fn alnum() -> String {
        format!("{LOWER}{UPPER}{DIGITS}")
    }

    /// `[\w-]`: letters, digits, `_` and `-`.
    fn word() -> String {
        format!("{}_-", alnum())
    }

    fn lower_digits() -> String {
        format!("{LOWER}{DIGITS}")
    }

    /// A seeded Slack bot token (`xoxb-` + 12 and 13 digits + 24 letters/digits).
    fn seeded_slack(chars: &mut SeededChars) -> String {
        format!(
            "{}-{}-{}-{}",
            "xoxb",
            chars.draw(DIGITS, 12),
            chars.draw(DIGITS, 13),
            chars.draw(&alnum(), 24)
        )
    }

    /// One category row: the expected rule id, a positive and its benign twin.
    struct CategoryRow {
        rule: &'static str,
        positive: String,
        twin: String,
    }

    /// One row per [`CATEGORY_RULE_IDS`] entry, in that order. Every positive is
    /// assembled at runtime from separate parts and seeded characters; each twin
    /// keeps the prefix with a value the rule's regex or entropy floor rejects.
    fn category_rows() -> Vec<CategoryRow> {
        let mut chars = SeededChars::new();
        let b64 = format!("{}+/", alnum());
        let bare = |rule, positive: String, twin: String| CategoryRow {
            rule,
            positive: format!("# ref {positive}\n"),
            twin: format!("# ref {twin}\n"),
        };
        let pem = |body: &str| {
            format!(
                "{}{}\n{body}\n{}{}\n",
                "-----BEGIN ", "PRIVATE KEY-----", "-----END ", "PRIVATE KEY-----"
            )
        };
        let aws = ["AK", "IA"].concat();
        let aws_alphabet = format!("{UPPER}234567");
        let gcp = ["AI", "za"].concat();
        let github = ["ghp", "_"].concat();
        let gitlab = ["glpat", "-"].concat();
        let npm = ["npm", "_"].concat();
        let pypi = ["pypi-", "AgEIcHlwaS5vcmc"].concat();
        let stripe = ["sk", "_live_"].concat();
        let anthropic = ["sk-ant-", "api03-"].concat();
        let vault = ["hvb", "."].concat();
        let generic = ["auth", "_secret = "].concat();
        let datadog = ["data", "dog = "].concat();
        let curl = ["cu", "rl -u deploy"].concat();
        let jwt = |header_len: usize, chars: &mut SeededChars| {
            let header = chars.draw(&alnum(), header_len);
            let payload = chars.draw(&alnum(), 30);
            format!("ey{header}.ey{payload}.{}", chars.draw(&word(), 32))
        };
        vec![
            CategoryRow {
                rule: "private-key",
                positive: pem(&chars.draw(&b64, 64)),
                twin: pem(&chars.draw(&b64, 8)),
            },
            bare(
                "aws-access-token",
                format!("{aws}{}", chars.draw(&aws_alphabet, 16)),
                format!("{aws}{}", "A".repeat(16)),
            ),
            bare(
                "gcp-api-key",
                format!("{gcp}{}", chars.draw(&word(), 35)),
                format!("{gcp}{}", chars.draw(&word(), 34)),
            ),
            bare(
                "github-pat",
                format!("{github}{}", chars.draw(&alnum(), 36)),
                format!("{github}{}", "a".repeat(36)),
            ),
            bare(
                "gitlab-pat",
                format!("{gitlab}{}", chars.draw(&word(), 20)),
                format!("{gitlab}{}", chars.draw(&word(), 19)),
            ),
            bare(
                "npm-access-token",
                format!("{npm}{}", chars.draw(&lower_digits(), 36)),
                format!("{npm}{}", chars.draw(&lower_digits(), 35)),
            ),
            bare(
                "pypi-upload-token",
                format!("{pypi}{}", chars.draw(&word(), 60)),
                format!("{pypi}{}", chars.draw(&word(), 49)),
            ),
            bare(
                "slack-bot-token",
                seeded_slack(&mut chars),
                format!("{}-{}", "xoxb", chars.draw(DIGITS, 3)),
            ),
            bare(
                "stripe-access-token",
                format!("{stripe}{}", chars.draw(&alnum(), 24)),
                format!("{stripe}{}", chars.draw(&alnum(), 9)),
            ),
            bare(
                "anthropic-api-key",
                format!("{anthropic}{}AA", chars.draw(&word(), 93)),
                format!("{anthropic}{}AA", chars.draw(&word(), 92)),
            ),
            bare("jwt", jwt(20, &mut chars), jwt(16, &mut chars)),
            CategoryRow {
                rule: "generic-api-key",
                positive: format!("{generic}{}\n", chars.draw(&alnum(), 24)),
                twin: format!("{generic}{{{{ user.home }}}}\n"),
            },
            CategoryRow {
                rule: "datadog-access-token",
                positive: format!("{datadog}{}\n", chars.draw(&lower_digits(), 40)),
                twin: format!("{datadog}{}\n", chars.draw(&lower_digits(), 39)),
            },
            bare(
                "vault-batch-token",
                format!("{vault}{}", chars.draw(&word(), 140)),
                format!("{vault}{}", chars.draw(&word(), 137)),
            ),
            CategoryRow {
                rule: "curl-auth-user",
                positive: format!("# {curl}:Q{} example.invalid\n", chars.draw(&alnum(), 16)),
                twin: format!("# {curl} example.invalid\n"),
            },
        ]
    }

    #[test]
    fn test_category_positives() {
        let rows = category_rows();
        let table: Vec<&str> = rows.iter().map(|row| row.rule).collect();
        assert_eq!(table, CATEGORY_RULE_IDS);
        let path = Path::new(ANY_PATH);
        for row in &rows {
            let found = vendored()
                .first_finding(row.positive.as_bytes(), path)
                .map(|f| f.rule);
            assert_eq!(
                found.as_deref(),
                Some(row.rule),
                "positive for {}",
                row.rule
            );
            let twin = vendored().first_finding(row.twin.as_bytes(), path);
            assert_eq!(twin, None, "benign twin for {}", row.rule);
        }
    }

    /// The number of `ids` that `table` does not name.
    fn uncovered(ids: &[&str], table: &[&str]) -> usize {
        ids.iter().filter(|id| !table.contains(id)).count()
    }

    #[test]
    fn test_report_rules_without_positive_fixture() {
        let ids = vendored().rule_ids();
        let total = ids.len();
        let distinct: std::collections::HashSet<&str> = CATEGORY_RULE_IDS.into_iter().collect();
        assert_eq!(distinct.len(), CATEGORY_RULE_IDS.len(), "table ids repeat");
        for id in CATEGORY_RULE_IDS {
            assert!(ids.contains(&id), "{id} is not a vendored rule id");
        }
        // Control: a misspelled id is not a vendored rule id.
        assert!(!ids.contains(&"slack-bot-tokens"));

        let n = uncovered(&ids, &CATEGORY_RULE_IDS);
        println!("vendored rules without a positive fixture: {n} of {total}");
        assert_eq!(total, 222);
        assert_eq!(n, total - CATEGORY_RULE_IDS.len());
        // Opposite direction: one row fewer leaves one more rule uncovered.
        assert_eq!(uncovered(&ids, &CATEGORY_RULE_IDS[1..]), n + 1);
    }

    fn slack_finding(line: usize) -> Option<Finding> {
        Some(Finding {
            rule: "slack-bot-token".to_string(),
            line,
        })
    }

    #[test]
    fn test_marker_does_not_silence_gitleaks_rules() {
        let sanitizer = Sanitizer::new();
        let path = Path::new(ANY_PATH);
        let token = seeded_slack(&mut SeededChars::new());
        let marked = format!("# chat {token} {ALLOW_MARKER}\n");
        assert_eq!(
            sanitizer.scan_bytes(marked.as_bytes(), path),
            slack_finding(1)
        );

        // Control: the marker does silence the R11 tracer token on its line.
        let tracer = class_cycling_token();
        let unmarked_tracer = format!("# build fingerprint {tracer}\n");
        assert!(
            sanitizer
                .scan_bytes(unmarked_tracer.as_bytes(), path)
                .is_some()
        );
        let marked_tracer = format!("# build fingerprint {tracer} {ALLOW_MARKER}\n");
        assert_eq!(sanitizer.scan_bytes(marked_tracer.as_bytes(), path), None);
    }

    #[test]
    fn test_inline_gitleaks_allow_not_honoured() {
        let sanitizer = Sanitizer::new();
        let path = Path::new(ANY_PATH);
        let token = seeded_slack(&mut SeededChars::new());
        let allow_comment = ["gitleaks", ":allow"].concat();
        let line = format!("# chat {token} # {allow_comment}\n");
        assert_eq!(
            sanitizer.scan_bytes(line.as_bytes(), path),
            slack_finding(1)
        );
        // Control: the comment alone is clean, so the token is what is refused.
        let comment_only = format!("# chat # {allow_comment}\n");
        assert_eq!(sanitizer.scan_bytes(comment_only.as_bytes(), path), None);
    }

    #[test]
    fn test_full_scanner_templates_corpus_zero_hits() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let sanitizer = Sanitizer::new();
        let files = template_corpus_files();
        assert!(files.len() >= 33, "corpus has only {} files", files.len());
        for file in &files {
            let relative = file
                .strip_prefix(root)
                .expect("corpus file under the crate");
            let bytes = std::fs::read(file).expect("read corpus file");
            if let Some(f) = sanitizer.scan_bytes(&bytes, relative) {
                panic!("{}: {} at line {}", relative.display(), f.rule, f.line);
            }
        }

        // Negative control: a copy of one corpus file with a Slack token appended.
        let relative = Path::new("templates/projects/generic/gitconfig.repo");
        let original = std::fs::read(root.join(relative)).expect("read gitconfig template");
        let scratch = tempfile::tempdir().expect("temp dir");
        let copy = scratch.path().join("gitconfig.repo");
        let token = seeded_slack(&mut SeededChars::new());
        let mut leaked = original.clone();
        leaked.extend_from_slice(format!("# chat {token}\n").as_bytes());
        std::fs::write(&copy, &leaked).expect("write the copy");
        let bytes = std::fs::read(&copy).expect("read the copy");
        let line = 1 + original.iter().filter(|&&b| b == b'\n').count();
        assert_eq!(sanitizer.scan_bytes(&bytes, relative), slack_finding(line));
    }

    /// Writes the category corpus for the parity run against the gitleaks binary:
    /// `<rule-id>.txt` per category positive and `benign.txt` (the Slack twin), into
    /// the directory `UDE_PARITY_DIR` names. The port's own verdict on each file is
    /// asserted first, so the comparison is between two checked results.
    #[test]
    #[ignore = "writes the gitleaks parity corpus; run by the 02-06 parity command"]
    fn test_dump_parity_corpus() {
        let Some(dir) = std::env::var_os("UDE_PARITY_DIR") else {
            panic!("UDE_PARITY_DIR must name the directory for the parity corpus");
        };
        let dir = std::path::PathBuf::from(dir);
        let rows = category_rows();
        for row in &rows {
            let file = dir.join(format!("{}.txt", row.rule));
            std::fs::write(&file, &row.positive).expect("write category file");
            let found = vendored()
                .first_finding(row.positive.as_bytes(), &file)
                .map(|f| f.rule);
            assert_eq!(
                found.as_deref(),
                Some(row.rule),
                "port on {}",
                file.display()
            );
        }
        let slack = rows
            .iter()
            .find(|row| row.rule == "slack-bot-token")
            .expect("the table has a Slack row");
        let benign = dir.join("benign.txt");
        std::fs::write(&benign, &slack.twin).expect("write benign file");
        assert_eq!(
            vendored().first_finding(slack.twin.as_bytes(), &benign),
            None
        );
    }

    #[test]
    fn test_match_is_trimmed_of_lf() {
        // A match that begins and ends with LF bytes is trimmed before it is
        // reported (detect.go `strings.Trim(..., "\n")`): the line is the token's
        // line, and the match an allowlist sees is the token alone.
        let regex = "regex = '''\\n+tok_[a-z0-9]{8}\\n+'''\n";
        let text = "a\n\ntok_ab12cd34\n\nb";
        let plain = rules(&format!("[[rules]]\nid = \"lf\"\n{regex}"));
        assert_eq!(line_of(&plain, text, ANY_PATH), Some(3));
        let anchored = rules(&format!(
            "[[rules]]\nid = \"lf\"\n{regex}[[rules.allowlists]]\nregexTarget = \"match\"\n\
             regexes = ['''^tok_[a-z0-9]{{8}}$''']\n"
        ));
        assert_eq!(line_of(&anchored, text, ANY_PATH), None);
    }

    #[test]
    fn test_rule_allowlist_or_path_skips_file() {
        // OR: a matching path allows the file even when the allowlist's regex
        // matches nothing.
        let set = rules(concat!(
            "[[rules]]\nid = \"or\"\nregex = '''tok_[a-z0-9]{8}'''\n",
            "[[rules.allowlists]]\npaths = ['''^fixtures/''']\nregexes = ['''^never$''']\n",
        ));
        let text = "tok_ab12cd34";
        assert_eq!(line_of(&set, text, "fixtures/a.txt"), None);
        assert_eq!(line_of(&set, text, "src/a.txt"), Some(1));
    }

    #[test]
    fn test_rule_allowlist_and_needs_path_and_stopword() {
        // AND with paths and stopwords: the path alone never skips the file.
        let set = rules(concat!(
            "[[rules]]\nid = \"and\"\nregex = '''tok_([a-z0-9]{8})'''\n",
            "[[rules.allowlists]]\ncondition = \"AND\"\npaths = ['''^fixtures/''']\n",
            "stopwords = [\"demo\"]\n",
        ));
        assert_eq!(line_of(&set, "tok_ab12cd34", "fixtures/a.txt"), Some(1));
        assert_eq!(line_of(&set, "tok_demo1234", "fixtures/a.txt"), None);
        assert_eq!(line_of(&set, "tok_demo1234", "src/a.txt"), Some(1));
    }

    #[test]
    fn test_line_target_is_the_whole_line() {
        // The line target runs from after the previous LF to before the next one.
        let set = rules(concat!(
            "[[rules]]\nid = \"ln\"\nregex = '''tok_[a-z0-9]{8}'''\n",
            "[[rules.allowlists]]\nregexTarget = \"line\"\n",
            "regexes = ['''^SAFE''', '''marker$''']\n",
        ));
        assert_eq!(line_of(&set, "x\nSAFE tok_ab12cd34\ny", ANY_PATH), None);
        assert_eq!(line_of(&set, "x\ntok_ab12cd34 marker\ny", ANY_PATH), None);
        // Controls: the anchors fail when the text is not at the line's edge.
        assert_eq!(
            line_of(&set, "x\nUNSAFE tok_ab12cd34\ny", ANY_PATH),
            Some(2)
        );
        assert_eq!(
            line_of(&set, "x\ntok_ab12cd34 marker!\ny", ANY_PATH),
            Some(2)
        );
    }

    #[test]
    fn test_secret_group_zero_is_unset() {
        // gitleaks reads `secretGroup = 0` as unset: the first non-empty group.
        let set = rules(concat!(
            "[[rules]]\nid = \"grp\"\nregex = '''(?:(foo)|bar)=([A-Za-z0-9]{16})'''\n",
            "entropy = 3.0\nsecretGroup = 0\n",
        ));
        let distinct = "a1B2c3D4e5F6g7H8";
        assert_eq!(line_of(&set, &format!("foo={distinct}"), ANY_PATH), None);
        assert_eq!(line_of(&set, &format!("bar={distinct}"), ANY_PATH), Some(1));
    }
}
