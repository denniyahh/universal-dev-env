use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use tempfile::tempdir;

/// Repository-local git variables (`git rev-parse --local-env-vars`). This crate cannot
/// import the binary's `process::GIT_LOCAL_ENV_VARS`, so it keeps its own copy.
const FIXTURE_GIT_LOCAL_ENV_VARS: [&str; 15] = [
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

const FIXTURE_GITCONFIG: &str =
    "[user]\n\tname = UDE Tester\n\temail = tester@example.com\n[commit]\n\tgpgsign = false\n";

/// The only `ude` Command constructor in this file: drops the host
/// `TYPESAFE_API_KEY` so no test run can reach the TypeSafe API with the
/// operator's key or have its exit code depend on a remote verdict.
fn ude_command(bin: &str) -> Command {
    let mut cmd = Command::new(bin);
    cmd.env_remove("TYPESAFE_API_KEY");
    // Git discovery stops at the temp dir: a TMPDIR inside a checkout would otherwise
    // put every "no git" test inside that repository, and init would write to it.
    cmd.env("GIT_CEILING_DIRECTORIES", std::env::temp_dir());
    cmd
}

/// The only git Command constructor in this file: ignores inherited repo-local
/// variables, reads `global_config` as its only global config, never prompts.
fn scrubbed_git(dir: &Path, global_config: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir);
    for name in FIXTURE_GIT_LOCAL_ENV_VARS {
        cmd.env_remove(name);
    }
    cmd.env("GIT_CONFIG_GLOBAL", global_config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
        .stdin(Stdio::null());
    cmd
}

#[test]
fn test_fixture_commands_stop_git_discovery_at_the_temp_dir() {
    let ceiling = std::env::temp_dir();
    let global = ceiling.join("unused-gitconfig");
    for cmd in [ude_command("ude"), scrubbed_git(&ceiling, &global)] {
        let value = cmd
            .get_envs()
            .find(|(name, _)| *name == "GIT_CEILING_DIRECTORIES")
            .and_then(|(_, value)| value);
        assert_eq!(value, Some(ceiling.as_os_str()), "{:?}", cmd.get_program());
    }
}

/// Runs a fixture git step and panics with its args and stderr if it fails.
fn run_checked(mut cmd: Command, args: &[&str]) -> Output {
    let out = cmd
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("fixture step `git {}` did not start: {e}", args.join(" ")));
    assert!(
        out.status.success(),
        "fixture step `git {}` failed ({}): {}",
        args.join(" "),
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Hermetic git runner for fixtures, with its own gitconfig (test identity, no signing).
struct GitFixture {
    _home: tempfile::TempDir,
    gitconfig: PathBuf,
}

impl GitFixture {
    fn new() -> Self {
        let home = tempdir().expect("create fixture gitconfig dir");
        let gitconfig = home.path().join("gitconfig");
        fs::write(&gitconfig, FIXTURE_GITCONFIG).expect("write fixture gitconfig");
        Self {
            _home: home,
            gitconfig,
        }
    }

    fn cmd(&self, dir: &Path) -> Command {
        scrubbed_git(dir, &self.gitconfig)
    }

    fn run(&self, dir: &Path, args: &[&str]) {
        run_checked(self.cmd(dir), args);
    }

    fn output(&self, dir: &Path, args: &[&str]) -> String {
        let out = run_checked(self.cmd(dir), args);
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}

#[test]
fn test_cli_e2e_init_and_sync_lifecycle() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let proj = tempdir().unwrap();

    // 1. Run `ude init` with --path
    let init_status = ude_command(bin)
        .args([
            "init",
            "--path",
            &proj.path().to_string_lossy(),
            "--type",
            "rust",
        ])
        .status()
        .expect("ude init execution");
    assert!(init_status.success(), "ude init must succeed");

    // Verify state.json was created
    let state_file = proj.path().join(".ude/state.json");
    assert!(
        state_file.exists(),
        ".ude/state.json must be created by init"
    );
    let state_content = fs::read_to_string(&state_file).unwrap();
    assert!(state_content.contains("\"version\": 2"));
    assert!(state_content.contains(".gitconfig"));
    assert!(state_content.contains("mise.toml"));

    // 2. Run `ude sync status` - all managed files must be InSync
    let status_output = ude_command(bin)
        .current_dir(proj.path())
        .args(["sync", "status"])
        .output()
        .expect("ude sync status execution");
    assert!(status_output.status.success());
    let stdout = String::from_utf8_lossy(&status_output.stdout);
    assert!(
        stdout.contains(".gitconfig: In Sync"),
        "Output was: {stdout}"
    );
    assert!(
        stdout.contains("mise.toml: In Sync"),
        "Output was: {stdout}"
    );

    // 3. Edit local file
    let loc_mise = proj.path().join("mise.toml");
    fs::write(&loc_mise, "# Locally modified\n[tools]\nrust = '1.99'\n").unwrap();

    let status_output2 = ude_command(bin)
        .current_dir(proj.path())
        .args(["sync", "status"])
        .output()
        .expect("ude sync status execution");
    let stdout2 = String::from_utf8_lossy(&status_output2.stdout);
    assert!(
        stdout2.contains("mise.toml: Locally Modified"),
        "Output was: {stdout2}"
    );
    assert!(
        stdout2.contains(".gitconfig: In Sync"),
        "Output was: {stdout2}"
    );
}

#[test]
fn test_sync_status_crlf_only_change_in_sync() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let proj = tempdir().unwrap();

    let init = ude_command(bin)
        .args([
            "init",
            "--path",
            &proj.path().to_string_lossy(),
            "--type",
            "rust",
        ])
        .output()
        .expect("ude init execution");
    assert!(
        init.status.success(),
        "ude init must succeed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let status = || -> String {
        let mut cmd = ude_command(bin);
        cmd.current_dir(proj.path())
            .args(["sync", "status"])
            .stdin(Stdio::null());
        for name in FIXTURE_GIT_LOCAL_ENV_VARS {
            cmd.env_remove(name);
        }
        let out = cmd.output().expect("ude sync status execution");
        String::from_utf8_lossy(&out.stdout).to_string()
    };

    let gitconfig = proj.path().join(".gitconfig");
    let lf = fs::read_to_string(&gitconfig).unwrap();
    assert!(
        lf.contains('\n') && !lf.contains('\r'),
        "precondition: init writes .gitconfig with LF line endings"
    );
    let crlf = lf.replace('\n', "\r\n");
    fs::write(&gitconfig, &crlf).unwrap();
    let stdout = status();
    assert!(
        stdout.contains(".gitconfig: In Sync"),
        "a CRLF-only rewrite must stay In Sync. Output was: {stdout}"
    );

    // Negative control: a real edit to one line of the CRLF file is drift.
    fs::write(&gitconfig, crlf.replacen("\r\n", " ; edited\r\n", 1)).unwrap();
    let stdout = status();
    assert!(
        stdout.contains(".gitconfig: Locally Modified"),
        "an edited CRLF file must be Locally Modified. Output was: {stdout}"
    );
}

#[test]
fn test_cli_project_init_and_doctor_host_flags() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let proj = tempdir().unwrap();

    // Verify `ude project init <path>` subcommand syntax works
    let init_status = ude_command(bin)
        .args([
            "project",
            "init",
            &proj.path().to_string_lossy(),
            "--type",
            "node",
        ])
        .status()
        .expect("ude project init execution");
    assert!(init_status.success(), "ude project init must succeed");
    assert!(proj.path().join(".ude/state.json").exists());

    // Verify `ude doctor --host` executes
    let doctor_output = ude_command(bin)
        .args(["doctor", "--host"])
        .output()
        .expect("ude doctor --host execution");
    let code = doctor_output.status.code().unwrap_or(-1);
    // Exit code must be either 0, 1 (warnings), or 2 (critical security failed)
    assert!(
        code == 0 || code == 1 || code == 2,
        "doctor --host returned unexpected exit code {code}"
    );
}

#[test]
fn test_doctor_critical_security_exit_code_2_and_negative_control() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let fake_home = tempdir().unwrap();

    // Run doctor in an isolated environment missing allowed_signers
    let doctor_output = ude_command(bin)
        .args(["doctor", "--host"])
        .env("HOME", fake_home.path())
        .env("XDG_CONFIG_HOME", fake_home.path().join(".config"))
        .env("GIT_CONFIG_GLOBAL", fake_home.path().join(".gitconfig"))
        .output()
        .expect("ude doctor execution");

    // Per docs/SPECIFICATION.md:80, missing allowed signers must exit 2
    assert_eq!(
        doctor_output.status.code(),
        Some(2),
        "doctor must exit 2 on critical security invariant failure (missing allowed_signers)"
    );

    // Negative control: satisfy the critical security invariants
    let config_dir = fake_home.path().join(".config");
    fs::create_dir_all(config_dir.join("git")).unwrap();
    fs::write(
        config_dir.join("git/allowed_signers"),
        "alice@example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI...\n",
    )
    .unwrap();

    // Configure git signing in isolated environment: the global config the ude
    // binary reads under HOME=<fake home> (pinned below so both sides use one file)
    let home_gitconfig = fake_home.path().join(".gitconfig");
    run_checked(
        scrubbed_git(fake_home.path(), &home_gitconfig),
        &["config", "--global", "gpg.format", "ssh"],
    );
    run_checked(
        scrubbed_git(fake_home.path(), &home_gitconfig),
        &["config", "--global", "commit.gpgsign", "true"],
    );

    let doctor_fixed = ude_command(bin)
        .args(["doctor", "--host"])
        .env("HOME", fake_home.path())
        .env("XDG_CONFIG_HOME", &config_dir)
        .env("GIT_CONFIG_GLOBAL", &home_gitconfig)
        .output()
        .expect("ude doctor execution");

    assert_ne!(
        doctor_fixed.status.code(),
        Some(2),
        "doctor must NOT exit 2 once critical security invariants are satisfied"
    );
}

#[test]
fn test_init_fails_on_existing_regular_file_and_preserves_custom_gitignore() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();

    // 1. Defect 5 reproduction: destination is an existing regular file
    let file_dest = temp.path().join("existing_file.txt");
    fs::write(&file_dest, "regular file content").unwrap();

    let init_fail = ude_command(bin)
        .args([
            "project",
            "init",
            &file_dest.to_string_lossy(),
            "--type",
            "node",
        ])
        .status()
        .expect("ude init execution");
    assert!(
        !init_fail.success(),
        "init must fail when destination is a regular file"
    );

    // 2. Defect 5 reproduction: re-running initialization must not overwrite custom .gitignore
    let proj_dir = temp.path().join("new_project");
    fs::create_dir_all(&proj_dir).unwrap();
    let custom_rule = "my_custom_secret_build_output/\n";
    fs::write(proj_dir.join(".gitignore"), custom_rule).unwrap();

    let init_success = ude_command(bin)
        .args([
            "project",
            "init",
            &proj_dir.to_string_lossy(),
            "--type",
            "rust",
        ])
        .status()
        .expect("ude init execution");
    assert!(
        init_success.success(),
        "init must succeed for directory destination"
    );

    let gitignore_after = fs::read_to_string(proj_dir.join(".gitignore")).unwrap();
    assert!(
        gitignore_after.contains("my_custom_secret_build_output/"),
        "Custom .gitignore rules must NOT be overwritten by init"
    );
    assert!(
        gitignore_after.contains("graphify-out/*"),
        "UDE required ignores must be appended to existing .gitignore"
    );
}

#[test]
fn test_doctor_empty_allowed_signers_exits_code_2() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let fake_home = tempdir().unwrap();
    let config_dir = fake_home.path().join(".config");
    fs::create_dir_all(config_dir.join("git")).unwrap();
    // Empty signers file (comments only)
    fs::write(
        config_dir.join("git/allowed_signers"),
        "# Only a comment\n\n",
    )
    .unwrap();

    let doctor_output = ude_command(bin)
        .args(["doctor", "--host"])
        .env("HOME", fake_home.path())
        .env("XDG_CONFIG_HOME", &config_dir)
        .output()
        .expect("ude doctor execution");

    assert_eq!(
        doctor_output.status.code(),
        Some(2),
        "doctor must exit 2 when allowed_signers has no valid SSH signing key fingerprints"
    );
}

#[test]
fn test_doctor_repo_assertions_detect_unconfigured_include_and_hooks() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();
    let repo_dir = temp.path().join("test_repo");
    fs::create_dir_all(&repo_dir).unwrap();

    // Initialize git repo
    let git = GitFixture::new();
    git.run(&repo_dir, &["init", "-b", "main"]);

    // Case 1: missing .gitconfig and missing hooks
    let fake_home = temp.path().join("home");
    fs::create_dir_all(&fake_home).unwrap();
    let doctor_out = ude_command(bin)
        .args([
            "doctor",
            "--repo",
            &repo_dir.to_string_lossy(),
            "--format",
            "json",
        ])
        .env("HOME", &fake_home)
        .env("XDG_CONFIG_HOME", fake_home.join(".config"))
        .env("GIT_CONFIG_GLOBAL", fake_home.join(".gitconfig"))
        .output()
        .expect("ude doctor execution");

    let stdout = String::from_utf8_lossy(&doctor_out.stdout);
    assert!(
        stdout.contains("Missing root .gitconfig template")
            || stdout.contains("does not include root .gitconfig")
    );
    assert!(stdout.contains("Repository core.hooksPath is unset"));

    // Case 2: inert files (empty .gitconfig, hooksPath=/dev/null, empty hook)
    fs::write(repo_dir.join(".gitconfig"), "").unwrap();
    git.run(
        &repo_dir,
        &["config", "--local", "core.hooksPath", "/dev/null"],
    );
    let hooks_dir = repo_dir.join("scripts/hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    fs::write(hooks_dir.join("pre-commit"), "").unwrap();

    let doctor_out2 = ude_command(bin)
        .args([
            "doctor",
            "--repo",
            &repo_dir.to_string_lossy(),
            "--format",
            "json",
        ])
        .env("HOME", &fake_home)
        .env("XDG_CONFIG_HOME", fake_home.join(".config"))
        .env("GIT_CONFIG_GLOBAL", fake_home.join(".gitconfig"))
        .output()
        .expect("ude doctor execution");
    let stdout2 = String::from_utf8_lossy(&doctor_out2.stdout);
    assert!(
        stdout2.contains("empty (0 bytes)") || stdout2.contains("does not include root .gitconfig")
    );
    assert!(stdout2.contains("expected 'scripts/hooks'"));
}

#[test]
fn test_sync_push_pr_does_not_advance_ancestor_state() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();
    let local_proj = temp.path().join("local_proj");
    fs::create_dir_all(&local_proj).unwrap();

    // 1. Initialize project with ude init
    let init_status = ude_command(bin)
        .args([
            "init",
            "--path",
            &local_proj.to_string_lossy(),
            "--type",
            "rust",
        ])
        .status()
        .expect("ude init execution");
    assert!(init_status.success());

    // 2. Read baseline ancestor hash of mise.toml
    let state_file = local_proj.join(".ude/state.json");
    let state_before: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
    let mise_hash_before = state_before["files"]["mise.toml"]["ancestor_hash"]
        .as_str()
        .unwrap()
        .to_string();

    // 3. Capture baseline content and edit local mise.toml
    let loc_mise = local_proj.join("mise.toml");
    let init_content = fs::read_to_string(&loc_mise).unwrap();
    fs::write(&loc_mise, format!("{init_content}\n# Custom edit\n")).unwrap();

    // 4. Setup a fake remote repo with a templates directory containing baseline
    let git = GitFixture::new();
    let remote_dir = temp.path().join("remote_dir");
    git.run(temp.path(), &["init", "-b", "main", "remote_dir"]);
    let tdir = remote_dir.join("templates/projects/agents");
    fs::create_dir_all(&tdir).unwrap();
    fs::write(tdir.join("mise.toml.jinja"), &init_content).unwrap();
    git.run(&remote_dir, &["add", "-A"]);
    git.run(&remote_dir, &["commit", "-m", "base commit"]);

    // 5. Run `ude sync push --remote <remote_dir> --pr` in hermetic environment
    let fake_home = temp.path().join("fake_home");
    fs::create_dir_all(&fake_home).unwrap();
    let fake_config = fake_home.join(".config");
    fs::create_dir_all(fake_config.join("git")).unwrap();
    let gitconfig = fake_config.join("git/config");
    fs::write(
        &gitconfig,
        "[user]\n  name = UDE Tester\n  email = tester@example.com\n[commit]\n  gpgsign = false\n",
    )
    .unwrap();
    let cache_dir = temp.path().join("cache");
    fs::create_dir_all(&cache_dir).unwrap();

    let push_pr_out = ude_command(bin)
        .current_dir(&local_proj)
        .args([
            "sync",
            "push",
            "--remote",
            &remote_dir.to_string_lossy(),
            "--pr",
        ])
        .env("HOME", &fake_home)
        .env("XDG_CONFIG_HOME", &fake_config)
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env("XDG_CACHE_HOME", &cache_dir)
        .output()
        .expect("ude sync push --pr execution");
    assert!(
        push_pr_out.status.success(),
        "ude sync push --pr must exit 0! Out: {}\nErr: {}",
        String::from_utf8_lossy(&push_pr_out.stdout),
        String::from_utf8_lossy(&push_pr_out.stderr)
    );

    // 6. Verify: Canonical ancestor state in .ude/state.json must NOT advance to local edit!
    let state_after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
    let mise_hash_after = state_after["files"]["mise.toml"]["ancestor_hash"]
        .as_str()
        .unwrap();
    assert_eq!(
        mise_hash_after, mise_hash_before,
        "PR push must NOT advance canonical ancestor state before PR merge!"
    );
}

