import { ipc } from './client';

export interface Identity {
  id: string;
  label: string;
  userName: string;
  userEmail: string;
  keyPath: string;
  matchPath: string | null;
  hostAlias: string | null;
  gitHost: string | null;
}

export const listIdentities = () => ipc<Identity[]>('list_identities');
export const createIdentity = (i: Omit<Identity, 'id'>) =>
  ipc<Identity>('create_identity', i as any);
export const updateIdentity = (id: string, updated: Identity) =>
  ipc<Identity>('update_identity', { id, updated });
export const deleteIdentity = (
  id: string,
  opts: { deleteFiles?: boolean } = {},
) => ipc<void>('delete_identity', { id, deleteFiles: opts.deleteFiles ?? false });

export type ScannedProvenanceKind = 'gitconfig_include_if' | 'ssh_key_orphan';

export interface ScannedProvenance {
  kind: ScannedProvenanceKind;
  detail: string;
}

export interface ScannedIdentity {
  label: string;
  userName: string | null;
  userEmail: string | null;
  keyPath: string | null;
  matchPath: string | null;
  conflictsWithExisting: boolean;
  conflictsWithExistingKey: boolean;
  provenance: ScannedProvenance;
}

export const scanExistingIdentities = () =>
  ipc<ScannedIdentity[]>('scan_existing_identities');
