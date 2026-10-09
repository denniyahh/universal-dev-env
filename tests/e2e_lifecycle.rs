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

    // Verify `ude doctor --host` flag alias is accepted (not rejected with code 2)
    let doctor_output = Command::new(bin)
        .args(["doctor", "--host"])
        .output()
        .expect("ude doctor --host execution");
    // Should run audit without clap argument parsing error
    assert_ne!(
        doctor_output.status.code(),
        Some(2),
        "doctor --host must not exit 2 (clap syntax rejection)"
    );
}