#[test]
fn test_sync_push_remote_failure_does_not_advance_ancestor_state() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();
    let local_proj = temp.path().join("local_proj");
    fs::create_dir_all(&local_proj).unwrap();

    // 1. Initialize project
    let init_status = ude_command(bin)
        .args([
            "init",
            "--path",
            &local_proj.to_string_lossy(),
            "--type",
            "rust",
        ])
        .status()
        .expect("ude init execution");
    assert!(init_status.success());

    // 2. Read baseline ancestor hash of mise.toml
    let state_file = local_proj.join(".ude/state.json");
    let state_before: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
    let mise_hash_before = state_before["files"]["mise.toml"]["ancestor_hash"]
        .as_str()
        .unwrap()
        .to_string();

    // 3. Capture baseline content and edit local mise.toml
    let loc_mise = local_proj.join("mise.toml");
    let init_content = fs::read_to_string(&loc_mise).unwrap();
    fs::write(&loc_mise, format!("{init_content}\n# Custom edit\n")).unwrap();

    // 4. Setup a bare remote git repo with an active pre-receive rejection hook
    let git = GitFixture::new();
    let bare_remote = temp.path().join("bare_remote.git");
    let bare_url = bare_remote.to_string_lossy();
    git.run(temp.path(), &["init", "--bare", "-b", "main", &bare_url]);

    // Seed the bare remote with an initial main branch containing baseline template
    let seed_dir = temp.path().join("seed_repo");
    git.run(
        temp.path(),
        &["init", "-b", "main", &seed_dir.to_string_lossy()],
    );
    let tdir = seed_dir.join("templates/projects/agents");
    fs::create_dir_all(&tdir).unwrap();
    fs::write(tdir.join("mise.toml.jinja"), &init_content).unwrap();
    git.run(&seed_dir, &["add", "-A"]);
    git.run(&seed_dir, &["commit", "-m", "seed commit"]);
    git.run(&seed_dir, &["remote", "add", "origin", &bare_url]);
    git.run(&seed_dir, &["push", "origin", "main"]);

    // Install a rejecting pre-receive hook in the bare remote to simulate remote reject / network policy drop
    let hooks_dir = bare_remote.join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    let reject_hook = hooks_dir.join("pre-receive");
    fs::write(
        &reject_hook,
        "#!/bin/sh\necho 'REJECTED: simulated network push policy error' >&2\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&reject_hook, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let fake_home = temp.path().join("fake_home");
    fs::create_dir_all(&fake_home).unwrap();
    let fake_config = fake_home.join(".config");
    fs::create_dir_all(fake_config.join("git")).unwrap();
    let gitconfig = fake_config.join("git/config");
    fs::write(
        &gitconfig,
        "[user]\n  name = UDE Tester\n  email = tester@example.com\n[commit]\n  gpgsign = false\n",
    )
    .unwrap();
    let cache_dir = temp.path().join("cache");
    fs::create_dir_all(&cache_dir).unwrap();

    // 5. Run `ude sync push --remote <bare_remote>`
    // Remote git push will fail because of the rejecting pre-receive hook
    let push_fail = ude_command(bin)
        .current_dir(&local_proj)
        .args(["sync", "push", "--remote", &bare_remote.to_string_lossy()])
        .env("HOME", &fake_home)
        .env("XDG_CONFIG_HOME", &fake_config)
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env("XDG_CACHE_HOME", &cache_dir)
        .output()
        .expect("ude sync push execution");

    // A remote hook rejection is a hard failure (exit 2), unlike an upstream race (1).
    assert_eq!(
        push_fail.status.code(),
        Some(2),
        "ude sync push must exit 2 when the remote rejects the push! Err: {}",
        String::from_utf8_lossy(&push_fail.stderr)
    );
    let stderr = String::from_utf8_lossy(&push_fail.stderr);
    let stdout = String::from_utf8_lossy(&push_fail.stdout);
    assert!(
        stderr.contains("Failed to push to remote") || stdout.contains("Failed to push to remote"),
        "CLI must report remote push failure! Out: {stdout}\nErr: {stderr}"
    );

    // 6. Verify ancestor state in .ude/state.json did NOT advance!
    let state_after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
    let mise_hash_after = state_after["files"]["mise.toml"]["ancestor_hash"]
        .as_str()
        .unwrap();
    assert_eq!(
        mise_hash_after, mise_hash_before,
        "Failed remote push must NOT advance canonical ancestor state!"
    );

    // 7. Verify status still reports file as Locally Modified (ready to push)
    let status_out = ude_command(bin)
        .current_dir(&local_proj)
        .args(["sync", "status", "--remote", &bare_remote.to_string_lossy()])
        .env("HOME", &fake_home)
        .env("XDG_CONFIG_HOME", &fake_config)
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env("XDG_CACHE_HOME", &cache_dir)
        .output()
        .expect("ude sync status execution");
    let status_stdout = String::from_utf8_lossy(&status_out.stdout);
    assert!(
        status_stdout.contains("Locally Modified (ready to push)"),
        "Status must still report Locally Modified after failed push! Output: {status_stdout}"
    );

    // 8. Negative Control: Remove the rejecting hook and push again
    fs::remove_file(&reject_hook).unwrap();
    let push_success = ude_command(bin)
        .current_dir(&local_proj)
        .args(["sync", "push", "--remote", &bare_remote.to_string_lossy()])
        .env("HOME", &fake_home)
        .env("XDG_CONFIG_HOME", &fake_config)
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env("XDG_CACHE_HOME", &cache_dir)
        .output()
        .expect("ude sync push execution");
    assert!(
        push_success.status.success(),
        "ude sync push must succeed once remote rejection is cleared! Stderr: {}",
        String::from_utf8_lossy(&push_success.stderr)
    );

    let state_final: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
    let mise_hash_final = state_final["files"]["mise.toml"]["ancestor_hash"]
        .as_str()
        .unwrap();
    assert_ne!(
        mise_hash_final, mise_hash_before,
        "Successful push MUST advance ancestor state to the newly synchronized hash!"
    );
}

/// Every path under `root` (relative) with the bytes of each file; directories map to None.
fn snapshot_tree(root: &Path) -> std::collections::BTreeMap<PathBuf, Option<Vec<u8>>> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .map(|entry| {
            let entry = entry.expect("walk decoy repository");
            let rel = entry.path().strip_prefix(root).unwrap().to_path_buf();
            let bytes = entry
                .file_type()
                .is_file()
                .then(|| fs::read(entry.path()).expect("read decoy file"));
            (rel, bytes)
        })
        .collect()
}

#[test]
fn test_sync_ignores_inherited_git_dir() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();
    let git = GitFixture::new();

    // Project created with ude init; its mise.toml seeds the remote template.
    let local_proj = temp.path().join("local_proj");
    fs::create_dir_all(&local_proj).unwrap();
    let init = ude_command(bin)
        .args([
            "init",
            "--path",
            &local_proj.to_string_lossy(),
            "--type",
            "rust",
        ])
        .output()
        .expect("ude init execution");
    assert!(
        init.status.success(),
        "ude init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    let init_content = fs::read_to_string(local_proj.join("mise.toml")).unwrap();

    // Bare remote seeded with templates/projects/agents/mise.toml.jinja.
    let bare_remote = temp.path().join("bare_remote.git");
    let bare_url = bare_remote.to_string_lossy().to_string();
    git.run(temp.path(), &["init", "--bare", "-b", "main", &bare_url]);
    let seed_dir = temp.path().join("seed_repo");
    git.run(
        temp.path(),
        &["init", "-b", "main", &seed_dir.to_string_lossy()],
    );
    let tdir = seed_dir.join("templates/projects/agents");
    fs::create_dir_all(&tdir).unwrap();
    fs::write(tdir.join("mise.toml.jinja"), &init_content).unwrap();
    git.run(&seed_dir, &["add", "-A"]);
    git.run(&seed_dir, &["commit", "-m", "seed commit"]);
    git.run(&seed_dir, &["remote", "add", "origin", &bare_url]);
    git.run(&seed_dir, &["push", "origin", "main"]);

    // Decoy repository that an inherited GIT_DIR points at.
    let decoy = temp.path().join("decoy.git");
    git.run(
        temp.path(),
        &["init", "--bare", "-q", &decoy.to_string_lossy()],
    );
    let decoy_before = snapshot_tree(&decoy);

    // Negative control: plain git honours the inherited GIT_DIR and lands in the decoy.
    let probe = run_checked(
        {
            let mut cmd = git.cmd(temp.path());
            cmd.env("GIT_DIR", &decoy);
            cmd
        },
        &["rev-parse", "--absolute-git-dir"],
    );
    assert_eq!(
        PathBuf::from(String::from_utf8_lossy(&probe.stdout).trim()),
        fs::canonicalize(&decoy).unwrap(),
        "negative control: git with GIT_DIR set must resolve the decoy"
    );

    let fake_home = temp.path().join("fake_home");
    let fake_config = fake_home.join(".config");
    fs::create_dir_all(fake_config.join("git")).unwrap();
    let gitconfig = fake_config.join("git/config");
    fs::write(
        &gitconfig,
        "[user]\n  name = UDE Tester\n  email = tester@example.com\n[commit]\n  gpgsign = false\n",
    )
    .unwrap();
    let cache_dir = temp.path().join("cache");
    fs::create_dir_all(&cache_dir).unwrap();
    let ude_with_decoy = |args: &[&str]| {
        ude_command(bin)
            .current_dir(&local_proj)
            .args(args)
            .env("GIT_DIR", &decoy)
            .env("HOME", &fake_home)
            .env("XDG_CONFIG_HOME", &fake_config)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("XDG_CACHE_HOME", &cache_dir)
            .output()
            .expect("ude execution")
    };

    // sync status against the real remote while GIT_DIR points at the decoy. The first
    // run clones into an empty cache; the second takes the existing-cache path (fetch,
    // checkout -B, reset --hard, clean), where an inherited GIT_DIR redirects git.
    for run in ["clone", "fetch"] {
        let status = ude_with_decoy(&["sync", "status", "--remote", &bare_url]);
        let status_stdout = String::from_utf8_lossy(&status.stdout);
        assert!(
            status.status.success(),
            "ude sync status ({run} run) must succeed with an inherited GIT_DIR. Out: {status_stdout}\nErr: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        assert!(
            status_stdout.contains("Remote Upstream"),
            "status output ({run} run) lacks the remote line: {status_stdout}"
        );
        let cache_checkouts: Vec<PathBuf> = fs::read_dir(cache_dir.join("ude"))
            .expect("ude cache dir exists")
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                let name = path.file_name().unwrap().to_string_lossy();
                name.starts_with("remote-") && name.ends_with("-main") && path.join(".git").is_dir()
            })
            .collect();
        assert_eq!(
            cache_checkouts.len(),
            1,
            "expected one cache checkout with its own .git ({run} run): {cache_checkouts:?}"
        );
        assert_eq!(
            snapshot_tree(&decoy),
            decoy_before,
            "ude sync status ({run} run) modified the decoy repository named by GIT_DIR"
        );
    }

    // sync push of a local edit: the real remote advances, the decoy stays untouched.
    let main_before = git.output(&bare_remote, &["rev-parse", "refs/heads/main"]);
    fs::write(
        local_proj.join("mise.toml"),
        format!("{init_content}\n# edit pushed with an inherited GIT_DIR\n"),
    )
    .unwrap();
    let push = ude_with_decoy(&["sync", "push", "--remote", &bare_url]);
    assert!(
        push.status.success(),
        "ude sync push must succeed with an inherited GIT_DIR. Out: {}\nErr: {}",
        String::from_utf8_lossy(&push.stdout),
        String::from_utf8_lossy(&push.stderr)
    );
    assert_ne!(
        git.output(&bare_remote, &["rev-parse", "refs/heads/main"]),
        main_before,
        "ude sync push did not advance the real remote's main. Out: {}",
        String::from_utf8_lossy(&push.stdout)
    );
    assert_eq!(
        snapshot_tree(&decoy),
        decoy_before,
        "ude sync push modified the decoy repository named by GIT_DIR"
    );
}

