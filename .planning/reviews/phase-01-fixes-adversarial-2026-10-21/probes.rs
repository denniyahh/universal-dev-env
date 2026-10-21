// ===== external-review probes: each PASSES iff the claimed defect reproduces =====
#[cfg(unix)]
#[test]
fn probe_a_unreadable_committed_file_is_published() {
    use std::os::unix::fs::PermissionsExt;
    for unreadable in [true, false] {
        let temp = tempdir().unwrap();
        let git = GitFixture::new();
        let ude = sync_cli(temp.path());
        let project = project_with_local_templates(temp.path(), "project");
        git.run(temp.path(), &["init", "-q", &project.to_string_lossy()]);
        fs::remove_file(project.join(GITCONFIG_TEMPLATE)).unwrap();
        fs::write(project.join(".gitconfig"), "[user]\n\tname = x\n").unwrap();
        git.run(&project, &["add", ".gitconfig"]);
        commit_without_project_hooks(&git, &project, "chore: own gitconfig");
        let f = project.join(".gitconfig");
        if unreadable { fs::set_permissions(&f, fs::Permissions::from_mode(0o000)).unwrap(); }
        reinit_rust(&ude, &project);
        fs::set_permissions(&f, fs::Permissions::from_mode(0o644)).unwrap();
        let push = ude(&project, &["sync", "push"]);
        eprintln!("A unreadable={unreadable} push={:?} published={}", push.status.code(), project.join(GITCONFIG_TEMPLATE).exists());
        assert_eq!(project.join(GITCONFIG_TEMPLATE).exists(), unreadable, "{}", outputs(&push));
    }
}

#[test]
fn probe_b_uncommitted_gitignore_published() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    fs::remove_file(project.join(GITIGNORE_TEMPLATE)).unwrap();
    fs::write(project.join(".gitignore"), "secrets/\n").unwrap();
    reinit_rust(&ude, &project);
    let status = outputs(&ude(&project, &["sync", "status"]));
    let push = ude(&project, &["sync", "push"]);
    eprintln!("B status-line={:?} push={:?}", line_with(&status, ".gitignore:"), push.status.code());
    assert!(project.join(GITIGNORE_TEMPLATE).exists(), "{}", outputs(&push));
}

#[test]
fn probe_c_uncommitted_unkept_file_deleted_by_pull() {
    for template_absent in [true, false] {
        let temp = tempdir().unwrap();
        let ude = sync_cli(temp.path());
        let project = project_with_local_templates(temp.path(), "project");
        if template_absent { fs::remove_file(project.join("templates/projects/generic/container/check-in-container.sh")).unwrap(); }
        fs::create_dir_all(project.join("scripts")).unwrap();
        fs::write(project.join("scripts/check-in-container.sh"), "#!/bin/sh\necho mine\n").unwrap();
        reinit_rust(&ude, &project);
        let after_init = fs::read_to_string(project.join("scripts/check-in-container.sh")).ok();
        let status = outputs(&ude(&project, &["sync", "status"]));
        let pull = ude(&project, &["sync", "pull"]);
        let exists = project.join("scripts/check-in-container.sh").exists();
        eprintln!("C absent={template_absent} after_init={after_init:?} status={:?} pull={:?} exists_after_pull={exists}", line_with(&status, "check-in-container"), pull.status.code());
        if template_absent { assert!(!exists, "file survived"); }
    }
}

#[test]
fn probe_d_monorepo_hooks_not_registered() {
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let mono = temp.path().join("mono");
    fs::create_dir_all(&mono).unwrap();
    git.run(temp.path(), &["init", "-q", &mono.to_string_lossy()]);
    let project = project_with_local_templates(&mono, "svc");
    reinit_rust(&ude, &project);
    let hp = git.cmd(&mono).args(["config", "--get", "core.hooksPath"]).output().unwrap();
    eprintln!("D hooksPath rc={:?} out={:?}", hp.status.code(), String::from_utf8_lossy(&hp.stdout));
    assert_ne!(hp.status.code(), Some(0));
}

#[test]
fn probe_e_empty_parent_git_dir_aborts_init() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let outer = temp.path().join("outer");
    fs::create_dir_all(outer.join(".git")).unwrap();
    let project = project_with_local_templates(&outer, "svc");
    let path = project.to_string_lossy().to_string();
    let init = ude(&project, &["init", "--path", &path, "--type", "rust"]);
    eprintln!("E init={:?} {}", init.status.code(), outputs(&init).lines().last().unwrap_or(""));
    assert_ne!(init.status.code(), Some(0));
}

#[cfg(unix)]
#[test]
fn probe_f_symlinked_root_exits_0_without_state() {
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let real = project_with_local_templates(temp.path(), "real");
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let path = link.to_string_lossy().to_string();
    let init = ude(temp.path(), &["init", "--path", &path, "--type", "rust"]);
    eprintln!("F init={:?} state={} gitconfig={} warn={}", init.status.code(), real.join(".ude/state.json").exists(), real.join(".gitconfig").exists(), outputs(&init).contains("Failed to persist"));
    assert_eq!(init.status.code(), Some(0));
    assert!(!real.join(".ude/state.json").exists());
}

#[test]
fn probe_g_reappeared_then_deleted_template_deletes_kept_file() {
    let temp = tempdir().unwrap();
    let git = GitFixture::new();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    git.run(temp.path(), &["init", "-q", &project.to_string_lossy()]);
    let original = fs::read(project.join(GITCONFIG_TEMPLATE)).unwrap();
    fs::remove_file(project.join(GITCONFIG_TEMPLATE)).unwrap();
    fs::write(project.join(".gitconfig"), &original).unwrap();
    git.run(&project, &["add", ".gitconfig"]);
    commit_without_project_hooks(&git, &project, "chore: own gitconfig");
    reinit_rust(&ude, &project);
    fs::write(project.join(GITCONFIG_TEMPLATE), &original).unwrap();
    let s1 = outputs(&ude(&project, &["sync", "status"]));
    let p1 = ude(&project, &["sync", "pull"]);
    fs::remove_file(project.join(GITCONFIG_TEMPLATE)).unwrap();
    let p2 = ude(&project, &["sync", "pull"]);
    let exists = project.join(".gitconfig").exists();
    eprintln!("G s1={:?} p1={:?} p2={:?} exists={exists}", line_with(&s1, ".gitconfig:"), p1.status.code(), p2.status.code());
    assert!(!exists);
}

#[cfg(unix)]
#[test]
fn probe_h_unreadable_gitignore_partial_init() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempdir().unwrap();
    let ude = sync_cli(temp.path());
    let project = project_with_local_templates(temp.path(), "project");
    fs::write(project.join(".gitignore"), "x\n").unwrap();
    fs::set_permissions(project.join(".gitignore"), fs::Permissions::from_mode(0o000)).unwrap();
    let path = project.to_string_lossy().to_string();
    let init = ude(&project, &["init", "--path", &path, "--type", "rust"]);
    fs::set_permissions(project.join(".gitignore"), fs::Permissions::from_mode(0o644)).unwrap();
    eprintln!("H init={:?} gitconfig_written={}", init.status.code(), project.join(".gitconfig").exists());
    assert_eq!(init.status.code(), Some(2));
    assert!(project.join(".gitconfig").exists());
}
