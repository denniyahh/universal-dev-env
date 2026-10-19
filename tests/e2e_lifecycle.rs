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
        .stdin(Stdio::null());
    cmd
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
    let init_status = Command::new(bin)
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
    let status_output = Command::new(bin)
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

    let status_output2 = Command::new(bin)
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

    let init = Command::new(bin)
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
        let mut cmd = Command::new(bin);
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
    let init_status = Command::new(bin)
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
    let doctor_output = Command::new(bin)
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
    let doctor_output = Command::new(bin)
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

    let doctor_fixed = Command::new(bin)
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

    let init_fail = Command::new(bin)
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

    let init_success = Command::new(bin)
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

    let doctor_output = Command::new(bin)
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
    let doctor_out = Command::new(bin)
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

    let doctor_out2 = Command::new(bin)
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
    let init_status = Command::new(bin)
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

    let push_pr_out = Command::new(bin)
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
    let init_status = Command::new(bin)
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
    let push_fail = Command::new(bin)
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
    let status_out = Command::new(bin)
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
    let push_success = Command::new(bin)
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
    let init = Command::new(bin)
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
        Command::new(bin)
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

    let init = Command::new(bin)
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
        let mut cmd = Command::new(bin);
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
    let init = Command::new(bin)
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
    let bare = root.join(format!("{name}.git"));
    let bare_url = bare.to_string_lossy().to_string();
    git.run(root, &["init", "--bare", "-q", "-b", "main", &bare_url]);
    let seed = root.join(format!("{name}-seed"));
    git.run(root, &["init", "-q", "-b", "main", &seed.to_string_lossy()]);
    let tdir = seed.join("templates/projects/agents");
    fs::create_dir_all(&tdir).unwrap();
    fs::write(tdir.join("mise.toml.jinja"), mise).unwrap();
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
        let mut cmd = Command::new(bin);
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
    // D-12 through the CLI: .gitconfig is unchanged locally and absent from the remote.
    assert!(
        !project.join(".gitconfig").exists(),
        ".gitconfig must be removed. Out: {stdout}"
    );
    assert!(
        stdout.contains("Removed .gitconfig (deleted upstream)"),
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
    let bin = env!("CARGO_BIN_EXE_ude");
    let fake_home = root.join("fake_home");
    let fake_config = fake_home.join(".config");
    fs::create_dir_all(fake_config.join("git")).unwrap();
    let gitconfig = fake_config.join("git/config");
    fs::write(&gitconfig, FIXTURE_GITCONFIG).unwrap();
    let cache_dir = root.join("cache");
    fs::create_dir_all(&cache_dir).unwrap();
    move |project: &Path, args: &[&str]| {
        let mut cmd = Command::new(bin);
        cmd.current_dir(project).args(args);
        for name in FIXTURE_GIT_LOCAL_ENV_VARS {
            cmd.env_remove(name);
        }
        // No host TypeSafe key: push must not reach the semantic drift API.
        cmd.env_remove("TYPESAFE_API_KEY")
            .env("HOME", &fake_home)
            .env("XDG_CONFIG_HOME", &fake_config)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_CACHE_HOME", &cache_dir)
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