#[test]
fn test_sync_corrupt_state_fails_loudly() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let proj = tempdir().unwrap();

    let init = ude_command(bin)
        .args([
            "init",
            "--path",
            &proj.path().to_string_lossy(),
            "--type",
            "rust",
        ])
        .output()
        .expect("ude init execution");
    assert!(
        init.status.success(),
        "ude init must succeed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let state_file = proj.path().join(".ude/state.json");
    let original = fs::read(&state_file).expect("init writes .ude/state.json");

    let ude = |args: &[&str]| -> Output {
        let mut cmd = ude_command(bin);
        cmd.current_dir(proj.path()).args(args).stdin(Stdio::null());
        for name in FIXTURE_GIT_LOCAL_ENV_VARS {
            cmd.env_remove(name);
        }
        cmd.output().expect("ude execution")
    };
    let describe = |out: &Output| {
        format!(
            "exit {:?}\nOut: {}\nErr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };

    // Corrupt JSON: status and pull fail with exit 2 and never reset the file.
    let corrupt = b"{not json";
    fs::write(&state_file, corrupt).unwrap();
    for args in [&["sync", "status"][..], &["sync", "pull"][..]] {
        let out = ude(args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "`ude {}` on a corrupt state.json must exit 2. {}",
            args.join(" "),
            describe(&out)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("state.json") && stderr.contains("corrupt"),
            "`ude {}` must name the corrupt state file. {}",
            args.join(" "),
            describe(&out)
        );
        assert_eq!(
            fs::read(&state_file).unwrap(),
            corrupt,
            "`ude {}` modified the corrupt state.json",
            args.join(" ")
        );
    }

    // A state file from a newer ude is refused, not reset.
    let future = br#"{"version": 99, "files": {}}"#;
    fs::write(&state_file, future).unwrap();
    let out = ude(&["sync", "status"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "a newer state.json must exit 2. {}",
        describe(&out)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("newer"),
        "a newer state.json must be reported as newer. {}",
        describe(&out)
    );
    assert_eq!(fs::read(&state_file).unwrap(), future);

    // Negative control: the original state loads and status succeeds.
    fs::write(&state_file, &original).unwrap();
    let out = ude(&["sync", "status"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "status on the original state must succeed. {}",
        describe(&out)
    );
}

/// Runs `ude init --type rust` at `path` and returns the generated mise.toml.
fn init_rust_project(bin: &str, path: &Path) -> String {
    fs::create_dir_all(path).unwrap();
    let init = ude_command(bin)
        .args(["init", "--path", &path.to_string_lossy(), "--type", "rust"])
        .output()
        .expect("ude init execution");
    assert!(
        init.status.success(),
        "ude init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    fs::read_to_string(path.join("mise.toml")).unwrap()
}

/// A bare remote at `<root>/<name>.git` whose main holds only
/// templates/projects/agents/mise.toml.jinja with `mise`; returns its path.
fn seed_mise_remote(git: &GitFixture, root: &Path, name: &str, mise: &str) -> PathBuf {
    seed_mise_remote_at(git, root, name, MISE_TEMPLATE, mise)
}

/// A bare remote at `<root>/<name>.git` whose main holds only `template` (a path under
/// the repo root) with `mise`; returns its path.
fn seed_mise_remote_at(
    git: &GitFixture,
    root: &Path,
    name: &str,
    template: &str,
    mise: &str,
) -> PathBuf {
    let bare = root.join(format!("{name}.git"));
    let bare_url = bare.to_string_lossy().to_string();
    git.run(root, &["init", "--bare", "-q", "-b", "main", &bare_url]);
    let seed = root.join(format!("{name}-seed"));
    git.run(root, &["init", "-q", "-b", "main", &seed.to_string_lossy()]);
    let target = seed.join(template);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, mise).unwrap();
    git.run(&seed, &["add", "-A"]);
    git.run(&seed, &["commit", "-q", "-m", "seed"]);
    git.run(&seed, &["push", "-q", &bare_url, "main"]);
    bare
}

/// Every file under `root` (relative) whose name ends in `.conflict`.
fn conflict_sidecars(root: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .map(|entry| entry.expect("walk project"))
        .filter(|entry| {
            entry.file_type().is_file()
                && entry.file_name().to_string_lossy().ends_with(".conflict")
        })
        .map(|entry| entry.path().strip_prefix(root).unwrap().to_path_buf())
        .collect()
}

#[test]
fn test_sync_pull_conflict_exits_1_and_writes_sidecar() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();
    let git = GitFixture::new();

    let fake_home = temp.path().join("fake_home");
    let fake_config = fake_home.join(".config");
    fs::create_dir_all(fake_config.join("git")).unwrap();
    let gitconfig = fake_config.join("git/config");
    fs::write(&gitconfig, FIXTURE_GITCONFIG).unwrap();
    let cache_dir = temp.path().join("cache");
    fs::create_dir_all(&cache_dir).unwrap();
    let ude = |project: &Path, args: &[&str]| {
        let mut cmd = ude_command(bin);
        cmd.current_dir(project).args(args);
        for name in FIXTURE_GIT_LOCAL_ENV_VARS {
            cmd.env_remove(name);
        }
        cmd.env("HOME", &fake_home)
            .env("XDG_CONFIG_HOME", &fake_config)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("XDG_CACHE_HOME", &cache_dir)
            .stdin(Stdio::null())
            .output()
            .expect("ude execution")
    };

    // Project and remote agree on mise.toml; then both sides append different lines.
    let project = temp.path().join("project");
    let init_mise = init_rust_project(bin, &project);
    assert!(
        project.join(".gitconfig").is_file(),
        "ude init creates .gitconfig"
    );
    let bare = seed_mise_remote(&git, temp.path(), "remote", &init_mise);
    let bare_url = bare.to_string_lossy().to_string();
    let local_mise = format!("{init_mise}# local edit\n");
    fs::write(project.join("mise.toml"), &local_mise).unwrap();
    let clone = temp.path().join("upstream-clone");
    git.run(
        temp.path(),
        &["clone", "-q", &bare_url, &clone.to_string_lossy()],
    );
    fs::write(
        clone.join("templates/projects/agents/mise.toml.jinja"),
        format!("{init_mise}# upstream edit\n"),
    )
    .unwrap();
    git.run(&clone, &["commit", "-q", "-am", "upstream edit"]);
    git.run(&clone, &["push", "-q", "origin", "main"]);

    let pull = ude(&project, &["sync", "pull", "--remote", &bare_url]);
    let stdout = String::from_utf8_lossy(&pull.stdout);
    let stderr = String::from_utf8_lossy(&pull.stderr);
    assert_eq!(
        pull.status.code(),
        Some(1),
        "a conflicted pull must exit 1. Out: {stdout}\nErr: {stderr}"
    );
    let sidecar = fs::read_to_string(project.join("mise.toml.conflict")).unwrap_or_else(|e| {
        panic!("mise.toml.conflict missing ({e}). Out: {stdout}\nErr: {stderr}")
    });
    assert!(sidecar.contains("<<<<<<< LOCAL\n"), "{sidecar}");
    assert!(sidecar.contains(">>>>>>> UPSTREAM\n"), "{sidecar}");
    assert_eq!(
        fs::read_to_string(project.join("mise.toml")).unwrap(),
        local_mise,
        "the conflicted local file must stay byte-identical"
    );
    assert!(
        format!("{stdout}{stderr}").contains("ude sync resolve mise.toml"),
        "pull output must point at the resolve command. Out: {stdout}\nErr: {stderr}"
    );
    // .gitconfig is unchanged locally and absent from a remote that never had it, so
    // pull keeps it (deletion needs the remote's history to show it; see
    // test_pull_deletes_only_templates_the_remote_history_deleted for D-12).
    assert!(
        project.join(".gitconfig").exists(),
        ".gitconfig must be kept. Out: {stdout}"
    );
    assert!(
        stdout.contains("Kept .gitconfig: its template is missing from this remote"),
        "Out: {stdout}"
    );
    // Negative control for that part: the conflicted file is not removed.
    assert!(!stdout.contains("Removed mise.toml"), "Out: {stdout}");
    assert!(project.join("mise.toml").is_file());
    assert_eq!(
        conflict_sidecars(&project),
        vec![PathBuf::from("mise.toml.conflict")]
    );

    let status = ude(&project, &["sync", "status", "--remote", &bare_url]);
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(
        status_out.contains("mise.toml: Unresolved conflict"),
        "Out: {status_out}\nErr: {}",
        String::from_utf8_lossy(&status.stderr)
    );

    // Negative control: a project and remote with no conflicting edit pull with exit 0
    // and leave no sidecar.
    let clean = temp.path().join("clean-project");
    let clean_mise = init_rust_project(bin, &clean);
    let clean_bare = seed_mise_remote(&git, temp.path(), "clean-remote", &clean_mise);
    let clean_pull = ude(
        &clean,
        &["sync", "pull", "--remote", &clean_bare.to_string_lossy()],
    );
    assert_eq!(
        clean_pull.status.code(),
        Some(0),
        "Out: {}\nErr: {}",
        String::from_utf8_lossy(&clean_pull.stdout),
        String::from_utf8_lossy(&clean_pull.stderr)
    );
    assert_eq!(conflict_sidecars(&clean), Vec::<PathBuf>::new());

    // D-11: the managed gitignore template ignores sidecars and nothing they sit beside.
    let template = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/projects/generic/gitignore.repo"),
    )
    .unwrap();
    assert!(
        template.lines().any(|line| line == "*.conflict"),
        "gitignore.repo lacks a `*.conflict` line"
    );
    assert!(
        !template.contains("{{"),
        "gitignore.repo must not be templated"
    );
    let probe = temp.path().join("ignore-probe");
    git.run(temp.path(), &["init", "-q", &probe.to_string_lossy()]);
    fs::write(probe.join(".gitignore"), &template).unwrap();
    let ignored = |path: &str| {
        let out = git
            .cmd(&probe)
            .args(["check-ignore", "-q", "--no-index", path])
            .output()
            .expect("git check-ignore");
        match out.status.code() {
            Some(0) => true,
            Some(1) => false,
            other => panic!("git check-ignore {path} failed ({other:?})"),
        }
    };
    assert!(ignored(".gitconfig.conflict"));
    assert!(ignored("scripts/hooks/pre-commit.conflict"));
    assert!(
        !ignored(".gitconfig"),
        "the managed file itself must stay tracked"
    );
    assert!(!ignored("mise.toml"));
}

/// A `ude` runner for sync tests: fake HOME / XDG dirs under `root`, the fixture
/// gitconfig as the only global config (no system config), inherited repo-local git
/// variables removed.
fn sync_cli(root: &Path) -> impl Fn(&Path, &[&str]) -> Output {
    sync_cli_with_env(root, Vec::new())
}

/// [`sync_cli`] with `extra` variables set on every run.
fn sync_cli_with_env(
    root: &Path,
    extra: Vec<(String, String)>,
) -> impl Fn(&Path, &[&str]) -> Output {
    let bin = env!("CARGO_BIN_EXE_ude");
    let fake_home = root.join("fake_home");
    let fake_config = fake_home.join(".config");
    fs::create_dir_all(fake_config.join("git")).unwrap();
    let gitconfig = fake_config.join("git/config");
    fs::write(&gitconfig, FIXTURE_GITCONFIG).unwrap();
    let cache_dir = root.join("cache");
    fs::create_dir_all(&cache_dir).unwrap();
    move |project: &Path, args: &[&str]| {
        let mut cmd = ude_command(bin);
        cmd.current_dir(project).args(args);
        for name in FIXTURE_GIT_LOCAL_ENV_VARS {
            cmd.env_remove(name);
        }
        cmd.env("HOME", &fake_home)
            .env("XDG_CONFIG_HOME", &fake_config)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_CACHE_HOME", &cache_dir)
            .envs(extra.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::null())
            .output()
            .expect("ude execution")
    }
}

/// Exit code plus both streams, for assertion messages.
fn outputs(out: &Output) -> String {
    format!(
        "exit {:?}\nOut: {}\nErr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Commits `files` (path under the repo root, content) to `bare`'s main through a
/// fresh clone at `clone`.
fn commit_upstream(git: &GitFixture, bare: &Path, clone: &Path, files: &[(&str, &str)]) {
    if !clone.exists() {
        git.run(
            clone.parent().unwrap(),
            &[
                "clone",
                "-q",
                &bare.to_string_lossy(),
                &clone.to_string_lossy(),
            ],
        );
    }
    git.run(clone, &["pull", "-q", "origin", "main"]);
    for (path, content) in files {
        let target = clone.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, content).unwrap();
    }
    git.run(clone, &["add", "-A"]);
    git.run(clone, &["commit", "-q", "-m", "upstream edit"]);
    git.run(clone, &["push", "-q", "origin", "main"]);
}

/// Content of `path` on `bare`'s main branch.
fn remote_file(git: &GitFixture, bare: &Path, path: &str) -> String {
    git.output(bare, &["show", &format!("main:{path}")])
}

const MISE_TEMPLATE: &str = "templates/projects/agents/mise.toml.jinja";

#[test]
fn test_sync_resolve_end_to_end() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());

    // Conflicting appends to mise.toml locally and upstream.
    let project = temp.path().join("project");
    let init_mise = init_rust_project(bin, &project);
    let bare = seed_mise_remote(&git, temp.path(), "remote", &init_mise);
    let bare_url = bare.to_string_lossy().to_string();
    let local_mise = format!("{init_mise}# local edit\n");
    fs::write(project.join("mise.toml"), &local_mise).unwrap();
    commit_upstream(
        &git,
        &bare,
        &temp.path().join("upstream-clone"),
        &[(MISE_TEMPLATE, &format!("{init_mise}# upstream edit\n"))],
    );

    let pull = ude(&project, &["sync", "pull", "--remote", &bare_url]);
    assert_eq!(pull.status.code(), Some(1), "{}", outputs(&pull));
    assert!(project.join("mise.toml.conflict").is_file());

    // Resolve needs no --remote: it works from the recorded upstream version.
    let resolve = ude(&project, &["sync", "resolve", "mise.toml"]);
    assert_eq!(resolve.status.code(), Some(0), "{}", outputs(&resolve));
    assert!(
        String::from_utf8_lossy(&resolve.stdout).contains("Resolved mise.toml"),
        "{}",
        outputs(&resolve)
    );
    assert!(
        !project.join("mise.toml.conflict").exists(),
        "resolve must remove the sidecar"
    );
    assert_eq!(
        fs::read_to_string(project.join("mise.toml")).unwrap(),
        local_mise,
        "resolve keeps the local file as is"
    );

    let status = ude(&project, &["sync", "status", "--remote", &bare_url]);
    assert!(
        String::from_utf8_lossy(&status.stdout).contains("mise.toml: Locally Modified"),
        "{}",
        outputs(&status)
    );

    let push = ude(&project, &["sync", "push", "--remote", &bare_url]);
    assert_eq!(push.status.code(), Some(0), "{}", outputs(&push));
    assert_eq!(
        format!("{}\n", remote_file(&git, &bare, MISE_TEMPLATE)),
        local_mise,
        "push publishes the resolved local content"
    );

    // Negative control: there is nothing left to resolve.
    let again = ude(&project, &["sync", "resolve", "mise.toml"]);
    assert_eq!(again.status.code(), Some(2), "{}", outputs(&again));
    assert!(
        String::from_utf8_lossy(&again.stderr)
            .to_lowercase()
            .contains("no open conflict"),
        "{}",
        outputs(&again)
    );
}

const GITIGNORE_TEMPLATE: &str = "templates/projects/generic/gitignore.repo";

#[test]
fn test_sync_push_skips_unresolved_conflict_exit_1() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());

    // Remote holds the project's init mise.toml and .gitignore.
    let project = temp.path().join("project");
    let init_mise = init_rust_project(bin, &project);
    let init_gitignore = fs::read_to_string(project.join(".gitignore")).unwrap();
    let bare = seed_mise_remote(&git, temp.path(), "remote", &init_mise);
    let bare_url = bare.to_string_lossy().to_string();
    let clone = temp.path().join("upstream-clone");
    commit_upstream(
        &git,
        &bare,
        &clone,
        &[(GITIGNORE_TEMPLATE, &init_gitignore)],
    );

    // Local-only .gitignore edit; conflicting mise.toml edits on both sides.
    let local_gitignore = format!("{init_gitignore}# local ignore edit\n");
    fs::write(project.join(".gitignore"), &local_gitignore).unwrap();
    let local_mise = format!("{init_mise}# local edit\n");
    fs::write(project.join("mise.toml"), &local_mise).unwrap();
    let upstream_mise = format!("{init_mise}# upstream edit\n");
    commit_upstream(&git, &bare, &clone, &[(MISE_TEMPLATE, &upstream_mise)]);

    let pull = ude(&project, &["sync", "pull", "--remote", &bare_url]);
    assert_eq!(pull.status.code(), Some(1), "{}", outputs(&pull));

    let push = ude(&project, &["sync", "push", "--remote", &bare_url]);
    let push_text = outputs(&push);
    assert_eq!(
        push.status.code(),
        Some(1),
        "a push that skipped a conflict must exit 1. {push_text}"
    );
    assert!(
        push_text
            .contains("mise.toml: unresolved conflict, skipped (run ude sync resolve mise.toml)"),
        "{push_text}"
    );
    assert_eq!(
        format!("{}\n", remote_file(&git, &bare, GITIGNORE_TEMPLATE)),
        local_gitignore,
        "the eligible .gitignore edit is published. {push_text}"
    );
    assert_eq!(
        format!("{}\n", remote_file(&git, &bare, MISE_TEMPLATE)),
        upstream_mise,
        "the conflicted mise.toml is not published. {push_text}"
    );
    assert_eq!(
        fs::read_to_string(project.join("mise.toml")).unwrap(),
        local_mise,
        "push never rewrites local files"
    );

    // --force keeps its agent-rule meaning only: the conflict is still skipped.
    let forced = ude(
        &project,
        &["sync", "push", "--force", "--remote", &bare_url],
    );
    assert_eq!(forced.status.code(), Some(1), "{}", outputs(&forced));
    assert_eq!(
        format!("{}\n", remote_file(&git, &bare, MISE_TEMPLATE)),
        upstream_mise,
        "--force must not publish the conflicted file. {}",
        outputs(&forced)
    );

    // Negative control: once resolved, the push publishes it and exits 0.
    let resolve = ude(&project, &["sync", "resolve", "mise.toml"]);
    assert_eq!(resolve.status.code(), Some(0), "{}", outputs(&resolve));
    let resolved_push = ude(&project, &["sync", "push", "--remote", &bare_url]);
    assert_eq!(
        resolved_push.status.code(),
        Some(0),
        "{}",
        outputs(&resolved_push)
    );
    assert_eq!(
        format!("{}\n", remote_file(&git, &bare, MISE_TEMPLATE)),
        local_mise
    );
}

/// The single remote cache checkout under `<cache>/ude` (named `remote-*`, not the
/// `.lock` file).
#[cfg(unix)]
fn remote_cache_checkout(cache: &Path) -> PathBuf {
    let checkouts: Vec<PathBuf> = fs::read_dir(cache.join("ude"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.starts_with("remote-") && !name.ends_with(".lock")
        })
        .collect();
    assert_eq!(
        checkouts.len(),
        1,
        "expected one cache checkout: {checkouts:?}"
    );
    checkouts.into_iter().next().unwrap()
}

