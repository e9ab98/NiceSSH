import { ipc } from './client';

export const isGitRepo = (path: string) => ipc<boolean>('is_git_repo', { path });
export const applyIdentityToRepo = (projectId: string, identityId: string) =>
  ipc<void>('apply_identity_to_repo', { projectId, identityId });
export const getRecentCommits = (path: string, limit = 10) =>
  ipc<{ hash: string; subject: string }[]>('get_recent_commits', { path, limit });
export const testSshConnection = (identityId: string) =>
  ipc<{ ok: boolean; message: string; timedOut: boolean }>('test_ssh_connection', { identityId });

export interface RepoGitConfig {
  hasConfig: boolean;
  userName: string | null;
  userEmail: string | null;
  sshKeyPath: string | null;
  managedByNicessh: boolean;
  /// Number of `sshCommand` lines found in `.git/config`. A clean
  /// nicessh-managed repo has exactly 1; older builds (or stray
  /// writes) leave multiple behind, which the audit dialog flags.
  sshCommandCount: number;
}

export const getRepoGitConfig = (path: string) =>
  ipc<RepoGitConfig>('get_repo_git_config', { path });

export type RepoAuditStatus = 'clean' | 'dirty' | 'no-config' | 'no-identity';

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
