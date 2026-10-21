pub mod audit;
pub mod cli;
pub mod manifest;
pub mod process;
pub mod sanitizer;
pub mod semantic;
pub mod sync;
pub mod template;

use clap::Parser;
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Exit code for drift, conflicts or skipped files.
const EXIT_DRIFT: i32 = 1;
/// Exit code for hard failures (I/O, state, remote).
const EXIT_HARD_FAILURE: i32 = 2;

fn main() {
    let args = cli::Cli::parse();

    match args.command {
        cli::Commands::Doctor(doctor_args) => {
            let auditor = audit::Auditor::new();
            let host_reports = auditor.audit_host();

            let repo_reports = if !doctor_args.host_only {
                let target_repo = doctor_args.repo.unwrap_or_else(|| PathBuf::from("."));
                Some((target_repo.clone(), auditor.audit_repo(&target_repo)))
            } else {
                None
            };

            let has_critical = host_reports.iter().any(|r| r.is_critical())
                || repo_reports
                    .as_ref()
                    .map(|(_, reps)| reps.iter().any(|r| r.is_critical()))
                    .unwrap_or(false);

            let has_failures = host_reports.iter().any(|r| r.is_fail())
                || repo_reports
                    .as_ref()
                    .map(|(_, reps)| reps.iter().any(|r| r.is_fail()))
                    .unwrap_or(false);

            if doctor_args.format == "json" {
                #[derive(serde::Serialize)]
                struct JsonDoctorReport {
                    host: Vec<audit::AuditReport>,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    repo: Option<Vec<audit::AuditReport>>,
                }

                let json_data = JsonDoctorReport {
                    host: host_reports,
                    repo: repo_reports.map(|(_, reports)| reports),
                };

                println!("{}", serde_json::to_string_pretty(&json_data).unwrap());
                if has_critical {
                    std::process::exit(2);
                } else if has_failures {
                    std::process::exit(1);
                }
                return;
            }

            println!("{}", "==> Running UDE Environment Doctor...".bold());

            // Run host audit
            println!("\n{}", "--- Host Machine Environment ---".cyan());
            for r in &host_reports {
                match &r.status {
                    audit::CheckStatus::Pass => {
                        println!("  {} [{}] {}", "✓".green(), r.category, r.name);
                    }
                    audit::CheckStatus::Warning(w) => {
                        println!("  {} [{}] {}: {}", "!".yellow(), r.category, r.name, w);
                    }
                    audit::CheckStatus::Fail(f) => {
                        println!("  {} [{}] {}: {}", "✗".red(), r.category, r.name, f);
                    }
                    audit::CheckStatus::Critical(c) => {
                        println!(
                            "  {} [{}] {}: {}",
                            "CRITICAL".red().bold(),
                            r.category,
                            r.name,
                            c
                        );
                    }
                }
            }

            if let Some((target_repo, reports)) = repo_reports {
                println!(
                    "\n{}",
                    format!("--- Repository Baseline ({}) ---", target_repo.display()).cyan()
                );
                for r in &reports {
                    match &r.status {
                        audit::CheckStatus::Pass => {
                            println!("  {} [{}] {}", "✓".green(), r.category, r.name);
                        }
                        audit::CheckStatus::Warning(w) => {
                            println!("  {} [{}] {}: {}", "!".yellow(), r.category, r.name, w);
                        }
                        audit::CheckStatus::Fail(f) => {
                            println!("  {} [{}] {}: {}", "✗".red(), r.category, r.name, f);
                        }
                        audit::CheckStatus::Critical(c) => {
                            println!(
                                "  {} [{}] {}: {}",
                                "CRITICAL".red().bold(),
                                r.category,
                                r.name,
                                c
                            );
                        }
                    }
                }
            }

            if has_critical {
                eprintln!(
                    "\n{}",
                    "Doctor detected critical security invariant failure(s). Exit code 2."
                        .red()
                        .bold()
                );
                std::process::exit(2);
            } else if has_failures {
                eprintln!(
                    "\n{}",
                    "Doctor detected failing checks. Remediation required."
                        .red()
                        .bold()
                );
                std::process::exit(1);
            } else {
                println!(
                    "\n{}",
                    "==> Doctor check completed. All required baselines verified.".bold()
                );
            }
        }
        cli::Commands::Init(init_args)
        | cli::Commands::Project(cli::ProjectCommands {
            action: cli::ProjectAction::Init(init_args),
        }) => {
            let project_path = init_args.resolved_path();
            if project_path.is_file() {
                eprintln!(
                    "  {} Cannot initialize project: '{}' is an existing regular file, not a directory.",
                    "✗".red(),
                    project_path.display()
                );
                std::process::exit(EXIT_HARD_FAILURE);
            }

            if let Err(e) = fs::create_dir_all(&project_path) {
                eprintln!(
                    "  {} Failed to create project directory {}: {e}",
                    "✗".red(),
                    project_path.display()
                );
                std::process::exit(EXIT_HARD_FAILURE);
            }

            // Records of an earlier `ude init` or sync. Unreadable state is never reset
            // (D-06), so init stops before touching any file.
            let prior_state = match sync::ProjectState::load_from_dir(&project_path) {
                Ok(state) => state,
                Err(e) => {
                    eprintln!("  {} Initialization failed: {e}", "✗".red());
                    std::process::exit(EXIT_HARD_FAILURE);
                }
            };
            // A managed file with any record (ancestor, tombstone or open conflict)
            // belongs to sync: a re-run of init neither writes it nor replaces its
            // record, so it cannot discard a local edit, resurrect a deletion or drop
            // an open conflict. Pull brings it up to date.
            let tracked = |rel: &str| prior_state.has_record(Path::new(rel));
            // A re-run without --type keeps the recorded type: the clap default would
            // otherwise switch the project to rust and the next pull rewrite mise.toml.
            let project_type = init_args
                .project_type
                .clone()
                .or_else(|| prior_state.project_type.clone())
                .unwrap_or_else(|| "rust".to_string());
            let managed_rels = [
                std::path::PathBuf::from(".gitconfig"),
                std::path::PathBuf::from(".gitignore"),
                std::path::PathBuf::from("scripts/hooks/commit-msg"),
                std::path::PathBuf::from("scripts/hooks/pre-commit"),
                std::path::PathBuf::from("scripts/hooks/pre-push"),
                std::path::PathBuf::from("scripts/check-in-container.sh"),
                std::path::PathBuf::from("scratch/check-publish-boundary.sh"),
                std::path::PathBuf::from("scratch/new-worktree.sh"),
                std::path::PathBuf::from("scratch/sync-personal-env.sh"),
                std::path::PathBuf::from("mise.toml"),
                std::path::PathBuf::from(".agents/AGENTS.md"),
            ];
            // A managed file with no record that git already tracks is kept as committed
            // (a fresh clone has no state.json, which is per machine): its recorded
            // ancestor is the template, so a committed edit reads as a local change
            // instead of being overwritten.
            let committed = match committed_paths(&project_path, &managed_rels) {
                Ok(paths) => paths,
                Err(e) => {
                    eprintln!("  {} Initialization failed: {e}", "✗".red());
                    std::process::exit(EXIT_HARD_FAILURE);
                }
            };
            let kept_committed: Vec<&PathBuf> = managed_rels
                .iter()
                .filter(|rel| {
                    !prior_state.has_record(rel)
                        && committed.contains(*rel)
                        && fs::symlink_metadata(project_path.join(rel)).is_ok()
                })
                .collect();
            let keep = |rel: &str| {
                kept_committed
                    .iter()
                    .any(|kept| kept.as_path() == Path::new(rel))
            };

            // Init writes only inside the project: a symlinked destination, or a
            // symlinked directory on the way to one, would be written through. Every
            // path init may create or write is checked before anything is touched.
            // This is a one-time check, not race-safe: a symlink created while init
            // runs is still followed (a Phase 2 todo).
            let mut write_targets: Vec<PathBuf> = ["scripts/hooks", "scratch", ".agents"]
                .iter()
                .map(PathBuf::from)
                .collect();
            write_targets.extend(
                managed_rels
                    .iter()
                    .filter(|rel| !prior_state.has_record(rel) && !kept_committed.contains(rel))
                    .cloned(),
            );
            if init_args.mempalace {
                write_targets.push(PathBuf::from(".planning/.mempalace-stage/mempalace.yaml"));
            }
            if init_args.graphify {
                write_targets.push(PathBuf::from("graphify-out/.graphify_root"));
            }
            for rel in &write_targets {
                if let Err(e) = sync::refuse_symlink_under(&project_path, &project_path.join(rel)) {
                    eprintln!("  {} Initialization failed: {e}", "✗".red());
                    std::process::exit(EXIT_HARD_FAILURE);
                }
            }

            let project_name = project_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "project".to_string());

            println!(
                "{}",
                format!(
                    "==> Initializing portable project baseline at '{}' (type: {})...",
                    project_path.display(),
                    project_type
                )
                .bold()
            );

            let ctx = template::TemplateContext::detect_for_project(
                &project_path,
                &project_name,
                &project_type,
            );
            let renderer = template::TemplateRenderer::new();

            // Locate canonical templates directory. Without one init would write
            // nothing yet record the user's files as managed, and the next pull would
            // delete them.
            let root_template_dir = find_templates_dir();
            if root_template_dir.is_none() {
                eprintln!(
                    "  {} Initialization failed: no ude templates directory found",
                    "✗".red()
                );
                std::process::exit(EXIT_HARD_FAILURE);
            }

            // 1. Create target project folders
            let hooks_dir = project_path.join("scripts/hooks");
            let scratch_dir = project_path.join("scratch");
            let agents_dir = project_path.join(".agents");
            let container_dir = project_path.join("scripts");
            if let Err(e) = fs::create_dir_all(&hooks_dir) {
                eprintln!("  {} Initialization failed: {e}", "✗".red());
                std::process::exit(EXIT_HARD_FAILURE);
            }
            if let Err(e) = fs::create_dir_all(&scratch_dir) {
                eprintln!("  {} Initialization failed: {e}", "✗".red());
                std::process::exit(EXIT_HARD_FAILURE);
            }
            if let Err(e) = fs::create_dir_all(&agents_dir) {
                eprintln!("  {} Initialization failed: {e}", "✗".red());
                std::process::exit(EXIT_HARD_FAILURE);
            }
            if let Err(e) = fs::create_dir_all(&container_dir) {
                eprintln!("  {} Initialization failed: {e}", "✗".red());
                std::process::exit(EXIT_HARD_FAILURE);
            }

            // A pre-existing .gitignore init kept instead of materializing; its template
            // becomes the recorded ancestor (CR-03).
            let mut kept_gitignore = false;

            // 2. Materialize .gitconfig include & .gitignore
            if let Some(ref tdir) = root_template_dir {
                if !tracked(".gitconfig")
                    && !keep(".gitconfig")
                    && let Err(e) = copy_template_file(
                        tdir,
                        &tdir.join("projects/generic/gitconfig.repo"),
                        &project_path.join(".gitconfig"),
                    )
                {
                    eprintln!("  {} Initialization failed: {e}", "✗".red());
                    std::process::exit(EXIT_HARD_FAILURE);
                }

                // Preserve existing customized .gitignore rules if already present
                let target_gitignore = project_path.join(".gitignore");
                let src_gitignore = tdir.join("projects/generic/gitignore.repo");
                if tracked(".gitignore") || keep(".gitignore") {
                    // Sync owns it, or git does; init's graphify append would read as a
                    // local edit.
                } else if target_gitignore.exists() {
                    kept_gitignore = true;
                    // Opaque bytes: a non-UTF-8 file is appended to, and an unreadable
                    // one stops init instead of being replaced by the block alone.
                    let existing = match fs::read(&target_gitignore) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            eprintln!("  {} Cannot read existing .gitignore: {e}", "✗".red());
                            std::process::exit(EXIT_HARD_FAILURE);
                        }
                    };
                    let marker = b"graphify-out/*";
                    if !existing.windows(marker.len()).any(|w| w == marker) {
                        let append_block = "\n# Generated knowledge graphs / reports (allowlist pattern)\ngraphify-out/*\n!graphify-out/graph.json\n!graphify-out/manifest.json\n";
                        let mut updated = existing;
                        if !updated.ends_with(b"\n") {
                            updated.push(b'\n');
                        }
                        updated.extend_from_slice(append_block.as_bytes());
                        if let Err(e) = fs::write(&target_gitignore, updated) {
                            eprintln!("  {} Failed to update existing .gitignore: {e}", "✗".red());
                            std::process::exit(EXIT_HARD_FAILURE);
                        }
                    }
                } else if src_gitignore.exists()
                    && let Err(e) = copy_template_file(tdir, &src_gitignore, &target_gitignore)
                {
                    eprintln!("  {} Initialization failed: {e}", "✗".red());
                    std::process::exit(EXIT_HARD_FAILURE);
                }

                // Materialize hook suite
                for hook in &["commit-msg", "pre-commit", "pre-push"] {
                    let rel = format!("scripts/hooks/{hook}");
                    if tracked(&rel) || keep(&rel) {
                        continue;
                    }
                    let src_hook = tdir.join(format!("projects/generic/hooks/{hook}"));
                    let dst_hook = hooks_dir.join(hook);
                    if let Err(e) = copy_template_file(tdir, &src_hook, &dst_hook) {
                        eprintln!("  {} Initialization failed: {e}", "✗".red());
                        std::process::exit(EXIT_HARD_FAILURE);
                    }
                    set_executable(&dst_hook);
                }

                // Materialize container parity check
                if !tracked("scripts/check-in-container.sh")
                    && !keep("scripts/check-in-container.sh")
                {
                    let src_cont = tdir.join("projects/generic/container/check-in-container.sh");
                    let dst_cont = container_dir.join("check-in-container.sh");
                    if let Err(e) = copy_template_file(tdir, &src_cont, &dst_cont) {
                        eprintln!("  {} Initialization failed: {e}", "✗".red());
                        std::process::exit(EXIT_HARD_FAILURE);
                    }
                    set_executable(&dst_cont);
                }

                // Materialize scratch scripts
                for s in &[
                    "check-publish-boundary.sh",
                    "new-worktree.sh",
                    "sync-personal-env.sh",
                ] {
                    let rel = format!("scratch/{s}");
                    if tracked(&rel) || keep(&rel) {
                        continue;
                    }
                    let src_s = tdir.join(format!("projects/agents/scratch/{s}"));
                    let dst_s = scratch_dir.join(s);
                    if let Err(e) = copy_template_file(tdir, &src_s, &dst_s) {
                        eprintln!("  {} Initialization failed: {e}", "✗".red());
                        std::process::exit(EXIT_HARD_FAILURE);
                    }
                    set_executable(&dst_s);
                }

                // Materialize rendered templates (mise.toml, agents.md, mempalace)
                if !tracked("mise.toml")
                    && !keep("mise.toml")
                    && let Err(e) = render_and_write(
                        &renderer,
                        tdir,
                        &tdir.join("projects/agents/mise.toml.jinja"),
                        &project_path.join("mise.toml"),
                        &ctx,
                    )
                {
                    eprintln!("  {} Initialization failed: {e}", "✗".red());
                    std::process::exit(EXIT_HARD_FAILURE);
                }

                if !tracked(".agents/AGENTS.md")
                    && !keep(".agents/AGENTS.md")
                    && let Err(e) = render_and_write(
                        &renderer,
                        tdir,
                        &tdir.join("projects/agents/agents.md.jinja"),
                        &agents_dir.join("AGENTS.md"),
                        &ctx,
                    )
                {
                    eprintln!("  {} Initialization failed: {e}", "✗".red());
                    std::process::exit(EXIT_HARD_FAILURE);
                }

                if init_args.mempalace {
                    let stage_dir = project_path.join(".planning/.mempalace-stage");
                    if let Err(e) = fs::create_dir_all(&stage_dir) {
                        eprintln!("  {} Initialization failed: {e}", "✗".red());
                        std::process::exit(EXIT_HARD_FAILURE);
                    }
                    if let Err(e) = render_and_write(
                        &renderer,
                        tdir,
                        &tdir.join("projects/agents/mempalace.yaml.jinja"),
                        &stage_dir.join("mempalace.yaml"),
                        &ctx,
                    ) {
                        eprintln!("  {} Initialization failed: {e}", "✗".red());
                        std::process::exit(EXIT_HARD_FAILURE);
                    }
                }

                if init_args.graphify {
                    let graphify_dir = project_path.join("graphify-out");
                    if let Err(e) = fs::create_dir_all(&graphify_dir) {
                        eprintln!("  {} Initialization failed: {e}", "✗".red());
                        std::process::exit(EXIT_HARD_FAILURE);
                    }
                    if let Err(e) = fs::write(graphify_dir.join(".graphify_root"), ".\n") {
                        eprintln!("  {} Initialization failed: {e}", "✗".red());
                        std::process::exit(EXIT_HARD_FAILURE);
                    }
                }
            }

            // 3. Register git include and hooks in local git repo if .git exists
            if project_path.join(".git").exists() {
                // Activate .gitconfig include
                let mut include_cmd = Command::new("git");
                include_cmd.current_dir(&project_path).args([
                    "config",
                    "--local",
                    "include.path",
                    "../.gitconfig",
                ]);
                let _ = process::run_bounded(
                    include_cmd,
                    "git config include.path",
                    process::LOCAL_TIMEOUT,
                );

                // Set core.hooksPath
                let mut hooks_cmd = Command::new("git");
                hooks_cmd.current_dir(&project_path).args([
                    "config",
                    "--local",
                    "core.hooksPath",
                    "scripts/hooks",
                ]);
                let _ = process::run_bounded(
                    hooks_cmd,
                    "git config core.hooksPath",
                    process::LOCAL_TIMEOUT,
                );

                // Ensure tracked hooks have mode 100755 in git index
                for hook in &["commit-msg", "pre-commit", "pre-push"] {
                    let mut chmod_cmd = Command::new("git");
                    chmod_cmd.current_dir(&project_path).args([
                        "update-index",
                        "--add",
                        "--chmod=+x",
                        &format!("scripts/hooks/{hook}"),
                    ]);
                    let _ = process::run_bounded(
                        chmod_cmd,
                        "git update-index --chmod=+x",
                        process::LOCAL_TIMEOUT,
                    );
                }
            }

            // 4. Record baseline ancestor checksums in .ude/state.json. Every prior
            // record (files, tombstones, open conflicts) is carried forward unchanged.
            let mut project_state = prior_state.clone();
            // The type that was rendered: "generic" resolves to a detected type, and
            // sync must render the same one or mise.toml reads as changed upstream.
            project_state.project_type = Some(ctx.project_type.clone());
            let mut unmanaged_kept: Vec<&PathBuf> = Vec::new();
            let mut unmanaged_local: Vec<&PathBuf> = Vec::new();
            for rel in &managed_rels {
                // A tracked file keeps its prior record: init did not write it, and a
                // replacement ancestor (the current template or the local bytes) would
                // turn a stale file into a local edit, or a local edit into In Sync.
                if prior_state.has_record(rel) {
                    continue;
                }
                // A kept file (a pre-existing .gitignore, or any file git tracks) records
                // what init would have written from the template as its ancestor, so
                // kept content reads as a local modification: pull then merges or
                // conflicts instead of replacing it (CR-03). With no readable template, a
                // fresh clone cannot tell a template deleted upstream from one that never
                // existed, so a committed file is left unmanaged (a kept tombstone): pull
                // never deletes it, push never resurrects or creates a template from it.
                // An unreadable one stops init rather than lose that marker. A file init
                // did not write because it has no template is the user's own and is left
                // unmanaged the same way. An uncommitted kept .gitignore records nothing.
                let committed_keep = keep(&rel.to_string_lossy());
                let kept =
                    committed_keep || (kept_gitignore && rel.as_path() == Path::new(".gitignore"));
                let template = || {
                    kept_ancestor(&renderer, root_template_dir.as_deref(), rel, &ctx)
                        .unwrap_or_else(|e| {
                            eprintln!("  {} Initialization failed: {e}", "✗".red());
                            std::process::exit(EXIT_HARD_FAILURE);
                        })
                };
                let recorded = if kept {
                    match template() {
                        None if committed_keep => {
                            let bytes = fs::read(project_path.join(rel)).unwrap_or_else(|e| {
                                eprintln!(
                                    "  {} Cannot read committed {}: {e}",
                                    "✗".red(),
                                    rel.display()
                                );
                                std::process::exit(EXIT_HARD_FAILURE);
                            });
                            let hash = sync::SyncEngine::fingerprint(&bytes);
                            project_state.record_tombstone(rel, hash, true);
                            unmanaged_kept.push(rel);
                            None
                        }
                        bytes => bytes,
                    }
                } else {
                    match fs::read(project_path.join(rel)) {
                        Ok(bytes) if template().is_none() => {
                            let hash = sync::SyncEngine::fingerprint(&bytes);
                            project_state.record_tombstone(rel, hash, true);
                            unmanaged_local.push(rel);
                            None
                        }
                        read => read.ok(),
                    }
                };
                if let Some(bytes) = recorded {
                    let hash = sync::SyncEngine::fingerprint(&bytes);
                    let content = String::from_utf8(bytes)
                        .ok()
                        .map(|text| sync::SyncEngine::normalize_eol(&text));
                    project_state.record_file(rel, hash, content);
                }
            }
            // Without state.json the unmanaged markers are lost and push would treat
            // a user's own file as publishable.
            if let Err(e) = project_state.save_to_dir(&project_path) {
                eprintln!(
                    "  {} Initialization failed: cannot persist .ude/state.json: {e}",
                    "✗".red()
                );
                std::process::exit(EXIT_HARD_FAILURE);
            } else {
                println!(
                    "  {} Recorded baseline ancestor state in .ude/state.json",
                    "✓".green()
                );
            }

            for rel in &kept_committed {
                if unmanaged_kept.contains(rel) {
                    println!(
                        "  {} Kept committed {} (not overwritten); no shared template, so sync leaves it unmanaged until one is published",
                        "✓".green(),
                        rel.display()
                    );
                } else {
                    println!(
                        "  {} Kept committed {} (not overwritten); sync status compares it with the template",
                        "✓".green(),
                        rel.display()
                    );
                }
            }
            for rel in &unmanaged_local {
                println!(
                    "  {} Kept existing {} (not overwritten); no shared template, so sync leaves it unmanaged until one is published",
                    "✓".green(),
                    rel.display()
                );
            }

            // A tracked file that is missing stays missing (D-15). Say so, and what
            // push will do with it, instead of reporting it as set up.
            let missing = missing_tracked_files(&project_path, &prior_state, &managed_rels);
            // What sync sees for each missing file decides what push will do with it.
            let scanned = match (&root_template_dir, missing.is_empty()) {
                (Some(tdir), false) => sync::SyncEngine::new()
                    .scan_project(&project_path, tdir, Some(&ctx))
                    .unwrap_or_else(|e| {
                        eprintln!(
                            "  {} Could not compare missing files with their templates: {e}",
                            "!".yellow()
                        );
                        Vec::new()
                    }),
                _ => Vec::new(),
            };
            for rel in &missing {
                let status = scanned
                    .iter()
                    .find(|state| &state.relative_path == rel)
                    .map(|state| &state.status);
                print_missing_tracked_warning(root_template_dir.as_deref(), rel, status);
            }
            // (line when every file is present, label when some are missing, which
            // managed paths the line covers)
            type InitStep = (&'static str, &'static str, fn(&str) -> bool);
            let steps: [InitStep; 3] = [
                (
                    "Created project directories and scaffolding",
                    "Project scaffolding",
                    |rel| {
                        matches!(
                            rel,
                            ".gitignore" | "scripts/check-in-container.sh" | ".agents/AGENTS.md"
                        )
                    },
                ),
                (
                    "Configured tracked .gitconfig & chained scripts/hooks",
                    ".gitconfig & scripts/hooks",
                    |rel| rel == ".gitconfig" || rel.starts_with("scripts/hooks/"),
                ),
                (
                    "Initialized workflow tasks (mise.toml, scratch/)",
                    "Workflow tasks (mise.toml, scratch/)",
                    |rel| rel == "mise.toml" || rel.starts_with("scratch/"),
                ),
            ];
            for (done, label, covers) in steps {
                let absent = missing
                    .iter()
                    .filter(|rel| covers(&rel.to_string_lossy()))
                    .count();
                if absent == 0 {
                    println!("  {} {done}", "✓".green());
                } else {
                    println!(
                        "  {} {label}: {absent} tracked file(s) missing, see warnings above",
                        "!".yellow()
                    );
                }
            }
            if missing.is_empty() {
                println!("{}", "==> Project initialization complete.".bold());
            } else {
                println!(
                    "{}",
                    format!(
                        "==> Project initialization complete; {} tracked file(s) missing.",
                        missing.len()
                    )
                    .bold()
                );
            }
        }
        cli::Commands::Sync(sync_args) => {
            let engine = sync::SyncEngine::new();
            let base_dirs = directories::BaseDirs::new();
            let user_home = base_dirs
                .as_ref()
                .map(|b| b.home_dir().to_string_lossy().to_string())
                .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
            let xdg_config = base_dirs
                .as_ref()
                .map(|b| b.config_dir().to_string_lossy().to_string())
                .unwrap_or_else(|| format!("{user_home}/.config"));
            let cache_dir = base_dirs
                .as_ref()
                .map(|b| b.cache_dir().join("ude"))
                .unwrap_or_else(|| PathBuf::from(format!("{user_home}/.cache/ude")));

            // Check if .ude.toml exists in current directory or user config
            let local_manifest = std::fs::read_to_string(".ude.toml")
                .ok()
                .and_then(|c| manifest::UdeManifest::parse_str(&c).ok());

            let remote_url = sync_args.remote.or_else(|| {
                local_manifest
                    .as_ref()
                    .and_then(|m| m.sync.central_repo.clone())
            });

            let branch = sync_args.branch.unwrap_or_else(|| {
                local_manifest
                    .as_ref()
                    .map(|m| m.sync.branch.clone())
                    .unwrap_or_else(|| "main".to_string())
            });

            let local_project = PathBuf::from(".");

            match sync_args.action {
                // Resolve is offline: it reads only state.json and the local file, so it
                // runs before (and without) any remote cache access or lock.
                cli::SyncAction::Resolve { file } => {
                    run_sync_resolve(&engine, &local_project, &file);
                }
                cli::SyncAction::Status => {
                    let (root_template_dir, _remote_checkout_root, remote_cache) =
                        open_sync_templates(&engine, remote_url.as_deref(), &branch, &cache_dir);
                    // A remote's history must show a template's deletion before pull acts.
                    let engine = match &remote_cache {
                        Some(cache) => {
                            engine.with_deletion_history(cache.checkout_dir.clone(), &branch)
                        }
                        None => engine,
                    };
                    println!("{}", "==> Checking UDE environment sync status...".bold());
                    if let Some(ref r_url) = remote_url {
                        println!(
                            "  {} Remote Upstream: {} ({})",
                            "ℹ".blue(),
                            process::redact_userinfo(r_url),
                            branch
                        );
                    }
                    let sync_ctx = template::TemplateContext::detect_for_project(
                        &local_project,
                        "project",
                        "generic",
                    );
                    let states = match engine.scan_project(
                        &local_project,
                        &root_template_dir,
                        Some(&sync_ctx),
                    ) {
                        Ok(states) => states,
                        Err(e) => {
                            eprintln!(
                                "  {} Status failed: could not compare the project with upstream: {e}",
                                "✗".red()
                            );
                            std::process::exit(EXIT_HARD_FAILURE);
                        }
                    };
                    for s in states {
                        match s.status {
                            sync::SyncStatus::InSync => {
                                println!(
                                    "  {} {}: In Sync",
                                    "✓".green(),
                                    s.relative_path.display()
                                );
                            }
                            sync::SyncStatus::LocallyModified => {
                                let hint =
                                    locally_modified_hint(&root_template_dir, &s.relative_path);
                                println!(
                                    "  {} {}: Locally Modified ({hint})",
                                    "!".yellow(),
                                    s.relative_path.display()
                                );
                            }
                            sync::SyncStatus::UpstreamModified => {
                                println!(
                                    "  {} {}: Upstream Updated (ready to pull)",
                                    "↓".cyan(),
                                    s.relative_path.display()
                                );
                            }
                            sync::SyncStatus::Merged => {
                                println!(
                                    "  {} {}: Merged Cleanly (3-way text merge ready to pull)",
                                    "⚡".magenta(),
                                    s.relative_path.display()
                                );
                            }
                            sync::SyncStatus::Conflicted => {
                                println!(
                                    "  {} {}: Conflicted (requires manual resolution)",
                                    "✗".red(),
                                    s.relative_path.display()
                                );
                            }
                            sync::SyncStatus::Overridden => {
                                println!(
                                    "  {} {}: Layered Override Active (*.local)",
                                    "ℹ".blue(),
                                    s.relative_path.display()
                                );
                            }
                            sync::SyncStatus::LocalDeleted => {
                                let logic_template =
                                    logic_template(&root_template_dir, &s.relative_path);
                                let hint = match (push_boundary(&s.relative_path), logic_template) {
                                    (PushBoundary::Never, _) => {
                                        "local-only path, push never publishes it; restore it to clear".to_string()
                                    }
                                    (_, Some(template)) => format!(
                                        "push skips it: its template has logic; {}",
                                        deletion_logic_remedy(&root_template_dir, &template)
                                    ),
                                    (PushBoundary::Publishable, None) => {
                                        "push to propagate; push deletes the shared template, restore the file to keep it".to_string()
                                    }
                                    (PushBoundary::ForceOnly, None) => {
                                        "push with --force to propagate; that deletes the shared template, restore the file to keep it".to_string()
                                    }
                                };
                                println!(
                                    "  {} {}: Deleted locally ({hint})",
                                    "!".yellow(),
                                    s.relative_path.display()
                                );
                            }
                            sync::SyncStatus::UpstreamDeleted => {
                                println!(
                                    "  {} {}: Deleted upstream (pull to remove)",
                                    "↓".cyan(),
                                    s.relative_path.display()
                                );
                            }
                            sync::SyncStatus::MissingUpstream => {
                                println!(
                                    "  {} {}: Missing upstream (this remote never had its template; pull keeps it, push never publishes it)",
                                    "ℹ".blue(),
                                    s.relative_path.display()
                                );
                            }
                            sync::SyncStatus::BothDeleted => {
                                println!(
                                    "  {} {}: Deleted on both sides (pull to drop tracking)",
                                    "↓".cyan(),
                                    s.relative_path.display()
                                );
                            }
                            sync::SyncStatus::Unmanaged => {
                                println!(
                                    "  {} {}: Unmanaged (no shared template; kept locally, never published)",
                                    "ℹ".blue(),
                                    s.relative_path.display()
                                );
                            }
                            sync::SyncStatus::Absent => {
                                println!(
                                    "  {} {}: Not present locally or upstream (not tracked)",
                                    "ℹ".blue(),
                                    s.relative_path.display()
                                );
                            }
                            sync::SyncStatus::ConflictPending => {
                                println!(
                                    "  {} {}: Unresolved conflict (see {}.conflict; run ude sync resolve {})",
                                    "✗".red(),
                                    s.relative_path.display(),
                                    s.relative_path.display(),
                                    s.relative_path.display()
                                );
                            }
                        }
                    }
                }
                cli::SyncAction::Pull => {
                    let (root_template_dir, _remote_checkout_root, remote_cache) =
                        open_sync_templates(&engine, remote_url.as_deref(), &branch, &cache_dir);
                    // A remote's history must show a template's deletion before pull acts.
                    let engine = match &remote_cache {
                        Some(cache) => {
                            engine.with_deletion_history(cache.checkout_dir.clone(), &branch)
                        }
                        None => engine,
                    };
                    println!(
                        "{}",
                        "==> Pulling updates from central configuration store...".bold()
                    );
                    if let Some(ref r_url) = remote_url {
                        println!(
                            "  {} Remote Upstream: {} ({})",
                            "ℹ".blue(),
                            process::redact_userinfo(r_url),
                            branch
                        );
                    }
                    let sync_ctx = template::TemplateContext::detect_for_project(
                        &local_project,
                        "project",
                        "generic",
                    );
                    match engine.pull_updates(&local_project, &root_template_dir, &sync_ctx) {
                        sync::PullResult::Success(report) => {
                            if report.applied.is_empty()
                                && report.deleted.is_empty()
                                && report.conflicts.is_empty()
                                && report.converged.is_empty()
                            {
                                println!("  {} No pending upstream updates to apply.", "✓".green());
                            } else if !report.applied.is_empty() {
                                println!(
                                    "  {} Successfully pulled and materialized {} upstream update(s).",
                                    "✓".green(),
                                    report.applied.len()
                                );
                            }
                            for file in &report.deleted {
                                println!(
                                    "  {} Removed {} (deleted upstream)",
                                    "✓".green(),
                                    file.display()
                                );
                            }
                            for file in &report.missing_upstream {
                                println!(
                                    "  {} Kept {}: its template is missing from this remote, whose history never deleted it",
                                    "ℹ".blue(),
                                    file.display()
                                );
                            }
                            for file in &report.locally_deleted_pending {
                                let logic = logic_template(&root_template_dir, file);
                                let advice = match push_boundary(file) {
                                    PushBoundary::Publishable | PushBoundary::ForceOnly
                                        if logic.is_some() =>
                                    {
                                        format!(
                                            "push skips it: its template has logic; {}",
                                            deletion_logic_remedy(
                                                &root_template_dir,
                                                logic.as_deref().unwrap_or(file)
                                            )
                                        )
                                    }
                                    PushBoundary::Publishable => "push to propagate".to_string(),
                                    PushBoundary::ForceOnly => {
                                        "push with --force to propagate".to_string()
                                    }
                                    PushBoundary::Never => format!(
                                        "a local-only path, push never publishes it; {}",
                                        boundary_restore_hint(&root_template_dir, file)
                                    ),
                                };
                                println!(
                                    "  {} {}: deleted locally — {advice}",
                                    "!".yellow(),
                                    file.display()
                                );
                            }
                            for file in &report.converged {
                                println!(
                                    "  {} {}: conflict resolved by convergence",
                                    "✓".green(),
                                    file.display()
                                );
                            }
                            for file in &report.conflicts {
                                eprintln!(
                                    "  {} {}: conflicting local and upstream changes; wrote {}.conflict",
                                    "✗".red(),
                                    file.display(),
                                    file.display()
                                );
                                eprintln!("    resolve with: ude sync resolve {}", file.display());
                            }
                            if !report.conflicts.is_empty() {
                                std::process::exit(EXIT_DRIFT);
                            }
                        }
                        sync::PullResult::ScanFailed { reason } => {
                            eprintln!(
                                "  {} Pull failed: could not compare the project with upstream: {reason}",
                                "✗".red()
                            );
                            std::process::exit(EXIT_HARD_FAILURE);
                        }
                        sync::PullResult::IoFailed { path, reason } => {
                            eprintln!(
                                "  {} Pull failed due to I/O error on {}: {}",
                                "✗".red(),
                                path.display(),
                                reason
                            );
                            std::process::exit(EXIT_HARD_FAILURE);
                        }
                    }
                }
                cli::SyncAction::Push { force, pr } => {
                    let (root_template_dir, remote_checkout_root, remote_cache) =
                        open_sync_templates(&engine, remote_url.as_deref(), &branch, &cache_dir);
                    // The same deletion check as status and pull, so push agrees with them.
                    let engine = match &remote_cache {
                        Some(cache) => {
                            engine.with_deletion_history(cache.checkout_dir.clone(), &branch)
                        }
                        None => engine,
                    };
                    println!("{}", "==> Running Sanitization Gate & Sync Push...".bold());

                    // If TypeSafe AI API is configured, evaluate semantic drift on agent rules
                    let mut allow_agent_rules = force;
                    let typesafe = semantic::TypeSafeClient::new();
                    if typesafe.is_available() {
                        println!(
                            "  {} TypeSafe System One (Jev) enabled for semantic drift auditing",
                            "✓".green()
                        );
                        let candidate_rules = local_project.join(".agents/AGENTS.md");
                        let upstream_rules =
                            root_template_dir.join("projects/agents/agents.md.jinja");
                        // Never send a symlink's target: push refuses the link below, but
                        // only after this check would have transmitted it.
                        let linked = sync::refuse_symlink_under(&local_project, &candidate_rules)
                            .and_then(|()| {
                                sync::refuse_symlink_under(&root_template_dir, &upstream_rules)
                            });
                        if let Err(e) = &linked {
                            println!(
                                "  {} TypeSafe semantic evaluation skipped: {e}",
                                "!".yellow()
                            );
                        }
                        if linked.is_ok()
                            && candidate_rules.exists()
                            && upstream_rules.exists()
                            && let (Ok(local_txt), Ok(up_txt)) = (
                                fs::read_to_string(&candidate_rules),
                                fs::read_to_string(&upstream_rules),
                            )
                            && local_txt != up_txt
                        {
                            // Pre-scan secrets locally BEFORE sending candidate text to any external API
                            let pre_sanitizer = sanitizer::Sanitizer::new();
                            if let sanitizer::SanitizationResult::SecretDetected {
                                reason, ..
                            } = pre_sanitizer.check_secrets(&local_txt)
                            {
                                eprintln!(
                                    "  {} Security Gate: Secret detected in {} before API transmission: {}",
                                    "✗".red(),
                                    candidate_rules.display(),
                                    reason
                                );
                                std::process::exit(1);
                            }

                            match typesafe.evaluate_drift("AGENTS.md", &up_txt, &local_txt) {
                                Ok(semantic::SemanticDriftVerdict::PolicyShift) => {
                                    if !force {
                                        eprintln!(
                                            "  {} TypeSafe Verdict: [policy_shift] Staged rule modification alters or weakens core governance policies.",
                                            "✗".red()
                                        );
                                        eprintln!(
                                            "     Use --force to override if this policy change is intentional."
                                        );
                                        std::process::exit(1);
                                    } else {
                                        println!(
                                            "  {} TypeSafe Warning: [policy_shift] Overriding policy change due to --force flag.",
                                            "!".yellow()
                                        );
                                        allow_agent_rules = true;
                                    }
                                }
                                Ok(semantic::SemanticDriftVerdict::ProjectException) => {
                                    println!(
                                        "  {} TypeSafe Notice: [project_exception] Local rule additions detected. Ensure repository-specific quirks belong in *.local rather than universal baseline.",
                                        "ℹ".blue()
                                    );
                                }
                                Ok(semantic::SemanticDriftVerdict::BenignRephrase) => {
                                    println!(
                                        "  {} TypeSafe Verdict: [benign_rephrase] Clear phrasing / stylistic refinement.",
                                        "✓".green()
                                    );
                                    allow_agent_rules = true;
                                }
                                Ok(semantic::SemanticDriftVerdict::Uncertain(u)) => {
                                    println!(
                                        "  {} TypeSafe Notice: Semantic verdict: {}",
                                        "ℹ".blue(),
                                        u
                                    );
                                }
                                Err(e) => {
                                    println!(
                                        "  {} TypeSafe semantic evaluation bypassed: {}",
                                        "!".yellow(),
                                        e
                                    );
                                }
                            }
                        }
                    }

                    let persist_immediately = remote_checkout_root.is_none();
                    match engine.push_updates(
                        &local_project,
                        &root_template_dir,
                        &user_home,
                        &xdg_config,
                        allow_agent_rules,
                        persist_immediately,
                    ) {
                        sync::PushResult::Success {
                            synced_files,
                            staged_files,
                            updated_records,
                            deleted,
                            skipped_conflicts,
                            skipped_merged,
                            skipped_boundary,
                            skipped_templates,
                            skipped_missing_upstream,
                        } => {
                            println!(
                                "  {} Sanitization gate passed: clean of secrets & parameters applied.",
                                "✓".green()
                            );
                            println!(
                                "  {} Successfully synchronized {} file(s) into upstream template store.",
                                "✓".green(),
                                synced_files
                            );
                            for file in &deleted {
                                println!(
                                    "  {} Deleted upstream template for {}",
                                    "✓".green(),
                                    file.display()
                                );
                            }
                            for file in &skipped_conflicts {
                                eprintln!(
                                    "  {} {}: unresolved conflict, skipped (run ude sync resolve {})",
                                    "✗".red(),
                                    file.display(),
                                    file.display()
                                );
                            }
                            for file in &skipped_merged {
                                eprintln!(
                                    "  {} {}: merged with upstream changes, skipped (run ude sync pull first)",
                                    "✗".red(),
                                    file.display()
                                );
                            }
                            for file in &skipped_boundary {
                                let change = if local_project.join(file).symlink_metadata().is_ok()
                                {
                                    "edited locally"
                                } else {
                                    "deleted locally"
                                };
                                let reason = match push_boundary(file) {
                                    PushBoundary::ForceOnly => {
                                        "agent rules are published only with --force; skipped (re-run with --force)".to_string()
                                    }
                                    _ => format!(
                                        "but it is a local-only path that push never publishes; skipped ({})",
                                        boundary_restore_hint(&root_template_dir, file)
                                    ),
                                };
                                eprintln!("  {} {}: {change}, {reason}", "✗".red(), file.display());
                            }
                            for file in &skipped_missing_upstream {
                                println!(
                                    "  {} {}: its template is missing from this remote, so push never publishes it",
                                    "ℹ".blue(),
                                    file.display()
                                );
                            }
                            for file in &skipped_templates {
                                let template = sync::map_to_upstream(&root_template_dir, file)
                                    .unwrap_or_else(|| file.clone());
                                if fs::symlink_metadata(local_project.join(file)).is_err() {
                                    eprintln!(
                                        "  {} {}: upstream template {} contains template logic; deleting it would remove it for every project; skipped ({})",
                                        "✗".red(),
                                        file.display(),
                                        shared_template_path(&root_template_dir, &template)
                                            .display(),
                                        deletion_logic_remedy(&root_template_dir, &template)
                                    );
                                    continue;
                                }
                                eprintln!(
                                    "  {} {}: upstream template {} contains template logic that a push of the rendered file would erase; skipped ({})",
                                    "✗".red(),
                                    file.display(),
                                    shared_template_path(&root_template_dir, &template).display(),
                                    template_logic_remedy(file, &root_template_dir, &template)
                                );
                            }

                            if let Some(ref remote_checkout) = remote_checkout_root {
                                println!(
                                    "  {} Pushing synchronized templates to remote repository...",
                                    "==>".cyan()
                                );
                                let fetched_sha = remote_cache
                                    .as_ref()
                                    .map_or("", |cache| cache.fetched_sha.as_str());
                                match engine.push_to_remote(
                                    remote_checkout,
                                    &branch,
                                    pr,
                                    &staged_files,
                                    fetched_sha,
                                ) {
                                    Ok(msg) => {
                                        println!("  {} {msg}", "✓".green());
                                        if !pr {
                                            let mut project_state =
                                                match sync::ProjectState::load_from_dir(
                                                    &local_project,
                                                ) {
                                                    Ok(state) => state,
                                                    Err(e) => {
                                                        eprintln!(
                                                            "  {} Failed to read sync state: {e}",
                                                            "✗".red()
                                                        );
                                                        std::process::exit(EXIT_HARD_FAILURE);
                                                    }
                                                };
                                            for (rel, hash, content) in updated_records {
                                                project_state.record_file(&rel, hash, content);
                                            }
                                            for rel in &deleted {
                                                project_state.remove_file(rel);
                                            }
                                            if let Err(e) =
                                                project_state.save_to_dir(&local_project)
                                            {
                                                eprintln!(
                                                    "  {} Warning: Failed to save updated ancestor state: {e}",
                                                    "!".yellow()
                                                );
                                            }
                                        } else {
                                            println!(
                                                "  {} Proposed changes pushed to branch. Canonical ancestor state preserves tracking '{}' until PR is merged.",
                                                "ℹ".cyan(),
                                                branch
                                            );
                                        }
                                    }
                                    Err(sync::PushRemoteError::Failed(err)) => {
                                        eprintln!(
                                            "  {} Failed to push to remote: {err}",
                                            "✗".red()
                                        );
                                        std::process::exit(EXIT_HARD_FAILURE);
                                    }
                                    // D-18: nothing was published and the cache commit
                                    // was undone; ancestors are not advanced.
                                    Err(sync::PushRemoteError::UpstreamMoved { branch }) => {
                                        eprintln!(
                                            "  {} Upstream branch {branch} moved since UDE fetched it; nothing was published.",
                                            "✗".red()
                                        );
                                        eprintln!("  upstream changed, run ude sync pull");
                                        std::process::exit(EXIT_DRIFT);
                                    }
                                }
                            }
                            // Published what was eligible; skipped files still need the
                            // developer (D-10, D-17).
                            if !skipped_conflicts.is_empty()
                                || !skipped_merged.is_empty()
                                || !skipped_boundary.is_empty()
                                || !skipped_templates.is_empty()
                            {
                                std::process::exit(EXIT_DRIFT);
                            }
                        }
                        sync::PushResult::SecretBlocked { path, reason } => {
                            eprintln!(
                                "  {} Push rejected on {}: {}",
                                "✗".red(),
                                path.display(),
                                reason
                            );
                            std::process::exit(1);
                        }
                        sync::PushResult::ScanFailed { reason } => {
                            eprintln!(
                                "  {} Push failed: could not compare the project with upstream: {reason}",
                                "✗".red()
                            );
                            std::process::exit(EXIT_HARD_FAILURE);
                        }
                        sync::PushResult::IoFailed { path, reason } => {
                            eprintln!(
                                "  {} Push failed due to I/O error on {}: {}",
                                "✗".red(),
                                path.display(),
                                reason
                            );
                            std::process::exit(EXIT_HARD_FAILURE);
                        }
                        sync::PushResult::GitRemoteFailed { reason } => {
                            eprintln!("  {} Remote sync failed: {}", "✗".red(), reason);
                            std::process::exit(EXIT_HARD_FAILURE);
                        }
                    }
                }
            }
        }
    }
}