/// mise.toml's ancestor hash in the project's state.json.
#[cfg(unix)]
fn mise_ancestor(project: &Path) -> String {
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(project.join(".ude/state.json")).unwrap())
            .unwrap();
    state["files"]["mise.toml"]["ancestor_hash"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
#[cfg(unix)]
fn test_force_does_not_bypass_upstream_moved() {
    use std::os::unix::fs::PermissionsExt;
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());

    let project = temp.path().join("project");
    let init_mise = init_rust_project(bin, &project);
    let bare = seed_mise_remote(&git, temp.path(), "remote", &init_mise);
    let bare_url = bare.to_string_lossy().to_string();
    let local_mise = format!("{init_mise}# local edit\n");
    fs::write(project.join("mise.toml"), &local_mise).unwrap();
    let ancestor_before = mise_ancestor(&project);

    let status = ude(&project, &["sync", "status", "--remote", &bare_url]);
    assert_eq!(status.status.code(), Some(0), "{}", outputs(&status));

    // Another machine publishes between UDE's fetch and its push: the cache's
    // post-checkout hook (run by ensure_remote_repo's `git checkout -B` after the fetch)
    // pushes an empty commit from a second clone.
    let other = temp.path().join("other-clone");
    git.run(
        temp.path(),
        &["clone", "-q", &bare_url, &other.to_string_lossy()],
    );
    let marker = temp.path().join("race-injected");
    let hook = remote_cache_checkout(&temp.path().join("cache")).join(".git/hooks/post-checkout");
    fs::write(
        &hook,
        format!(
            "#!/bin/sh\nunset $(git rev-parse --local-env-vars)\ncd '{}' && git commit -q --allow-empty -m 'concurrent publish' && git push -q origin main && touch '{}'\nexit 0\n",
            other.display(),
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

    let raced = ude(
        &project,
        &["sync", "push", "--force", "--remote", &bare_url],
    );
    let raced_text = outputs(&raced);
    assert!(
        marker.exists(),
        "post-checkout hook did not fire. {raced_text}"
    );
    assert_eq!(raced.status.code(), Some(1), "{raced_text}");
    assert!(
        raced_text.contains("upstream changed, run ude sync pull"),
        "{raced_text}"
    );
    assert_eq!(
        git.output(&bare, &["rev-parse", "refs/heads/main"]),
        git.output(&other, &["rev-parse", "HEAD"]),
        "the remote branch must keep the other machine's commit. {raced_text}"
    );
    assert_eq!(
        format!("{}\n", remote_file(&git, &bare, MISE_TEMPLATE)),
        init_mise,
        "nothing may be published. {raced_text}"
    );
    assert_eq!(
        mise_ancestor(&project),
        ancestor_before,
        "an aborted push must not advance ancestor state"
    );

    // Negative control: without the race the same forced push publishes and exits 0.
    fs::remove_file(&hook).unwrap();
    let pushed = ude(
        &project,
        &["sync", "push", "--force", "--remote", &bare_url],
    );
    assert_eq!(pushed.status.code(), Some(0), "{}", outputs(&pushed));
    assert_eq!(
        format!("{}\n", remote_file(&git, &bare, MISE_TEMPLATE)),
        local_mise
    );
    assert_ne!(mise_ancestor(&project), ancestor_before);
}

#[test]
fn test_ude_command_drops_host_typesafe_key() {
    let cmd = ude_command(env!("CARGO_BIN_EXE_ude"));
    let removed = cmd
        .get_envs()
        .any(|(name, value)| name == "TYPESAFE_API_KEY" && value.is_none());
    assert!(
        removed,
        "ude_command must env_remove TYPESAFE_API_KEY so tests never use the host key"
    );
}

/// Edits mise.toml in a fresh project whose remote keeps the template at `template`,
/// pushes, then pulls. The push must publish the edit and the pull must keep it.
fn assert_push_publishes_and_pull_keeps_edit(temp: &Path, layout: &str, template: &str) {
    let bin = env!("CARGO_BIN_EXE_ude");
    let git = GitFixture::new();
    let root = temp.join(layout);
    fs::create_dir_all(&root).unwrap();
    let ude = sync_cli(&root);

    let project = root.join("project");
    let init_mise = init_rust_project(bin, &project);
    let bare = seed_mise_remote_at(&git, &root, "remote", template, &init_mise);
    let bare_url = bare.to_string_lossy().to_string();
    let head_before = git.output(&bare, &["rev-parse", "refs/heads/main"]);
    let local_mise = format!("{init_mise}# local edit\n");
    fs::write(project.join("mise.toml"), &local_mise).unwrap();

    let push = ude(&project, &["sync", "push", "--remote", &bare_url]);
    let push_text = outputs(&push);
    assert_eq!(push.status.code(), Some(0), "[{layout}] {push_text}");
    assert_ne!(
        git.output(&bare, &["rev-parse", "refs/heads/main"]),
        head_before,
        "[{layout}] push must move the remote branch. {push_text}"
    );
    assert_eq!(
        format!("{}\n", remote_file(&git, &bare, template)),
        local_mise,
        "[{layout}] push must publish the local edit. {push_text}"
    );

    let pull = ude(&project, &["sync", "pull", "--remote", &bare_url]);
    assert_eq!(pull.status.code(), Some(0), "[{layout}] {}", outputs(&pull));
    assert_eq!(
        fs::read_to_string(project.join("mise.toml")).unwrap(),
        local_mise,
        "[{layout}] a pull after a successful push must keep the local edit. {}",
        outputs(&pull)
    );
}

#[test]
fn test_sync_push_root_layout_remote_publishes_and_pull_keeps_edit() {
    let temp = tempdir().unwrap();
    // Control: templates under `templates/` in the remote.
    assert_push_publishes_and_pull_keeps_edit(temp.path(), "templates-layout", MISE_TEMPLATE);
    // Templates at the remote's root (no `templates/` directory).
    assert_push_publishes_and_pull_keeps_edit(
        temp.path(),
        "root-layout",
        "projects/agents/mise.toml.jinja",
    );
}

#[test]
fn test_sync_refuses_a_remote_that_is_not_a_template_repo() {
    let temp = tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_ude");
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let project = temp.path().join("project");
    init_rust_project(bin, &project);
    let gitconfig = fs::read(project.join(".gitconfig")).unwrap();
    // Repositories that are not ude template trees: one with only a README, and one with
    // its own projects/ directory that holds no ude template.
    for (name, file) in [
        ("readme", "README.md"),
        ("own-projects", "projects/website/index.html"),
    ] {
        let bare = temp.path().join(format!("{name}.git"));
        let bare_url = bare.to_string_lossy().to_string();
        git.run(
            temp.path(),
            &["init", "--bare", "-q", "-b", "main", &bare_url],
        );
        let seed = temp.path().join(format!("{name}-seed"));
        git.run(
            temp.path(),
            &["init", "-q", "-b", "main", &seed.to_string_lossy()],
        );
        let path = seed.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not templates\n").unwrap();
        git.run(&seed, &["add", "-A"]);
        git.run(&seed, &["commit", "-q", "-m", "seed"]);
        git.run(&seed, &["push", "-q", &bare_url, "main"]);
        let head_before = git.output(&bare, &["rev-parse", "refs/heads/main"]);

        for action in ["status", "pull", "push"] {
            let out = ude(&project, &["sync", action, "--remote", &bare_url]);
            let text = outputs(&out);
            assert_eq!(out.status.code(), Some(2), "{name} {action}: {text}");
            assert!(
                text.contains("not a ude template repository"),
                "{name} {action}: {text}"
            );
        }
        assert_eq!(
            fs::read(project.join(".gitconfig")).unwrap(),
            gitconfig,
            "{name}"
        );
        assert_eq!(
            git.output(&bare, &["rev-parse", "refs/heads/main"]),
            head_before,
            "{name}"
        );
    }
}

#[test]
fn test_pull_deletes_only_templates_the_remote_history_deleted() {
    let temp = tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_ude");
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());

    // A full template remote whose history deletes the .gitconfig template.
    let full = seed_full_template_remote(&git, temp.path(), "full");
    let full_url = full.to_string_lossy().to_string();
    let seed = temp.path().join("full-seed");
    git.run(&seed, &["rm", "-q", GITCONFIG_TEMPLATE]);
    git.run(&seed, &["commit", "-q", "-m", "delete gitconfig template"]);
    // A later commit puts the deletion beyond the cache's depth-1 clone, so confirming
    // it needs the full history.
    fs::write(seed.join("README.md"), "later change\n").unwrap();
    git.run(&seed, &["add", "README.md"]);
    git.run(&seed, &["commit", "-q", "-m", "later change"]);
    git.run(&seed, &["push", "-q", &full_url, "main"]);
    let deleted = temp.path().join("deleted");
    init_rust_project(bin, &deleted);
    // file:// makes the cache clone really shallow (git ignores --depth for a plain path).
    let full_sync_url = format!("file://{}", full.display());
    let status = outputs(&ude(
        &deleted,
        &["sync", "status", "--remote", &full_sync_url],
    ));
    assert!(status.contains(".gitconfig: Deleted upstream"), "{status}");
    let pull = ude(&deleted, &["sync", "pull", "--remote", &full_sync_url]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert!(!deleted.join(".gitconfig").exists(), "{}", outputs(&pull));

    // A remote that holds one template and never had the others.
    let thin = temp.path().join("thin.git");
    let thin_url = thin.to_string_lossy().to_string();
    git.run(
        temp.path(),
        &["init", "--bare", "-q", "-b", "main", &thin_url],
    );
    let thin_seed = temp.path().join("thin-seed");
    git.run(
        temp.path(),
        &["init", "-q", "-b", "main", &thin_seed.to_string_lossy()],
    );
    copy_repo_templates(&temp.path().join("all-templates"));
    let hook = thin_seed.join(PRE_COMMIT_TEMPLATE);
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::copy(
        temp.path()
            .join("all-templates/projects/generic/hooks/pre-commit"),
        &hook,
    )
    .unwrap();
    git.run(&thin_seed, &["add", "-A"]);
    git.run(&thin_seed, &["commit", "-q", "-m", "seed"]);
    git.run(&thin_seed, &["push", "-q", &thin_url, "main"]);
    let kept = temp.path().join("kept");
    init_rust_project(bin, &kept);
    let status = outputs(&ude(&kept, &["sync", "status", "--remote", &thin_url]));
    assert!(!status.contains("Deleted upstream"), "{status}");
    assert!(status.contains(".gitconfig: Missing upstream"), "{status}");
    assert!(
        status.contains("scripts/hooks/pre-commit: In Sync"),
        "{status}"
    );
    let pull = ude(&kept, &["sync", "pull", "--remote", &thin_url]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    for rel in [
        ".gitconfig",
        "mise.toml",
        ".agents/AGENTS.md",
        "scripts/hooks/pre-push",
    ] {
        assert!(
            kept.join(rel).exists(),
            "pull deleted {rel}: {}",
            outputs(&pull)
        );
    }
}

#[test]
fn test_pull_confirms_a_deletion_made_while_resolving_a_merge() {
    let temp = tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_ude");
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let remote = seed_full_template_remote(&git, temp.path(), "merged");
    let remote_url = remote.to_string_lossy().to_string();
    let seed = temp.path().join("merged-seed");
    let template = seed.join(GITCONFIG_TEMPLATE);
    let original = fs::read_to_string(&template).unwrap();
    // Both branches edit the template; the merge conflict is resolved by deleting it.
    git.run(&seed, &["checkout", "-q", "-b", "side"]);
    fs::write(&template, format!("{original}# side\n")).unwrap();
    git.run(&seed, &["commit", "-q", "-am", "side edit"]);
    git.run(&seed, &["checkout", "-q", "main"]);
    fs::write(&template, format!("{original}# main\n")).unwrap();
    git.run(&seed, &["commit", "-q", "-am", "main edit"]);
    let merge = git
        .cmd(&seed)
        .args(["merge", "-q", "side"])
        .output()
        .unwrap();
    assert!(!merge.status.success(), "the fixture merge must conflict");
    git.run(&seed, &["rm", "-q", GITCONFIG_TEMPLATE]);
    git.run(&seed, &["commit", "-q", "--no-edit"]);
    fs::write(seed.join("README.md"), "later change\n").unwrap();
    git.run(&seed, &["add", "README.md"]);
    git.run(&seed, &["commit", "-q", "-m", "later change"]);
    git.run(&seed, &["push", "-q", &remote_url, "main"]);

    let project = temp.path().join("project");
    init_rust_project(bin, &project);
    let sync_url = format!("file://{}", remote.display());
    let status = outputs(&ude(&project, &["sync", "status", "--remote", &sync_url]));
    assert!(status.contains(".gitconfig: Deleted upstream"), "{status}");
    let pull = ude(&project, &["sync", "pull", "--remote", &sync_url]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert!(!project.join(".gitconfig").exists(), "{}", outputs(&pull));
}

#[test]
fn test_edited_file_with_template_the_remote_never_had_is_missing_upstream() {
    let temp = tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_ude");
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let project = temp.path().join("project");
    let init_mise = init_rust_project(bin, &project);
    let remote = seed_mise_remote(&git, temp.path(), "thin", &init_mise);
    let remote_url = remote.to_string_lossy().to_string();
    let edited = append_to(&project.join(".gitconfig"), "# local edit\n");

    let status = outputs(&ude(&project, &["sync", "status", "--remote", &remote_url]));
    assert!(status.contains(".gitconfig: Missing upstream"), "{status}");
    let pull = ude(&project, &["sync", "pull", "--remote", &remote_url]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert!(
        !project.join(".gitconfig.conflict").exists(),
        "{}",
        outputs(&pull)
    );
    assert_eq!(
        fs::read_to_string(project.join(".gitconfig")).unwrap(),
        edited
    );
    // Push agrees with status: no conflict to skip, and nothing is published.
    let head_before = git.output(&remote, &["rev-parse", "refs/heads/main"]);
    let push = ude(&project, &["sync", "push", "--remote", &remote_url]);
    let push_text = outputs(&push);
    assert_eq!(push.status.code(), Some(0), "{push_text}");
    assert!(!push_text.contains("unresolved conflict"), "{push_text}");
    assert_eq!(
        git.output(&remote, &["rev-parse", "refs/heads/main"]),
        head_before
    );
}

/// A bare remote at `<root>/<name>.git` whose main holds a copy of this repository's
/// `templates/` tree, so a fresh `ude init` project starts in sync with it.
fn seed_full_template_remote(git: &GitFixture, root: &Path, name: &str) -> PathBuf {
    let bare = root.join(format!("{name}.git"));
    let bare_url = bare.to_string_lossy().to_string();
    git.run(root, &["init", "--bare", "-q", "-b", "main", &bare_url]);
    let seed = root.join(format!("{name}-seed"));
    git.run(root, &["init", "-q", "-b", "main", &seed.to_string_lossy()]);
    copy_repo_templates(&seed.join("templates"));
    git.run(&seed, &["add", "-A"]);
    git.run(&seed, &["commit", "-q", "-m", "seed"]);
    git.run(&seed, &["push", "-q", &bare_url, "main"]);
    bare
}

/// Copies this repository's `templates/` tree to `dest`.
fn copy_repo_templates(dest: &Path) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    for entry in walkdir::WalkDir::new(&source) {
        let entry = entry.expect("walk templates");
        let rel = entry.path().strip_prefix(&source).unwrap();
        let target = dest.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).unwrap();
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

const GITCONFIG_TEMPLATE: &str = "templates/projects/generic/gitconfig.repo";
const NEW_WORKTREE_TEMPLATE: &str = "templates/projects/agents/scratch/new-worktree.sh";

#[test]
fn test_sync_push_skips_boundary_deletion_and_publishes_rest() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());

    let project = temp.path().join("project");
    init_rust_project(bin, &project);
    let bare = seed_full_template_remote(&git, temp.path(), "remote");
    let bare_url = bare.to_string_lossy().to_string();
    let scratch_template = remote_file(&git, &bare, NEW_WORKTREE_TEMPLATE);

    // A scratch script (publish boundary) deleted locally, plus an eligible edit.
    let scratch = project.join("scratch/new-worktree.sh");
    let scratch_bytes = fs::read(&scratch).unwrap();
    fs::remove_file(&scratch).unwrap();
    let local_gitconfig = format!(
        "{}# local edit\n",
        fs::read_to_string(project.join(".gitconfig")).unwrap()
    );
    fs::write(project.join(".gitconfig"), &local_gitconfig).unwrap();

    let push = ude(&project, &["sync", "push", "--remote", &bare_url]);
    let push_text = outputs(&push);
    assert_eq!(
        push.status.code(),
        Some(1),
        "a push that skipped a boundary path must exit 1. {push_text}"
    );
    assert!(
        push_text.contains(
            "scratch/new-worktree.sh: deleted locally, but it is a local-only path that push never publishes; skipped"
        ),
        "{push_text}"
    );
    assert_eq!(
        format!("{}\n", remote_file(&git, &bare, GITCONFIG_TEMPLATE)),
        local_gitconfig,
        "the eligible .gitconfig edit is published. {push_text}"
    );
    assert_eq!(
        remote_file(&git, &bare, NEW_WORKTREE_TEMPLATE),
        scratch_template,
        "the scratch template must be untouched. {push_text}"
    );

    // Pull keeps the deletion but must not advise a push that would refuse it.
    let pull = ude(&project, &["sync", "pull", "--remote", &bare_url]);
    let pull_text = outputs(&pull);
    assert_eq!(pull.status.code(), Some(0), "{pull_text}");
    assert!(!scratch.exists(), "pull keeps the local deletion (D-15)");
    assert!(
        pull_text.contains("scratch/new-worktree.sh: deleted locally")
            && !pull_text.contains("scratch/new-worktree.sh: deleted locally — push to propagate"),
        "{pull_text}"
    );
    assert!(pull_text.contains("local-only path"), "{pull_text}");

    // Negative control: restoring the script clears the skip and the push exits 0.
    fs::write(&scratch, &scratch_bytes).unwrap();
    let restored = ude(&project, &["sync", "push", "--remote", &bare_url]);
    assert_eq!(restored.status.code(), Some(0), "{}", outputs(&restored));
}

#[test]
fn test_sync_push_refuses_to_overwrite_template_logic() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());

    let project = temp.path().join("project");
    let init_mise = init_rust_project(bin, &project);
    let bare = seed_full_template_remote(&git, temp.path(), "remote");
    let bare_url = bare.to_string_lossy().to_string();
    let template_before = remote_file(&git, &bare, MISE_TEMPLATE);
    assert!(
        template_before.contains("{%"),
        "the seeded template has logic"
    );

    fs::write(
        project.join("mise.toml"),
        format!("{init_mise}[env]\nFOO = \"1\"\n"),
    )
    .unwrap();
    let local_gitconfig = format!(
        "{}# local edit\n",
        fs::read_to_string(project.join(".gitconfig")).unwrap()
    );
    fs::write(project.join(".gitconfig"), &local_gitconfig).unwrap();

    let push = ude(&project, &["sync", "push", "--remote", &bare_url]);
    let push_text = outputs(&push);
    assert_eq!(
        push.status.code(),
        Some(1),
        "a push that skipped a template must exit 1. {push_text}"
    );
    assert!(
        push_text.contains("mise.toml: upstream template")
            && push_text.contains("mise.toml.jinja contains template logic"),
        "{push_text}"
    );
    assert_eq!(
        remote_file(&git, &bare, MISE_TEMPLATE),
        template_before,
        "the template must stay byte-identical. {push_text}"
    );
    // Negative control: the non-template .gitconfig edit is published.
    assert_eq!(
        format!("{}\n", remote_file(&git, &bare, GITCONFIG_TEMPLATE)),
        local_gitconfig,
        "{push_text}"
    );
}

#[test]
fn test_first_pull_keeps_custom_gitignore_preserved_by_init() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let bare = seed_full_template_remote(&git, temp.path(), "remote");
    let bare_url = bare.to_string_lossy().to_string();

    // A project that already has its own .gitignore before `ude init`.
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join(".gitignore"),
        "custom_rule_one/\ncustom_rule_two.log\n",
    )
    .unwrap();
    init_rust_project(bin, &project);
    let kept = fs::read_to_string(project.join(".gitignore")).unwrap();
    assert!(kept.contains("custom_rule_one/") && kept.contains("custom_rule_two.log"));

    let status = ude(&project, &["sync", "status", "--remote", &bare_url]);
    let status_text = outputs(&status);
    let pull = ude(&project, &["sync", "pull", "--remote", &bare_url]);
    let pull_text = outputs(&pull);
    let after = fs::read_to_string(project.join(".gitignore")).unwrap();
    let sidecar = project.join(".gitignore.conflict");
    assert!(
        (after.contains("custom_rule_one/") && after.contains("custom_rule_two.log"))
            || sidecar.is_file(),
        "the first pull silently dropped the custom rules. {pull_text}\n.gitignore:\n{after}"
    );
    assert_eq!(
        after, kept,
        "pull must leave the kept .gitignore alone. {pull_text}"
    );
    // The kept file reads as a local modification, not as an ancestor the template
    // updates.
    assert!(
        status_text.contains(".gitignore: Locally Modified"),
        "{status_text}"
    );

    // Negative control: a project with no .gitignore of its own starts in sync.
    let fresh = temp.path().join("fresh");
    init_rust_project(bin, &fresh);
    let fresh_status = ude(&fresh, &["sync", "status", "--remote", &bare_url]);
    assert!(
        outputs(&fresh_status).contains(".gitignore: In Sync"),
        "{}",
        outputs(&fresh_status)
    );
    let fresh_pull = ude(&fresh, &["sync", "pull", "--remote", &bare_url]);
    assert_eq!(
        fresh_pull.status.code(),
        Some(0),
        "{}",
        outputs(&fresh_pull)
    );
    assert_eq!(
        fs::read_to_string(fresh.join(".gitignore")).unwrap(),
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(GITIGNORE_TEMPLATE)).unwrap()
    );
}

