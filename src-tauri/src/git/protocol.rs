//! Protocol-driven write decisions for `.git/config` and the
//! per-identity sub-gitconfig. Centralises the "HTTPS must not
//! carry `[core] sshCommand`" rule so every writer agrees.
//!
//! URL classification itself lives in
//! `crate::git_config::classify_remote_url`; this module owns the
//! downstream policy that turns a protocol string into a binary
//! "is sshCommand meaningful for this remote" decision.

/// Returns `true` when an `[core] sshCommand = ...` line is
/// meaningful for the given remote protocol. The policy is:
///
/// - `Some("ssh")`   -> `true`  (SSH / git:// / ssh+git://)
/// - `Some("git")`   -> `true`  (treated as SSH-family, conservative)
/// - `Some("https")` -> `false` (HTTPS goes through `git-credential`,
///                        sshCommand is dead weight and may shadow
///                        a sibling SSH project's [core] via the
///                        shared `~/.gitconfig-<label>` includeIf)
/// - `Some("unknown") | None` -> `true` (no remote = treat as SSH
///                        so a later switch to SSH is seamless;
///                        matches existing `BindOutcome` default)
///
/// This replaces the three ad-hoc HTTPS/SSH branches in
/// `apply_identity_to_repo` / `clean_repo_gitconfig` /
/// `splice_user_only_into_config` so the rule cannot drift
/// between writers.
pub fn should_emit_sshcommand_for_protocol(protocol: Option<&str>) -> bool {
    !matches!(protocol, Some("https"))
}

#[cfg(test)]
mod tests {
    use super::should_emit_sshcommand_for_protocol;

    #[test]
    fn ssh_protocol_emits_sshcommand() {
        assert!(should_emit_sshcommand_for_protocol(Some("ssh")));
        assert!(should_emit_sshcommand_for_protocol(Some("git")));
        assert!(should_emit_sshcommand_for_protocol(Some("unknown")));
        assert!(should_emit_sshcommand_for_protocol(None));
    }

    #[test]
    fn https_protocol_does_not_emit_sshcommand() {
        assert!(!should_emit_sshcommand_for_protocol(Some("https")));
    }
}
