//! M3 integration test: Flow A (apply identity to repo) + Flow B (assign identity to project)
//! end-to-end. Verifies that calling the production `apply_identity_to_repo` command
//! writes all three artifacts on disk: repo `.git/config`, `~/.gitconfig` (includeIf),
//! and `~/.gitconfig-<label>` (per-identity subfile).

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use nicessh_lib::commands::git::apply_identity_to_repo;
use nicessh_lib::config_store::{self, Identity, Project};

// Integration tests live in a separate crate, so they can't reach the
// in-crate `#[cfg(test)] test_helpers::HOME_LOCK`. We re-implement the
// same isolation contract here: serialize, redirect HOME to a temp dir,
// restore HOME on exit (success or panic), best-effort cleanup.
static HOME_LOCK: Mutex<()> = Mutex::new(());

fn with_temp_home<F: FnOnce()>(suffix: &str, f: F) {
    let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev = env::var_os("HOME");
    let tmp = std::env::temp_dir()
        .join(format!("nicessh-flow-ab-{}-{}", std::process::id(), suffix));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    env::set_var("HOME", &tmp);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    if let Some(p) = prev {
        env::set_var("HOME", p);
    } else {
        env::remove_var("HOME");
    }
    let _ = fs::remove_dir_all(&tmp);
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

fn write_config_with_identity_and_project(repo_path: &std::path::Path) -> (String, String) {
    let mut cfg = config_store::read().unwrap();
    let identity = Identity {
        id: config_store::new_id(),
        label: "Work".into(),
        user_name: "Alice".into(),
        user_email: "alice@example.com".into(),
        key_path: "~/.ssh/id_work".into(),
        match_path: Some("~/work".into()),
        host_alias: Some("github.com".into()),
        git_host: Some("github.com".into()),
    };
    let project = Project {
        id: config_store::new_id(),
        name: "myrepo".into(),
        path: repo_path.to_string_lossy().to_string(),
        identity_id: Some(identity.id.clone()),
    };
    let identity_id = identity.id.clone();
    let project_id = project.id.clone();
    cfg.identities.push(identity);
    cfg.projects.push(project);
    config_store::write_snapshot(&cfg, "test_setup", "fixture for flow_a_b").unwrap();
    (project_id, identity_id)
}

#[test]
fn test_apply_identity_to_repo_writes_all_three_files() {
    with_temp_home("apply", || {
        // Build a fake git repo inside the temp HOME.
        let home = env::var("HOME").unwrap();
        let repo = PathBuf::from(&home).join("projects/myrepo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(
            repo.join(".git/config"),
            "[core]\n    repositoryformatversion = 0\n",
        )
        .unwrap();

        let (project_id, identity_id) = write_config_with_identity_and_project(&repo);

        // Exercise the real production command.
        apply_identity_to_repo(project_id, identity_id).expect("apply should succeed");

        // 1. Repo gitconfig got the managed [user] / [core] sshCommand block.
        let repo_cfg = fs::read_to_string(repo.join(".git/config")).unwrap();
        assert!(
            repo_cfg.contains("sshCommand"),
            "repo .git/config should contain sshCommand, got:\n{}",
            repo_cfg
        );
        assert!(repo_cfg.contains("nicessh-managed"));

        // 2. ~/.gitconfig got the [includeIf] block.
        let gitconfig = fs::read_to_string(format!("{}/.gitconfig", home)).unwrap();
        assert!(
            gitconfig.contains("includeIf"),
            "~/.gitconfig should contain includeIf, got:\n{}",
            gitconfig
        );

        // 3. ~/.gitconfig-<label> got the [user] block.
        let id_gc = fs::read_to_string(format!("{}/.gitconfig-work", home)).unwrap();
        assert!(
            id_gc.contains("name ="),
            "~/.gitconfig-work should contain 'name =', got:\n{}",
            id_gc
        );
        assert!(id_gc.contains("Alice"));
        assert!(id_gc.contains("alice@example.com"));
    });
}
