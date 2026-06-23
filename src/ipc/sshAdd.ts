import { ipc } from './client';

export const tryUnlockKey = async (keyPath: string, passphrase: string): Promise<boolean> => {
  try {
    return await ipc<boolean>('ssh_add_test', { keyPath, passphrase });
  } catch {
    return false;
  }
};

// Probe whether the key is passphrase-protected. If `false`, the caller
// can skip the PassphraseDialog and call `tryUnlockKey` with an empty
// passphrase (ssh-add will accept unencrypted keys outright).
export const isKeyEncrypted = async (keyPath: string): Promise<boolean> => {
  try {
    return await ipc<boolean>('is_key_encrypted', { keyPath });
  } catch {
    // On probe failure, assume encrypted so we still prompt — safer than
    // silently skipping the dialog.
    return true;
  }
};
