import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor } from '@testing-library/react';

const { mockCheck } = vi.hoisted(() => ({ mockCheck: vi.fn() }));

vi.mock('@tauri-apps/plugin-updater', () => ({
  check: mockCheck,
}));

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('0.1.13'),
}));

vi.mock('sonner', () => ({
  toast: vi.fn(),
  Toaster: () => null,
}));

// Mock UpdateToast so we don't pull in its DOM structure for this test.
vi.mock('../../src/features/updateNotification/UpdateToast', () => ({
  UpdateToast: () => null,
}));

import { toast } from 'sonner';
import { useUpdateCheck } from '../../src/hooks/useUpdateCheck';
import { LS_KEYS } from '../../src/lib/update';

function Probe() {
  useUpdateCheck();
  return null;
}

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  mockCheck.mockReset();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('useUpdateCheck', () => {
  it('does nothing when notify is off', async () => {
    localStorage.setItem(LS_KEYS.notifyOnUpdate, 'false');
    render(<Probe />);
    await vi.runOnlyPendingTimersAsync();
    expect(mockCheck).not.toHaveBeenCalled();
    expect(toast).not.toHaveBeenCalled();
  });

  it('skips check when cache is fresh', async () => {
    localStorage.setItem(LS_KEYS.checked, new Date().toISOString());
    render(<Probe />);
    await vi.runOnlyPendingTimersAsync();
    expect(mockCheck).not.toHaveBeenCalled();
    expect(toast).not.toHaveBeenCalled();
  });

  it('calls check and toasts when an update is found and not dismissed', async () => {
    mockCheck.mockResolvedValue({
      version: '0.1.42',
      currentVersion: '0.1.13',
      date: '2026-06-18T00:00:00Z',
      body: 'Bug fixes',
      rawJson: {},
      downloadAndInstall: vi.fn(),
      close: vi.fn(),
    } as any);
    render(<Probe />);
    await vi.advanceTimersByTimeAsync(5000);
    expect(mockCheck).toHaveBeenCalledTimes(1);
    expect(toast).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ id: 'nicessh-update', duration: Infinity })
    );
  });

  it('does not toast when the version was dismissed', async () => {
    localStorage.setItem(LS_KEYS.dismissed, '0.1.42');
    mockCheck.mockResolvedValue({
      version: '0.1.42',
      currentVersion: '0.1.13',
      date: '',
      body: '',
      rawJson: {},
      downloadAndInstall: vi.fn(),
      close: vi.fn(),
    } as any);
    render(<Probe />);
    await vi.advanceTimersByTimeAsync(5000);
    expect(mockCheck).toHaveBeenCalledTimes(1);
    expect(toast).not.toHaveBeenCalled();
  });

  it('does not toast when check returns null', async () => {
    mockCheck.mockResolvedValue(null);
    render(<Probe />);
    await vi.advanceTimersByTimeAsync(5000);
    expect(mockCheck).toHaveBeenCalledTimes(1);
    expect(toast).not.toHaveBeenCalled();
  });
});
