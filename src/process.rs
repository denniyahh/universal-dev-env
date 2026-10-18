//! Bounded subprocess runner for every `git` / `gh` spawn in UDE (SYNC-03).
// RED stub: the runner has no production callers until the GREEN commit wires them.
#![allow(dead_code)]

use std::process::{Command, ExitStatus};
use std::time::Duration;

/// Time limit for local git queries (config reads, index updates, ls-files).
pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(30);

/// Time limit for network operations (clone, fetch, push, ls-remote) and every `gh` call.
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(60);

/// Interval between child and reader polls.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// How long the output reader threads may take to reach EOF after the process
/// group was killed before they are detached.
const KILL_GRACE: Duration = Duration::from_secs(1);

/// Repository-local git variables (`git rev-parse --local-env-vars`, git 2.56.0).
/// An inherited value would redirect a spawned git to another repository.
pub const GIT_LOCAL_ENV_VARS: [&str; 15] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

/// Exit status and captured output of a child run through [`run_bounded`].
#[derive(Debug)]
pub struct RunOutput {
    /// Exit status of the child process.
    pub status: ExitStatus,
    /// Everything the child wrote to stdout, in order.
    pub stdout: Vec<u8>,
    /// Everything the child wrote to stderr, in order.
    pub stderr: Vec<u8>,
}

/// RED stub: reproduces today's behaviour (no scrub, no non-interactive env).
pub fn prepare_command(_cmd: &mut Command) {}

