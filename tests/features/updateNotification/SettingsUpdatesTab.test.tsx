import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';

const { mockCheck, mockUpdate } = vi.hoisted(() => {
  const mockUpdate = {
    version: '0.1.42',
    currentVersion: '0.1.13',
    date: '',
    body: '',
    rawJson: {},
    downloadAndInstall: vi.fn(),
    close: vi.fn(),
  };
  return { mockCheck: vi.fn(), mockUpdate: mockUpdate as any };
});

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

vi.mock('@tauri-apps/plugin-updater', () => ({
  check: mockCheck,
}));

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('0.1.13'),
}));

import { SettingsUpdatesTab } from '../../../src/features/updateNotification/SettingsUpdatesTab';
import { LS_KEYS } from '../../../src/lib/update';

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  mockCheck.mockReset();
  mockUpdate.downloadAndInstall.mockReset();
  mockUpdate.close.mockReset();
  mockUpdate.downloadAndInstall.mockResolvedValue(undefined);
  mockUpdate.close.mockResolvedValue(undefined);
  mockCheck.mockResolvedValue(null);
  
  // Mock global fetch
  global.fetch = vi.fn().mockResolvedValue({
    ok: true,
    json: () => Promise.resolve({ body: '## Test Changelog\n- Fix bug' }),
  } as any);
});

describe('SettingsUpdatesTab', () => {
  it('shows current version from getCurrentVersion', async () => {
    render(<SettingsUpdatesTab />);
    await waitFor(() =>
      expect(screen.getByText('[t:settings.updates.currentValue|version=0.1.13]')).toBeInTheDocument()
    );
  });

  it('shows "Not yet checked" before any check has run', async () => {
    render(<SettingsUpdatesTab />);
    await waitFor(() =>
      expect(screen.getByText('[t:settings.updates.notChecked]')).toBeInTheDocument()
    );
  });

  it('Check now triggers checkForUpdate bypassing the cache', async () => {
    localStorage.setItem(LS_KEYS.checked, new Date().toISOString());
    render(<SettingsUpdatesTab />);
    fireEvent.click(screen.getByText('[t:settings.updates.check]'));
    await waitFor(() => expect(mockCheck).toHaveBeenCalledTimes(1));
  });

  it('shows "Up to date" when check returns null', async () => {
    mockCheck.mockResolvedValue(null);
    render(<SettingsUpdatesTab />);
    fireEvent.click(screen.getByText('[t:settings.updates.check]'));
    await waitFor(() =>
      expect(screen.getByText('[t:settings.updates.upToDate]')).toBeInTheDocument()
    );
  });

  it('Update now button is hidden when no update is available', async () => {
    mockCheck.mockResolvedValue(null);
    render(<SettingsUpdatesTab />);
    fireEvent.click(screen.getByText('[t:settings.updates.check]'));
    await waitFor(() =>
      expect(screen.queryByText('[t:settings.updates.update]')).not.toBeInTheDocument()
    );
  });

  it('Update now button appears when check returns a newer version', async () => {
    mockCheck.mockResolvedValue(mockUpdate);
    render(<SettingsUpdatesTab />);
    fireEvent.click(screen.getByText('[t:settings.updates.check]'));
    await waitFor(() =>
      expect(screen.getByText('[t:settings.updates.update]')).toBeInTheDocument()
    );
  });

  it('notify toggle writes to localStorage on change', () => {
    render(<SettingsUpdatesTab />);
    const toggle = screen.getByRole('checkbox');
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    expect(localStorage.getItem(LS_KEYS.notifyOnUpdate)).toBe('false');
  });
});
