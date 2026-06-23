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
}

export const getRepoGitConfig = (path: string) =>
  ipc<RepoGitConfig>('get_repo_git_config', { path });

export interface GlobalGitConfig {
  hasConfig: boolean;
  userName: string | null;
  userEmail: string | null;
  sshKeyPath: string | null;
}

export const getGlobalGitConfig = () =>
  ipc<GlobalGitConfig>('get_global_git_config');