/// A project at `<root>/<name>` holding a copy of this repository's `templates/`, so
/// `ude` run inside it uses that copy for init and as the local sync upstream.
fn project_with_local_templates(root: &Path, name: &str) -> PathBuf {
    let project = root.join(name);
    copy_repo_templates(&project.join("templates"));
    project
}

/// `ude init --type rust` on `project`, run from inside it; panics on failure.
fn reinit_rust(ude: &impl Fn(&Path, &[&str]) -> Output, project: &Path) {
    let path = project.to_string_lossy().to_string();
    let init = ude(project, &["init", "--path", &path, "--type", "rust"]);
    assert_eq!(init.status.code(), Some(0), "{}", outputs(&init));
}

#[test]
fn test_reinit_keeps_ancestor_of_unedited_gitignore() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let template = project.join(GITIGNORE_TEMPLATE);

    reinit_rust(&ude, &project);
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(status.contains(".gitignore: In Sync"), "{status}");

    // Another developer changes the shared template.
    let upstream = format!(
        "{}new_upstream_rule/\n",
        fs::read_to_string(&template).unwrap()
    );
    fs::write(&template, &upstream).unwrap();
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(status.contains(".gitignore: Upstream Updated"), "{status}");

    // Re-running init without editing .gitignore must not turn it into a local edit.
    reinit_rust(&ude, &project);
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(
        status.contains(".gitignore: Upstream Updated")
            && !status.contains(".gitignore: Locally Modified"),
        "{status}"
    );
    let push = ude(&project, &["sync", "push"]);
    assert_eq!(
        fs::read_to_string(&template).unwrap(),
        upstream,
        "push must not revert the upstream change. {}",
        outputs(&push)
    );
    assert_eq!(push.status.code(), Some(0), "{}", outputs(&push));
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).unwrap(),
        upstream,
        "pull applies the upstream change. {}",
        outputs(&pull)
    );

    // Negative control: a real edit survives a re-init as a local modification.
    let edited = format!("{upstream}edited_rule/\n");
    fs::write(project.join(".gitignore"), &edited).unwrap();
    reinit_rust(&ude, &project);
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(status.contains(".gitignore: Locally Modified"), "{status}");
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).unwrap(),
        edited
    );

    // Negative control (CR-03): a custom .gitignore with no prior state reads as a
    // local modification and pull keeps it.
    let custom = project_with_local_templates(temp.path(), "custom");
    let custom_rules = "custom_rule_one/\ngraphify-out/*\n";
    fs::write(custom.join(".gitignore"), custom_rules).unwrap();
    reinit_rust(&ude, &custom);
    let status = outputs(&ude(&custom, &["sync", "status"]));
    assert!(status.contains(".gitignore: Locally Modified"), "{status}");
    let pull = ude(&custom, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert_eq!(
        fs::read_to_string(custom.join(".gitignore")).unwrap(),
        custom_rules,
        "{}",
        outputs(&pull)
    );
}

#[test]
fn test_init_refuses_corrupt_prior_state() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = temp.path().join("project");
    let state = project.join(".ude/state.json");
    fs::create_dir_all(state.parent().unwrap()).unwrap();
    fs::write(&state, "{not json").unwrap();
    let path = project.to_string_lossy().to_string();

    let init = ude(&project, &["init", "--path", &path, "--type", "rust"]);
    assert_eq!(init.status.code(), Some(2), "{}", outputs(&init));
    assert_eq!(fs::read_to_string(&state).unwrap(), "{not json");
    assert!(
        !project.join(".gitignore").exists() && !project.join("mise.toml").exists(),
        "init must stop before touching any file. {}",
        outputs(&init)
    );

    // Negative control: without the corrupt state the same init succeeds.
    fs::remove_file(&state).unwrap();
    reinit_rust(&ude, &project);
}

/// The line of `text` that mentions `needle`, or "" when none does.
fn line_with<'a>(text: &'a str, needle: &str) -> &'a str {
    text.lines()
        .find(|line| line.contains(needle))
        .unwrap_or("")
}

#[test]
fn test_push_never_deletes_a_template_with_logic() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    reinit_rust(&ude, &project);
    fs::remove_file(project.join("mise.toml")).unwrap();
    // Negative control: a template without logic is still deleted by push (D-14).
    fs::remove_file(project.join(".gitconfig")).unwrap();

    let path = project.to_string_lossy().to_string();
    let init = outputs(&ude(&project, &["init", "--path", &path, "--type", "rust"]));
    let init_line = line_with(&init, "mise.toml is tracked but missing");
    assert!(init_line.contains("push skips the deletion"), "{init}");
    assert!(
        line_with(&init, ".gitconfig is tracked but missing").contains("will delete"),
        "{init}"
    );
    let status = outputs(&ude(&project, &["sync", "status"]));
    let mise_line = line_with(&status, "mise.toml: Deleted locally");
    assert!(
        mise_line.contains("push skips it: its template has logic"),
        "{status}"
    );
    // The template is named by its path in the shared templates, not a local copy's path.
    assert!(
        mise_line.contains("delete projects/agents/mise.toml.jinja from the shared templates")
            && !mise_line.contains(&project.to_string_lossy().to_string()),
        "{status}"
    );
    let pull = outputs(&ude(&project, &["sync", "pull"]));
    assert!(
        line_with(&pull, "mise.toml: deleted locally")
            .contains("push skips it: its template has logic"),
        "{pull}"
    );
    assert!(
        line_with(&status, ".gitconfig: Deleted locally")
            .contains("push deletes the shared template"),
        "{status}"
    );

    let push = ude(&project, &["sync", "push"]);
    let text = outputs(&push);
    assert_eq!(push.status.code(), Some(1), "{text}");
    assert!(
        line_with(&text, "mise.toml: upstream template")
            .contains("deleting it would remove it for every project"),
        "{text}"
    );
    assert!(
        project.join(MISE_TEMPLATE).exists(),
        "push deleted a template with logic"
    );
    assert!(!project.join(GITCONFIG_TEMPLATE).exists(), "{text}");
}

#[test]
fn test_status_and_push_agree_on_edit_under_template_logic() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());

    let project = temp.path().join("project");
    let init_mise = init_rust_project(bin, &project);
    let bare = seed_full_template_remote(&git, temp.path(), "remote");
    let bare_url = bare.to_string_lossy().to_string();
    let env_block = "[env]\nFOO = \"1\"\n";
    fs::write(project.join("mise.toml"), format!("{init_mise}{env_block}")).unwrap();
    let local_gitconfig = format!(
        "{}# local edit\n",
        fs::read_to_string(project.join(".gitconfig")).unwrap()
    );
    fs::write(project.join(".gitconfig"), &local_gitconfig).unwrap();

    let status = outputs(&ude(&project, &["sync", "status", "--remote", &bare_url]));
    let mise_line = line_with(&status, "mise.toml: Locally Modified");
    assert!(
        !mise_line.is_empty()
            && !mise_line.contains("ready to push")
            && mise_line.contains("template has logic")
            && mise_line.contains("move the change into mise.local.toml"),
        "{status}"
    );
    // Negative control: an edit under a template without logic is still pushable.
    assert!(
        status.contains(".gitconfig: Locally Modified (ready to push)"),
        "{status}"
    );

    let push = ude(&project, &["sync", "push", "--remote", &bare_url]);
    let push_text = outputs(&push);
    assert_eq!(push.status.code(), Some(1), "{push_text}");
    let skip_line = line_with(&push_text, "mise.toml: upstream template");
    assert!(
        skip_line.contains("for this project only, move the change into mise.local.toml")
            && skip_line.contains("for every project, edit"),
        "{push_text}"
    );
    assert_eq!(
        format!("{}\n", remote_file(&git, &bare, GITCONFIG_TEMPLATE)),
        local_gitconfig,
        "the .gitconfig edit is published. {push_text}"
    );

    // Following the hint clears the skip.
    fs::write(project.join("mise.local.toml"), env_block).unwrap();
    fs::write(project.join("mise.toml"), &init_mise).unwrap();
    let status = outputs(&ude(&project, &["sync", "status", "--remote", &bare_url]));
    assert!(
        status.contains("mise.toml: Layered Override Active"),
        "{status}"
    );
    let push = ude(&project, &["sync", "push", "--remote", &bare_url]);
    assert_eq!(push.status.code(), Some(0), "{}", outputs(&push));
}

/// `.ude/state.json` of `project`, parsed.
fn state_json(project: &Path) -> serde_json::Value {
    let text = fs::read_to_string(project.join(".ude/state.json")).expect("read state.json");
    serde_json::from_str(&text).expect("parse state.json")
}

/// Appends `text` to the file at `path` and returns the new content.
fn append_to(path: &Path, text: &str) -> String {
    let content = format!("{}{text}", fs::read_to_string(path).unwrap());
    fs::write(path, &content).unwrap();
    content
}

const PRE_PUSH_TEMPLATE: &str = "templates/projects/generic/hooks/pre-push";
const COMMIT_MSG_TEMPLATE: &str = "templates/projects/generic/hooks/commit-msg";
const PRE_COMMIT_TEMPLATE: &str = "templates/projects/generic/hooks/pre-commit";
const AGENTS_TEMPLATE: &str = "templates/projects/agents/agents.md.jinja";

#[test]
fn test_reinit_leaves_tracked_gitignore_without_graphify_block() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let template = project.join(GITIGNORE_TEMPLATE);

    reinit_rust(&ude, &project);
    // Another developer drops the graphify block from the shared template.
    let upstream: String = fs::read_to_string(&template)
        .unwrap()
        .lines()
        .filter(|line| !line.contains("graphify-out"))
        .map(|line| format!("{line}\n"))
        .collect();
    fs::write(&template, &upstream).unwrap();
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(status.contains(".gitignore: Upstream Updated"), "{status}");
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).unwrap(),
        upstream
    );

    // Re-running init must leave the tracked file to sync: no graphify append.
    reinit_rust(&ude, &project);
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).unwrap(),
        upstream,
        "re-init must not rewrite a tracked .gitignore"
    );
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(
        status.contains(".gitignore: In Sync") && !status.contains(".gitignore: Locally Modified"),
        "{status}"
    );
    let push = ude(&project, &["sync", "push"]);
    assert_eq!(push.status.code(), Some(0), "{}", outputs(&push));
    assert_eq!(
        fs::read_to_string(&template).unwrap(),
        upstream,
        "push must not re-add the graphify block upstream. {}",
        outputs(&push)
    );

    // Negative control: a fresh project starts In Sync with the template as is.
    let fresh = project_with_local_templates(temp.path(), "fresh");
    reinit_rust(&ude, &fresh);
    let status = outputs(&ude(&fresh, &["sync", "status"]));
    assert!(status.contains(".gitignore: In Sync"), "{status}");
    assert_eq!(
        fs::read_to_string(fresh.join(".gitignore")).unwrap(),
        fs::read_to_string(fresh.join(GITIGNORE_TEMPLATE)).unwrap()
    );

    // Negative control (CR-03): a custom .gitignore with no prior record still gets
    // the graphify block appended on first init and reads as a local modification.
    let custom = project_with_local_templates(temp.path(), "custom");
    fs::write(custom.join(".gitignore"), "custom_rule_one/\n").unwrap();
    reinit_rust(&ude, &custom);
    let kept = fs::read_to_string(custom.join(".gitignore")).unwrap();
    assert!(
        kept.starts_with("custom_rule_one/\n") && kept.contains("graphify-out/*"),
        "{kept}"
    );
    let status = outputs(&ude(&custom, &["sync", "status"]));
    assert!(status.contains(".gitignore: Locally Modified"), "{status}");
}

#[test]
fn test_reinit_keeps_record_of_file_whose_template_is_missing() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let template = project.join(PRE_PUSH_TEMPLATE);
    let hook = project.join("scripts/hooks/pre-push");

    reinit_rust(&ude, &project);
    let edited = append_to(&hook, "# local hook edit\n");
    let record_before = state_json(&project)["files"]["scripts/hooks/pre-push"].clone();
    assert!(record_before.is_object(), "{record_before}");

    // The template is missing while init re-runs.
    let parked = temp.path().join("parked-pre-push");
    fs::rename(&template, &parked).unwrap();
    reinit_rust(&ude, &project);
    fs::rename(&parked, &template).unwrap();

    assert_eq!(fs::read_to_string(&hook).unwrap(), edited);
    assert_eq!(
        state_json(&project)["files"]["scripts/hooks/pre-push"],
        record_before,
        "re-init must carry the record forward unchanged"
    );
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(
        status.contains("scripts/hooks/pre-push: Locally Modified"),
        "{status}"
    );
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert_eq!(
        fs::read_to_string(&hook).unwrap(),
        edited,
        "pull must not overwrite the local edit. {}",
        outputs(&pull)
    );
}

#[test]
fn test_reinit_keeps_local_edit_of_file_it_would_rewrite() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");

    reinit_rust(&ude, &project);
    let edited = append_to(&project.join(".gitconfig"), "# local edit\n");
    // An unedited file whose template changed upstream since the first init.
    let pre_commit = project.join("scripts/hooks/pre-commit");
    let init_pre_commit = fs::read_to_string(&pre_commit).unwrap();
    let upstream_pre_commit = append_to(&project.join(PRE_COMMIT_TEMPLATE), "# upstream edit\n");

    reinit_rust(&ude, &project);
    assert_eq!(
        fs::read_to_string(project.join(".gitconfig")).unwrap(),
        edited,
        "re-init must not overwrite a local edit"
    );
    assert_eq!(
        fs::read_to_string(&pre_commit).unwrap(),
        init_pre_commit,
        "re-init leaves a tracked file to sync"
    );
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(
        status.contains(".gitconfig: Locally Modified (ready to push)"),
        "{status}"
    );
    assert!(
        status.contains("scripts/hooks/pre-commit: Upstream Updated"),
        "{status}"
    );
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert_eq!(
        fs::read_to_string(&pre_commit).unwrap(),
        upstream_pre_commit
    );
    assert_eq!(
        fs::read_to_string(project.join(".gitconfig")).unwrap(),
        edited
    );
}

#[test]
fn test_open_conflict_survives_reinit() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");

    reinit_rust(&ude, &project);
    let local = append_to(&project.join(".gitignore"), "local_rule/\n");
    append_to(&project.join(GITIGNORE_TEMPLATE), "upstream_rule/\n");
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(1), "{}", outputs(&pull));
    let sidecar = project.join(".gitignore.conflict");
    let notes = append_to(&sidecar, "# my resolution notes\n");
    let conflict_before = state_json(&project)["conflicts"][".gitignore"].clone();
    assert!(conflict_before.is_object(), "{conflict_before}");

    reinit_rust(&ude, &project);
    assert_eq!(fs::read_to_string(&sidecar).unwrap(), notes);
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).unwrap(),
        local
    );
    assert_eq!(
        state_json(&project)["conflicts"][".gitignore"],
        conflict_before,
        "re-init must keep the open conflict"
    );

    let resolve = ude(&project, &["sync", "resolve", ".gitignore"]);
    assert_eq!(resolve.status.code(), Some(0), "{}", outputs(&resolve));
    assert!(!sidecar.exists(), "{}", outputs(&resolve));
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(status.contains(".gitignore: Locally Modified"), "{status}");
}

