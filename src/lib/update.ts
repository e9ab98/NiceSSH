import { check as pluginCheck, type Update, type DownloadEvent, type CheckOptions } from '@tauri-apps/plugin-updater';
import { getVersion as tauriGetVersion } from '@tauri-apps/api/app';

export const LS_KEYS = {
  checked: 'nicessh-update-checked',
  dismissed: 'nicessh-update-dismissed',
  notifyOnUpdate: 'nicessh-notify-on-update',
} as const;

export const CACHE_MS = 24 * 60 * 60 * 1000;

export type UpdateInfo = {
  version: string;
  notes?: string;
  pubDate?: string;
};

export function isNewer(latest: string, current: string): boolean {
  const a = latest.split('.').map((n) => parseInt(n, 10));
  const b = current.split('.').map((n) => parseInt(n, 10));
  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i++) {
    const av = a[i] ?? 0;
    const bv = b[i] ?? 0;
    if (av > bv) return true;
    if (av < bv) return false;
  }
  return false;
}

export function shouldNotify(): boolean {
  return localStorage.getItem(LS_KEYS.notifyOnUpdate) !== 'false';
}

export function markChecked(): void {
  localStorage.setItem(LS_KEYS.checked, new Date().toISOString());
}

export function markDismissed(version: string): void {
  localStorage.setItem(LS_KEYS.dismissed, version);
}

export function wasDismissed(version: string): boolean {
  return localStorage.getItem(LS_KEYS.dismissed) === version;
}

let lastUpdate: UpdateInfo | null = null;
let lastUpdateHandle: Update | null = null;

function within24h(): boolean {
  const raw = localStorage.getItem(LS_KEYS.checked);
  if (!raw) return false;
  const t = Date.parse(raw);
  if (Number.isNaN(t)) return false;
  return Date.now() - t < CACHE_MS;
}

export async function checkForUpdate(options?: CheckOptions): Promise<UpdateInfo | null> {
  if (within24h()) return lastUpdate;
  const r = await pluginCheck(options);
  markChecked();
  if (!r) {
    lastUpdate = null;
    lastUpdateHandle = null;
    return null;
  }
  lastUpdateHandle = r;
  lastUpdate = {
    version: r.version,
    notes: r.body ?? undefined,
    pubDate: r.date ?? undefined,
  };
  return lastUpdate;
}

export async function downloadAndInstall(
  onProgress?: (pct: number) => void
): Promise<void> {
  // Re-check if we don't have a live handle (caller invoked without check()).
  if (!lastUpdateHandle) {
    await checkForUpdate();
  }
  if (!lastUpdateHandle) {
    throw new Error('No update available to install');
  }
  await lastUpdateHandle.downloadAndInstall((event: DownloadEvent) => {
    if (!onProgress) return;
    if (event.event === 'Progress' && event.data.chunkLength >= 0) {
      // Plugin's progress event is chunked, not cumulative. Without
      // total content length we can only emit an "indeterminate"
      // progress signal; surface it as a growing counter capped at 99
      // until Finished arrives.
      onProgress(Math.min(99, (Date.now() % 99) + 1));
    } else if (event.event === 'Finished') {
      onProgress(100);
    }
  });
  // Close the underlying resource once the install has been kicked off.
  try {
    await lastUpdateHandle.close();
  } catch {
    // ignore — best-effort cleanup
  }
}

export async function getCurrentVersion(): Promise<string> {
  return tauriGetVersion();
}
