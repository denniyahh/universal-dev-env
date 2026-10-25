//! R11 entropy screen for credential-shaped tokens (SEC-01, D-04, D-05).
//!
//! A pure byte-level scorer: no decoding, every non-candidate byte (non-ASCII
//! included) is a delimiter. It is a backstop behind the named rules, not the
//! primary guard. Measured limits (CONTEXT D-05, RESEARCH probe): hex-only and
//! single-case secrets are never caught here, about 3-7% of random mixed-case and
//! 18-23% of base36 keys at 20-40 chars pass by chance, and base36 tokens of
//! exactly 64 chars are blocked only about 16-20% (risk T-02-07, accepted).

use std::ops::Range;
use std::sync::LazyLock;

/// Rule name reported for an entropy-only hit.
pub const RULE_NAME: &str = "high-entropy";

/// Minimum candidate length in bytes, padding included (D-04).
const MIN_TOKEN_LEN: usize = 20;

/// Most base64 `=` padding bytes kept on a candidate (D-04 as amended, OD-1).
const MAX_PADDING: usize = 2;

/// Fraction of the length-capped maximum entropy a token must reach (D-05).
const ENTROPY_FACTOR: f64 = 0.80;

/// Length above which the entropy bar stops rising: the bar is
/// `ENTROPY_FACTOR × log2(min(len, ENTROPY_LEN_CAP))` (D-05).
const ENTROPY_LEN_CAP: usize = 64;

/// Numerator of the randomness factor 0.75 (D-05), kept as an exact ratio.
const SWITCH_FACTOR_NUM: u64 = 3;

/// Denominator of the randomness factor 0.75 (D-05).
const SWITCH_FACTOR_DEN: u64 = 4;

/// Size of the uppercase class `A-Z` in the randomness test (D-05).
const UPPER_CLASS_SIZE: u64 = 26;

/// Size of the lowercase class `a-z` in the randomness test (D-05).
const LOWER_CLASS_SIZE: u64 = 26;

/// Size of the digit class `0-9` in the randomness test (D-05).
const DIGIT_CLASS_SIZE: u64 = 10;

/// Hex digest lengths D-02 exempts: SHA-1 and git object ids (40), SHA-256 (64)
/// and SHA-512 (128).
const HEX_DIGEST_LENGTHS: [usize; 3] = [40, 64, 128];

/// UUID group lengths, 8-4-4-4-12 (D-02).
const UUID_GROUP_LENGTHS: [usize; 5] = [8, 4, 4, 4, 12];

/// SRI body length for sha256: base64 of a 32-byte digest, padding included.
const SRI_SHA256_BODY_LEN: usize = 44;

/// SRI body length for sha384: base64 of a 48-byte digest (no padding).
const SRI_SHA384_BODY_LEN: usize = 64;

/// SRI body length for sha512: base64 of a 64-byte digest, padding included.
const SRI_SHA512_BODY_LEN: usize = 88;

/// The SRI shapes D-02 exempts: algorithm prefix, exact body length, and the
/// number of `=` padding bytes that end the body (1, 0 and 2 for 32-, 48- and
/// 64-byte digests). Any other length or padding count is scored (review
/// concern 2: a credential behind a digest name is not a digest).
const SRI_SHAPES: [(&[u8], usize, usize); 3] = [
    (b"sha256-", SRI_SHA256_BODY_LEN, 1),
    (b"sha384-", SRI_SHA384_BODY_LEN, 0),
    (b"sha512-", SRI_SHA512_BODY_LEN, 2),
];

/// An ssh public-key body after a key type and spaces or tabs on the same line
/// (D-02 as amended 2026-10-23: key body only). The key type may start the line
/// or follow whitespace or one of `"` `'` `=` `:` `,` (review concern 8), which
/// covers JSON values, unspaced and quoted gitconfig values, and git's
/// literal-key prefix `key::` (git-config `user.signingkey`). Capture 1 is the
/// body; the rest of the line is still scored.
static SSH_KEY_BODY: LazyLock<regex::bytes::Regex> = LazyLock::new(|| {
    regex::bytes::RegexBuilder::new(concat!(
        r#"(?:^|[ \t"'=:,])"#,
        r"(?:ssh-ed25519|ssh-rsa|ssh-dss|ecdsa-sha2-nistp(?:256|384|521)",
        r"|sk-ssh-ed25519@openssh\.com|sk-ecdsa-sha2-nistp256@openssh\.com)",
        r"[ \t]+(AAAA[A-Za-z0-9+/]+={0,3})",
    ))
    .unicode(false)
    .build()
    .expect("valid ssh key-body pattern")
});

/// 1-based number of the first line (split on LF) holding a token that passes
/// R11, if any. Never returns the token itself.
pub fn first_high_entropy_line(bytes: &[u8]) -> Option<usize> {
    bytes
        .split(|&b| b == b'\n')
        .position(line_has_high_entropy_token)
        .map(|index| index + 1)
}

/// True when some candidate of `line` is not a D-02 shape and passes R11.
fn line_has_high_entropy_token(line: &[u8]) -> bool {
    let ssh_bodies = ssh_key_body_spans(line);
    candidate_tokens(line).any(|(start, token)| {
        let inside_ssh_body = ssh_bodies
            .iter()
            .any(|body| body.start <= start && start + token.len() <= body.end);
        !is_exempt_shape(token) && !inside_ssh_body && passes_r11(token)
    })
}