/// How the private publish boundary treats a managed path in `ude sync push`.
enum PushBoundary {
    /// Push publishes it.
    Publishable,
    /// Push publishes it only with `--force` (agent rules).
    ForceOnly,
    /// Push never publishes it (`scratch/` and other local-only paths).
    Never,
}

fn push_boundary(path: &Path) -> PushBoundary {
    let sanitizer = sanitizer::Sanitizer::new();
    if !sanitizer.is_local_only_path(path, false) {
        PushBoundary::Publishable
    } else if !sanitizer.is_local_only_path(path, true) {
        PushBoundary::ForceOnly
    } else {
        PushBoundary::Never
    }
}

/// Status hint for a locally modified `file`, in the order `ude sync push` checks it:
/// a local-only path first, then the template-logic gate (CR-02), which `--force`
/// cannot clear, then the `--force` requirement for agent rules.
fn locally_modified_hint(template_root: &Path, file: &Path) -> String {
    let boundary = push_boundary(file);
    if matches!(boundary, PushBoundary::Never) {
        return "local-only path, never pushed".to_string();
    }
    if let Some(template) = sync::map_to_upstream(template_root, file) {
        match sync::template_has_logic(&template) {
            Ok(true) => {
                return format!(
                    "push skips it: its template has logic; {}",
                    template_logic_remedy(file, template_root, &template)
                );
            }
            Err(reason) => return format!("push cannot check its template: {reason}"),
            Ok(false) => {}
        }
    }
    match boundary {
        PushBoundary::ForceOnly => "push with --force to publish".to_string(),
        _ => "ready to push".to_string(),
    }
}