/// RED stub: reproduces today's behaviour (plain std output, no deadline, no label).
pub fn run_bounded(
    mut cmd: Command,
    _label: &str,
    _timeout: Duration,
) -> Result<RunOutput, String> {
    cmd.output()
        .map(|out| RunOutput {
            status: out.status,
            stdout: out.stdout,
            stderr: out.stderr,
        })
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsStr;
    use std::process::Stdio;
    use std::time::Instant;

    const LABEL: &str = "test command";

    /// Kills a recorded PID with the `kill` binary when dropped, so a failed
    /// assertion still cleans up the background process it was checking.
    struct PidGuard(u32);

    impl Drop for PidGuard {
        fn drop(&mut self) {
            let _ = Command::new("kill")
                .args(["-KILL", &self.0.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    #[cfg(unix)]
    fn sh(script: &str, args: &[&OsStr]) -> Command {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script).arg("sh").args(args);
        cmd
    }

    #[cfg(target_os = "linux")]
    fn read_pid(path: &std::path::Path) -> u32 {
        let text = std::fs::read_to_string(path).expect("read pidfile");
        let trimmed = text.trim();
        assert!(!trimmed.is_empty(), "pidfile empty");
        trimmed.parse().expect("pidfile holds a PID")
    }

    #[cfg(target_os = "linux")]
    fn parse_pid(stdout: &[u8]) -> u32 {
        let text = String::from_utf8_lossy(stdout);
        let trimmed = text.trim();
        assert!(!trimmed.is_empty(), "child printed no PID");
        trimmed.parse().expect("child printed a PID")
    }

    /// Alive = /proc/PID present and its State is neither zombie nor dead.
    #[cfg(target_os = "linux")]
    fn is_alive(pid: u32) -> bool {
        let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
            return false;
        };
        let state = status
            .lines()
            .find_map(|line| line.strip_prefix("State:"))
            .expect("/proc status has a State line")
            .trim();
        !(state.starts_with('Z') || state.starts_with('X'))
    }

    /// A SIGKILLed process closes its pipes a moment before the kernel marks it
    /// a zombie; give it a short window so the check measures the kill, not that race.
    #[cfg(target_os = "linux")]
    fn dies_within(pid: u32, window: Duration) -> bool {
        let limit = Instant::now() + window;
        while is_alive(pid) {
            if Instant::now() >= limit {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }

    #[cfg(target_os = "linux")]
    fn setsid_path() -> std::path::PathBuf {
        which::which("setsid").expect("setsid (util-linux) not found on PATH")
    }

    #[test]
    fn test_run_bounded_captures_git_output() {
        let mut version = Command::new("git");
        version.arg("--version");
        let out = run_bounded(version, LABEL, Duration::from_secs(10)).expect("git --version runs");
        assert!(
            out.status.success(),
            "git --version failed: {:?}",
            out.status
        );
        assert!(
            out.stdout.starts_with(b"git version"),
            "unexpected stdout: {:?}",
            String::from_utf8_lossy(&out.stdout)
        );

        // Negative control: the spawn works, the command fails.
        let mut bogus = Command::new("git");
        bogus.arg("definitely-not-a-subcommand");
        let out = run_bounded(bogus, LABEL, Duration::from_secs(10)).expect("git spawns");
        assert!(
            !out.status.success(),
            "bogus git subcommand reported success"
        );
    }

    #[test]
    fn test_spawn_failure_names_label() {
        let mut cmd = Command::new("ude-test-no-such-program-7f3a9c");
        cmd.arg("https://user:secret@example.invalid/x");
        let err = match run_bounded(cmd, "fake fetch", Duration::from_secs(10)) {
            Ok(out) => panic!("missing program spawned: {:?}", out.status),
            Err(e) => e,
        };
        assert!(
            err.contains("Failed to spawn"),
            "error lacks spawn context: {err}"
        );
        assert!(err.contains("fake fetch"), "error lacks the label: {err}");
        assert!(!err.contains("secret"), "error leaks an argument: {err}");
        assert!(
            !err.contains("example.invalid"),
            "error leaks an argument: {err}"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_timeout_kills_group() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let pidfile = dir.path().join("bg.pid");
        let cmd = sh(
            r#"sleep 5 & echo $! > "$1"; sleep 5"#,
            &[pidfile.as_os_str()],
        );

        let start = Instant::now();
        let result = run_bounded(cmd, LABEL, Duration::from_millis(300));
        let elapsed = start.elapsed();
        let grandchild = PidGuard(read_pid(&pidfile));

        let err = match result {
            Ok(out) => panic!("hung child returned Ok after {elapsed:?}: {:?}", out.status),
            Err(e) => e,
        };
        assert!(err.contains(LABEL), "timeout error lacks the label: {err}");
        assert!(
            err.contains("timed out after 0.3s"),
            "timeout error lacks the limit: {err}"
        );
        assert!(elapsed < Duration::from_secs(3), "timeout took {elapsed:?}");
        assert!(
            dies_within(grandchild.0, Duration::from_secs(1)),
            "background grandchild {} survived the timeout: the process group was not killed",
            grandchild.0
        );

        // Negative control: a fast child under the same limit is not killed.
        let out = run_bounded(sh("echo ok", &[]), LABEL, Duration::from_millis(300))
            .expect("fast child returns Ok");
        assert!(out.status.success());
        assert_eq!(out.stdout, b"ok\n");
    }

    #[test]
    #[cfg(unix)]
    fn test_drains_large_output() {
        const MIB: usize = 1_048_576;
        let cmd = sh(
            "head -c 1048576 /dev/zero; head -c 1048576 /dev/zero >&2",
            &[],
        );
        let out = run_bounded(cmd, LABEL, Duration::from_secs(10)).expect("large output drains");
        assert!(out.status.success());
        assert_eq!(out.stdout.len(), MIB, "stdout length");
        assert_eq!(out.stderr.len(), MIB, "stderr length");

        // Negative control: a silent child returns empty buffers and its real status.
        let out = run_bounded(sh("true", &[]), LABEL, Duration::from_secs(10))
            .expect("silent child runs");
        assert!(out.status.success());
        assert!(out.stdout.is_empty(), "stdout not empty: {:?}", out.stdout);
        assert!(out.stderr.is_empty(), "stderr not empty: {:?}", out.stderr);
    }

    #[test]
    #[cfg(unix)]
    fn test_env_scrub() {
        // Structural: prepare_command removes every repo-local variable and sets the
        // non-interactive variables, leaving the global config override alone.
        let mut cmd = Command::new("git");
        cmd.env("GIT_DIR", "/decoy")
            .env("GIT_CONFIG_GLOBAL", "/keep");
        prepare_command(&mut cmd);
        let envs: HashMap<&OsStr, Option<&OsStr>> = cmd.get_envs().collect();
        for var in GIT_LOCAL_ENV_VARS {
            assert_eq!(
                envs.get(OsStr::new(var)),
                Some(&None),
                "{var} is not removed"
            );
        }
        assert_eq!(
            envs.get(OsStr::new("GIT_TERMINAL_PROMPT")),
            Some(&Some(OsStr::new("0")))
        );
        assert_eq!(
            envs.get(OsStr::new("GH_PROMPT_DISABLED")),
            Some(&Some(OsStr::new("1")))
        );
        assert_eq!(
            envs.get(OsStr::new("GH_NO_UPDATE_NOTIFIER")),
            Some(&Some(OsStr::new("1")))
        );
        assert_eq!(
            envs.get(OsStr::new("GIT_CONFIG_GLOBAL")),
            Some(&Some(OsStr::new("/keep")))
        );

        // Behavioural: the child really sees the scrubbed environment.
        let script = r#"printf "%s|%s" "${GIT_DIR-unset}" "${GIT_TERMINAL_PROMPT-unset}""#;
        let mut scrubbed = sh(script, &[]);
        scrubbed.env("GIT_DIR", "/decoy");
        let out =
            run_bounded(scrubbed, LABEL, Duration::from_secs(10)).expect("scrubbed child runs");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "unset|0");

        // Negative control: the same command through plain std sees the decoy. The
        // prompt variable is removed so a value exported by the host cannot leak in.
        let mut plain = sh(script, &[]);
        plain
            .env("GIT_DIR", "/decoy")
            .env_remove("GIT_TERMINAL_PROMPT");
        let out = plain.output().expect("plain child runs");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "/decoy|unset");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_background_process_without_pipes_survives() {
        // A background job that closed the inherited pipes survives a normal leader exit.
        let start = Instant::now();
        let out = run_bounded(
            sh("sleep 6 >/dev/null 2>&1 & echo $!", &[]),
            LABEL,
            Duration::from_secs(10),
        )
        .expect("leader exits normally");
        let elapsed = start.elapsed();
        let detached = PidGuard(parse_pid(&out.stdout));
        assert!(elapsed < Duration::from_secs(2), "call took {elapsed:?}");
        assert!(
            is_alive(detached.0),
            "background job without pipes was killed"
        );

        // Negative control: a background job holding the pipes (same group) is killed at
        // the deadline, and the call still returns Ok with the leader's output.
        let start = Instant::now();
        let out = run_bounded(
            sh("sleep 6 & echo $!", &[]),
            LABEL,
            Duration::from_millis(500),
        )
        .expect("leader exited, pipe-holding group member killed");
        let elapsed = start.elapsed();
        let holder = PidGuard(parse_pid(&out.stdout));
        assert!(elapsed < Duration::from_secs(3), "call took {elapsed:?}");
        assert!(
            dies_within(holder.0, Duration::from_secs(1)),
            "pipe-holding background job {} survived",
            holder.0
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_pipe_holder_outside_group_timeout_path() {
        let setsid = setsid_path();
        let dir = tempfile::TempDir::new().expect("tempdir");
        let pidfile = dir.path().join("holder.pid");
        let cmd = sh(
            r#""$2" sleep 10 & echo $! > "$1"; sleep 10"#,
            &[pidfile.as_os_str(), setsid.as_os_str()],
        );

        let start = Instant::now();
        let result = run_bounded(cmd, LABEL, Duration::from_millis(300));
        let elapsed = start.elapsed();
        let holder = PidGuard(read_pid(&pidfile));

        let err = match result {
            Ok(out) => panic!("hung child returned Ok after {elapsed:?}: {:?}", out.status),
            Err(e) => e,
        };
        assert!(err.contains(LABEL), "timeout error lacks the label: {err}");
        assert!(
            err.contains("timed out after 0.3s"),
            "timeout error lacks the limit: {err}"
        );
        assert!(elapsed < Duration::from_secs(5), "timeout took {elapsed:?}");
        // Negative control: the holder really was outside the killed group, so the early
        // return came from the bounded reader join, not from the group kill.
        assert!(is_alive(holder.0), "outside holder {} is dead", holder.0);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_pipe_holder_outside_group_post_exit_path() {
        let setsid = setsid_path();
        let dir = tempfile::TempDir::new().expect("tempdir");
        let pidfile = dir.path().join("holder.pid");
        let cmd = sh(
            r#""$2" sleep 10 & echo $! > "$1""#,
            &[pidfile.as_os_str(), setsid.as_os_str()],
        );

        let start = Instant::now();
        let result = run_bounded(cmd, LABEL, Duration::from_millis(300));
        let elapsed = start.elapsed();
        let holder = PidGuard(read_pid(&pidfile));

        let err = match result {
            Ok(out) => panic!(
                "pipes held open but returned Ok after {elapsed:?}: {:?}",
                out.status
            ),
            Err(e) => e,
        };
        assert!(err.contains(LABEL), "error lacks the label: {err}");
        assert!(
            err.contains("still held open"),
            "error does not name the held pipes: {err}"
        );
        assert!(elapsed < Duration::from_secs(5), "call took {elapsed:?}");
        // Negative control: the holder is alive, so it was outside the group.
        assert!(is_alive(holder.0), "outside holder {} is dead", holder.0);
    }
}
