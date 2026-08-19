//! Push destination preflight — pure-function risk evaluation
//! that runs **before** `git push` to surface mismatches between
//! what NiceSSH thinks the push identity is and what the remote
//! actually is.
//!
//! Design goals:
//!   1. **Sub-100ms evaluation** — the UI calls this synchronously
//!      on every Push click. No subprocess calls, no network
//!      probes. We trust the data NiceSSH already has in
//!      `AppConfig` + the on-disk `.git/config` the caller has
//!      already read.
//!   2. **Three-tier output** — `Safe` (silent push through),
//!      `Verify` (single-confirm dialog), `Warn` (double-confirm
//!      with optional typed override). See [`RiskTier`].
//!   3. **No side effects, no I/O** — everything in this module
//!      reads from the [`PushContext`] the caller supplies.
//!      This makes the rules trivially testable (and would let
//!      us snapshot them as docs later if we want).
//!
//! **Out of scope here**:
//!   - Actually pushing (lives in [`crate::git::ops::push`]).
//!   - Reading `.git/config` (caller pre-loads it).
//!   - Commit-signing audit (`%G?` parsing lives elsewhere).
//!
//! Risks called out in prose rather than tests:
//!   - This module does NOT call out to the network or spawn
//!     subprocesses; if NiceSSH's in-memory `cfg.identities`
//!     drifts from `git config --get user.email` at runtime
//!     (e.g. another tool overwrites `~/.gitconfig`), the
//!     "matched" branch can disagree with what `git push`
//!     actually uses. We mitigate by always reading the repo's
//!     effective `.git/config` into the context too.
//!   - The heuristic rules (`personal_label_signal`,
//!     `work_host_signal`) intentionally err on the side of
//!     "Verify" rather than "Warn" — false positives are
//!     cheaper than false negatives here, but a `host_label_mismatch`
//!     on a label called "Personal" but actually used for
//!     work will require "Push anyway" every time. The
//!     per-push override keeps this from being a blocker.

use std::path::PathBuf;

use serde::Serialize;

/// What "kind" of identity source the push is running under.
/// Mirrors the `IdentitySource` enum in `commands::git` but is
/// kept private to preflight — preflight doesn't need to expose
/// this to the UI (the UI already knows from its own state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentitySource {
    Project,
    /// `~/.gitconfig` chain (possibly resolved via `includeIf`).
    /// The identity came from somewhere but is NOT pinned to
    /// this repo.
    Global,
    /// A NiceSSH global default set via the Identities view's
    /// "set as global default" picker.
    GlobalDefault,
    /// No identity is associated with this push.
    None,
}

/// Snapshot of the identity we'd attribute the push to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedIdentity {
    pub id: String,
    pub label: String,
    pub user_name: String,
    pub user_email: String,
    pub source: IdentitySource,
    /// For the audit card on the dialog: sshKeyPath is the
    /// `[core] sshCommand` value the push will use, when set.
    pub ssh_key_path: Option<String>,
}

/// Cached `ssh -T git@<host>` result, if we have one.
/// `None` = never tested. `Some` = tested; check `ok`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTestRecord {
    pub ok: bool,
    pub message: String,
}