/// Where a local edit of `file` can go when push skips it because its upstream
/// `template` has template logic.
/// The upstream template of `file` when it has template logic, which push never deletes.
fn logic_template(template_root: &Path, file: &Path) -> Option<PathBuf> {
    sync::map_to_upstream(template_root, file)
        .filter(|template| sync::template_has_logic(template).unwrap_or(false))
}

/// What to do about a local deletion that push skips because the template has logic.
fn deletion_logic_remedy(template_root: &Path, template: &Path) -> String {
    format!(
        "restore the file to keep it; to remove it for every project, delete {} from the shared templates",
        shared_template_path(template_root, template).display()
    )
}

/// `template` as a path in the shared templates (`projects/...`), not the local or cache
/// copy the engine reads it from.
fn shared_template_path<'a>(template_root: &Path, template: &'a Path) -> &'a Path {
    template.strip_prefix(template_root).unwrap_or(template)
}

fn template_logic_remedy(file: &Path, template_root: &Path, template: &Path) -> String {
    let local = sync::local_override_path(file);
    let project_only = if file == Path::new("mise.toml") {
        format!(
            "move the change into {}, which mise loads over mise.toml",
            local.display()
        )
    } else {
        format!(
            "keep it as a layered override by creating {}",
            local.display()
        )
    };
    format!(
        "for this project only, {project_only}; for every project, edit {} in the shared templates",
        shared_template_path(template_root, template).display()
    )
}

