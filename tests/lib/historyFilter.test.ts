import { describe, it, expect } from 'vitest';
import { filterEntries, groupOf, FILTER_GROUPS } from '../../src/lib/historyFilter';
import type { HistoryIndexEntry } from '../../src/ipc/history';

const entry = (over: Partial<HistoryIndexEntry>): HistoryIndexEntry => ({
  id: 'x',
  timestamp: '2026-01-01T00:00:00Z',
  operation: 'apply_identity_to_repo',
  summary: 'Applied identity Work to repo /Users/x/proj',
  fileCount: 1,
  ...over,
});

describe('groupOf', () => {
  it('maps apply_identity_to_repo* to apply', () => {
    expect(groupOf('apply_identity_to_repo')).toBe('apply');
    expect(groupOf('apply_identity_to_repo_user_only')).toBe('apply');
  });
  it('maps git_config_* ops to gitconfig', () => {
    expect(groupOf('git_config_append_include')).toBe('gitconfig');
    expect(groupOf('git_config_write_identity')).toBe('gitconfig');
    expect(groupOf('git_config_write_identity_user_only')).toBe('gitconfig');
  });
  it('maps clean_repo_gitconfig to clean', () => {
    expect(groupOf('clean_repo_gitconfig')).toBe('clean');
  });
  it('maps write_repo_remote and set_global_git_config to repo', () => {
    expect(groupOf('write_repo_remote')).toBe('repo');
    expect(groupOf('set_global_git_config')).toBe('repo');
  });
  it('maps ssh config block ops to sshConfig', () => {
    expect(groupOf('upsert_github_host_block')).toBe('sshConfig');
    expect(groupOf('add_managed_host_block')).toBe('sshConfig');
    expect(groupOf('update_managed_host_block')).toBe('sshConfig');
    expect(groupOf('delete_managed_host_block')).toBe('sshConfig');
  });
  it('maps edit/reset/clear to settings', () => {
    expect(groupOf('edit')).toBe('settings');
    expect(groupOf('reset')).toBe('settings');
    expect(groupOf('clear')).toBe('settings');
  });
  it('falls back to all for unknown operations', () => {
    expect(groupOf('totally_made_up_op')).toBe('all');
  });
});

describe('filterEntries', () => {
  const data: HistoryIndexEntry[] = [
    entry({ id: 'a', operation: 'apply_identity_to_repo', summary: 'Applied identity Work' }),
    entry({ id: 'b', operation: 'clean_repo_gitconfig', summary: 'Cleaned .git/config' }),
    entry({ id: 'c', operation: 'upsert_github_host_block', summary: 'Added github.com Host' }),
    entry({ id: 'd', operation: 'set_global_git_config', summary: 'Set global default' }),
  ];

  it('returns all entries when query and group are both defaults', () => {
    expect(filterEntries(data, '', 'all')).toEqual(data);
  });

  it('trims and lowercases the query', () => {
    const out = filterEntries(data, '  WORK  ', 'all');
    expect(out.map((e) => e.id)).toEqual(['a']);
  });

  it('filters by group only', () => {
    const out = filterEntries(data, '', 'sshConfig');
    expect(out.map((e) => e.id)).toEqual(['c']);
  });

  it('combines query and group (AND)', () => {
    // "git" matches b (clean .git/config) and c (github.com Host), but not a or d
    const all = filterEntries(data, 'git', 'all');
    expect(all.map((e) => e.id).sort()).toEqual(['b', 'c'].sort());
    // group=repo + query "global" should match only d
    const both = filterEntries(data, 'global', 'repo');
    expect(both.map((e) => e.id)).toEqual(['d']);
    // group=apply + query that matches "Work" should return only a
    const narrowed = filterEntries(data, 'Work', 'apply');
    expect(narrowed.map((e) => e.id)).toEqual(['a']);
  });

  it('returns empty list when nothing matches', () => {
    expect(filterEntries(data, 'no-such-text', 'all')).toEqual([]);
  });

  it('preserves order from the input (newest first)', () => {
    const out = filterEntries(data, '', 'all');
    expect(out.map((e) => e.id)).toEqual(['a', 'b', 'c', 'd']);
  });

  it('handles empty entries list', () => {
    expect(filterEntries([], 'x', 'apply')).toEqual([]);
  });

  it('whitespace-only query is treated as no query', () => {
    const out = filterEntries(data, '   ', 'repo');
    expect(out.map((e) => e.id)).toEqual(['d']);
  });
});

describe('FILTER_GROUPS', () => {
  it('lists all groups in display order', () => {
    expect(FILTER_GROUPS).toEqual(['all', 'apply', 'gitconfig', 'clean', 'repo', 'sshConfig', 'settings']);
  });
});
