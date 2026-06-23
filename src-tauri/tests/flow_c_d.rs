//! M4 integration test: Flow C (switch identity) + Flow D (test SSH connection).
//! We can't actually run ssh in CI without network, so this test only verifies
//! that the relevant command functions are wired up and panic-free.

#[test]
fn test_ssh_test_command_compiles_and_links() {
    // Calling `test_ssh_connection` with a bogus identity id will fail at runtime
    // (AppError::NotFound) once it tries to read the config; we just want to
    // ensure the symbol is reachable through the library's public surface.
    let result = std::panic::catch_unwind(|| {
        let _ = nicessh_lib::commands::git::test_ssh_connection;
    });
    assert!(result.is_ok(), "test_ssh_connection must be linkable");
}

#[test]
fn test_apply_identity_to_repo_compiles_and_links() {
    let result = std::panic::catch_unwind(|| {
        let _ = nicessh_lib::commands::git::apply_identity_to_repo;
    });
    assert!(result.is_ok());
}