/// Managed files in `rels` that `state` tracks with an ancestor record and no open
/// conflict but that are absent locally (read like `scan_project` does): sync reads
/// each as deleted locally, and init leaves it deleted (D-15).
fn missing_tracked_files(
    project: &Path,
    state: &sync::ProjectState,
    rels: &[PathBuf],
) -> Vec<PathBuf> {
    rels.iter()
        .filter(|rel| state.get_ancestor_hash(rel).is_some() && state.conflict(rel).is_none())
        .filter(|rel| {
            matches!(
                fs::metadata(project.join(rel)),
                Err(e) if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                )
            )
        })
        .cloned()
        .collect()
}

/// Warns that init left the tracked file `rel` missing, what `ude sync push` will do
/// with its shared template, and how to restore the file.
fn print_missing_tracked_warning(
    template_root: Option<&Path>,
    rel: &Path,
    status: Option<&sync::SyncStatus>,
) {
    let template = template_root.and_then(|root| sync::map_to_upstream(root, rel));
    if status == Some(&sync::SyncStatus::BothDeleted) {
        println!(
            "  {} {} is tracked but missing; init does not recreate it, and its shared template is deleted too, so ude sync push deletes nothing and ude sync pull stops tracking it",
            "!".yellow(),
            rel.display()
        );
        println!(
            "    To keep it in this project only, run ude sync pull first, then restore it (git restore {} if it is committed)",
            rel.display()
        );
        return;
    }
    let has_logic = template_root
        .and_then(|root| logic_template(root, rel))
        .is_some();
    let consequence = match push_boundary(rel) {
        _ if status == Some(&sync::SyncStatus::Conflicted) => {
            "its shared template changed since it was recorded, so ude sync push skips it as a conflict and ude sync pull records the conflict"
        }
        PushBoundary::Publishable | PushBoundary::ForceOnly if has_logic => {
            "ude sync push skips the deletion because its shared template has logic"
        }
        PushBoundary::Publishable => "a plain ude sync push will delete its shared template",
        PushBoundary::ForceOnly => {
            "a plain ude sync push refuses the deletion, but ude sync push --force will delete its shared template"
        }
        PushBoundary::Never => "ude sync push never publishes this path",
    };
    println!(
        "  {} {} is tracked but missing; init does not recreate it, and {consequence}",
        "!".yellow(),
        rel.display()
    );
    let recovery = match (push_boundary(rel), template) {
        (PushBoundary::Never, Some(template)) => {
            format!("Restore it from {} to clear this", template.display())
        }
        (PushBoundary::Never, None) => "Restore it locally to clear this".to_string(),
        (_, Some(template)) if template.extension().is_some_and(|ext| ext == "jinja") => {
            format!(
                "To keep it, restore it before pushing (git restore {} if it is committed; init does not re-render its template {} while it is tracked), then check ude sync status",
                rel.display(),
                template.display()
            )
        }
        (_, Some(template)) => format!(
            "To keep it, restore it before pushing (git restore {} if it is committed, or copy {}), then check ude sync status",
            rel.display(),
            template.display()
        ),
        (_, None) => format!(
            "To keep it, restore it before pushing (git restore {} if it is committed), then check ude sync status",
            rel.display()
        ),
    };
    println!("    {recovery}");
}

