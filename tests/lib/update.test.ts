import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { Update, DownloadEvent } from '@tauri-apps/plugin-updater';

// Hoist the mock factory so vi.mock can reference it before imports are evaluated.
const { mockCheck, mockUpdate } = vi.hoisted(() => {
  const mockUpdate = {
    version: '0.1.42',
    currentVersion: '0.1.13',
    date: '2026-06-18T00:00:00Z',
    body: 'Bug fixes',
    rawJson: {},
    downloadAndInstall: vi.fn(),
    close: vi.fn(),
  };
  return {
    mockCheck: vi.fn(),
    mockUpdate: mockUpdate as unknown as Update & typeof mockUpdate,
  };
});

vi.mock('@tauri-apps/plugin-updater', () => ({
  check: mockCheck,
}));

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('0.1.13'),
}));

import { getVersion } from '@tauri-apps/api/app';
import {
  checkForUpdate,
  downloadAndInstall,
  getCurrentVersion,
  isNewer,
  shouldNotify,
  markChecked,
  markDismissed,
  wasDismissed,
  LS_KEYS,
  CACHE_MS,
} from '../../src/lib/update';

const mockedGetVersion = vi.mocked(getVersion);

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  vi.useRealTimers();
  mockUpdate.downloadAndInstall.mockReset();
  mockUpdate.close.mockReset();
  mockUpdate.version = '0.1.42';
  mockUpdate.currentVersion = '0.1.13';
  mockUpdate.date = '2026-06-18T00:00:00Z';
  mockUpdate.body = 'Bug fixes';
  mockUpdate.downloadAndInstall.mockResolvedValue(undefined);
  mockUpdate.close.mockResolvedValue(undefined);
  mockCheck.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('isNewer', () => {
  it('returns true when latest is a higher patch', () => {
    expect(isNewer('0.1.42', '0.1.13')).toBe(true);
  });
  it('returns true when latest is a higher minor', () => {
    expect(isNewer('0.2.0', '0.1.99')).toBe(true);
  });
  it('returns true when latest is a higher major', () => {
    expect(isNewer('1.0.0', '0.99.99')).toBe(true);
  });
  it('returns false when versions are equal', () => {
    expect(isNewer('0.1.13', '0.1.13')).toBe(false);
  });
  it('returns false when latest is older', () => {
    expect(isNewer('0.1.10', '0.1.13')).toBe(false);
  });
});

describe('LS_KEYS', () => {
  it('exposes the three storage keys', () => {
    expect(LS_KEYS.checked).toBe('nicessh-update-checked');
    expect(LS_KEYS.dismissed).toBe('nicessh-update-dismissed');
    expect(LS_KEYS.notifyOnUpdate).toBe('nicessh-notify-on-update');
  });
});

describe('shouldNotify', () => {
  it('returns true when localStorage is empty', () => {
    expect(shouldNotify()).toBe(true);
  });
  it('returns true when value is "true"', () => {
    localStorage.setItem(LS_KEYS.notifyOnUpdate, 'true');
    expect(shouldNotify()).toBe(true);
  });
  it('returns false when value is "false"', () => {
    localStorage.setItem(LS_KEYS.notifyOnUpdate, 'false');
    expect(shouldNotify()).toBe(false);
  });
});

describe('markChecked / cache', () => {
  it('writes an ISO timestamp to LS_KEYS.checked', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-06-18T12:00:00Z'));
    markChecked();
    const v = localStorage.getItem(LS_KEYS.checked);
    expect(v).toBe(new Date('2026-06-18T12:00:00Z').toISOString());
  });
});

describe('markDismissed / wasDismissed', () => {
  it('round-trips a version string', () => {
    markDismissed('0.1.42');
    expect(wasDismissed('0.1.42')).toBe(true);
    expect(wasDismissed('0.1.43')).toBe(false);
  });
});

describe('checkForUpdate', () => {
  it('skips the plugin call when cache is fresh', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-06-18T12:00:00Z'));
    markChecked();
    vi.setSystemTime(new Date('2026-06-18T13:00:00Z')); // 1h later, within 24h
    const r = await checkForUpdate();
    expect(r).toBeNull();
    expect(mockCheck).not.toHaveBeenCalled();
  });

  it('calls the plugin when cache is stale (24h+1)', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-06-18T12:00:00Z'));
    markChecked();
    vi.setSystemTime(new Date('2026-06-19T12:00:01Z')); // 24h + 1ms
    mockCheck.mockResolvedValue(mockUpdate);
    const r = await checkForUpdate();
    expect(mockCheck).toHaveBeenCalledTimes(1);
    expect(r).toEqual({
      version: '0.1.42',
      notes: 'Bug fixes',
      pubDate: '2026-06-18T00:00:00Z',
    });
  });

  it('returns null when the plugin reports no update', async () => {
    mockCheck.mockResolvedValue(null);
    const r = await checkForUpdate();
    expect(r).toBeNull();
  });

  it('writes a fresh ISO timestamp to LS_KEYS.checked after a real check', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-06-18T12:00:00Z'));
    mockCheck.mockResolvedValue(null);
    await checkForUpdate();
    expect(localStorage.getItem(LS_KEYS.checked)).toBe(
      new Date('2026-06-18T12:00:00Z').toISOString()
    );
  });
});

describe('downloadAndInstall', () => {
  it('calls the live Update instance downloadAndInstall and closes the handle', async () => {
    mockCheck.mockResolvedValue(mockUpdate);
    await checkForUpdate(); // populates lastUpdateHandle
    const onProgress = vi.fn();
    await downloadAndInstall(onProgress);
    expect(mockUpdate.downloadAndInstall).toHaveBeenCalledTimes(1);
    expect(mockUpdate.close).toHaveBeenCalledTimes(1);
  });

  it('emits a progress callback when the plugin fires Progress and Finished events', async () => {
    mockCheck.mockResolvedValue(mockUpdate);
    await checkForUpdate();
    const onProgress = vi.fn();
    // Capture the event callback the wrapper passed to the plugin.
    let capturedCb: ((e: DownloadEvent) => void) | undefined;
    mockUpdate.downloadAndInstall.mockImplementation(async (cb) => {
      capturedCb = cb;
    });
    await downloadAndInstall(onProgress);
    expect(capturedCb).toBeDefined();
    capturedCb!({ event: 'Progress', data: { chunkLength: 1024 } });
    capturedCb!({ event: 'Finished' });
    // 99 cap for Progress + 100 for Finished = 2 calls.
    expect(onProgress.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(onProgress).toHaveBeenLastCalledWith(100);
  });

  it('throws when no update is available and no live handle exists', async () => {
    mockCheck.mockResolvedValue(null);
    await checkForUpdate();
    await expect(downloadAndInstall()).rejects.toThrow(/no update available/i);
  });
});

describe('getCurrentVersion', () => {
  it('returns the version from @tauri-apps/api/app', async () => {
    mockedGetVersion.mockResolvedValue('0.1.13');
    const v = await getCurrentVersion();
    expect(v).toBe('0.1.13');
  });
});

describe('CACHE_MS', () => {
  it('is 24 hours in milliseconds', () => {
    expect(CACHE_MS).toBe(24 * 60 * 60 * 1000);
  });
});