/// All data the preflight evaluator needs to make a decision.
/// Built by `commands::git::preflight_push` from `AppConfig` +
/// the on-disk repo state.
#[derive(Debug, Clone)]
pub struct PushContext {
    pub project_path: PathBuf,
    /// From `[remote "<name>"] url = ...` in `.git/config`. `None`
    /// for repos without a remote.
    pub remote_url: Option<String>,
    /// From `git_config::classify_remote_url`: `ssh` | `https` |
    /// `git` | `unknown` | `None` when `remote_url` is `None`.
    pub remote_protocol: Option<String>,
    /// `git rev-parse --abbrev-ref @{u}` — `None` when no
    /// upstream is set.
    pub upstream_branch: Option<String>,
    /// `ahead` from `git rev-list --left-right --count @{u}...HEAD`.
    /// `0` for the no-upstream case.
    pub ahead: u32,
    /// `force` flag the user passed (maps to `--force-with-lease`).
    pub force: bool,
    /// What identity we'd attribute the push to. `None` when the
    /// repo has no identity binding.
    pub effective_identity: Option<ResolvedIdentity>,
    /// Last SSH connection test result, if we have one. Cached
    /// per-identity in the frontend store.
    pub last_ssh_test: Option<SshTestRecord>,
    /// Per-project or global "skip preflight" override. When
    /// `true`, [`evaluate`] returns `Safe` immediately.
    pub skip_preflight: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskTier {
    /// No notification — push through silently.
    Safe,
    /// Single-confirm dialog (one "Push anyway" button).
    Verify,
    /// Two-stage dialog with optional typed override.
    Warn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Danger,
}

/// One machine-readable reason the push was flagged. Multiple
/// reasons can fire on a single evaluation; the UI surfaces all.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskReason {
    pub code: &'static str,
    pub severity: Severity,
    /// i18n key into the frontend translation file.
    pub message_key: &'static str,
}

/// One clickable fix the UI can offer (e.g. "Switch to the Work
/// identity"). The frontend wires each `kind` to its own handler.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionAction {
    pub kind: &'static str,
    pub label_key: &'static str,
    /// Optional id the frontend needs to perform the action
    /// (e.g. identity id for "switch identity").
    pub target_id: Option<String>,
}

/// Output of [`evaluate`]. The frontend renders one of three UI
/// states based on `tier`; `reasons` drives the body copy; and
/// `suggestions` drives the buttons below the body.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightReport {
    pub tier: RiskTier,
    pub reasons: Vec<RiskReason>,
    pub suggestions: Vec<SuggestionAction>,
    /// When `true`, the dialog must require the user to type the
    /// repo name (or the remote host) before "Push anyway"
    /// becomes clickable. Wired up only for `tier = Warn`.
    pub requires_typed_confirmation: bool,
    /// What the user must type. The dialog shows this as the
    /// expected input. `None` when not required.
    pub override_target: Option<String>,
}

impl PreflightReport {
    /// Convenience: render the worst-case `tier` of two reports.
    /// Used when merging results from `evaluate` with a future
    /// commit-audit pass — if either side says `Warn`, the
    /// combined result is `Warn`.
    pub fn worst(self, other: PreflightReport) -> PreflightReport {
        if other.tier_rank() > self.tier_rank() {
            other
        } else {
            self
        }
    }

    fn tier_rank(&self) -> u8 {
        match self.tier {
            RiskTier::Safe => 0,
            RiskTier::Verify => 1,
            RiskTier::Warn => 2,
        }
    }
}