/// How to clear a local change push will never publish: restore the file to the
/// upstream template, which neither pull (D-15) nor push does for a local deletion.
fn boundary_restore_hint(template_root: &Path, file: &Path) -> String {
    match sync::map_to_upstream(template_root, file) {
        Some(template) => format!(
            "restore it from the upstream template {} to clear this",
            template.display()
        ),
        None => "restore it locally to clear this".to_string(),
    }
}

/// Template root for a sync action: the cached checkout of `remote_url` (with its
/// checkout root and the [`sync::RemoteCache`], whose lock the caller holds for the
/// whole action and whose fetched head a direct push must build on), or the local
/// templates directory. Exits with [`EXIT_HARD_FAILURE`] when the remote cannot be
/// synchronized.
fn open_sync_templates(
    engine: &sync::SyncEngine,
    remote_url: Option<&str>,
    branch: &str,
    cache_dir: &Path,
) -> (PathBuf, Option<PathBuf>, Option<sync::RemoteCache>) {
    let Some(r_url) = remote_url else {
        // Without templates every managed file would read as deleted upstream.
        let Some(templates) = find_templates_dir() else {
            eprintln!(
                "  {} No ude templates directory found; pass --remote or set central_repo",
                "✗".red()
            );
            std::process::exit(EXIT_HARD_FAILURE);
        };
        return (templates, None, None);
    };
    match engine.ensure_remote_repo(r_url, branch, cache_dir) {
        Ok(cache) => {
            let tmpl_dir = cache.templates_dir.clone();
            let checkout_dir = cache.checkout_dir.clone();
            (tmpl_dir, Some(checkout_dir), Some(cache))
        }
        Err(e) => {
            eprintln!(
                "  {} Failed to sync with remote {}: {}",
                "✗".red(),
                process::redact_userinfo(r_url),
                e
            );
            std::process::exit(EXIT_HARD_FAILURE);
        }
    }
}