#[test]
fn test_tombstones_survive_reinit() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");

    reinit_rust(&ude, &project);
    // Upstream deletes two hook templates: commit-msg is unedited locally (D-12),
    // pre-push is edited locally and then kept as an unmanaged file (D-13).
    append_to(
        &project.join("scripts/hooks/pre-push"),
        "# local hook edit\n",
    );
    fs::remove_file(project.join(COMMIT_MSG_TEMPLATE)).unwrap();
    fs::remove_file(project.join(PRE_PUSH_TEMPLATE)).unwrap();
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(1), "{}", outputs(&pull));
    let resolve = ude(&project, &["sync", "resolve", "scripts/hooks/pre-push"]);
    assert_eq!(resolve.status.code(), Some(0), "{}", outputs(&resolve));
    assert!(!project.join("scripts/hooks/commit-msg").exists());
    let before = state_json(&project);
    let tombstones = before["tombstones"].clone();
    assert_eq!(
        tombstones["scripts/hooks/pre-push"]["kept_unmanaged"],
        serde_json::Value::Bool(true),
        "{before}"
    );
    assert!(
        tombstones["scripts/hooks/commit-msg"].is_object(),
        "{before}"
    );
    let status_before =
        String::from_utf8_lossy(&ude(&project, &["sync", "status"]).stdout).into_owned();

    reinit_rust(&ude, &project);
    let after = state_json(&project);
    assert_eq!(after["tombstones"], tombstones, "{after}");
    assert!(
        after["files"]["scripts/hooks/pre-push"].is_null(),
        "{after}"
    );
    assert!(
        after["files"]["scripts/hooks/commit-msg"].is_null(),
        "{after}"
    );
    let status_after =
        String::from_utf8_lossy(&ude(&project, &["sync", "status"]).stdout).into_owned();
    assert_eq!(status_after, status_before);
}

#[test]
fn test_status_and_push_agree_on_agent_rules_under_template_logic() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let agents = Path::new(".agents/AGENTS.md");

    // Agent rules (published only with --force) under a template with logic.
    let project = project_with_local_templates(temp.path(), "project");
    let template = project.join(AGENTS_TEMPLATE);
    let logic = append_to(
        &template,
        "{% if project_type %}<!-- typed -->{% endif %}\n",
    );
    reinit_rust(&ude, &project);
    append_to(&project.join(agents), "local agent rule\n");

    let status = outputs(&ude(&project, &["sync", "status"]));
    let status_line = line_with(&status, ".agents/AGENTS.md: Locally Modified");
    assert!(
        status_line.contains("template has logic")
            && status_line.contains(".agents/AGENTS.local.md")
            && !status_line.contains("--force"),
        "{status}"
    );
    for args in [&["sync", "push"][..], &["sync", "push", "--force"][..]] {
        let push = ude(&project, args);
        let push_text = outputs(&push);
        assert_eq!(push.status.code(), Some(1), "{args:?}: {push_text}");
        let skip_line = line_with(&push_text, ".agents/AGENTS.md:");
        assert!(
            skip_line.contains("template logic")
                && skip_line.contains(".agents/AGENTS.local.md")
                && !skip_line.contains("--force"),
            "{args:?}: {push_text}"
        );
        assert_eq!(fs::read_to_string(&template).unwrap(), logic, "{push_text}");
    }

    // Negative control: under a template without logic, --force is the remedy and it
    // publishes the edit.
    let plain = project_with_local_templates(temp.path(), "plain");
    reinit_rust(&ude, &plain);
    let edited = append_to(&plain.join(agents), "local agent rule\n");
    let status = outputs(&ude(&plain, &["sync", "status"]));
    assert!(
        status.contains(".agents/AGENTS.md: Locally Modified (push with --force to publish)"),
        "{status}"
    );
    let push = ude(&plain, &["sync", "push"]);
    let push_text = outputs(&push);
    assert_eq!(push.status.code(), Some(1), "{push_text}");
    assert!(
        line_with(&push_text, ".agents/AGENTS.md:").contains("re-run with --force"),
        "{push_text}"
    );
    let push = ude(&plain, &["sync", "push", "--force"]);
    assert_eq!(push.status.code(), Some(0), "{}", outputs(&push));
    assert_eq!(
        fs::read_to_string(plain.join(AGENTS_TEMPLATE)).unwrap(),
        edited
    );
}

#[test]
fn test_reinit_keeps_open_conflict_with_no_ancestor() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let template = project.join(PRE_PUSH_TEMPLATE);
    let hook = project.join("scripts/hooks/pre-push");

    // The template is missing at first init, so the hook gets no record; the user
    // writes their own, and the template then appears with different content.
    let parked = temp.path().join("parked-pre-push");
    fs::rename(&template, &parked).unwrap();
    reinit_rust(&ude, &project);
    fs::write(&hook, "#!/bin/sh\n# user hook\n").unwrap();
    fs::rename(&parked, &template).unwrap();
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(1), "{}", outputs(&pull));
    let before = state_json(&project);
    assert!(
        before["conflicts"]["scripts/hooks/pre-push"].is_object()
            && before["files"]["scripts/hooks/pre-push"].is_null(),
        "{before}"
    );

    reinit_rust(&ude, &project);
    assert_eq!(
        fs::read_to_string(&hook).unwrap(),
        "#!/bin/sh\n# user hook\n",
        "re-init must not overwrite the conflicted local file"
    );
    let after = state_json(&project);
    assert_eq!(after["conflicts"], before["conflicts"], "{after}");
    assert!(
        after["files"]["scripts/hooks/pre-push"].is_null(),
        "{after}"
    );
    let resolve = ude(&project, &["sync", "resolve", "scripts/hooks/pre-push"]);
    assert_eq!(resolve.status.code(), Some(0), "{}", outputs(&resolve));
}

/// `ude init --type rust` on `project`, run from inside it; returns both streams and
/// panics unless it exits 0.
fn init_output(ude: &impl Fn(&Path, &[&str]) -> Output, project: &Path) -> String {
    let path = project.to_string_lossy().to_string();
    let init = ude(project, &["init", "--path", &path, "--type", "rust"]);
    let text = outputs(&init);
    assert_eq!(init.status.code(), Some(0), "{text}");
    text
}

#[test]
fn test_project_gitignore_ignores_per_machine_state() {
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    git.run(temp.path(), &["init", "-q", &project.to_string_lossy()]);
    reinit_rust(&ude, &project);
    assert!(project.join(".ude/state.json").is_file());

    let ignored = |path: &str| {
        let out = git
            .cmd(&project)
            .args(["check-ignore", "-q", path])
            .output()
            .expect("git check-ignore");
        match out.status.code() {
            Some(0) => true,
            Some(1) => false,
            other => panic!("git check-ignore {path} failed ({other:?})"),
        }
    };
    // The ancestor ledger is per machine: committing it makes a second machine's init
    // treat every uncommitted managed file as deleted locally.
    assert!(
        ignored(".ude/state.json"),
        "init's .gitignore commits state.json"
    );
    // Negative controls: managed files and other `.ude/` content stay committable.
    assert!(
        !ignored(".gitconfig"),
        "the managed file itself must stay tracked"
    );
    assert!(!ignored(".ude/overrides/mise.toml"));

    // Second, independent count: what `git status` offers to commit.
    let untracked = git.output(
        &project,
        &["status", "--porcelain", "--untracked-files=all"],
    );
    assert!(!untracked.contains(".ude/state.json"), "{untracked}");
    assert!(untracked.contains("?? .gitconfig"), "{untracked}");
}

#[test]
fn test_reinit_warns_that_push_deletes_template_of_missing_tracked_file() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let hook = project.join("scripts/hooks/pre-push");
    reinit_rust(&ude, &project);

    // Negative control: a re-init with every managed file present warns about none.
    let clean = init_output(&ude, &project);
    assert!(!clean.contains("tracked but missing"), "{clean}");
    assert!(
        clean.contains("Configured tracked .gitconfig & chained scripts/hooks"),
        "{clean}"
    );

    fs::remove_file(&hook).unwrap();
    let init = init_output(&ude, &project);
    let warning = line_with(&init, "scripts/hooks/pre-push is tracked but missing");
    assert!(
        warning.contains("a plain ude sync push will delete its shared template"),
        "{init}"
    );
    assert!(
        !hook.exists(),
        "init must not recreate a deleted tracked file (D-15)"
    );
    assert!(
        !init.contains("Configured tracked .gitconfig & chained scripts/hooks"),
        "init claims the hooks are in place: {init}"
    );
    assert!(
        init.contains("Initialized workflow tasks (mise.toml, scratch/)"),
        "{init}"
    );

    let status = outputs(&ude(&project, &["sync", "status"]));
    let hint = line_with(&status, "scripts/hooks/pre-push: Deleted locally");
    assert!(
        hint.contains("push deletes the shared template"),
        "{status}"
    );

    // The warning's claim: a plain push deletes the shared template.
    let push = ude(&project, &["sync", "push"]);
    assert_eq!(push.status.code(), Some(0), "{}", outputs(&push));
    assert!(
        !project.join(PRE_PUSH_TEMPLATE).exists(),
        "{}",
        outputs(&push)
    );
}

#[test]
fn test_reinit_missing_file_warning_follows_push_boundary() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    reinit_rust(&ude, &project);
    for rel in [".agents/AGENTS.md", "scratch/new-worktree.sh", "mise.toml"] {
        fs::remove_file(project.join(rel)).unwrap();
    }

    let init = init_output(&ude, &project);
    let agents = line_with(&init, ".agents/AGENTS.md is tracked but missing");
    assert!(
        agents.contains("ude sync push --force will delete its shared template")
            && !agents.contains("a plain ude sync push will delete"),
        "{init}"
    );
    let scratch = line_with(&init, "scratch/new-worktree.sh is tracked but missing");
    assert!(
        scratch.contains("never publishes") && !scratch.contains("will delete"),
        "{init}"
    );
    // mise.toml's template has logic, which push never deletes (a plain-delete
    // template is covered by test_push_never_deletes_a_template_with_logic).
    let mise = line_with(&init, "mise.toml is tracked but missing");
    assert!(
        mise.contains("push skips the deletion") && !mise.contains("will delete"),
        "{init}"
    );
    for rel in [".agents/AGENTS.md", "scratch/new-worktree.sh", "mise.toml"] {
        assert!(!project.join(rel).exists(), "init recreated {rel}: {init}");
    }
    assert!(
        !init.contains("Initialized workflow tasks (mise.toml, scratch/)"),
        "init claims mise.toml and scratch/ are in place: {init}"
    );

    let status = outputs(&ude(&project, &["sync", "status"]));
    let hint = line_with(&status, ".agents/AGENTS.md: Deleted locally");
    assert!(hint.contains("deletes the shared template"), "{status}");

    // A plain push refuses the agent rules deletion and never publishes scratch/.
    let push = ude(&project, &["sync", "push"]);
    assert!(project.join(AGENTS_TEMPLATE).exists(), "{}", outputs(&push));
    assert!(
        project.join(NEW_WORKTREE_TEMPLATE).exists(),
        "{}",
        outputs(&push)
    );
    assert!(project.join(MISE_TEMPLATE).exists(), "{}", outputs(&push));
}

#[test]
fn test_reinit_does_not_warn_of_deletion_for_open_conflict() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    reinit_rust(&ude, &project);

    // pre-push: deleted locally while upstream changed it, so pull records a conflict
    // (D-15) and push skips it. commit-msg: deleted locally, upstream unchanged.
    fs::remove_file(project.join("scripts/hooks/pre-push")).unwrap();
    fs::remove_file(project.join("scripts/hooks/commit-msg")).unwrap();
    let upstream = append_to(&project.join(PRE_PUSH_TEMPLATE), "# upstream change\n");
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(1), "{}", outputs(&pull));
    let state = state_json(&project);
    assert!(
        state["conflicts"]["scripts/hooks/pre-push"].is_object(),
        "{state}"
    );

    let init = init_output(&ude, &project);
    assert!(
        !init.contains("scripts/hooks/pre-push is tracked but missing"),
        "push does not delete a conflicted file's template: {init}"
    );
    // Negative control: the unconflicted deletion in the same run is warned about.
    assert!(
        line_with(&init, "scripts/hooks/commit-msg is tracked but missing")
            .contains("a plain ude sync push will delete its shared template"),
        "{init}"
    );

    let push = ude(&project, &["sync", "push"]);
    assert_eq!(
        fs::read_to_string(project.join(PRE_PUSH_TEMPLATE)).unwrap(),
        upstream,
        "{}",
        outputs(&push)
    );
    assert!(
        !project.join(COMMIT_MSG_TEMPLATE).exists(),
        "{}",
        outputs(&push)
    );
}

/// Exit 0 of `git check-ignore -q <path>` in `project`: true when ignored, false when
/// not, panics on any other outcome.
fn check_ignored(git: &GitFixture, project: &Path, path: &str) -> bool {
    let out = git
        .cmd(project)
        .args(["check-ignore", "-q", path])
        .output()
        .expect("git check-ignore");
    match out.status.code() {
        Some(0) => true,
        Some(1) => false,
        other => panic!("git check-ignore {path} failed ({other:?})"),
    }
}

/// Commits the index of `project` with `message`. The hooks and commit signing that
/// `ude init` configures are not under test here and would make the fixture depend on
/// host tools (gitleaks) and signing keys.
fn commit_without_project_hooks(git: &GitFixture, project: &Path, message: &str) {
    git.run(
        project,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            message,
        ],
    );
}

/// `ude doctor --repo <project>`; returns both streams.
fn doctor_output(ude: &impl Fn(&Path, &[&str]) -> Output, project: &Path) -> String {
    let path = project.to_string_lossy().to_string();
    outputs(&ude(project, &["doctor", "--repo", &path]))
}

/// Status (`Pass`, `Warning`, ...) of doctor's per-machine state check on `project`,
/// read from `ude doctor --format json`; panics if the check is absent.
fn state_check_status(ude: &impl Fn(&Path, &[&str]) -> Output, project: &Path) -> String {
    let path = project.to_string_lossy().to_string();
    let json = ude(project, &["doctor", "--repo", &path, "--format", "json"]);
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).expect("doctor json");
    let state_check = report["repo"]
        .as_array()
        .expect("repo reports")
        .iter()
        .find(|r| r["name"] == "Per-machine sync state (.ude/state.json)")
        .unwrap_or_else(|| panic!("no state check: {report}"));
    state_check["status"]["status"]
        .as_str()
        .unwrap_or_else(|| panic!("state check has no status: {state_check}"))
        .to_string()
}

#[test]
fn test_doctor_reports_tracked_per_machine_state_with_remedy() {
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    git.run(temp.path(), &["init", "-q", &project.to_string_lossy()]);
    reinit_rust(&ude, &project);
    assert!(check_ignored(&git, &project, ".ude/state.json"));
    let remedy = "git rm --cached .ude/state.json";

    // Negative control (a): fresh init leaves state.json untracked, so no finding.
    let fresh = doctor_output(&ude, &project);
    assert!(fresh.contains("Per-machine sync state"), "{fresh}");
    assert!(!fresh.contains(remedy), "{fresh}");
    assert_eq!(state_check_status(&ude, &project), "Pass");

    // Committed despite the ignore rule (as before the template ignored it).
    git.run(&project, &["add", "-f", ".ude/state.json"]);
    commit_without_project_hooks(&git, &project, "chore: track state");
    let tracked = doctor_output(&ude, &project);
    assert!(
        tracked.contains(".ude/state.json is tracked by git"),
        "{tracked}"
    );
    assert!(tracked.contains(remedy), "{tracked}");
    // Same severity as doctor's other repository findings: a warning, which leaves the
    // exit code to the other checks.
    assert_eq!(state_check_status(&ude, &project), "Warning");

    // Negative control (b): after the remedy the finding is gone and the file stays.
    git.run(&project, &["rm", "-q", "--cached", ".ude/state.json"]);
    commit_without_project_hooks(&git, &project, "chore: untrack state");
    assert!(project.join(".ude/state.json").is_file());
    let fixed = doctor_output(&ude, &project);
    assert!(fixed.contains("Per-machine sync state"), "{fixed}");
    assert!(!fixed.contains(remedy), "{fixed}");
    assert_eq!(state_check_status(&ude, &project), "Pass");
}

#[test]
fn test_project_gitignore_ignores_state_staging_temp_files() {
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    git.run(temp.path(), &["init", "-q", &project.to_string_lossy()]);
    reinit_rust(&ude, &project);

    // tempfile's NamedTempFile::new_in(".ude") names: ".tmp" + 6 random characters.
    let staging_temp = ".ude/.tmpQ1w2E3";
    let override_file = ".ude/overrides/mise.toml";
    assert!(check_ignored(&git, &project, staging_temp));
    // Negative control: committed `.ude/` content is not ignored.
    assert!(!check_ignored(&git, &project, override_file));

    // Second, independent count: what `git status` offers to commit.
    fs::write(project.join(staging_temp), "{}").unwrap();
    fs::create_dir_all(project.join(".ude/overrides")).unwrap();
    fs::write(project.join(override_file), "[tools]\n").unwrap();
    let untracked = git.output(
        &project,
        &["status", "--porcelain", "--untracked-files=all"],
    );
    assert!(!untracked.contains(".tmpQ1w2E3"), "{untracked}");
    assert!(
        untracked.contains("?? .ude/overrides/mise.toml"),
        "{untracked}"
    );
}

