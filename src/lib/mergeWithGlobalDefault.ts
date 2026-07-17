/**
 * Fills the missing sides of a `RepoGitConfig.userName` /
 * `userEmail` pair from NiceSSH's recorded global default
 * identity. Pure: no I/O, no IPC, no side effects.
 *
 * Why this exists:
 *   - The NiceSSH "global default identity" is a UI-level concept
 *     stored in `cfg.globalDefaultIdentityId`. It points at one
 *     of the user's identities.
 *   - `get_repo_git_config` only knows about git's own config
 *     chain (`~/.gitconfig` + `includeIf`). It does NOT know
 *     about NiceSSH's UI-level pointer.
 *   - When a project has no `[user]` in its own `.git/config`
 *     AND no `[user]` anywhere in git's chain, we want the
 *     right-hand detail panel to still show "this is the name
 *     and email NiceSSH will use if you commit here" — i.e.
 *     the values from the global default identity.
 *
 * Rules:
 *   - For each side (name, email) that is `None` in the input,
 *     if the global default has a value, fill it in and tag the
 *     source as `'globalDefault'`.
 *   - Never overwrite a value already present in `repoConfig`,
 *     regardless of its current source.
 *   - When `globalDefaultId` is null OR the identity cannot be
 *     resolved, return the input unchanged.
 *   - When ALL sides are already filled, return the input
 *     unchanged (no-op fast path).
 *
 * See: docs/superpowers/specs/2026-07-16-import-project-global-default-fallback-designs.md
 */

import type { IdentitySource } from '../ipc/git';

export interface RepoConfigSubset {
  userName: string | null;
  userEmail: string | null;
  userNameSource: IdentitySource;
  userEmailSource: IdentitySource;
}

export interface GlobalDefaultInput {
  /// The id NiceSSH has recorded as the global default, or null.
  globalDefaultId: string | null;
  /// The resolved identity, or null if `globalDefaultId` is
  /// null OR the id is not in the identities list.
  globalDefault: { userName: string; userEmail: string } | null;
}

const norm = (s: string | null | undefined): string =>
  typeof s === 'string' && s.length > 0 ? s : '';

export function mergeWithGlobalDefault(
  repoConfig: RepoConfigSubset,
  input: GlobalDefaultInput,
): RepoConfigSubset {
  // Fast path: nothing to fill.
  if (repoConfig.userName && repoConfig.userEmail) {
    return repoConfig;
  }
  if (!input.globalDefault) {
    return repoConfig;
  }

  const next: RepoConfigSubset = { ...repoConfig };
  if (!repoConfig.userName) {
    const v = norm(input.globalDefault.userName);
    if (v) {
      next.userName = input.globalDefault.userName;
      next.userNameSource = 'globalDefault';
    }
  }
  if (!repoConfig.userEmail) {
    const v = norm(input.globalDefault.userEmail);
    if (v) {
      next.userEmail = input.globalDefault.userEmail;
      next.userEmailSource = 'globalDefault';
    }
  }
  return next;
}
