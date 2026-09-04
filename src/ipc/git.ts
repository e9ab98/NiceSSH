import { ipc, type IpcOptions } from './client';

export const isGitRepo = (path: string) => ipc<boolean>('is_git_repo', { path });
/// Initialize a non-git directory as a git repository and commit a
/// minimal initial revision attributed to the chosen identity.
/// Returns void on success; throws on validation/git failures.
/// See Rust `commands/git.rs::init_repo` for the exact step list.
export const initRepo = (path: string, identityId: string) =>
  ipc<void>('init_repo', { path, identityId });

// --- Quick Actions row (status / commit / push / pull / fetch) ---
// See Rust `commands/git.rs` for the exact step list and error
// surfaces. The Tauri command names use snake_case (`git_status`,
// `git_commit`, ...) to avoid colliding with the JS-side `status`
// / `commit` identifiers that are also used as the IPC *channel*
// names in some transports.

export interface RepoStatus {
  porcelain: string;
  ahead: number | null;
  behind: number | null;
  hasUpstream: boolean;
}

export const gitStatus = (path: string) =>
  ipc<RepoStatus>('git_status', { path });

export const gitCommit = (path: string, message: string, addAll: boolean, options?: IpcOptions) =>
  ipc<string>('git_commit', { path, message, addAll }, options);

export interface OverrideAck {
  tier: string;
  riskCodes: string[];
  typedOverride?: string | null;
}

export const gitPush = (
  path: string,
  force: boolean,
  overrideAck: OverrideAck | null,
) => ipc<string>('git_push', { path, force, overrideAck });

export const gitPull = (path: string, rebase: boolean) =>
  ipc<string>('git_pull', { path, rebase });

// ── Push preflight ──────────────────────────────────────
export type RiskTier = 'safe' | 'verify' | 'warn';

export type Severity = 'info' | 'warning' | 'danger';

export interface RiskReason {
  code: string;
  severity: Severity;
  messageKey: string;
}

export interface SuggestionAction {
  kind: string;
  labelKey: string;
  targetId: string | null;
}

export interface ResolvedIdentityForPush {
  id: string;
  label: string;
  userName: string;
  userEmail: string;
  source: 'project' | 'global' | 'globalDefault' | 'none';
  sshKeyPath: string | null;
}

export interface SshTestRecord {
  ok: boolean;
  message: string;
}

export interface PreflightReport {
  tier: RiskTier;
  reasons: RiskReason[];
  suggestions: SuggestionAction[];
  requiresTypedConfirmation: boolean;
  overrideTarget: string | null;
}

export const preflightPush = (
  projectId: string,
  path: string,
  force: boolean,
  skipPreflight: boolean,
) =>
  ipc<PreflightReport>('preflight_push', {
    projectId,
    path,
    force,
    skipPreflight,
  });

export const gitFetch = (path: string) =>
  ipc<string>('git_fetch', { path });
export type BindOutcome = 'ssh-style' | 'user-only' | 'needs-remote';

export const applyIdentityToRepo = (projectId: string, identityId: string) =>
  ipc<BindOutcome>('apply_identity_to_repo', { projectId, identityId });

/// HTTPS counterpart of `applyIdentityToRepo`: write a User's
/// `(name, email)` pair straight into the project's `.git/config`
/// without going through an Identity record. Identity is conceptually
/// "key + user + match path" — for HTTPS projects none of those
/// apply, so the switcher dialog selects from the User pool and
/// dispatches here.
export const applyUserToRepo = (projectId: string, userId: string) =>
  ipc<BindOutcome>('apply_user_to_repo', { projectId, userId });

export const writeRepoRemote = (
  path: string,
  url: string,
  name: string = 'origin',
) => ipc<string>('write_repo_remote', { path, name, url });
export const getRecentCommits = (path: string, limit = 10) =>
  ipc<{ hash: string; subject: string }[]>('get_recent_commits', { path, limit });
export const testSshConnection = (identityId: string) =>
  ipc<ConnectionTestResult>('test_ssh_connection', { identityId });

export interface ConnectionTestResult {
  ok: boolean;
  message: string;
  timedOut: boolean;
  needsCredentials?: boolean;
  credentialsSaved?: boolean;
}

export const testHttpsConnection = (path: string) =>
  ipc<ConnectionTestResult>('test_https_connection', { path });

