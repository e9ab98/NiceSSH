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
  return {
    mockCheck: vi.fn(),
    mockUpdate: mockUpdate as any,
  };
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
  toast: Object.assign(vi.fn(), { dismiss: vi.fn(), error: vi.fn() }),
}));

vi.mock('@tauri-apps/plugin-updater', () => ({
  check: mockCheck,
}));

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('0.1.13'),
}));

import { toast } from 'sonner';
import { UpdateToast } from '../../../src/features/updateNotification/UpdateToast';
import { LS_KEYS } from '../../../src/lib/update';

const mockedToast = vi.mocked(toast);

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  mockCheck.mockReset();
  mockUpdate.downloadAndInstall.mockReset();
  mockUpdate.close.mockReset();
  mockUpdate.downloadAndInstall.mockResolvedValue(undefined);
  mockUpdate.close.mockResolvedValue(undefined);
  // Pre-seed check() with a valid Update so downloadAndInstall in
  // src/lib/update.ts has a live handle to work with.
  mockCheck.mockResolvedValue(mockUpdate);
  mockedToast.dismiss.mockReset();
  mockedToast.error.mockReset();
});

describe('UpdateToast', () => {
  it('renders the title with the version substituted', () => {
    render(<UpdateToast version="0.1.42" />);
    expect(screen.getByText('[t:update.toast.title|version=0.1.42]')).toBeInTheDocument();
  });

  it('Update now calls downloadAndInstall and shows the restart button on completion', async () => {
    render(<UpdateToast version="0.1.42" />);
    fireEvent.click(screen.getByText('[t:update.toast.update]'));
    await waitFor(() => expect(mockUpdate.downloadAndInstall).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(screen.getByText('[t:update.toast.restart]')).toBeInTheDocument()
    );
  });

  it('Later dismisses the toast and marks the version', () => {
    render(<UpdateToast version="0.1.42" />);
    fireEvent.click(screen.getByText('[t:update.toast.later]'));
    expect(localStorage.getItem(LS_KEYS.dismissed)).toBe('0.1.42');
    expect(mockedToast.dismiss).toHaveBeenCalledWith('nicessh-update');
  });

  it('X button dismisses the toast and marks the version', () => {
    render(<UpdateToast version="0.1.42" />);
    fireEvent.click(screen.getByTestId('update-toast-close'));
    expect(localStorage.getItem(LS_KEYS.dismissed)).toBe('0.1.42');
    expect(mockedToast.dismiss).toHaveBeenCalledWith('nicessh-update');
  });

  it('Restart button dismisses the toast (no @tauri-apps/plugin-process dep needed)', async () => {
    render(<UpdateToast version="0.1.42" />);
    fireEvent.click(screen.getByText('[t:update.toast.update]'));
    await waitFor(() =>
      expect(screen.getByText('[t:update.toast.restart]')).toBeInTheDocument()
    );
    fireEvent.click(screen.getByText('[t:update.toast.restart]'));
    expect(mockedToast.dismiss).toHaveBeenCalledWith('nicessh-update');
  });

  it('download failure shows toast.error and re-enables the update button', async () => {
    mockUpdate.downloadAndInstall.mockRejectedValue(new Error('network down'));
    render(<UpdateToast version="0.1.42" />);
    fireEvent.click(screen.getByText('[t:update.toast.update]'));
    await waitFor(() =>
      expect(mockedToast.error).toHaveBeenCalledWith('[t:update.toast.failed]')
    );
    await new Promise((r) => setTimeout(r, 250));
    expect(screen.getByText('[t:update.toast.update]')).toBeEnabled();
  });
});
