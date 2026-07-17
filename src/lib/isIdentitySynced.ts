/**
 * Decides whether the identity the user sees in the "Git Identity"
 * panel matches what the repo's `.git/config` actually contains.
 *
 * The two views are intentionally separate:
 *   - Identity panel: what NiceSSH *thinks* this project is bound to
 *   - Git Config: what Git itself will use to commit
 *
 * When the two disagree the user is about to commit under a name
 * or email they don't expect. Surface that with a warning so they
 * can re-bind.
 *
 * Returns:
 *   - `true` when in sync (or nothing meaningful to compare)
 *   - `false` when identity disagrees with repoConfig
 *   - `null` when the input state isn't applicable (non-tracked
 *     kinds, or repoConfig not yet loaded) — caller should NOT
 *     show a warning.
 *
 * See: docs/superpowers/specs/2026-07-16-import-project-global-default-fallback-designs.md
 */

import type { Identity } from '../ipc/identities';

type RepoConfigSnapshot = {
  userName?: string | null;
  userEmail?: string | null;
};

type DetectedState =
  | { kind: 'none' }
  | { kind: 'user-only' }
  | { kind: 'untracked'; keyPath: string }
  | { kind: 'tracked'; identity: Identity; source: 'config' | 'git' };

const norm = (s: string | null | undefined): string =>
  typeof s === 'string' ? s.trim() : '';

export function isIdentitySynced(
  detected: DetectedState,
  repoConfig: RepoConfigSnapshot | null,
): boolean | null {
  // Only the `tracked` state has an identity we can compare against
  // the repo's effective config. For every other kind, the user
  // already has a different signal (badge, empty state) — no
  // "out of sync" warning needed.
  if (detected.kind !== 'tracked') return null;
  if (!repoConfig) return null;

  const idName = norm(detected.identity.userName);
  const idEmail = norm(detected.identity.userEmail);
  const repoName = norm(repoConfig.userName);
  const repoEmail = norm(repoConfig.userEmail);

  // If the repo has neither name nor email, we can't claim a
  // mismatch — git just hasn't been told anything yet. Treat as
  // "no opinion" and skip the warning.
  if (!repoName && !repoEmail) return true;

  return idName === repoName && idEmail === repoEmail;
}