/// `ude sync resolve <file>`: prints what the resolution means, or the error to stderr
/// with exit [`EXIT_HARD_FAILURE`].
fn run_sync_resolve(engine: &sync::SyncEngine, local_project: &Path, file: &Path) {
    match engine.resolve_conflict(local_project, file) {
        Ok(outcome) => {
            let meaning = match outcome {
                sync::ResolveOutcome::KeptLocal => {
                    "local content will be published on the next push"
                }
                sync::ResolveOutcome::AcceptedLocalDeletion => {
                    "the next push deletes the upstream template if it still exists"
                }
                sync::ResolveOutcome::KeptUnmanaged => {
                    "kept as an unmanaged file; it will not be republished"
                }
                sync::ResolveOutcome::DroppedDeleted => "deletion accepted; tracking removed",
            };
            println!("  {} Resolved {}: {meaning}", "✓".green(), file.display());
        }
        Err(e) => {
            eprintln!("  {} {e}", "✗".red());
            std::process::exit(EXIT_HARD_FAILURE);
        }
    }
}

/// A ude template tree, as opposed to a project's own `templates/` (Django, Flask, Helm).
fn is_ude_templates(dir: &Path) -> bool {
    dir.join("projects/generic").is_dir()
}

fn find_templates_dir() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("templates")];
    if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
        candidates.push(Path::new(manifest_dir).join("templates"));
    }
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(parent) = exe_path.parent()
    {
        for rel in &["../../templates", "../templates", "../share/ude/templates"] {
            candidates.push(parent.join(rel));
        }
    }
    if let Some(base_dirs) = directories::BaseDirs::new() {
        candidates.push(base_dirs.data_dir().join("ude/templates"));
    }
    first_ude_templates(candidates)
}

