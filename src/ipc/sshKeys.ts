import { ipc } from './client';

export interface GeneratedKey {
  privatePath: string;
  publicKey: string;
  fingerprint: string;
}

export interface SshKeyInfo {
  name: string;
  privatePath: string;
  publicPath: string | null;
  keyType: string | null;
  fingerprint: string | null;
  comment: string | null;
}

export const listKeys = () => ipc<SshKeyInfo[]>('list_keys');
export const generateKey = (params: { name: string; keyType: string; comment: string; passphrase: string | null }) =>
  ipc<GeneratedKey>('generate_key', params);
export const deleteKey = (name: string) => ipc<void>('delete_key', { name });
export const getPublicKey = (name: string) => ipc<string>('get_public_key', { name });
export const copyPublicKey = (name: string) => ipc<string>('copy_public_key_to_clipboard', { name });