/// A file outside every project and template tree holding `HOST_SECRET`, standing in
/// for a host file a planted symlink points at.
#[cfg(unix)]
const HOST_SECRET: &str = "HOST_ONLY_SECRET_CONTENT\n";

#[cfg(unix)]
fn host_secret_file(root: &Path) -> PathBuf {
    let path = root.join("host_secret");
    fs::write(&path, HOST_SECRET).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn test_pull_refuses_symlinked_template() {
    use std::os::unix::fs::symlink;
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    reinit_rust(&ude, &project);
    let template = project.join(GITCONFIG_TEMPLATE);

    // Negative control: a regular upstream change is pulled.
    let upstream = append_to(&template, "# upstream change\n");
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert_eq!(
        fs::read_to_string(project.join(".gitconfig")).unwrap(),
        upstream
    );

    // The template becomes a symlink to a host file.
    fs::remove_file(&template).unwrap();
    symlink(host_secret_file(temp.path()), &template).unwrap();
    let pull = ude(&project, &["sync", "pull"]);
    let text = outputs(&pull);
    assert_eq!(pull.status.code(), Some(2), "{text}");
    assert!(text.contains("symlink"), "{text}");
    assert_eq!(
        fs::read_to_string(project.join(".gitconfig")).unwrap(),
        upstream,
        "pull copied the symlink target into the project"
    );
}

#[cfg(unix)]
#[test]
fn test_init_refuses_symlinked_template() {
    use std::os::unix::fs::symlink;
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let template = project.join(GITCONFIG_TEMPLATE);
    fs::remove_file(&template).unwrap();
    symlink(host_secret_file(temp.path()), &template).unwrap();

    let path = project.to_string_lossy().to_string();
    let init = ude(&project, &["init", "--path", &path, "--type", "rust"]);
    let text = outputs(&init);
    assert_ne!(init.status.code(), Some(0), "{text}");
    assert!(text.contains("symlink"), "{text}");
    let written = fs::read_to_string(project.join(".gitconfig")).unwrap_or_default();
    assert!(!written.contains("HOST_ONLY_SECRET"), "{written}");
}

#[cfg(unix)]
#[test]
fn test_init_refuses_symlinked_destination_file() {
    use std::os::unix::fs::symlink;
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let outside = temp.path().join("CLAUDE.md");
    fs::write(&outside, "# precious shared rules\n").unwrap();
    fs::create_dir_all(project.join(".agents")).unwrap();
    let agents = project.join(".agents/AGENTS.md");
    symlink(&outside, &agents).unwrap();

    let path = project.to_string_lossy().to_string();
    let init = ude(&project, &["init", "--path", &path, "--type", "rust"]);
    let text = outputs(&init);
    assert_eq!(init.status.code(), Some(2), "{text}");
    assert!(text.contains("symlink"), "{text}");
    assert_eq!(
        fs::read_to_string(&outside).unwrap(),
        "# precious shared rules\n",
        "init wrote through the symlink"
    );
    assert!(
        !project.join(".gitconfig").exists(),
        "init wrote files before refusing"
    );

    // Negative control: a regular file at the same path is written.
    fs::remove_file(&agents).unwrap();
    reinit_rust(&ude, &project);
    let meta = fs::symlink_metadata(&agents).unwrap();
    assert!(meta.file_type().is_file(), "{meta:?}");
    assert_ne!(fs::read_to_string(&agents).unwrap(), "");
}

#[cfg(unix)]
#[test]
fn test_init_refuses_symlinked_destination_dir() {
    use std::os::unix::fs::symlink;
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let outside = temp.path().join("outside_scripts");
    fs::create_dir_all(&outside).unwrap();
    let scripts = project.join("scripts");
    symlink(&outside, &scripts).unwrap();

    let path = project.to_string_lossy().to_string();
    let init = ude(&project, &["init", "--path", &path, "--type", "rust"]);
    let text = outputs(&init);
    assert_eq!(init.status.code(), Some(2), "{text}");
    assert!(text.contains("symlink"), "{text}");
    let leaked: Vec<_> = fs::read_dir(&outside).unwrap().collect();
    assert!(
        leaked.is_empty(),
        "init created {leaked:?} outside the project"
    );

    // Negative control: a regular directory at the same path is written into.
    fs::remove_file(&scripts).unwrap();
    reinit_rust(&ude, &project);
    assert!(project.join("scripts/hooks/pre-commit").is_file());
}

#[test]
fn test_reinit_without_type_keeps_recorded_type() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let path = project.to_string_lossy().to_string();
    let recorded_type = |project: &Path| -> String {
        let state = fs::read_to_string(project.join(".ude/state.json")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&state).unwrap();
        json["project_type"].as_str().unwrap().to_string()
    };

    let init = ude(&project, &["init", "--path", &path, "--type", "node"]);
    assert_eq!(init.status.code(), Some(0), "{}", outputs(&init));
    let node_mise = fs::read_to_string(project.join("mise.toml")).unwrap();

    let init = ude(&project, &["init", "--path", &path]);
    assert_eq!(init.status.code(), Some(0), "{}", outputs(&init));
    assert_eq!(recorded_type(&project), "node");
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(status.contains("mise.toml: In Sync"), "{status}");
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert_eq!(
        fs::read_to_string(project.join("mise.toml")).unwrap(),
        node_mise
    );

    // Negative control: an explicit --type still changes the recorded type.
    reinit_rust(&ude, &project);
    assert_eq!(recorded_type(&project), "rust");
}

#[test]
fn test_init_keeps_non_utf8_gitignore_rules() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let original: &[u8] = b"# caf\xe9 rules\nsecrets/\n.env\n";
    fs::write(project.join(".gitignore"), original).unwrap();

    reinit_rust(&ude, &project);
    let after = fs::read(project.join(".gitignore")).unwrap();
    assert!(
        after.starts_with(original),
        "existing rules lost: {}",
        String::from_utf8_lossy(&after)
    );
    assert!(
        String::from_utf8_lossy(&after).contains("graphify-out/*"),
        "{}",
        String::from_utf8_lossy(&after)
    );
}

#[cfg(unix)]
#[test]
fn test_init_refuses_unreadable_gitignore() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let gitignore = project.join(".gitignore");
    fs::write(&gitignore, "secrets/\n.env\n").unwrap();
    fs::set_permissions(&gitignore, fs::Permissions::from_mode(0o200)).unwrap();
    // Negative control: the fixture really is unreadable (not the case as root).
    let readable = fs::read(&gitignore).is_ok();

    let path = project.to_string_lossy().to_string();
    let init = ude(&project, &["init", "--path", &path, "--type", "rust"]);
    fs::set_permissions(&gitignore, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!readable, "fixture .gitignore is readable; run as non-root");
    let text = outputs(&init);
    assert_eq!(init.status.code(), Some(2), "{text}");
    assert!(text.contains(".gitignore"), "{text}");
    assert_eq!(fs::read_to_string(&gitignore).unwrap(), "secrets/\n.env\n");
}

#[cfg(unix)]
#[test]
fn test_push_refuses_symlinked_local_file() {
    use std::os::unix::fs::symlink;
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    reinit_rust(&ude, &project);
    let template = project.join(GITCONFIG_TEMPLATE);
    let before = fs::read_to_string(&template).unwrap();

    let local = project.join(".gitconfig");
    fs::remove_file(&local).unwrap();
    symlink(host_secret_file(temp.path()), &local).unwrap();
    let push = ude(&project, &["sync", "push"]);
    let text = outputs(&push);
    assert_eq!(push.status.code(), Some(2), "{text}");
    assert!(text.contains("symlink"), "{text}");
    assert_eq!(
        fs::read_to_string(&template).unwrap(),
        before,
        "push published the symlink target"
    );

    // Negative control: the same edit as a regular file is published.
    fs::remove_file(&local).unwrap();
    let edited = format!("{before}# local edit\n");
    fs::write(&local, &edited).unwrap();
    let push = ude(&project, &["sync", "push"]);
    assert_eq!(push.status.code(), Some(0), "{}", outputs(&push));
    assert_eq!(fs::read_to_string(&template).unwrap(), edited);
}

/// A `ude` runner whose TypeSafe check is on, with a fake key, and whose `curl` is a
/// stub that saves the request body to the returned log and fails, so nothing leaves
/// the machine.
#[cfg(unix)]
fn typesafe_stub_cli(root: &Path) -> (impl Fn(&Path, &[&str]) -> Output, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let stub_dir = root.join("stub_bin");
    fs::create_dir_all(&stub_dir).unwrap();
    let log = root.join("curl_request.json");
    let argv = root.join("curl_argv.txt");
    let curl = stub_dir.join("curl");
    fs::write(
        &curl,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\ncat > '{}'\nexit 1\n",
            argv.display(),
            log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        stub_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let ude = sync_cli_with_env(
        root,
        vec![
            ("PATH".to_string(), path),
            ("TYPESAFE_API_KEY".to_string(), "stub-key".to_string()),
        ],
    );
    (ude, log)
}

#[cfg(unix)]
#[test]
fn test_push_typesafe_check_does_not_follow_symlinked_agents() {
    use std::os::unix::fs::symlink;
    let temp = tempdir().unwrap();
    let (ude, log) = typesafe_stub_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    reinit_rust(&ude, &project);
    let private = temp.path().join("private_notes.md");
    fs::write(&private, "PRIVATE_PROSE_MARKER\n").unwrap();
    let agents = project.join(".agents/AGENTS.md");
    fs::remove_file(&agents).unwrap();
    symlink(&private, &agents).unwrap();

    let push = ude(&project, &["sync", "push"]);
    let text = outputs(&push);
    assert_eq!(push.status.code(), Some(2), "{text}");
    assert!(text.contains("symlink"), "{text}");
    let sent = fs::read_to_string(&log).unwrap_or_default();
    assert!(!sent.contains("PRIVATE_PROSE_MARKER"), "sent: {sent}");

    // Negative control: the same text as a regular file reaches the stub.
    fs::remove_file(&agents).unwrap();
    fs::write(&agents, "PRIVATE_PROSE_MARKER\n").unwrap();
    let push = ude(&project, &["sync", "push"]);
    let sent = fs::read_to_string(&log).unwrap_or_default();
    assert!(sent.contains("PRIVATE_PROSE_MARKER"), "{}", outputs(&push));
}

#[cfg(unix)]
#[test]
fn test_typesafe_key_is_not_passed_on_the_curl_command_line() {
    let temp = tempdir().unwrap();
    let (ude, stdin_log) = typesafe_stub_cli(temp.path());
    let argv_log = temp.path().join("curl_argv.txt");
    let project = project_with_local_templates(temp.path(), "project");
    reinit_rust(&ude, &project);
    append_to(&project.join(".agents/AGENTS.md"), "\nA local rule edit.\n");

    let push = ude(&project, &["sync", "push"]);
    // Negative control: the stub really ran, so an empty argv cannot pass by accident.
    let argv = fs::read_to_string(&argv_log).expect("curl stub was not invoked");
    assert!(
        argv.contains("api.typesafe.ai"),
        "argv: {argv}\n{}",
        outputs(&push)
    );
    assert!(
        !argv.contains("stub-key"),
        "API key visible in argv: {argv}"
    );
    let stdin = fs::read_to_string(&stdin_log).unwrap();
    assert!(
        stdin.contains("Authorization: Bearer stub-key"),
        "stdin: {stdin}"
    );
    assert!(stdin.contains("A local rule edit."), "stdin: {stdin}");
}

#[cfg(unix)]
#[test]
fn test_scan_failure_is_not_reported_as_a_state_file_error() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    reinit_rust(&ude, &project);
    let outside = temp.path().join("outside");
    fs::write(&outside, "[core]\n").unwrap();
    fs::remove_file(project.join(".gitconfig")).unwrap();
    std::os::unix::fs::symlink(&outside, project.join(".gitconfig")).unwrap();

    for action in ["status", "pull"] {
        let out = ude(&project, &["sync", action]);
        let text = outputs(&out);
        assert_eq!(out.status.code(), Some(2), "{action}: {text}");
        assert!(
            text.contains("could not compare the project with upstream"),
            "{action}: {text}"
        );
        assert!(text.contains("symlink"), "{action}: {text}");
        assert!(
            !text.contains("I/O error on .ude/state.json"),
            "{action}: {text}"
        );
        assert!(
            !text.contains("Failed to read sync state"),
            "{action}: {text}"
        );
    }
}

#[cfg(unix)]
#[test]
fn test_push_does_not_treat_dangling_symlink_as_deletion() {
    use std::os::unix::fs::symlink;
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    reinit_rust(&ude, &project);

    let hook = project.join("scripts/hooks/pre-push");
    fs::remove_file(&hook).unwrap();
    symlink(temp.path().join("does-not-exist"), &hook).unwrap();
    let push = ude(&project, &["sync", "push"]);
    let text = outputs(&push);
    assert_eq!(push.status.code(), Some(2), "{text}");
    assert!(text.contains("symlink"), "{text}");
    assert!(
        project.join(PRE_PUSH_TEMPLATE).exists(),
        "a dangling symlink was propagated as a deletion: {text}"
    );
}

#[cfg(unix)]
#[test]
fn test_dangling_state_symlink_fails_loudly() {
    use std::os::unix::fs::symlink;
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    reinit_rust(&ude, &project);

    let state = project.join(".ude/state.json");
    fs::remove_file(&state).unwrap();
    // Negative control: no state file at all is a fresh project, not an error.
    let status = ude(&project, &["sync", "status"]);
    assert_eq!(status.status.code(), Some(0), "{}", outputs(&status));

    symlink(temp.path().join("does-not-exist"), &state).unwrap();
    let status = ude(&project, &["sync", "status"]);
    let text = outputs(&status);
    assert_eq!(status.status.code(), Some(2), "{text}");
    assert!(text.contains("symlink"), "{text}");
}

#[test]
fn test_first_init_on_fresh_clone_keeps_committed_edits() {
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let machine_a = project_with_local_templates(temp.path(), "machine_a");
    git.run(temp.path(), &["init", "-q", &machine_a.to_string_lossy()]);
    reinit_rust(&ude, &machine_a);
    git.run(&machine_a, &["add", "-A"]);
    commit_without_project_hooks(&git, &machine_a, "chore: init");

    // Committed edits on machine A: a copied file and a rendered one.
    let gitconfig = append_to(&machine_a.join(".gitconfig"), "[alias]\n\tco = checkout\n");
    let mise = append_to(&machine_a.join("mise.toml"), "# team task\n");
    git.run(&machine_a, &["add", "-A"]);
    commit_without_project_hooks(&git, &machine_a, "chore: team edits");
    let status_a = outputs(&ude(&machine_a, &["sync", "status"]));
    assert!(
        line_with(&status_a, ".gitconfig:").contains("Locally Modified"),
        "{status_a}"
    );

    // Machine B clones; state.json is per machine, so B has no records.
    let machine_b = temp.path().join("machine_b");
    git.run(
        temp.path(),
        &[
            "clone",
            "-q",
            &machine_a.to_string_lossy(),
            &machine_b.to_string_lossy(),
        ],
    );
    assert!(!machine_b.join(".ude/state.json").exists());
    let init = init_output(&ude, &machine_b);

    assert_eq!(
        fs::read_to_string(machine_b.join(".gitconfig")).unwrap(),
        gitconfig,
        "init overwrote a committed edit: {init}"
    );
    assert_eq!(
        fs::read_to_string(machine_b.join("mise.toml")).unwrap(),
        mise,
        "init overwrote a committed edit: {init}"
    );
    assert!(init.contains("Kept committed .gitconfig"), "{init}");
    let diff = git.output(&machine_b, &["diff", "--name-only"]);
    assert!(diff.is_empty(), "init changed committed files: {diff}");

    // B sees what A sees: the edits as local modifications, the rest in sync.
    let status_b = outputs(&ude(&machine_b, &["sync", "status"]));
    assert!(
        line_with(&status_b, ".gitconfig:").contains("Locally Modified"),
        "{status_b}"
    );
    assert!(
        line_with(&status_b, "mise.toml:").contains("Locally Modified"),
        "{status_b}"
    );
    // Negative control: unedited committed files (copied and rendered) stay in sync.
    assert!(
        line_with(&status_b, "scripts/hooks/pre-push:").contains("In Sync"),
        "{status_b}"
    );
    assert!(
        line_with(&status_b, ".agents/AGENTS.md:").contains("In Sync"),
        "{status_b}"
    );
}