/// The first candidate that is a ude template tree. `None` when there is none, which
/// init and sync refuse rather than read every managed file as deleted upstream.
fn first_ude_templates(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates
        .into_iter()
        .find(|candidate| is_ude_templates(candidate))
}

fn copy_template_file(root: &Path, src: &Path, dst: &Path) -> Result<(), String> {
    sync::refuse_symlink_under(root, src)?;
    if src.exists() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "Failed to create parent directory {}: {e}",
                    parent.display()
                )
            })?;
        }
        fs::copy(src, dst)
            .map_err(|e| format!("Failed to copy {} to {}: {e}", src.display(), dst.display()))?;
    }
    Ok(())
}

fn render_and_write(
    renderer: &template::TemplateRenderer,
    root: &Path,
    src: &Path,
    dst: &Path,
    ctx: &template::TemplateContext,
) -> Result<(), String> {
    if let Some(rendered) = rendered_template(renderer, root, src, ctx)? {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "Failed to create parent directory {}: {e}",
                    parent.display()
                )
            })?;
        }
        fs::write(dst, rendered).map_err(|e| format!("Failed to write {}: {e}", dst.display()))?;
    }
    Ok(())
}

/// The template `src` (below `root`) rendered with `ctx`, `None` when it does not exist.
fn rendered_template(
    renderer: &template::TemplateRenderer,
    root: &Path,
    src: &Path,
    ctx: &template::TemplateContext,
) -> Result<Option<String>, String> {
    sync::refuse_symlink_under(root, src)?;
    if !src.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(src)
        .map_err(|e| format!("Failed to read template {}: {e}", src.display()))?;
    renderer
        .render_str(&content, ctx)
        .map(Some)
        .map_err(|e| format!("Failed to render template {}: {e}", src.display()))
}