/// D-02 whole-token shape exemptions: a hex digest, a UUID or an SRI hash.
///
/// Whole tokens only, so a digest-shaped substring of a longer token is scored.
/// The hex and UUID shapes are single-case only (a digest or UUID is printed in
/// one case). Those two can never change an R11 outcome on their own: single-case
/// hex of the exempt lengths carries at most log2(16) = 4.0 bits per byte against
/// a bar of 4.26 or more, and a single-case UUID at most log2(17) = 4.09 against
/// 4.14. They are kept because D-02 lists them; `test_exempt_shape_predicate`
/// pins every branch.
fn is_exempt_shape(token: &[u8]) -> bool {
    is_hex_digest(token) || is_uuid(token) || is_sri_hash(token)
}

/// All bytes are `0-9a-f`, or all are `0-9A-F`.
fn is_single_case_hex(bytes: &[u8]) -> bool {
    bytes.iter().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        || bytes.iter().all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'F'))
}

/// A single-case hex digest of one of the [`HEX_DIGEST_LENGTHS`] (D-02).
fn is_hex_digest(token: &[u8]) -> bool {
    HEX_DIGEST_LENGTHS.contains(&token.len()) && is_single_case_hex(token)
}

/// A single-case 8-4-4-4-12 UUID (D-02).
fn is_uuid(token: &[u8]) -> bool {
    let groups: Vec<&[u8]> = token.split(|&b| b == b'-').collect();
    groups
        .iter()
        .map(|group| group.len())
        .eq(UUID_GROUP_LENGTHS)
        && is_single_case_hex(&groups.concat())
}

/// A standard base64 digit `A-Z a-z 0-9 + /`.
fn is_base64_digit(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/')
}

/// An SRI hash of exactly one of the [`SRI_SHAPES`] (D-02).
fn is_sri_hash(token: &[u8]) -> bool {
    SRI_SHAPES.iter().any(|&(prefix, body_len, padding)| {
        token.strip_prefix(prefix).is_some_and(|body| {
            body.len() == body_len && {
                let (digits, pad) = body.split_at(body_len - padding);
                digits.iter().all(|&b| is_base64_digit(b)) && pad.iter().all(|&b| b == b'=')
            }
        })
    })
}

/// Byte ranges of the ssh public-key bodies on one line ([`SSH_KEY_BODY`]).
/// A candidate lying wholly inside one of them is not scored.
fn ssh_key_body_spans(line: &[u8]) -> Vec<Range<usize>> {
    SSH_KEY_BODY
        .captures_iter(line)
        .filter_map(|captures| captures.get(1))
        .map(|body| body.range())
        .collect()
}

/// True for a candidate body byte: `A-Z a-z 0-9 + / _ - .` (D-04).
fn is_body_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'_' | b'-' | b'.')
}

/// Splits `bytes` at every byte `keep` rejects, yielding each piece (empty ones
/// included) with its start offset in `bytes`.
fn pieces(bytes: &[u8], keep: impl Fn(u8) -> bool) -> impl Iterator<Item = (usize, &[u8])> {
    bytes
        .split(move |&b| !keep(b))
        .scan(0, |next_start, piece| {
            let start = *next_start;
            *next_start += piece.len() + 1;
            Some((start, piece))
        })
}

/// Candidate tokens of one line, each with its start offset (D-04 as amended
/// 2026-10-25, REVIEWS OD-1).
///
/// A candidate is a maximal run of body bytes ([`is_body_byte`]), extended by the
/// `=` bytes directly after it when there are 1 to [`MAX_PADDING`] of them and the
/// next byte is absent (end of line) or neither a body byte nor `=`. Every other
/// `=` and every other byte is a delimiter, so `NAME=value` scores `NAME` and
/// `value` separately, while a padded base64 value (`...==`) stays one candidate.
/// Only candidates of at least [`MIN_TOKEN_LEN`] bytes, padding included, are
/// yielded.
fn candidate_tokens(line: &[u8]) -> impl Iterator<Item = (usize, &[u8])> {
    pieces(line, |b| is_body_byte(b) || b == b'=')
        .flat_map(|(segment_start, segment)| {
            let trailing = segment.iter().rev().take_while(|&&b| b == b'=').count();
            let body_len = segment.len() - trailing;
            let padding = if (1..=MAX_PADDING).contains(&trailing) {
                trailing
            } else {
                0
            };
            pieces(&segment[..body_len], |b| b != b'=')
                .filter(|(_, run)| !run.is_empty())
                .map(move |(start, run)| {
                    let end = start + run.len();
                    let end = if end == body_len { end + padding } else { end };
                    (segment_start + start, &segment[start..end])
                })
        })
        .filter(|(_, token)| token.len() >= MIN_TOKEN_LEN)
}

/// Shannon entropy in bits over the token's byte frequencies:
/// H = −Σ p_b·log2(p_b) over the distinct bytes b, with p_b = (count of b) /
/// (token length). The candidates are ASCII, so bytes are characters.
fn shannon_entropy(token: &[u8]) -> f64 {
    let mut counts = [0usize; 256];
    for &b in token {
        counts[usize::from(b)] += 1;
    }
    let len = token.len() as f64;
    counts
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let p = count as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// Character class of an alphanumeric byte in the randomness test.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Upper,
    Lower,
    Digit,
}

impl CharClass {
    const ALL: [CharClass; 3] = [CharClass::Upper, CharClass::Lower, CharClass::Digit];

