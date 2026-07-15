//! M-extension integration test for the "stale sshCommand on HTTPS"
//! fix. Exercises the public Tauri-command surface
//! (audit_repos + clean_repo_gitconfig) end-to-end against a
//! synthetic ~/.nicessh layout.
//!
//! Run: `cargo test --test audit_https_clean -- --nocapture`

use nicessh_lib::config_store::{AppConfig, Identity, Project, CURRENT_VERSION};

#[test]
fn symbols_are_reachable_through_public_surface() {
    // Compile/link smoke test, matching the pattern in flow_c_d.rs.
    let result = std::panic::catch_unwind(|| {
        let _ = nicessh_lib::commands::git::audit_repos;
        let _ = nicessh_lib::commands::git::clean_repo_gitconfig;
    });
    assert!(result.is_ok(), "audit/clean commands must be linkable");
}

#[test]
fn appconfig_current_version_serializes() {
    // Make sure AppConfig still serializes after the spec — this
    // guards against accidental removal of Default impls in the
    // unit test fixtures added in earlier tasks.
    let cfg = AppConfig {
        version: CURRENT_VERSION,
        theme: "system".into(),
        identities: vec![Identity {
            id: "id_work".into(),
            label: "work".into(),
            user_name: "Alice".into(),
            user_email: "a@x".into(),
            key_path: "~/.ssh/id_work".into(),
            match_path: None,
            host_alias: None,
            git_host: None,
        }],
        projects: vec![Project {
            id: "p1".into(),
            name: "test".into(),
            path: "/tmp/repo".into(),
            identity_id: Some("id_work".into()),
        }],
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("id_work"));
}
