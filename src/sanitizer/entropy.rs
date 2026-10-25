//! R11 entropy screen for credential-shaped tokens (SEC-01, D-04, D-05).

/// Rule name reported for an entropy-only hit.
pub const RULE_NAME: &str = "high-entropy";

/// Minimum candidate length in bytes, padding included (D-04).
// RED stub: candidate_tokens uses it once implemented.
#[cfg_attr(not(test), allow(dead_code))]
const MIN_TOKEN_LEN: usize = 20;

/// Fraction of the length-capped maximum entropy a token must reach (D-05).
const ENTROPY_FACTOR: f64 = 0.80;

/// Length above which the entropy bar stops rising: the bar is
/// `ENTROPY_FACTOR × log2(min(len, ENTROPY_LEN_CAP))` (D-05).
const ENTROPY_LEN_CAP: usize = 64;

/// 1-based number of the first line holding a token that passes R11, if any.
pub fn first_high_entropy_line(bytes: &[u8]) -> Option<usize> {
    bytes
        .split(|&b| b == b'\n')
        .position(|line| candidate_tokens(line).any(|(_, token)| passes_r11(token)))
        .map(|index| index + 1)
}

/// Candidate tokens of one line with their start offsets.
fn candidate_tokens(_line: &[u8]) -> impl Iterator<Item = (usize, &[u8])> {
    std::iter::empty()
}

/// Shannon entropy of the token's byte frequencies, in bits.
fn shannon_entropy(_token: &[u8]) -> f64 {
    0.0
}

/// The randomness test of D-05.
fn passes_randomness_test(_token: &[u8]) -> bool {
    false
}

/// R11: the entropy test and the randomness test both pass.
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
}