    /// The class of `b`, or `None` for a symbol (symbols are skipped).
    fn of(b: u8) -> Option<Self> {
        match b {
            b'A'..=b'Z' => Some(Self::Upper),
            b'a'..=b'z' => Some(Self::Lower),
            b'0'..=b'9' => Some(Self::Digit),
            _ => None,
        }
    }

    fn size(self) -> u64 {
        match self {
            Self::Upper => UPPER_CLASS_SIZE,
            Self::Lower => LOWER_CLASS_SIZE,
            Self::Digit => DIGIT_CLASS_SIZE,
        }
    }
}

/// The D-05 randomness test, over the token's alphanumeric bytes only.
///
/// With n alphanumeric bytes, c adjacent class changes among them, T the sum of
/// the present class sizes and S the sum of their squares, D-05 requires
/// `c / (n − 1) >= 0.75 × E` with `E = 1 − S / T²` and `E > 0`. This evaluates the
/// same comparison multiplied out, `c × 4 × T² >= 3 × (n − 1) × (T² − S)`, in exact
/// integer arithmetic, so a tie is decided exactly. Fewer than two classes present
/// (E = 0, which also covers fewer than two alphanumeric bytes) fails.
fn passes_randomness_test(token: &[u8]) -> bool {
    let classes: Vec<CharClass> = token.iter().filter_map(|&b| CharClass::of(b)).collect();
    let (present, total, squares) = CharClass::ALL
        .into_iter()
        .filter(|class| classes.contains(class))
        .fold((0u64, 0u64, 0u64), |(present, total, squares), class| {
            (
                present + 1,
                total + class.size(),
                squares + class.size() * class.size(),
            )
        });
    if present < 2 {
        return false;
    }
    let pairs = classes.len() as u64 - 1;
    let changes = classes.windows(2).filter(|w| w[0] != w[1]).count() as u64;
    changes * SWITCH_FACTOR_DEN * total * total
        >= SWITCH_FACTOR_NUM * pairs * (total * total - squares)
}

/// R11 (D-05): the entropy test and the randomness test both pass.
fn passes_r11(token: &[u8]) -> bool {
    shannon_entropy(token) >= ENTROPY_FACTOR * (token.len().min(ENTROPY_LEN_CAP) as f64).log2()
        && passes_randomness_test(token)
}

#[cfg(test)]
use proptest::sample::select;
#[cfg(test)]
use proptest::strategy::{Strategy, ValueTree};
#[cfg(test)]
use proptest::test_runner::TestRunner;

/// Seeded fixture generator: every character is one uniform index draw over an
/// explicit alphabet, the distribution of the R11 calibration probes.
///
/// It never uses a proptest regex-string strategy: those skew character
/// frequencies, so a rate measured on them measures the generator, not R11.
#[cfg(test)]
pub(crate) struct SeededChars {
    runner: TestRunner,
}

#[cfg(test)]
impl SeededChars {
    /// A generator on proptest's fixed deterministic seed.
    pub(crate) fn new() -> Self {
        Self {
            runner: TestRunner::deterministic(),
        }
    }

    /// `len` characters, each drawn uniformly from the characters of `alphabet`
    /// (a character listed twice is twice as likely).
    pub(crate) fn draw(&mut self, alphabet: &str, len: usize) -> String {
        let strategy = select(alphabet.chars().collect::<Vec<char>>());
        (0..len)
            .map(|_| {
                strategy
                    .new_tree(&mut self.runner)
                    .expect("select over a non-empty alphabet never rejects")
                    .current()
            })
            .collect()
    }
}

/// A 32-char token that passes R11 by construction: position `i` takes the
/// `i / 3`-th lowercase letter, uppercase letter or digit by `i % 3`, so all 32
/// characters are distinct (entropy 5.0 bits) and every adjacent pair changes class.
#[cfg(test)]
pub(crate) fn class_cycling_token() -> String {
    const LEN: usize = 32;
    const CLASSES: usize = 3;
    (0..LEN)
        .map(|i| {
            let k = u8::try_from(i / CLASSES).expect("index below 256");
            match i % CLASSES {
                0 => char::from(b'a' + k),
                1 => char::from(b'A' + k),
                _ => char::from(b'0' + k),
            }
        })
        .collect()
}

