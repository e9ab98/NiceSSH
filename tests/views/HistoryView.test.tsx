import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { HistoryIndexEntry } from '../../src/ipc/history';

const { mockListHistory, mockRollback } = vi.hoisted(() => ({
  mockListHistory: vi.fn(),
  mockRollback: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, string | number>) => {
      if (!vars) return `[t:${key}]`;
      return `[t:${key}|${Object.entries(vars).map(([k, v]) => `${k}=${v}`).join(',')}]`;
    },
  }),
}));

vi.mock('sonner', () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn(), dismiss: vi.fn() }),
}));

vi.mock('../../src/ipc/history', () => ({
  listHistory: mockListHistory,
  rollback: mockRollback,
}));

import { HistoryView } from '../../src/views/HistoryView';

const sample = (over: Partial<HistoryIndexEntry>): HistoryIndexEntry => ({
  id: 'x',
  timestamp: '2026-01-01T00:00:00Z',
  operation: 'apply_identity_to_repo',
  summary: 'Applied identity Work',
  fileCount: 1,
  ...over,
});

beforeEach(() => {
  mockListHistory.mockReset();
  mockRollback.mockReset();
  // jsdom does not implement confirm() in older versions; stub it
  if (!window.confirm || (window.confirm as any).mockRestore) {
    window.confirm = vi.fn(() => true) as any;
  } else {
    window.confirm = vi.fn(() => true) as any;
  }
});

describe('HistoryView', () => {
  it('renders the empty state when backend returns no entries', async () => {
    mockListHistory.mockResolvedValue([]);
    render(<HistoryView />);
    await waitFor(() => expect(screen.getByText('[t:history.empty]')).toBeInTheDocument());
  });

  it('renders entries and the count chip', async () => {
    mockListHistory.mockResolvedValue([
      sample({ id: '1', summary: 'Applied identity Work' }),
      sample({ id: '2', operation: 'clean_repo_gitconfig', summary: 'Cleaned .git/config' }),
    ]);
    render(<HistoryView />);
    await waitFor(() => {
      expect(screen.getByText('Applied identity Work')).toBeInTheDocument();
      expect(screen.getByText('Cleaned .git/config')).toBeInTheDocument();
    });
    // "2 entries" plural — t shim returns "2 entries"
    expect(screen.getByText('[t:history.count|count=2]')).toBeInTheDocument();
  });

  it('filters by free-text query against summary', async () => {
    mockListHistory.mockResolvedValue([
      sample({ id: '1', summary: 'Applied identity Work' }),
      sample({ id: '2', operation: 'clean_repo_gitconfig', summary: 'Cleaned .git/config' }),
      sample({ id: '3', operation: 'upsert_github_host_block', summary: 'Added github.com Host' }),
    ]);
    render(<HistoryView />);
    await waitFor(() => screen.getByText('Applied identity Work'));
    const input = screen.getByPlaceholderText('[t:history.searchPlaceholder]') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'github' } });
    await waitFor(() => {
      expect(screen.queryByText('Applied identity Work')).toBeNull();
      expect(screen.getByText('Added github.com Host')).toBeInTheDocument();
    });
    expect(screen.getByText('[t:history.filteredCount|shown=1,total=3]')).toBeInTheDocument();
  });

  it('filters by group chip and shows noMatches when nothing matches', async () => {
    mockListHistory.mockResolvedValue([
      sample({ id: '1', summary: 'Applied identity Work' }),
      sample({ id: '2', operation: 'clean_repo_gitconfig', summary: 'Cleaned .git/config' }),
    ]);
    render(<HistoryView />);
    await waitFor(() => screen.getByText('Applied identity Work'));
    fireEvent.click(screen.getByRole('button', { name: '[t:history.filterClean]' }));
    await waitFor(() => {
      expect(screen.queryByText('Applied identity Work')).toBeNull();
      expect(screen.getByText('Cleaned .git/config')).toBeInTheDocument();
    });
    // Now switch to a group with no entries
    fireEvent.click(screen.getByRole('button', { name: '[t:history.filterSettings]' }));
    await waitFor(() => {
      expect(screen.getByText('[t:history.noMatches]')).toBeInTheDocument();
    });
    // Switch back to All
    fireEvent.click(screen.getByRole('button', { name: '[t:history.filterAll]' }));
    await waitFor(() => {
      expect(screen.getByText('Applied identity Work')).toBeInTheDocument();
    });
  });

  it('calls rollback when the Revert button is clicked and confirm() returns true', async () => {
    mockListHistory.mockResolvedValue([
      sample({ id: 'abc', summary: 'Applied identity Work' }),
    ]);
    mockRollback.mockResolvedValue(undefined);
    render(<HistoryView />);
    await waitFor(() => screen.getByText('Applied identity Work'));
    fireEvent.click(screen.getByRole('button', { name: '[t:history.revert]' }));
    await waitFor(() => {
      expect(mockRollback).toHaveBeenCalledWith('abc');
    });
  });
});