#[test]
fn test_first_init_in_monorepo_subdir_keeps_committed_edits() {
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let mono_a = temp.path().join("mono_a");
    git.run(temp.path(), &["init", "-q", &mono_a.to_string_lossy()]);
    let svc_a = project_with_local_templates(&mono_a, "svc");
    reinit_rust(&ude, &svc_a);
    let gitconfig = append_to(&svc_a.join(".gitconfig"), "[alias]\n\tco = checkout\n");
    git.run(&mono_a, &["add", "-A"]);
    commit_without_project_hooks(&git, &mono_a, "chore: add svc");

    // A fresh clone has no state.json; the project has no .git of its own.
    let mono_b = temp.path().join("mono_b");
    git.run(
        temp.path(),
        &[
            "clone",
            "-q",
            &mono_a.to_string_lossy(),
            &mono_b.to_string_lossy(),
        ],
    );
    let svc_b = mono_b.join("svc");
    assert!(!svc_b.join(".ude/state.json").exists());
    assert!(!svc_b.join(".git").exists());
    let init = init_output(&ude, &svc_b);

    assert_eq!(
        fs::read_to_string(svc_b.join(".gitconfig")).unwrap(),
        gitconfig,
        "init overwrote a committed edit: {init}"
    );
    assert!(init.contains("Kept committed .gitconfig"), "{init}");
    let diff = git.output(&mono_b, &["diff", "--name-only"]);
    assert!(diff.is_empty(), "init changed committed files: {diff}");
}

#[test]
fn test_fresh_clone_does_not_resurrect_template_deleted_upstream() {
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let machine_a = project_with_local_templates(temp.path(), "machine_a");
    git.run(temp.path(), &["init", "-q", &machine_a.to_string_lossy()]);
    reinit_rust(&ude, &machine_a);
    git.run(&machine_a, &["add", "-A"]);
    commit_without_project_hooks(&git, &machine_a, "chore: init");

    // Machine B clones (no state.json); upstream has since deleted two templates.
    // .gitconfig is unedited; pre-push gets an uncommitted edit before init.
    let machine_b = temp.path().join("machine_b");
    git.run(
        temp.path(),
        &[
            "clone",
            "-q",
            &machine_a.to_string_lossy(),
            &machine_b.to_string_lossy(),
        ],
    );
    let gitconfig_template = machine_b.join(GITCONFIG_TEMPLATE);
    let pre_push_template = machine_b.join(PRE_PUSH_TEMPLATE);
    fs::remove_file(&gitconfig_template).unwrap();
    fs::remove_file(&pre_push_template).unwrap();
    let gitconfig = fs::read_to_string(machine_b.join(".gitconfig")).unwrap();
    let pre_push = append_to(&machine_b.join("scripts/hooks/pre-push"), "# local only\n");
    let init = init_output(&ude, &machine_b);
    assert!(init.contains("no shared template"), "{init}");

    // Neither deleted nor published: sync leaves both alone.
    let status = outputs(&ude(&machine_b, &["sync", "status"]));
    for rel in [".gitconfig:", "scripts/hooks/pre-push:"] {
        assert!(line_with(&status, rel).contains("Unmanaged"), "{status}");
    }
    let pull = ude(&machine_b, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert_eq!(
        fs::read_to_string(machine_b.join(".gitconfig")).unwrap(),
        gitconfig
    );
    assert_eq!(
        fs::read_to_string(machine_b.join("scripts/hooks/pre-push")).unwrap(),
        pre_push,
        "pull deleted an uncommitted edit"
    );
    let push = ude(&machine_b, &["sync", "push"]);
    assert_eq!(push.status.code(), Some(0), "{}", outputs(&push));
    assert!(
        !gitconfig_template.exists(),
        "push resurrected a deleted template"
    );
    assert!(
        !pre_push_template.exists(),
        "push resurrected a deleted template"
    );
    // Negative control: a committed file whose template still exists stays in sync.
    assert!(
        line_with(&status, "scripts/hooks/pre-commit:").contains("In Sync"),
        "{status}"
    );
}

#[test]
fn test_init_keeps_committed_gitignore_when_template_is_absent() {
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    git.run(temp.path(), &["init", "-q", &project.to_string_lossy()]);
    fs::remove_file(project.join(GITIGNORE_TEMPLATE)).unwrap();
    fs::write(project.join(".gitignore"), "secrets/\n").unwrap();
    git.run(&project, &["add", ".gitignore"]);
    commit_without_project_hooks(&git, &project, "chore: own gitignore");

    reinit_rust(&ude, &project);
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(
        line_with(&status, ".gitignore:").contains("Unmanaged"),
        "{status}"
    );
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).unwrap(),
        "secrets/\n"
    );
    let push = ude(&project, &["sync", "push"]);
    assert_eq!(push.status.code(), Some(0), "{}", outputs(&push));
    assert!(
        !project.join(GITIGNORE_TEMPLATE).exists(),
        "push published a template that never existed"
    );
}

#[test]
fn test_init_keeps_uncommitted_gitignore_when_template_is_absent() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    fs::remove_file(project.join(GITIGNORE_TEMPLATE)).unwrap();
    fs::write(project.join(".gitignore"), "secrets/\n").unwrap();

    reinit_rust(&ude, &project);
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(
        !line_with(&status, ".gitignore:").contains("Deleted upstream"),
        "{status}"
    );
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    let kept = fs::read_to_string(project.join(".gitignore")).unwrap();
    assert!(kept.starts_with("secrets/\n"), "{kept}");
}

const CONTAINER_TEMPLATE: &str = "templates/projects/generic/container/check-in-container.sh";

#[test]
fn test_init_leaves_uncommitted_file_without_template_unmanaged() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    fs::remove_file(project.join(CONTAINER_TEMPLATE)).unwrap();
    let script = project.join("scripts/check-in-container.sh");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, "#!/bin/sh\necho mine\n").unwrap();

    reinit_rust(&ude, &project);
    let status = outputs(&ude(&project, &["sync", "status"]));
    let line = line_with(&status, "check-in-container.sh:");
    assert!(line.contains("Unmanaged"), "{status}");
    // The template never existed, so status must not claim it was deleted upstream.
    assert!(line.contains("no shared template"), "{status}");
    assert!(!line.contains("deleted upstream"), "{status}");
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert_eq!(
        fs::read_to_string(&script).unwrap(),
        "#!/bin/sh\necho mine\n",
        "pull deleted or replaced a file the user created"
    );
    let push = ude(&project, &["sync", "push"]);
    assert_eq!(push.status.code(), Some(0), "{}", outputs(&push));
    assert!(
        !project.join(CONTAINER_TEMPLATE).exists(),
        "push published a template that never existed"
    );
}

#[test]
fn test_init_writes_template_over_uncommitted_file_when_template_exists() {
    // Negative control for the unmanaged case above: with a template, init writes it.
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let script = project.join("scripts/check-in-container.sh");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, "#!/bin/sh\necho mine\n").unwrap();

    reinit_rust(&ude, &project);
    assert_eq!(
        fs::read(&script).unwrap(),
        fs::read(project.join(CONTAINER_TEMPLATE)).unwrap()
    );
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(
        line_with(&status, "check-in-container.sh:").contains("In Sync"),
        "{status}"
    );
}

#[cfg(unix)]
#[test]
fn test_init_refuses_unreadable_committed_file_without_template() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    git.run(temp.path(), &["init", "-q", &project.to_string_lossy()]);
    fs::remove_file(project.join(GITCONFIG_TEMPLATE)).unwrap();
    let gitconfig = project.join(".gitconfig");
    fs::write(&gitconfig, "[alias]\n\tco = checkout\n").unwrap();
    git.run(&project, &["add", ".gitconfig"]);
    commit_without_project_hooks(&git, &project, "chore: own gitconfig");
    fs::set_permissions(&gitconfig, fs::Permissions::from_mode(0o200)).unwrap();
    // Negative control: the fixture really is unreadable (not the case as root).
    let readable = fs::read(&gitconfig).is_ok();

    let path = project.to_string_lossy().to_string();
    let init = ude(&project, &["init", "--path", &path, "--type", "rust"]);
    fs::set_permissions(&gitconfig, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!readable, "fixture .gitconfig is readable; run as non-root");
    let text = outputs(&init);
    assert_eq!(init.status.code(), Some(2), "{text}");
    assert!(text.contains("Cannot read committed .gitconfig"), "{text}");
    assert!(
        !project.join(".ude/state.json").exists(),
        "init recorded state without the file's unmanaged marker"
    );
}

#[test]
fn test_init_ignores_unusable_git_dir_above_project() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let outer = temp.path().join("outer");
    fs::create_dir_all(outer.join(".git")).unwrap();
    let project = project_with_local_templates(&outer, "svc");

    let init = init_output(&ude, &project);
    assert!(
        init.contains("not treating the project as a git checkout"),
        "{init}"
    );
    assert!(project.join(".ude/state.json").exists(), "{init}");
}

#[cfg(unix)]
#[test]
fn test_init_fails_when_state_cannot_be_saved() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let ude_dir = project.join(".ude");
    fs::create_dir_all(&ude_dir).unwrap();
    fs::set_permissions(&ude_dir, fs::Permissions::from_mode(0o500)).unwrap();
    // Negative control: the fixture really is unwritable (not the case as root).
    let writable = fs::write(ude_dir.join("probe"), "").is_ok();

    let path = project.to_string_lossy().to_string();
    let init = ude(&project, &["init", "--path", &path, "--type", "rust"]);
    fs::set_permissions(&ude_dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!writable, "fixture .ude is writable; run as non-root");
    let text = outputs(&init);
    assert_eq!(init.status.code(), Some(2), "{text}");
    assert!(text.contains("cannot persist .ude/state.json"), "{text}");
}

#[test]
fn test_init_records_the_detected_type_for_generic() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();

    let path = project.to_string_lossy().to_string();
    let init = ude(&project, &["init", "--path", &path, "--type", "generic"]);
    assert_eq!(init.status.code(), Some(0), "{}", outputs(&init));
    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(status.contains("mise.toml: In Sync"), "{status}");
    let state = fs::read_to_string(project.join(".ude/state.json")).unwrap();
    assert!(state.contains("\"project_type\": \"rust\""), "{state}");
}

#[test]
fn test_init_fails_when_parent_repo_index_is_unreadable() {
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let mono_a = temp.path().join("mono_a");
    git.run(temp.path(), &["init", "-q", &mono_a.to_string_lossy()]);
    let svc_a = project_with_local_templates(&mono_a, "svc");
    reinit_rust(&ude, &svc_a);
    let gitconfig = append_to(&svc_a.join(".gitconfig"), "[alias]\n\tco = checkout\n");
    git.run(&mono_a, &["add", "-A"]);
    commit_without_project_hooks(&git, &mono_a, "chore: add svc");
    let mono_b = temp.path().join("mono_b");
    git.run(
        temp.path(),
        &[
            "clone",
            "-q",
            &mono_a.to_string_lossy(),
            &mono_b.to_string_lossy(),
        ],
    );
    // git still recognises the repository, but cannot read its index.
    fs::write(mono_b.join(".git/index"), "garbage").unwrap();
    let svc_b = mono_b.join("svc");

    let path = svc_b.to_string_lossy().to_string();
    let init = ude(&svc_b, &["init", "--path", &path, "--type", "rust"]);
    let text = outputs(&init);
    assert_eq!(init.status.code(), Some(2), "{text}");
    assert!(text.contains("git ls-files failed"), "{text}");
    assert_eq!(
        fs::read_to_string(svc_b.join(".gitconfig")).unwrap(),
        gitconfig,
        "init overwrote a committed edit: {text}"
    );
}

#[test]
fn test_sync_ignores_a_project_templates_dir_that_is_not_ude() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = temp.path().join("web");
    fs::create_dir_all(&project).unwrap();
    let path = project.to_string_lossy().to_string();
    let init = ude(temp.path(), &["init", "--path", &path, "--type", "rust"]);
    assert_eq!(init.status.code(), Some(0), "{}", outputs(&init));
    // A web project's own templates/ (Django, Flask, Helm) is not a ude template tree.
    fs::create_dir_all(project.join("templates")).unwrap();
    fs::write(project.join("templates/index.html"), "<html></html>\n").unwrap();

    let status = outputs(&ude(&project, &["sync", "status"]));
    assert!(!status.contains("Deleted upstream"), "{status}");
    assert!(status.contains(".gitconfig: In Sync"), "{status}");
    let pull = ude(&project, &["sync", "pull"]);
    assert_eq!(pull.status.code(), Some(0), "{}", outputs(&pull));
    assert!(
        project.join(".gitconfig").exists(),
        "pull deleted managed files"
    );
}

#[test]
fn test_init_rejects_an_unknown_type() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    let path = project.to_string_lossy().to_string();

    let typo = ude(&project, &["init", "--path", &path, "--type", "rsut"]);
    let text = outputs(&typo);
    assert_eq!(typo.status.code(), Some(2), "{text}");
    assert!(text.contains("invalid value"), "{text}");
    assert!(!project.join(".ude/state.json").exists(), "{text}");
    // Negative control: a supported type is accepted.
    let node = ude(&project, &["init", "--path", &path, "--type", "node"]);
    assert_eq!(node.status.code(), Some(0), "{}", outputs(&node));
}

#[test]
fn test_init_can_skip_mempalace_and_graphify() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    for (flags, created) in [
        (vec!["--mempalace=false", "--graphify=false"], false),
        (vec![], true),
    ] {
        let project = project_with_local_templates(temp.path(), &format!("p{created}"));
        let path = project.to_string_lossy().to_string();
        let mut args = vec!["init", "--path", &path, "--type", "rust"];
        args.extend(flags.iter().copied());
        let init = ude(&project, &args);
        assert_eq!(init.status.code(), Some(0), "{args:?}: {}", outputs(&init));
        assert_eq!(
            project.join(".planning/.mempalace-stage").exists(),
            created,
            "{args:?}"
        );
        assert_eq!(project.join("graphify-out").exists(), created, "{args:?}");
    }
    // A bare flag still means true.
    let project = project_with_local_templates(temp.path(), "bare");
    let path = project.to_string_lossy().to_string();
    let init = ude(
        &project,
        &[
            "init",
            "--path",
            &path,
            "--type",
            "rust",
            "--mempalace",
            "--graphify",
        ],
    );
    assert_eq!(init.status.code(), Some(0), "{}", outputs(&init));
    assert!(project.join(".planning/.mempalace-stage").exists());
}

#[test]
fn test_init_fails_on_unusable_git_dir_in_project() {
    // Negative control for the case above: the project's own .git must work.
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "svc");
    fs::create_dir_all(project.join(".git")).unwrap();

    let path = project.to_string_lossy().to_string();
    let init = ude(&project, &["init", "--path", &path, "--type", "rust"]);
    let text = outputs(&init);
    assert_eq!(init.status.code(), Some(2), "{text}");
    assert!(text.contains("git ls-files failed"), "{text}");
}

#[test]
fn test_reinit_missing_file_warning_follows_upstream() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    reinit_rust(&ude, &project);

    // pre-push: deleted locally, changed upstream. commit-msg: deleted on both sides.
    // pre-commit: deleted locally only.
    for hook in ["pre-push", "commit-msg", "pre-commit"] {
        fs::remove_file(project.join("scripts/hooks").join(hook)).unwrap();
    }
    let upstream = append_to(&project.join(PRE_PUSH_TEMPLATE), "# upstream change\n");
    fs::remove_file(project.join(COMMIT_MSG_TEMPLATE)).unwrap();

    let init = init_output(&ude, &project);
    let conflicted = line_with(&init, "scripts/hooks/pre-push is tracked but missing");
    assert!(
        conflicted.contains("skips it as a conflict") && !conflicted.contains("will delete"),
        "{init}"
    );
    let both = line_with(&init, "scripts/hooks/commit-msg is tracked but missing");
    assert!(
        both.contains("deletes nothing") && !both.contains("will delete"),
        "{init}"
    );
    assert!(
        !init.contains("projects/generic/hooks/commit-msg"),
        "the remedy names a template that no longer exists: {init}"
    );
    // Negative control: a plain local deletion still warns that push deletes.
    assert!(
        line_with(&init, "scripts/hooks/pre-commit is tracked but missing")
            .contains("a plain ude sync push will delete its shared template"),
        "{init}"
    );

    let push = ude(&project, &["sync", "push"]);
    assert_eq!(
        fs::read_to_string(project.join(PRE_PUSH_TEMPLATE)).unwrap(),
        upstream,
        "{}",
        outputs(&push)
    );
    assert!(
        !project.join(PRE_COMMIT_TEMPLATE).exists(),
        "{}",
        outputs(&push)
    );

    // The both-deleted remedy: pull first, then restore; the next pull keeps the copy
    // and push never republishes it.
    let both = line_with(&init, "To keep it in this project only");
    assert!(
        both.contains("git restore scripts/hooks/commit-msg"),
        "{init}"
    );
    ude(&project, &["sync", "pull"]);
    let commit_msg = project.join("scripts/hooks/commit-msg");
    fs::write(&commit_msg, "#!/bin/sh\n").unwrap();
    let pull = ude(&project, &["sync", "pull"]);
    assert!(
        commit_msg.exists(),
        "pull deleted the restored copy: {}",
        outputs(&pull)
    );
    let push = ude(&project, &["sync", "push"]);
    assert!(
        !project.join(COMMIT_MSG_TEMPLATE).exists(),
        "{}",
        outputs(&push)
    );
}
