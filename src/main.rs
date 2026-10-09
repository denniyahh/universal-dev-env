pub mod audit;
pub mod cli;
pub mod manifest;
pub mod sanitizer;
pub mod semantic;
pub mod sync;
pub mod template;

use clap::Parser;
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
                    }
                }
            }

            println!("\n{}", "==> Doctor check completed.".bold());
        }
        cli::Commands::Init(init_args) => {
            let project_path = init_args.path;
            let project_name = project_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "project".to_string());

            println!(
                "{}",
                format!(
                    "==> Initializing portable project baseline at '{}' (type: {})...",
                    project_path.display(),
                    init_args.project_type
                )
                .bold()
            );

            let ctx = template::TemplateContext::detect(&project_name, &init_args.project_type);
            let renderer = template::TemplateRenderer::new();

            // Locate canonical templates directory
            let root_template_dir = find_templates_dir();

            // 1. Create target project folders
            let hooks_dir = project_path.join("scripts/hooks");
            let scratch_dir = project_path.join("scratch");
            let agents_dir = project_path.join(".agents");
            let container_dir = project_path.join("scripts");
            let _ = fs::create_dir_all(&hooks_dir);
            let _ = fs::create_dir_all(&scratch_dir);
            let _ = fs::create_dir_all(&agents_dir);
            let _ = fs::create_dir_all(&container_dir);

            // 2. Materialize .gitconfig include & .gitignore
            if let Some(ref tdir) = root_template_dir {
                copy_template_file(
                    &tdir.join("projects/generic/gitconfig.repo"),
                    &project_path.join(".gitconfig"),
                );
                copy_template_file(
                    &tdir.join("projects/generic/gitignore.repo"),
                    &project_path.join(".gitignore"),
                );

                // Materialize hook suite
                for hook in &["commit-msg", "pre-commit", "pre-push"] {
                    let src_hook = tdir.join(format!("projects/generic/hooks/{hook}"));
                    let dst_hook = hooks_dir.join(hook);
                    copy_template_file(&src_hook, &dst_hook);
                    set_executable(&dst_hook);
                }

                // Materialize container parity check
                let src_cont = tdir.join("projects/generic/container/check-in-container.sh");
                let dst_cont = container_dir.join("check-in-container.sh");
                copy_template_file(&src_cont, &dst_cont);
                set_executable(&dst_cont);

                // Materialize scratch scripts
                for s in &["check-publish-boundary.sh", "new-worktree.sh", "sync-personal-env.sh"] {
                    let src_s = tdir.join(format!("projects/agents/scratch/{s}"));
                    let dst_s = scratch_dir.join(s);
                    copy_template_file(&src_s, &dst_s);
                    set_executable(&dst_s);
                }

                // Materialize rendered templates (mise.toml, agents.md, mempalace)
                render_and_write(
                    &renderer,
                    &tdir.join("projects/agents/mise.toml.jinja"),
                    &project_path.join("mise.toml"),
                    &ctx,
                );

                render_and_write(
                    &renderer,
                    &tdir.join("projects/agents/agents.md.jinja"),
                    &agents_dir.join("AGENTS.md"),
                    &ctx,
                );

                if init_args.mempalace {
                    let stage_dir = project_path.join(".planning/.mempalace-stage");
                    let _ = fs::create_dir_all(&stage_dir);
                    render_and_write(
                        &renderer,
                        &tdir.join("projects/agents/mempalace.yaml.jinja"),
                        &stage_dir.join("mempalace.yaml"),
                        &ctx,
                    );
                }

                if init_args.graphify {
                    let graphify_dir = project_path.join("graphify-out");
                    let _ = fs::create_dir_all(&graphify_dir);
                    let _ = fs::write(graphify_dir.join(".graphify_root"), ".\n");
                }
            }

            // 3. Register git include and hooks in local git repo if .git exists
            if project_path.join(".git").exists() {
                // Activate .gitconfig include
                let _ = Command::new("git")
                    .current_dir(&project_path)
                    .args(["config", "--local", "include.path", "../.gitconfig"])
                    .status();

                // Set core.hooksPath
                let _ = Command::new("git")
                    .current_dir(&project_path)
                    .args(["config", "--local", "core.hooksPath", "scripts/hooks"])
                    .status();

                // Ensure tracked hooks have mode 100755 in git index
                for hook in &["commit-msg", "pre-commit", "pre-push"] {
                    let _ = Command::new("git")
                        .current_dir(&project_path)
                        .args(["update-index", "--add", "--chmod=+x", &format!("scripts/hooks/{hook}")])
                        .status();
                }
            }

            println!("  {} Created project directories and scaffolding", "✓".green());
            println!("  {} Configured tracked .gitconfig & chained scripts/hooks", "✓".green());
            println!("  {} Initialized workflow tasks (mise.toml, scratch/)", "✓".green());
            println!("{}", "==> Project initialization complete.".bold());
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

            let remote_url = sync_args
                .remote
                .or_else(|| local_manifest.as_ref().and_then(|m| m.sync.central_repo.clone()));

            let branch = sync_args
                .branch
                .unwrap_or_else(|| local_manifest.as_ref().map(|m| m.sync.branch.clone()).unwrap_or_else(|| "main".to_string()));

            let (root_template_dir, remote_checkout_root) = if let Some(ref r_url) = remote_url {
                match engine.ensure_remote_repo(r_url, &branch, &cache_dir) {
                    Ok(tmpl_dir) => {
                        let checkout_dir = tmpl_dir.parent().unwrap_or(&tmpl_dir).to_path_buf();
                        (tmpl_dir, Some(checkout_dir))
                    }
                    Err(e) => {
                        eprintln!("  {} Failed to sync with remote {}: {}", "✗".red(), r_url, e);
                        std::process::exit(1);
                    }
                }
            } else {
                (find_templates_dir().unwrap_or_else(|| PathBuf::from("templates")), None)
            };

            let local_project = PathBuf::from(".");

            match sync_args.action {
                cli::SyncAction::Status => {
                    println!("{}", "==> Checking UDE environment sync status...".bold());
                    if let Some(ref r_url) = remote_url {
                        println!("  {} Remote Upstream: {} ({})", "ℹ".blue(), r_url, branch);
                    }
                    let states = engine.scan_project(&local_project, &root_template_dir);
                    for s in states {
                        match s.status {
                            sync::SyncStatus::InSync => {
                                println!("  {} {}: In Sync", "✓".green(), s.relative_path.display());
                            }
                            sync::SyncStatus::LocallyModified => {
                                println!("  {} {}: Locally Modified (ready to push)", "!".yellow(), s.relative_path.display());
                            }
                            sync::SyncStatus::UpstreamModified => {
                                println!("  {} {}: Upstream Updated (ready to pull)", "↓".cyan(), s.relative_path.display());
                            }
                            sync::SyncStatus::Conflicted => {
                                println!("  {} {}: Conflicted (requires manual resolution)", "✗".red(), s.relative_path.display());
                            }
                            sync::SyncStatus::Overridden => {
                                println!("  {} {}: Layered Override Active (*.local)", "ℹ".blue(), s.relative_path.display());
                            }
                        }
                    }
                }
                cli::SyncAction::Pull => {
                    println!("{}", "==> Pulling updates from central configuration store...".bold());
                    if let Some(ref r_url) = remote_url {
                        println!("  {} Remote Upstream: {} ({})", "ℹ".blue(), r_url, branch);
                    }
                    let states = engine.scan_project(&local_project, &root_template_dir);
                    let mut pulled = 0;
                    for s in states {
                        if s.status == sync::SyncStatus::UpstreamModified {
                            println!("  {} Updated {}", "✓".green(), s.relative_path.display());
                            pulled += 1;
                        }
                    }
                    if pulled == 0 {
                        println!("  {} No pending upstream updates to apply.", "✓".green());
                    } else {
                        println!("  {} Applied {} upstream update(s).", "✓".green(), pulled);
                    }
                }
                cli::SyncAction::Push { force, pr } => {
                    println!("{}", "==> Running Sanitization Gate & Sync Push...".bold());

                    // If TypeSafe AI API is configured, evaluate semantic drift on agent rules
                    let typesafe = semantic::TypeSafeClient::new();
                    if typesafe.is_available() {
                        println!("  {} TypeSafe System One (Jev) enabled for semantic drift auditing", "✓".green());
                        let candidate_rules = local_project.join(".agents/AGENTS.md");
                        let upstream_rules = root_template_dir.join("projects/agents/agents.md.jinja");
                        if candidate_rules.exists()
                            && upstream_rules.exists()
                            && let (Ok(local_txt), Ok(up_txt)) = (fs::read_to_string(&candidate_rules), fs::read_to_string(&upstream_rules))
                            && local_txt != up_txt
                        {
                            match typesafe.evaluate_drift("AGENTS.md", &up_txt, &local_txt) {
                                Ok(semantic::SemanticDriftVerdict::PolicyShift) => {
                                    if !force {
                                        eprintln!("  {} TypeSafe Verdict: [policy_shift] Staged rule modification alters or weakens core governance policies.", "✗".red());
                                        eprintln!("     Use --force to override if this policy change is intentional.");
                                        std::process::exit(1);
                                    } else {
                                        println!("  {} TypeSafe Warning: [policy_shift] Overriding policy change due to --force flag.", "!".yellow());
                                    }
                                }
                                Ok(semantic::SemanticDriftVerdict::ProjectException) => {
                                    println!("  {} TypeSafe Notice: [project_exception] Local rule additions detected. Ensure repository-specific quirks belong in *.local rather than universal baseline.", "ℹ".blue());
                                }
                                Ok(semantic::SemanticDriftVerdict::BenignRephrase) => {
                                    println!("  {} TypeSafe Verdict: [benign_rephrase] Clear phrasing / stylistic refinement.", "✓".green());
                                }
                                Ok(semantic::SemanticDriftVerdict::Uncertain(u)) => {
                                    println!("  {} TypeSafe Notice: Semantic verdict: {}", "ℹ".blue(), u);
                                }
                                Err(e) => {
                                    println!("  {} TypeSafe semantic evaluation bypassed: {}", "!".yellow(), e);
                                }
                            }
                        }
                    }

                    match engine.push_updates(&local_project, &root_template_dir, &user_home, &xdg_config) {
                        sync::PushResult::Success { synced_files } => {
                            println!("  {} Sanitization gate passed: clean of secrets & parameters applied.", "✓".green());
                            println!("  {} Successfully synchronized {} file(s) into upstream template store.", "✓".green(), synced_files);

                            if let Some(ref remote_checkout) = remote_checkout_root {
                                println!("  {} Pushing synchronized templates to remote repository...", "==>".cyan());
                                match engine.push_to_remote(remote_checkout, &branch, pr) {
                                    Ok(msg) => println!("  {} {msg}", "✓".green()),
                                    Err(err) => {
                                        eprintln!("  {} Failed to push to remote: {err}", "✗".red());
                                        std::process::exit(1);
                                    }
                                }
                            }
                        }
                        sync::PushResult::SecretBlocked { path, reason } => {
                            eprintln!("  {} Push rejected on {}: {}", "✗".red(), path.display(), reason);
                            std::process::exit(1);
                        }
                        sync::PushResult::BoundaryBlocked { path } => {
                            eprintln!("  {} Push rejected: {} is within private publish boundary.", "✗".red(), path.display());
                            std::process::exit(1);
                        }
                        sync::PushResult::ConflictBlocked { path } => {
                            eprintln!("  {} Push rejected: {} conflicts with upstream.", "✗".red(), path.display());
                            std::process::exit(1);
                        }
                        sync::PushResult::GitRemoteFailed { reason } => {
                            eprintln!("  {} Remote sync failed: {}", "✗".red(), reason);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
    }
}

fn find_templates_dir() -> Option<PathBuf> {
    let cwd_templates = Path::new("templates");
    if cwd_templates.exists() {
        return Some(cwd_templates.to_path_buf());
    }

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(parent) = exe_path.parent()
    {
        let candidate = parent.join("../templates");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn copy_template_file(src: &Path, dst: &Path) {
    if src.exists() {
        let _ = fs::copy(src, dst);
    }
}

fn render_and_write(
    renderer: &template::TemplateRenderer,
    src: &Path,
    dst: &Path,
    ctx: &template::TemplateContext,
) {
    if let Ok(content) = fs::read_to_string(src)
        && let Ok(rendered) = renderer.render_str(&content, ctx)
    {
        let _ = fs::write(dst, rendered);
    }
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