export const testHttpsConnectionWithCredentials = (path: string, username: string, password: string) =>
  ipc<ConnectionTestResult>('test_https_connection_with_credentials', { path, username, password });

export type IdentitySource =
  | 'project'
  | 'global'
  | 'globalDefault'
  | 'none';

export interface RepoGitConfig {
  hasConfig: boolean;
  userName: string | null;
  userEmail: string | null;
  /// Where `userName` came from. `'project'` = repo's own
  /// `.git/config`; `'global'` = git's config chain (typically
  /// `~/.gitconfig`, possibly via `includeIf`); `'none'` = no
  /// value anywhere. UI uses this to render a source tag.
  userNameSource: IdentitySource;
  /// Same as `userNameSource` but for `userEmail`.
  userEmailSource: IdentitySource;
  sshKeyPath: string | null;
  managedByNicessh: boolean;
  /// Number of `sshCommand` lines found in `.git/config`. A clean
  /// nicessh-managed repo has exactly 1; older builds (or stray
  /// writes) leave multiple behind, which the audit dialog flags.
  sshCommandCount: number;
  /// First remote URL found in `[remote "<name>"] url = ...`, or
  /// null if the repo has no remote configured. Used to decide
  /// whether NiceSSH's `[core] sshCommand` write is meaningful
  /// (HTTPS repos do not use ssh). Multiple remotes are not all
  /// surfaced — the first one wins (typically `origin`).
  remoteUrl: string | null;
  /// One of `'ssh' | 'https' | 'git' | 'unknown'`. `unknown` covers
  /// file://, plain relative paths, git-lab internal URLs, etc.
  /// `null` iff `remoteUrl` is `null`.
  remoteProtocol: 'ssh' | 'https' | 'git' | 'unknown' | null;
}

export const getRepoGitConfig = (path: string) =>
  ipc<RepoGitConfig>('get_repo_git_config', { path });

export type RepoAuditStatus = 'clean' | 'dirty' | 'no-config' | 'no-identity' | 'stale-sshcommand-on-https';

export interface RepoAudit {
  projectId: string;
  projectName: string;
  projectPath: string;
  hasConfig: boolean;
  managedByNicessh: boolean;
  sshCommandCount: number;
  status: RepoAuditStatus;
  identityId: string | null;
  identityLabel: string | null;
  sshTestOk: boolean | null;
  sshTestMessage: string | null;
  /// Same shape as `RepoGitConfig.remoteProtocol`. Surfaced in the
  /// audit table so users can spot HTTPS repos with stale
  /// sshCommand lines.
  remoteUrl: string | null;
  remoteProtocol: 'ssh' | 'https' | 'git' | 'unknown' | null;
}

export const auditRepos = (runSshTests: boolean) =>
  ipc<RepoAudit[]>('audit_repos', { runSshTests });

export const cleanRepoGitconfig = (projectId: string) =>
  ipc<void>('clean_repo_gitconfig', { projectId });

export interface GlobalGitConfig {
  hasConfig: boolean;
  userName: string | null;
  userEmail: string | null;
  sshKeyPath: string | null;
}

export const getGlobalGitConfig = () =>
  ipc<GlobalGitConfig>('get_global_git_config');

export interface GlobalGitConfigChange {
  userName: string;
  userEmail: string;
  sshKeyPath: string;
}

export const setGlobalGitConfig = (identityId: string) =>
  ipc<GlobalGitConfigChange>('set_global_git_config', { identityId });

// --- Global default identity (advisory pointer in cfg) ---
//
// These back the `useGlobalDefaultStore`. `setGlobalGitConfig`
// remains the lower-level call that only rewrites `~/.gitconfig`;
// the new pair additionally records `globalDefaultIdentityId` in
// the NiceSSH config store so the UI can show "this is the
// global default" without re-parsing `~/.gitconfig` every render.
// `setGlobalDefaultIdentity` does both writes for callers that
// want the full "make this the global default" action in one
// round-trip; `unsetGlobalDefaultIdentity` clears the pointer but
// leaves `~/.gitconfig` untouched (see Rust doc-comment for the
// reason — never silently undo a user's hand-edits).
export const getGlobalDefaultIdentityId = () =>
  ipc<string | null>('get_global_default_identity_id');

export const setGlobalDefaultIdentity = (identityId: string) =>
  ipc<GlobalGitConfigChange>('set_global_default_identity', { identityId });

export const unsetGlobalDefaultIdentity = () =>
  ipc<void>('unset_global_default_identity');
