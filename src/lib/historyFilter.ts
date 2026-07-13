/**
 * Pure filter + group helpers for the history view.
 *
 * Kept separate from `views/HistoryView.tsx` so the filter logic is
 * unit-testable without rendering React, and so the mapping from
 * `operation` strings to user-facing group labels lives in one place.
 *
 * `commit_change(operation, ...)` is called from many places with
 * different operation strings. The view does not care about every
 * exact string — it cares about the user's mental category. We map
 * raw operations to a small fixed set of `FilterGroup` values.
 */

export type HistoryIndexEntryLike = {
  operation: string;
  summary: string;
};

export type FilterGroup = 'all' | 'apply' | 'gitconfig' | 'clean' | 'repo' | 'sshConfig' | 'settings';

export const FILTER_GROUPS: FilterGroup[] = [
  'all',
  'apply',
  'gitconfig',
  'clean',
  'repo',
  'sshConfig',
  'settings',
];

/**
 * Map a raw `operation` string (whatever the Rust backend stored in
 * `history::commit_change`) to a user-facing filter group. New
 * operations added on the backend should extend the right branch
 * here; unknown operations fall through to 'all' so they remain
 * visible rather than silently disappearing.
 */
export function groupOf(operation: string): FilterGroup {
  if (
    operation === 'apply_identity_to_repo' ||
    operation === 'apply_identity_to_repo_user_only'
  ) {
    return 'apply';
  }
  if (
    operation === 'git_config_append_include' ||
    operation === 'git_config_write_identity' ||
    operation === 'git_config_write_identity_user_only'
  ) {
    return 'gitconfig';
  }
  if (operation === 'clean_repo_gitconfig') {
    return 'clean';
  }
  if (operation === 'write_repo_remote' || operation === 'set_global_git_config') {
    return 'repo';
  }
  if (
    operation === 'upsert_github_host_block' ||
    operation === 'add_managed_host_block' ||
    operation === 'update_managed_host_block' ||
    operation === 'delete_managed_host_block'
  ) {
    return 'sshConfig';
  }
  if (operation === 'edit' || operation === 'reset' || operation === 'clear') {
    return 'settings';
  }
  return 'all';
}

/**
 * Filter a list of history entries by a free-text query (matched
 * against `summary`, case-insensitive) and a coarse group filter.
 *
 * Empty / whitespace-only query means "no text filter". A group
 * other than 'all' means "entries whose operation maps to that
 * group".
 *
 * Order is preserved (the backend already returns newest-first).
 */
export function filterEntries<T extends HistoryIndexEntryLike>(
  entries: T[],
  query: string,
  group: FilterGroup,
): T[] {
  const q = query.trim().toLowerCase();
  return entries.filter((e) => {
    if (group !== 'all' && groupOf(e.operation) !== group) {
      return false;
    }
    if (q.length > 0 && !e.summary.toLowerCase().includes(q)) {
      return false;
    }
    return true;
  });
}