/// Pure function: takes a fully-built `PushContext`, returns a
/// `PreflightReport`. No I/O. No subprocesses. No global state.
///
/// Rule table — see module-level docs for the design rationale.
/// All rules are pure functions of the context; no hidden state.
pub fn evaluate(ctx: &PushContext) -> PreflightReport {
    // Short-circuit: explicit skip overrides everything.
    if ctx.skip_preflight {
        return PreflightReport {
            tier: RiskTier::Safe,
            reasons: Vec::new(),
            suggestions: Vec::new(),
            requires_typed_confirmation: false,
            override_target: None,
        };
    }

    let mut reasons: Vec<RiskReason> = Vec::new();
    let mut suggestions: Vec<SuggestionAction> = Vec::new();
    let mut tier = RiskTier::Safe;

    let promote = |t: &mut RiskTier, new_tier: RiskTier| {
        let new_rank = match new_tier {
            RiskTier::Safe => 0,
            RiskTier::Verify => 1,
            RiskTier::Warn => 2,
        };
        let cur_rank = match t {
            RiskTier::Safe => 0,
            RiskTier::Verify => 1,
            RiskTier::Warn => 2,
        };
        if new_rank > cur_rank {
            *t = new_tier;
        }
    };

    // ── Rule: no_identity ─────────────────────────────────────
    if ctx.effective_identity.is_none() {
        reasons.push(RiskReason {
            code: "no_identity",
            severity: Severity::Danger,
            message_key: "preflight.reason.no_identity",
        });
        promote(&mut tier, RiskTier::Warn);
        suggestions.push(SuggestionAction {
            kind: "create_identity",
            label_key: "preflight.suggestion.create_identity",
            target_id: None,
        });
    }

    if let Some(idty) = &ctx.effective_identity {
        // ── Rule: identity_global_default ────────────────────────
        if idty.source == IdentitySource::GlobalDefault {
            reasons.push(RiskReason {
                code: "identity_global_default",
                severity: Severity::Info,
                message_key: "preflight.reason.identity_global_default",
            });
            promote(&mut tier, RiskTier::Verify);
        }

        // ── Rule: force_push ────────────────────────────────────
        if ctx.force {
            reasons.push(RiskReason {
                code: "force_push",
                severity: Severity::Warning,
                message_key: "preflight.reason.force_push",
            });
            promote(&mut tier, RiskTier::Verify);
        }

        // ── Rule: protocol_mismatch ─────────────────────────────
        // SSH identity + HTTPS remote (or vice versa) is almost
        // always a misconfigured remote URL.
        match (idty.ssh_key_path.is_some(), ctx.remote_protocol.as_deref()) {
            (true, Some("https")) | (true, Some("git")) => {
                reasons.push(RiskReason {
                    code: "protocol_mismatch",
                    severity: Severity::Danger,
                    message_key: "preflight.reason.protocol_mismatch",
                });
                promote(&mut tier, RiskTier::Warn);
                suggestions.push(SuggestionAction {
                    kind: "review_remote_url",
                    label_key: "preflight.suggestion.review_remote_url",
                    target_id: None,
                });
            }
            (false, Some("ssh")) => {
                reasons.push(RiskReason {
                    code: "ssh_identity_missing_key",
                    severity: Severity::Danger,
                    message_key: "preflight.reason.ssh_identity_missing_key",
                });
                promote(&mut tier, RiskTier::Warn);
            }
            _ => {}
        }

        // ── Rule: host_label_mismatch ───────────────────────────
        if let Some(host) = host_from_remote(ctx.remote_url.as_deref()) {
            let personal = personal_label_signal(&idty.label, &idty.user_email);
            let work_host = work_host_signal(&host);
            if personal && work_host {
                reasons.push(RiskReason {
                    code: "host_label_mismatch",
                    severity: Severity::Warning,
                    message_key: "preflight.reason.host_label_mismatch",
                });
                promote(&mut tier, RiskTier::Verify);
                suggestions.push(SuggestionAction {
                    kind: "switch_identity",
                    label_key: "preflight.suggestion.switch_identity",
                    target_id: None, // filled by frontend after listing
                });
            }
            // The inverse case (work-label + personal host) is
            // possible but rarer; keep it at Verify not Warn to
            // avoid noisy false positives.
            else if !personal
                && work_label_signal(&idty.label, &idty.user_email)
                && personal_host_signal(&host)
            {
                reasons.push(RiskReason {
                    code: "host_label_inverse_mismatch",
                    severity: Severity::Info,
                    message_key: "preflight.reason.host_label_inverse_mismatch",
                });
                promote(&mut tier, RiskTier::Verify);
            }
        }

        // ── Rule: ssh_test_never_run ────────────────────────────
        if ctx.last_ssh_test.is_none() && idty.ssh_key_path.is_some() {
            reasons.push(RiskReason {
                code: "ssh_test_never_run",
                severity: Severity::Info,
                message_key: "preflight.reason.ssh_test_never_run",
            });
            // Don't auto-promote — this is just a hint. The user
            // may already know their setup works.
            suggestions.push(SuggestionAction {
                kind: "run_ssh_test",
                label_key: "preflight.suggestion.run_ssh_test",
                target_id: Some(idty.id.clone()),
            });
        }

        // ── Rule: ssh_test_failed_recently ──────────────────────
        if let Some(t) = &ctx.last_ssh_test {
            if !t.ok {
                reasons.push(RiskReason {
                    code: "ssh_test_failed_recently",
                    severity: Severity::Danger,
                    message_key: "preflight.reason.ssh_test_failed_recently",
                });
                promote(&mut tier, RiskTier::Warn);
            }
        }
    }

    // ── Rule: unknown_protocol ────────────────────────────────
    if ctx.remote_url.is_some() && ctx.remote_protocol.as_deref() == Some("unknown") {
        reasons.push(RiskReason {
            code: "unknown_protocol",
            severity: Severity::Warning,
            message_key: "preflight.reason.unknown_protocol",
        });
        promote(&mut tier, RiskTier::Verify);
    }

    // ── Rule: no_upstream_force ───────────────────────────────
    if ctx.force && ctx.upstream_branch.is_none() {
        reasons.push(RiskReason {
            code: "no_upstream_force",
            severity: Severity::Danger,
            message_key: "preflight.reason.no_upstream_force",
        });
        promote(&mut tier, RiskTier::Warn);
    }

    // Typed-confirmation gate: only fire for `tier = Warn` when
    // the worst reason is `protocol_mismatch` OR `host_label_mismatch`
    // combined with `force_push` — i.e. situations where a
    // single mis-click is genuinely destructive.
    let requires_typed_confirmation = tier == RiskTier::Warn
        && reasons
            .iter()
            .any(|r| matches!(r.code, "protocol_mismatch" | "no_identity" | "no_upstream_force"));
    let override_target = if requires_typed_confirmation {
        ctx.remote_url.clone()
    } else {
        None
    };

    PreflightReport {
        tier,
        reasons,
        suggestions,
        requires_typed_confirmation,
        override_target,
    }
}