/// Every file under the template corpus directories `templates/` and
/// `template/`, sorted.
#[cfg(test)]
pub(crate) fn template_corpus_files() -> Vec<std::path::PathBuf> {
    const CORPUS_DIRS: [&str; 2] = ["templates", "template"];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<std::path::PathBuf> = CORPUS_DIRS
        .iter()
        .flat_map(|dir| walkdir::WalkDir::new(root.join(dir)))
        .map(|entry| entry.expect("walk the template corpus"))
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
    const DIGITS: &str = "0123456789";
    const HEX_LOWER: &str = "0123456789abcdef";

    /// Samples per seeded rate row.
    const SAMPLES: usize = 1000;

    /// Standard Base64 alphabet `A-Z a-z 0-9 + /` (64 symbols).
    fn b64() -> String {
        format!("{UPPER}{LOWER}{DIGITS}+/")
    }

    /// URL-safe Base64 alphabet `A-Z a-z 0-9 _ -` (64 symbols).
    fn url() -> String {
        format!("{UPPER}{LOWER}{DIGITS}_-")
    }

    fn blocks(text: &str) -> bool {
        first_high_entropy_line(text.as_bytes()).is_some()
    }

    /// Bar of the D-05 entropy test for a token of `len` bytes.
    fn entropy_bar(len: usize) -> f64 {
        ENTROPY_FACTOR * (len.min(ENTROPY_LEN_CAP) as f64).log2()
    }

    /// Fraction of `SAMPLES` draws of `sample` that R11 blocks.
    fn block_rate(seeded: &mut SeededChars, sample: &dyn Fn(&mut SeededChars) -> String) -> f64 {
        let blocked = (0..SAMPLES).filter(|_| blocks(&sample(seeded))).count();
        blocked as f64 / SAMPLES as f64
    }

    /// Required side of a seeded rate.
    enum Bound {
        AtLeast(f64),
        AtMost(f64),
    }

    type Sampler = Box<dyn Fn(&mut SeededChars) -> String>;

    fn draw_row(alphabet: String, len: usize) -> Sampler {
        Box::new(move |s: &mut SeededChars| s.draw(&alphabet, len))
    }

    #[test]
    fn test_r11_block_rates_seeded() {
        let b64_no_slash = format!("{UPPER}{LOWER}{DIGITS}+");
        let lower_digit = format!("{LOWER}{DIGITS}");
        let upper_digit = format!("{UPPER}{DIGITS}");
        let jwt: Sampler = Box::new(|s: &mut SeededChars| {
            let url = url();
            let header = s.draw(&url, 30);
            let payload = s.draw(&url, 40);
            let signature = s.draw(&url, 43);
            format!("eyJ{header}.eyJ{payload}.{signature}")
        });
        let leading_plus: Sampler =
            Box::new(|s: &mut SeededChars| format!("+{}", s.draw(&b64(), 23)));
        let contains_slash: Sampler = Box::new(move |s: &mut SeededChars| {
            let head = s.draw(&b64_no_slash, 12);
            let tail = s.draw(&b64(), 11);
            format!("{head}/{tail}")
        });
        let rows: Vec<(&str, Bound, Sampler)> = vec![
            ("b64-20", Bound::AtLeast(0.88), draw_row(b64(), 20)),
            ("b64-32", Bound::AtLeast(0.90), draw_row(b64(), 32)),
            ("b64-64", Bound::AtLeast(0.95), draw_row(b64(), 64)),
            ("url-20", Bound::AtLeast(0.88), draw_row(url(), 20)),
            ("url-32", Bound::AtLeast(0.90), draw_row(url(), 32)),
            ("url-64", Bound::AtLeast(0.95), draw_row(url(), 64)),
            ("aws-secret-40", Bound::AtLeast(0.95), draw_row(b64(), 40)),
            ("jwt-shaped", Bound::AtLeast(0.99), jwt),
            ("leading-plus-24", Bound::AtLeast(0.85), leading_plus),
            ("contains-slash-24", Bound::AtLeast(0.85), contains_slash),
            (
                "lower-digit-20",
                Bound::AtLeast(0.70),
                draw_row(lower_digit.clone(), 20),
            ),
            (
                "lower-digit-32",
                Bound::AtLeast(0.70),
                draw_row(lower_digit.clone(), 32),
            ),
            (
                "lower-digit-40",
                Bound::AtLeast(0.70),
                draw_row(lower_digit, 40),
            ),
            (
                "upper-digit-20",
                Bound::AtLeast(0.70),
                draw_row(upper_digit.clone(), 20),
            ),
            (
                "upper-digit-32",
                Bound::AtLeast(0.70),
                draw_row(upper_digit.clone(), 32),
            ),
            (
                "upper-digit-40",
                Bound::AtLeast(0.70),
                draw_row(upper_digit, 40),
            ),
            (
                "hex-32 (control)",
                Bound::AtMost(0.01),
                draw_row(HEX_LOWER.to_string(), 32),
            ),
            (
                "lower-32 (control)",
                Bound::AtMost(0.0),
                draw_row(LOWER.to_string(), 32),
            ),
        ];

        let mut failures = Vec::new();
        for (name, bound, sample) in &rows {
            let rate = block_rate(&mut SeededChars::new(), sample.as_ref());
            eprintln!("R11 seeded block rate {name}: {rate:.3}");
            let ok = match bound {
                Bound::AtLeast(floor) => rate >= *floor,
                Bound::AtMost(ceiling) => rate <= *ceiling,
            };
            if !ok {
                failures.push(format!("{name}: measured {rate:.3}"));
            }
        }
        assert!(
            failures.is_empty(),
            "rows outside their bound: {failures:?}"
        );
    }

    /// True when every symbol of `alphabet` occurs in `drawn` between `low` and
    /// `high` times inclusive.
    fn symbol_counts_in_band(drawn: &str, alphabet: &str, low: usize, high: usize) -> bool {
        alphabet.chars().all(|symbol| {
            let count = drawn.chars().filter(|&c| c == symbol).count();
            (low..=high).contains(&count)
        })
    }

    #[test]
    fn test_r11_fixture_generator_uniform() {
        const DRAWS: usize = 1000;
        const WIDTH: usize = 64;
        const LOW: usize = 850;
        const HIGH: usize = 1150;
        const SKEW_COPIES: usize = 14;
        let alphabet = b64();

        let mut seeded = SeededChars::new();
        let drawn: String = (0..DRAWS).map(|_| seeded.draw(&alphabet, WIDTH)).collect();
        let counts: Vec<usize> = alphabet
            .chars()
            .map(|symbol| drawn.chars().filter(|&c| c == symbol).count())
            .collect();
        eprintln!(
            "uniform generator symbol counts: min {:?} max {:?}",
            counts.iter().min(),
            counts.iter().max()
        );
        assert!(
            symbol_counts_in_band(&drawn, &alphabet, LOW, HIGH),
            "a uniform draw left the {LOW}-{HIGH} band: {counts:?}"
        );

        // Negative control: the same check rejects a skewed generator.
        let skewed_alphabet = format!("{alphabet}{}", "+".repeat(SKEW_COPIES - 1));
        let mut skewed = SeededChars::new();
        let skewed_drawn = skewed.draw(&skewed_alphabet, DRAWS * WIDTH);
        assert!(
            !symbol_counts_in_band(&skewed_drawn, &alphabet, LOW, HIGH),
            "the band check must reject `+` listed {SKEW_COPIES} times"
        );
    }

    #[test]
    fn test_r11_length_boundary() {
        const MAX_TRIES: usize = 100;
        let mut seeded = SeededChars::new();
        let sample = (0..MAX_TRIES)
            .map(|_| seeded.draw(&b64(), MIN_TOKEN_LEN))
            .find(|s| blocks(s) && passes_r11(&s.as_bytes()[..MIN_TOKEN_LEN - 1]))
            .expect("a blocking 20-char draw whose 19-char prefix passes R11 within 100 draws");
        let short = &sample[..MIN_TOKEN_LEN - 1];
        assert_eq!(
            first_high_entropy_line(short.as_bytes()),
            None,
            "a 19-byte run must never be scored"
        );
    }

    #[test]
    fn test_r11_entropy_tie_blocks() {
        let tie = "aB3dE6gH9jK2mN5p".repeat(2);
        assert_eq!(
            first_high_entropy_line(tie.as_bytes()),
            Some(1),
            "exact tie must block"
        );
        assert!(
            shannon_entropy(tie.as_bytes()) >= entropy_bar(tie.len())
                && shannon_entropy(tie.as_bytes()) <= entropy_bar(tie.len()),
            "fixture must sit exactly on the bar"
        );

        let first = tie.chars().next().expect("non-empty");
        let twin = format!("{}{first}", &tie[..tie.len() - 1]);
        assert!(
            shannon_entropy(twin.as_bytes()) < entropy_bar(twin.len()),
            "twin must fall below the bar"
        );
        assert_eq!(
            first_high_entropy_line(twin.as_bytes()),
            None,
            "twin must not block"
        );
    }

    /// Letter/digit class changes between adjacent characters.
    fn letter_digit_changes(token: &str) -> usize {
        token
            .as_bytes()
            .windows(2)
            .filter(|w| w[0].is_ascii_digit() != w[1].is_ascii_digit())
            .count()
    }

    /// Splits `symbols` into `parts` consecutive runs of near-equal size.
    fn runs(symbols: &[char], parts: usize) -> Vec<String> {
        let base = symbols.len() / parts;
        let extra = symbols.len() % parts;
        let mut rest = symbols;
        (0..parts)
            .map(|i| {
                let (run, tail) = rest.split_at(base + usize::from(i < extra));
                rest = tail;
                run.iter().collect()
            })
            .collect()
    }

    /// A 217-char token over `[a-z0-9]` (each letter six times, each digit six
    /// times plus one extra `0`) arranged in `letter_runs` letter runs alternating
    /// with `digit_runs` digit runs, starting with letters.
    fn alternating_token(letter_runs: usize, digit_runs: usize) -> String {
        const ROUNDS: usize = 6;
        let letters: Vec<char> = LOWER.chars().cycle().take(LOWER.len() * ROUNDS).collect();
        let mut digits: Vec<char> = DIGITS.chars().cycle().take(DIGITS.len() * ROUNDS).collect();
        digits.push('0');
        let letter_parts = runs(&letters, letter_runs);
        let digit_parts = runs(&digits, digit_runs);
        let mut token = String::new();
        for i in 0..letter_runs.max(digit_runs) {
            if let Some(run) = letter_parts.get(i) {
                token.push_str(run);
            }
            if let Some(run) = digit_parts.get(i) {
                token.push_str(run);
            }
        }
        token
    }

    #[test]
    fn test_r11_switch_tie_blocks() {
        const LEN: usize = 217;
        let tie = alternating_token(33, 33);
        let below = alternating_token(33, 32);
        // Fixture self-check: same length and symbols, 65 versus 64 changes.
        assert_eq!(tie.len(), LEN);
        assert_eq!(below.len(), LEN);
        assert_eq!(letter_digit_changes(&tie), 65);
        assert_eq!(letter_digit_changes(&below), 64);
        let mut tie_sorted: Vec<char> = tie.chars().collect();
        let mut below_sorted: Vec<char> = below.chars().collect();
        tie_sorted.sort_unstable();
        below_sorted.sort_unstable();
        assert_eq!(
            tie_sorted, below_sorted,
            "twins must hold the same symbol counts"
        );

        assert_eq!(
            first_high_entropy_line(tie.as_bytes()),
            Some(1),
            "65 changes is the tie and must block"
        );
        assert_eq!(
            first_high_entropy_line(below.as_bytes()),
            None,
            "64 changes must not block"
        );
        assert!(
            shannon_entropy(tie.as_bytes()) >= entropy_bar(LEN),
            "the entropy test must not be what decides"
        );
    }

    #[test]
    fn test_r11_single_class_never_blocks() {
        // About 610 of 1000 such draws clear the entropy bar (Python estimate at
        // implementation time); half of that is the floor.
        const MIN_ENTROPY_PASSES: usize = 300;
        const LEN: usize = 32;
        for alphabet in [LOWER, UPPER] {
            let mut seeded = SeededChars::new();
            let samples: Vec<String> = (0..SAMPLES).map(|_| seeded.draw(alphabet, LEN)).collect();
            let entropy_passes = samples
                .iter()
                .filter(|s| shannon_entropy(s.as_bytes()) >= entropy_bar(LEN))
                .count();
            let blocked = samples.iter().filter(|s| blocks(s)).count();
            eprintln!("single-class draws clearing the entropy bar: {entropy_passes} of {SAMPLES}");
            assert_eq!(blocked, 0, "single-class draws must never block");
            assert!(
                entropy_passes >= MIN_ENTROPY_PASSES,
                "only {entropy_passes} single-class draws cleared the entropy bar"
            );
        }
    }

    /// `.env`-style names, assembled from parts.
    fn env_prefixes() -> Vec<String> {
        [
            vec!["API", "TOKEN"],
            vec!["CONFIG", "PARAMETER", "KEY"],
            vec!["MY", "SERVICE", "CLIENT", "SECRET", "VALUE"],
        ]
        .iter()
        .map(|parts| format!("{}=", parts.join("_")))
        .collect()
    }

    #[test]
    fn test_r11_env_assignment_rates_seeded() {
        const FLOOR: f64 = 0.90;
        let mut seeded = SeededChars::new();
        let values: Vec<String> = (0..SAMPLES).map(|_| seeded.draw(&b64(), 32)).collect();
        let rate = |count: usize| count as f64 / SAMPLES as f64;

        let bare = values.iter().filter(|v| blocks(v)).count();
        eprintln!("bare 32-char Base64 rate: {:.3}", rate(bare));
        assert!(
            rate(bare) >= FLOOR,
            "bare rate {:.3} below {FLOOR}",
            rate(bare)
        );

        for prefix in env_prefixes() {
            let assigned = values
                .iter()
                .filter(|v| blocks(&format!("{prefix}{v}")))
                .count();
            let glued = values
                .iter()
                .filter(|v| passes_r11(format!("{prefix}{v}").as_bytes()))
                .count();
            eprintln!(
                "{prefix}: OD-1 rate {:.3}, glued rate {:.3}",
                rate(assigned),
                rate(glued)
            );
            assert_eq!(
                assigned,
                bare,
                "{prefix}: OD-1 rate {:.3} differs from bare {:.3}",
                rate(assigned),
                rate(bare)
            );
            // Negative control: scoring the glued run as one token misses more.
            assert!(
                rate(glued) < FLOOR,
                "{prefix}: glued rate {:.3} not below {FLOOR}",
                rate(glued)
            );
        }
    }

    fn tokens(line: &str) -> Vec<(usize, String)> {
        candidate_tokens(line.as_bytes())
            .map(|(start, token)| (start, String::from_utf8_lossy(token).into_owned()))
            .collect()
    }

    #[test]
    fn test_r11_tokenizer_padding_and_delimiters() {
        let mut seeded = SeededChars::new();
        let v42 = seeded.draw(&b64(), 42);
        let v43 = seeded.draw(&b64(), 43);
        let v30 = seeded.draw(&b64(), 30);
        let v32 = seeded.draw(&b64(), 32);
        let v25 = seeded.draw(&b64(), 25);
        let w25 = seeded.draw(&b64(), 25);

        assert_eq!(
            tokens(&format!("X={v42}==")),
            vec![(2, format!("{v42}=="))],
            "two padding"
        );
        assert_eq!(
            tokens(&format!("X={v43}=")),
            vec![(2, format!("{v43}="))],
            "one padding"
        );
        assert_eq!(
            tokens(&format!("X={v30}===")),
            vec![(2, v30.clone())],
            "three `=` are not padding"
        );

        let name = ["CONFIG", "PARAMETER", "KEY"].join("_");
        assert_eq!(
            tokens(&format!("{name}={v32}")),
            vec![(0, name.clone()), (name.len() + 1, v32.clone())],
            "`=` splits NAME=value"
        );
        assert_eq!(
            tokens(&format!("{v25}=={w25}")),
            vec![(0, v25.clone()), (v25.len() + 2, w25.clone())],
            "padding must end the run"
        );
        assert_eq!(
            tokens(&format!("\"sha256-{v43}=\"")),
            vec![(1, format!("sha256-{v43}="))],
            "padding before a quote"
        );
        assert!(tokens(&v30[..MIN_TOKEN_LEN - 1]).is_empty(), "19-byte run");
        assert_eq!(
            tokens(&v30[..MIN_TOKEN_LEN]),
            vec![(0, v30[..MIN_TOKEN_LEN].to_string())],
            "20-byte run"
        );
    }
    const HEX_UPPER: &str = "0123456789ABCDEF";

    /// Fixed leading bytes of an ssh-ed25519 public-key body (`AAAA` + the
    /// encoded key-type length and name).
    const SSH_ED25519_BODY_HEAD: &str = "AAAAC3NzaC1lZDI1NTE5AAAAI";

    /// Canonical lowercase UUID fixture.
    const UUID_LOWER: &str = "123e4567-e89b-12d3-a456-426614174000";

    /// Draws to try when selecting a seeded value by predicate.
    const MAX_DRAWS: usize = 100;

    /// The first of up to [`MAX_DRAWS`] draws of `len` chars over `alphabet` that
    /// `accept` takes, so a fixture's outcome rests on the shape logic, not on a
    /// lucky seed.
    fn first_draw_where(
        seeded: &mut SeededChars,
        alphabet: &str,
        len: usize,
        accept: impl Fn(&str) -> bool,
    ) -> String {
        (0..MAX_DRAWS)
            .map(|_| seeded.draw(alphabet, len))
            .find(|draw| accept(draw))
            .unwrap_or_else(|| panic!("no accepted {len}-char draw within {MAX_DRAWS} draws"))
    }

    /// A seeded 32-char Base64 token that passes R11 as one unexempted token.
    fn positive_token() -> String {
        first_draw_where(&mut SeededChars::new(), &b64(), 32, |d| {
            passes_r11(d.as_bytes())
        })
    }

    /// A seeded ssh-ed25519 key body that passes R11 as one unexempted token.
    fn ssh_body() -> String {
        let tail = first_draw_where(&mut SeededChars::new(), &b64(), 43, |d| {
            passes_r11(format!("{SSH_ED25519_BODY_HEAD}{d}").as_bytes())
        });
        format!("{SSH_ED25519_BODY_HEAD}{tail}")
    }

    #[test]
    fn test_r11_negative_fixtures() {
        let body = ssh_body();
        let token = positive_token();
        let mut seeded = SeededChars::new();
        let hex40 = seeded.draw(HEX_LOWER, 40);
        let hex64 = seeded.draw(HEX_LOWER, 64);
        let hex128 = seeded.draw(HEX_LOWER, 128);
        let sri384 = first_draw_where(&mut seeded, &b64(), 64, |d| {
            passes_r11(format!("sha384-{d}").as_bytes())
        });
        let lines: Vec<(&str, String)> =
            vec![
            ("CamelCase", "SanitizerBoundaryHardeningPhaseTwo".to_string()),
            ("snake_case", "sanitizer_boundaries_security_hardening".to_string()),
            (
                "file path",
                "/var/home/user/Github/universal-dev-env/src/sanitizer/mod.rs".to_string(),
            ),
            (
                "URL",
                "https://raw.githubusercontent.com/gitleaks/gitleaks/v8.30.1/config/gitleaks.toml"
                    .to_string(),
            ),
            ("target triple", "x86_64-pc-windows-msvc".to_string()),
            ("UUID", UUID_LOWER.to_string()),
            ("hex-40", hex40),
            ("hex-64", hex64),
            ("hex-128", hex128),
            ("SRI sha384", format!("sha384-{sri384}")),
            ("ssh line", format!("ssh-ed25519 {body} user@host")),
            (
                "gitconfig literal key",
                format!("\tsigningkey = key::ssh-ed25519 {body} user@host"),
            ),
            (
                "unspaced gitconfig",
                format!("signingkey=key::ssh-ed25519 {body}"),
            ),
            (
                "JSON value",
                format!("\"signingKey\": \"ssh-ed25519 {body} user@host\","),
            ),
            ("single-quoted", format!("'ssh-ed25519 {body}'")),
            ("comma-led", format!("a,ssh-ed25519 {body}")),
            (
                "sentence",
                "ThisIsAPerfectlyNormalSentenceWithWords".to_string(),
            ),
        ];
        for (name, line) in &lines {
            assert_eq!(
                first_high_entropy_line(line.as_bytes()),
                None,
                "{name} must scan clean"
            );
            // Paired positive: a random token later on the same line still blocks
            // (for the ssh lines: only the key body is exempt).
            assert_eq!(
                first_high_entropy_line(format!("{line} {token}").as_bytes()),
                Some(1),
                "{name} with a token appended must block"
            );
        }
    }

    #[test]
    fn test_r11_shape_exemption_is_whole_token() {
        let mut seeded = SeededChars::new();
        let shared = first_draw_where(&mut seeded, &b64(), 43, |d| {
            passes_r11(format!("xsha256-{d}=").as_bytes())
                && passes_r11(format!("sha256-{d}=").as_bytes())
        });
        let exempt = format!("sha256-{shared}=");
        assert!(
            passes_r11(exempt.as_bytes()),
            "the exempt SRI twin must pass R11 when scored"
        );
        assert_eq!(
            first_high_entropy_line(exempt.as_bytes()),
            None,
            "exact SRI sha256"
        );
        assert_eq!(
            first_high_entropy_line(format!("xsha256-{shared}=").as_bytes()),
            Some(1),
            "a glued SRI-like token is not a whole SRI token"
        );

        // Wrong-length SRI bodies are scored: (prefix, body chars, suffix).
        let wrong_lengths = [
            ("sha256-", 32, ""),
            ("sha256-", 43, ""),
            ("sha384-", 63, ""),
            ("sha512-", 43, "="),
        ];
        for (prefix, len, suffix) in wrong_lengths {
            let body = first_draw_where(&mut seeded, &b64(), len, |d| {
                passes_r11(format!("{prefix}{d}{suffix}").as_bytes())
            });
            assert_eq!(
                first_high_entropy_line(format!("{prefix}{body}{suffix}").as_bytes()),
                Some(1),
                "{prefix} + {len} chars + {suffix:?} must block"
            );
        }

        let body = ssh_body();
        assert_eq!(
            first_high_entropy_line(format!("ssh-ed25519 {body}").as_bytes()),
            None,
            "key body after a key type is exempt"
        );
        for context in ["key ", "key::", "xssh-ed25519 "] {
            assert_eq!(
                first_high_entropy_line(format!("{context}{body}").as_bytes()),
                Some(1),
                "body after {context:?} must block"
            );
        }
        assert_eq!(
            first_high_entropy_line(format!("ssh-ed25519\n{body}").as_bytes()),
            Some(2),
            "a key type on the previous line exempts nothing"
        );
    }

    #[test]
    fn test_exempt_shape_predicate() {
        let mut seeded = SeededChars::new();
        let mixed_hex = seeded.draw("0123456789abcdefABCDEF", 40);
        assert!(
            mixed_hex.bytes().any(|b| b.is_ascii_lowercase())
                && mixed_hex.bytes().any(|b| b.is_ascii_uppercase()),
            "fixture self-check: mixed-case hex holds both cases"
        );
        let uuid_mixed = format!(
            "{}{}",
            &UUID_LOWER[..9],
            UUID_LOWER[9..].to_ascii_uppercase()
        );
        let b64 = b64();
        let exempt = vec![
            ("lower hex 40", seeded.draw(HEX_LOWER, 40)),
            ("lower hex 64", seeded.draw(HEX_LOWER, 64)),
            ("lower hex 128", seeded.draw(HEX_LOWER, 128)),
            ("upper hex 40", seeded.draw(HEX_UPPER, 40)),
            ("lower UUID", UUID_LOWER.to_string()),
            ("upper UUID", UUID_LOWER.to_ascii_uppercase()),
            ("SRI sha256", format!("sha256-{}=", seeded.draw(&b64, 43))),
            ("SRI sha384", format!("sha384-{}", seeded.draw(&b64, 64))),
            ("SRI sha512", format!("sha512-{}==", seeded.draw(&b64, 86))),
        ];
        let mut not_exempt: Vec<(String, String)> = [39, 41, 63, 65, 127, 129]
            .into_iter()
            .map(|len| (format!("lower hex {len}"), seeded.draw(HEX_LOWER, len)))
            .collect();
        let shapes = [
            ("mixed-case hex 40", mixed_hex),
            ("mixed-case UUID", uuid_mixed),
            (
                "UUID with an 11-char last group",
                UUID_LOWER[..UUID_LOWER.len() - 1].to_string(),
            ),
            ("sha256- + 32", format!("sha256-{}", seeded.draw(&b64, 32))),
            (
                "sha256- + 43 without =",
                format!("sha256-{}", seeded.draw(&b64, 43)),
            ),
            (
                "sha256- + 42 + ==",
                format!("sha256-{}==", seeded.draw(&b64, 42)),
            ),
            ("sha384- + 63", format!("sha384-{}", seeded.draw(&b64, 63))),
            (
                "sha384- + 62 + ==",
                format!("sha384-{}==", seeded.draw(&b64, 62)),
            ),
            (
                "sha512- + 43 + =",
                format!("sha512-{}=", seeded.draw(&b64, 43)),
            ),
            (
                "sha512- + 86 without ==",
                format!("sha512-{}", seeded.draw(&b64, 86)),
            ),
            ("sha1- + 27 + =", format!("sha1-{}=", seeded.draw(&b64, 27))),
            (
                "xsha256- + 43 + =",
                format!("xsha256-{}=", seeded.draw(&b64, 43)),
            ),
            (
                "sha256- + 42 + _ + =",
                format!("sha256-{}_=", seeded.draw(&b64, 42)),
            ),
        ];
        not_exempt.extend(shapes.into_iter().map(|(name, t)| (name.to_string(), t)));

        for (name, token) in &exempt {
            assert!(is_exempt_shape(token.as_bytes()), "{name} must be exempt");
        }
        for (name, token) in &not_exempt {
            assert!(
                !is_exempt_shape(token.as_bytes()),
                "{name} must not be exempt"
            );
        }
    }

    #[test]
    fn test_r11_templates_corpus_zero_hits() {
        const MIN_FILES: usize = 33;
        const MIN_TOKENS: usize = 200;
        let files = template_corpus_files();
        let mut token_count = 0;
        let mut hits = Vec::new();
        for file in &files {
            let bytes = std::fs::read(file).expect("read a corpus file");
            token_count += bytes
                .split(|&b| b == b'\n')
                .map(|line| candidate_tokens(line).count())
                .sum::<usize>();
            if let Some(line) = first_high_entropy_line(&bytes) {
                hits.push(format!("{}:{line}", file.display()));
            }
        }
        eprintln!(
            "template corpus: {} files, {token_count} candidate tokens, {} hits",
            files.len(),
            hits.len()
        );
        assert!(
            files.len() >= MIN_FILES,
            "only {} corpus files read",
            files.len()
        );
        assert!(
            token_count >= MIN_TOKENS,
            "only {token_count} candidate tokens"
        );
        assert!(hits.is_empty(), "R11 hits in the template corpus: {hits:?}");

        // Negative control: a runtime token appended to a copy of a corpus file hits.
        let dir = tempfile::tempdir().expect("temp dir");
        let source = &files[0];
        let copy = dir.path().join(source.file_name().expect("file name"));
        std::fs::copy(source, &copy).expect("copy a corpus file");
        let mut bytes = std::fs::read(&copy).expect("read the copy");
        bytes.extend_from_slice(format!("\n{}\n", class_cycling_token()).as_bytes());
        std::fs::write(&copy, &bytes).expect("write the copy");
        assert!(
            first_high_entropy_line(&std::fs::read(&copy).expect("re-read the copy")).is_some(),
            "{}: appended token not found",
            copy.display()
        );
    }
}