/// What init would have written for the managed file `rel` from its template under
/// `template_root` (rendered for a `.jinja` template); `None` without a template.
fn kept_ancestor(
    renderer: &template::TemplateRenderer,
    template_root: Option<&Path>,
    rel: &Path,
    ctx: &template::TemplateContext,
) -> Result<Option<Vec<u8>>, String> {
    let Some(root) = template_root else {
        return Ok(None);
    };
    let Some(src) = sync::map_to_upstream(root, rel) else {
        return Ok(None);
    };
    if src.extension().is_some_and(|ext| ext == "jinja") {
        return Ok(rendered_template(renderer, root, &src, ctx)?.map(String::into_bytes));
    }
    sync::refuse_symlink_under(root, &src)?;
    match fs::read(&src) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Failed to read template {}: {e}", src.display())),
    }
}

/// The paths in `rels` that git tracks in `project`'s index; empty when neither
/// `project` nor any parent has a `.git` (a project in a monorepo subdirectory has none
/// of its own). A git failure is an error when the project has its own `.git`: init
/// must not overwrite files it could not check. A `.git` only in a parent that git does
/// not recognise as a repository (an empty leftover directory, a copied worktree) is
/// warned about and ignored, as if there were none; a repository git recognises but
/// cannot read (a corrupt index, dubious ownership) is an error.
fn committed_paths(
    project: &Path,
    rels: &[PathBuf],
) -> Result<std::collections::HashSet<PathBuf>, String> {
    let absolute = fs::canonicalize(project)
        .map_err(|e| format!("Failed to resolve {}: {e}", project.display()))?;
    if !absolute.ancestors().any(|dir| dir.join(".git").exists()) {
        return Ok(std::collections::HashSet::new());
    }
    let mut cmd = Command::new("git");
    cmd.current_dir(project)
        .args(["ls-files", "-z", "--"])
        .args(rels);
    let out = process::run_bounded(cmd, "git ls-files", process::LOCAL_TIMEOUT)?;
    if !out.status.success() {
        let reason = format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        if project.join(".git").exists() || recognised_repository(project)? {
            return Err(reason);
        }
        eprintln!(
            "  {} Warning: {reason}; not treating the project as a git checkout",
            "!".yellow()
        );
        return Ok(std::collections::HashSet::new());
    }
    Ok(out
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(String::from_utf8_lossy(path).into_owned()))
        .collect())
}

/// Whether git recognises `project` as inside a repository it should be able to read.
/// "Dubious ownership" counts: git found the repository and refuses it, which the user
/// must resolve.
fn recognised_repository(project: &Path) -> Result<bool, String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(project)
        .args(["rev-parse", "--is-inside-work-tree"]);
    let out = process::run_bounded(cmd, "git rev-parse", process::LOCAL_TIMEOUT)?;
    Ok(out.status.success() || String::from_utf8_lossy(&out.stderr).contains("dubious ownership"))
}

fn set_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);
            let _ = fs::set_permissions(path, permissions);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ude_tree(root: &Path) -> PathBuf {
        let tree = root.join("ude");
        fs::create_dir_all(tree.join("projects/generic")).unwrap();
        tree
    }

    fn foreign_tree(root: &Path) -> PathBuf {
        let tree = root.join("web/templates");
        fs::create_dir_all(&tree).unwrap();
        fs::write(tree.join("index.html"), "<html></html>\n").unwrap();
        tree
    }

    #[test]
    fn test_first_ude_templates_none_without_a_ude_tree() {
        let temp = tempfile::tempdir().unwrap();
        let candidates = [foreign_tree(temp.path()), temp.path().join("missing")];
        assert_eq!(first_ude_templates(candidates), None);
    }

    #[test]
    fn test_first_ude_templates_skips_a_foreign_templates_dir() {
        let temp = tempfile::tempdir().unwrap();
        let ude = ude_tree(temp.path());
        let candidates = [foreign_tree(temp.path()), ude.clone()];
        assert_eq!(first_ude_templates(candidates), Some(ude));
    }

    #[test]
    fn test_first_ude_templates_keeps_search_order() {
        let temp = tempfile::tempdir().unwrap();
        let first = ude_tree(&temp.path().join("a"));
        let second = ude_tree(&temp.path().join("b"));
        assert_eq!(first_ude_templates([first.clone(), second]), Some(first));
    }
}
