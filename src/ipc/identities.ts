import { ipc } from './client';

/// What kind of signing key an Identity.signingKeyId refers to.
/// `ssh` reuses the existing SSH keychain (no new secrets to manage);
/// `gpg` is opt-in for users with a pre-existing GPG workflow.
export type SigningKeyKind = 'ssh' | 'gpg';

export interface Identity {
  id: string;
  label: string;
  userName: string;
  userEmail: string;
  sshKeyId: string | null;
  matchPath: string | null;
  hostAlias: string | null;
  gitHost: string | null;
  // ── v3: commit signing config ────────────────────────────────
  /// When true, commits authored under this identity should be signed.
  /// Drives `[commit] gpgsign = true` in the per-identity sub-gitconfig.
  requireSignedCommits: boolean;
  /// Which signing key to use. `null` means "don't sign".
  signingKeyId: string | null;
  signingKeyKind: SigningKeyKind;
}

export const listIdentities = () => ipc<Identity[]>('list_identities');
// v3: payload shape changed from flat args to `{ input: ... }` to
// match the new struct-param `create_identity` Tauri command. The
// frontend wraps the full `Omit<Identity, 'id'>` under `input` so
// future field additions don't require touching the IPC signature.
export const createIdentity = (i: Omit<Identity, 'id'>) =>
  ipc<Identity>('create_identity', { input: i });
// update_identity has always taken a struct param (`{id, updated}`);
// v3 just narrows `updated` to the same `Omit<Identity, 'id'>` shape
// that `create_identity` uses, removing the duplicate type.
export const updateIdentity = (id: string, updated: Omit<Identity, 'id'>) =>
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
