import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

export async function ipc<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    const message = typeof e === 'object' && e && 'message' in e
      ? String((e as any).message)
      : String(e);
    toast.error(`${cmd} failed: ${message}`);
    throw e;
  }
}