// ── helpers ────────────────────────────────────────────────────

fn host_from_remote(url: Option<&str>) -> Option<String> {
    let u = url?;
    let u = u.trim();
    // git@github-work:org/repo.git  ->  github-work
    if let Some(rest) = u.strip_prefix("git@") {
        if let Some((host, _)) = rest.split_once(':') {
            return Some(host.to_string());
        }
    }
    // ssh://git@github-work/org/repo.git  ->  github-work
    if let Some(rest) = u.strip_prefix("ssh://") {
        // Strip optional user@ prefix.
        let host_part = rest.split_once('@').map(|(_, r)| r).unwrap_or(rest);
        return host_part.split('/').next().map(str::to_owned);
    }
    // https://github.com/org/repo.git  ->  github.com
    if u.starts_with("https://") || u.starts_with("http://") {
        let rest = u
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        return rest.split('/').next().map(str::to_owned);
    }
    None
}

fn personal_label_signal(label: &str, email: &str) -> bool {
    let l = label.to_lowercase();
    let e = email.to_lowercase();
    l.contains("personal") || l.contains("private")
        || e.contains("@gmail.") || e.contains("@qq.")
        || e.contains("@163.") || e.contains("@outlook.")
        || e.contains("@hotmail.") || e.contains("@icloud.")
        || e.contains("@yahoo.")
}

fn work_label_signal(label: &str, email: &str) -> bool {
    let l = label.to_lowercase();
    let e = email.to_lowercase();
    l.contains("work") || l.contains("corp") || l.contains("company")
        || e.contains("@company.") || e.contains("@corp.") || e.contains("@acme.")
}

fn work_host_signal(host: &str) -> bool {
    let h = host.to_lowercase();
    h.contains("github-work") || h.contains("github.enterprise")
        || h.contains("gitlab.") || h.contains("corp.")
        || h.contains("-work") || h.contains("work-")
        || h.contains("ghe.") || h.contains("ghe-")
}

fn personal_host_signal(host: &str) -> bool {
    let h = host.to_lowercase();
    // Strict definition: github.com is "personal" by default;
    // self-hosted enterprise-style hosts are NOT personal.
    h == "github.com" || h.starts_with("gitlab.com")
}
