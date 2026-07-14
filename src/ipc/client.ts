import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

export interface IpcOptions {
  /// When true, the wrapper does NOT emit a `toast.error` on
  /// failure. The caller is expected to catch the rejection and
  /// surface a domain-specific message (e.g. CommitDialog turns
  /// `nothing to commit` into a softer info toast and rethrows).
  /// The error is still rethrown so the caller's `catch` block
  /// runs as normal.
  silent?: boolean;
}

export async function ipc<T>(cmd: string, args?: Record<string, unknown>, options?: IpcOptions): Promise<T> {
  const silent = options?.silent ?? false;
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    if (!silent) {
      const message = typeof e === 'object' && e && 'message' in e
        ? String((e as any).message)
        : String(e);
      toast.error(`${cmd} failed: ${message}`);
    }
    throw e;
  }
}
