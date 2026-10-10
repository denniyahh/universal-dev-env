use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_cli_e2e_init_and_sync_lifecycle() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let proj = tempdir().unwrap();

    // 1. Run `ude init` with --path
    let init_status = Command::new(bin)
        .args(["init", "--path", &proj.path().to_string_lossy(), "--type", "rust"])
        .status()
        .expect("ude init execution");
    assert!(init_status.success(), "ude init must succeed");

    // Verify state.json was created
    let state_file = proj.path().join(".ude/state.json");
    assert!(state_file.exists(), ".ude/state.json must be created by init");
    let state_content = fs::read_to_string(&state_file).unwrap();
    assert!(state_content.contains("\"version\": 1"));
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
    assert!(stdout.contains(".gitconfig: In Sync"), "Output was: {stdout}");
    assert!(stdout.contains("mise.toml: In Sync"), "Output was: {stdout}");

    // 3. Edit local file
    let loc_mise = proj.path().join("mise.toml");
    fs::write(&loc_mise, "# Locally modified\n[tools]\nrust = '1.99'\n").unwrap();

    let status_output2 = Command::new(bin)
        .current_dir(proj.path())
        .args(["sync", "status"])
        .output()
        .expect("ude sync status execution");
    let stdout2 = String::from_utf8_lossy(&status_output2.stdout);
    assert!(stdout2.contains("mise.toml: Locally Modified"), "Output was: {stdout2}");
    assert!(stdout2.contains(".gitconfig: In Sync"), "Output was: {stdout2}");
}

