import { ipc } from './client';

export interface Identity {
  id: string;
  label: string;
  userName: string;
  userEmail: string;
  sshKeyId: string | null;
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

export interface ScannedProvenanceSource {
  kind: ScannedProvenanceKind;
  detail: string;
}

export interface ScannedProvenance {
  /** All sources backing this candidate (>=1). After the
   *  cross-source dedup pass a label found by both the gitconfig
   *  scanner AND the ssh-orphan scanner will list both. */
  sources: ScannedProvenanceSource[];
  /** Back-compat single-source view. When the same label is
   *  found by multiple scanners, this resolves to whichever
   *  source the backend prefers (gitconfig currently). */
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