#[test]
fn test_cli_project_init_and_doctor_host_flags() {
    let bin = env!("CARGO_BIN_EXE_ude");
    let proj = tempdir().unwrap();

    // Verify `ude project init <path>` subcommand syntax works
    let init_status = Command::new(bin)
        .args(["project", "init", &proj.path().to_string_lossy(), "--type", "node"])
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
    assert!(code == 0 || code == 1 || code == 2, "doctor --host returned unexpected exit code {code}");
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
    fs::write(config_dir.join("git/allowed_signers"), "alice@example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI...\n").unwrap();

    // Configure git signing in isolated environment
    let _ = Command::new("git")
        .args(["config", "--global", "gpg.format", "ssh"])
        .env("HOME", fake_home.path())
        .env("XDG_CONFIG_HOME", &config_dir)
        .status();
    let _ = Command::new("git")
        .args(["config", "--global", "commit.gpgsign", "true"])
        .env("HOME", fake_home.path())
        .env("XDG_CONFIG_HOME", &config_dir)
        .status();

    let doctor_fixed = Command::new(bin)
        .args(["doctor", "--host"])
        .env("HOME", fake_home.path())
        .env("XDG_CONFIG_HOME", &config_dir)
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
        .args(["project", "init", &file_dest.to_string_lossy(), "--type", "node"])
        .status()
        .expect("ude init execution");
    assert!(!init_fail.success(), "init must fail when destination is a regular file");

    // 2. Defect 5 reproduction: re-running initialization must not overwrite custom .gitignore
    let proj_dir = temp.path().join("new_project");
    fs::create_dir_all(&proj_dir).unwrap();
    let custom_rule = "my_custom_secret_build_output/\n";
    fs::write(proj_dir.join(".gitignore"), custom_rule).unwrap();

    let init_success = Command::new(bin)
        .args(["project", "init", &proj_dir.to_string_lossy(), "--type", "rust"])
        .status()
        .expect("ude init execution");
    assert!(init_success.success(), "init must succeed for directory destination");

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
    fs::write(config_dir.join("git/allowed_signers"), "# Only a comment\n\n").unwrap();

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
    let _ = Command::new("git").current_dir(&repo_dir).args(["init", "-b", "main"]).status();

    // Case 1: missing .gitconfig and missing hooks
    let fake_home = temp.path().join("home");
    fs::create_dir_all(&fake_home).unwrap();
    let doctor_out = Command::new(bin)
        .args(["doctor", "--repo", &repo_dir.to_string_lossy(), "--format", "json"])
        .env("HOME", &fake_home)
        .env("XDG_CONFIG_HOME", fake_home.join(".config"))
        .env("GIT_CONFIG_GLOBAL", fake_home.join(".gitconfig"))
        .output()
        .expect("ude doctor execution");

    let stdout = String::from_utf8_lossy(&doctor_out.stdout);
    assert!(stdout.contains("Missing root .gitconfig template") || stdout.contains("does not include root .gitconfig"));
    assert!(stdout.contains("Repository core.hooksPath is unset"));

    // Case 2: inert files (empty .gitconfig, hooksPath=/dev/null, empty hook)
    fs::write(repo_dir.join(".gitconfig"), "").unwrap();
    let _ = Command::new("git")
        .current_dir(&repo_dir)
        .args(["config", "--local", "core.hooksPath", "/dev/null"])
        .status();
    let hooks_dir = repo_dir.join("scripts/hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    fs::write(hooks_dir.join("pre-commit"), "").unwrap();

    let doctor_out2 = Command::new(bin)
        .args(["doctor", "--repo", &repo_dir.to_string_lossy(), "--format", "json"])
        .env("HOME", &fake_home)
        .env("XDG_CONFIG_HOME", fake_home.join(".config"))
        .env("GIT_CONFIG_GLOBAL", fake_home.join(".gitconfig"))
        .output()
        .expect("ude doctor execution");
    let stdout2 = String::from_utf8_lossy(&doctor_out2.stdout);
    assert!(stdout2.contains("empty (0 bytes)") || stdout2.contains("does not include root .gitconfig"));
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
        .args(["init", "--path", &local_proj.to_string_lossy(), "--type", "rust"])
        .status()
        .expect("ude init execution");
    assert!(init_status.success());

    // 2. Read baseline ancestor hash of mise.toml
    let state_file = local_proj.join(".ude/state.json");
    let state_before: serde_json::Value = serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
    let mise_hash_before = state_before["files"]["mise.toml"]["ancestor_hash"].as_str().unwrap().to_string();

    // 3. Edit local mise.toml
    let loc_mise = local_proj.join("mise.toml");
    fs::write(&loc_mise, "# Custom edit\n[tools]\nrust = 'nightly'\n").unwrap();

    // 4. Setup a fake remote repo with a templates directory
    let remote_dir = temp.path().join("remote_dir");
    let _ = Command::new("git").current_dir(temp.path()).args(["init", "-b", "main", "remote_dir"]).status();
    let tdir = remote_dir.join("templates/projects/agents");
    fs::create_dir_all(&tdir).unwrap();
    fs::write(tdir.join("mise.toml.jinja"), "[tools]\nrust = '1.85.0'\n").unwrap();
    let _ = Command::new("git").current_dir(&remote_dir).args(["add", "-A"]).status();
    let _ = Command::new("git").current_dir(&remote_dir).args([
        "-c", "user.name=Tester",
        "-c", "user.email=tester@example.com",
        "-c", "commit.gpgsign=false",
        "commit",
        "--no-gpg-sign",
        "-m", "base commit",
    ]).status();

    // 5. Run `ude sync push --repo <remote_dir> --pr`
    let _push_pr_out = Command::new(bin)
        .current_dir(&local_proj)
        .args(["sync", "push", "--repo", &remote_dir.to_string_lossy(), "--pr"])
        .output()
        .expect("ude sync push --pr execution");

    // 6. Verify: Canonical ancestor state in .ude/state.json must NOT advance to local edit!
    let state_after: serde_json::Value = serde_json::from_str(&fs::read_to_string(&state_file).unwrap()).unwrap();
    let mise_hash_after = state_after["files"]["mise.toml"]["ancestor_hash"].as_str().unwrap();
    assert_eq!(
        mise_hash_after,
        mise_hash_before,
        "PR push must NOT advance canonical ancestor state before PR merge!"
    );
}
