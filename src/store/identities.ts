import { create } from 'zustand';
import {
  Identity,
  listIdentities,
  createIdentity as apiCreate,
  updateIdentity as apiUpdate,
  deleteIdentity as apiDelete,
} from '../ipc/identities';

interface State {
  items: Identity[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  create: (i: Omit<Identity, 'id'>) => Promise<Identity>;
  update: (id: string, i: Identity) => Promise<void>;
  remove: (id: string, opts?: { deleteFiles?: boolean }) => Promise<void>;
}

export const useIdentitiesStore = create<State>((set) => ({
  items: [],
  loading: false,
  error: null,
  refresh: async () => {
    set({ loading: true, error: null });
    try {
      const items = await listIdentities();
      set({ items });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : String(error) });
      throw error;
    } finally {
      set({ loading: false });
    }
  },
  create: async (i) => {
    const created = await apiCreate(i);
    set((s) => ({ items: [...s.items, created] }));
    return created;
  },
  update: async (id, i) => {
    const updated = await apiUpdate(id, i);
    set((s) => ({ items: s.items.map((x) => (x.id === id ? updated : x)) }));
  },
  remove: async (id, opts) => {
    await apiDelete(id, opts);
    set((s) => ({ items: s.items.filter((x) => x.id !== id) }));
  },
}));


import { listKeys, type SshKeyInfo } from '../ipc/sshKeys';

export type { SshKeyInfo } from '../ipc/sshKeys';

interface KeysState {
  items: SshKeyInfo[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
}

/**
 * Centralised store for the registry of ssh_keys records.
 *
 * Earlier we kept this as local React state inside each view
 * and refreshed only on mount, which is exactly why the
 * Projects / IdentitySwitcher views briefly showed "未绑定 SSH
 * 密钥" right after the user had just imported a key in the
 * Identities view: the cached `keys` array was stale.
 *
 * Now both views, plus the IdentityForm dialog, read from the
 * same store. Call `refresh()` after any create / import /
 * delete so the rest of the UI sees the change.
 */
export const useKeysStore = create<KeysState>((set) => ({
  items: [],
  loading: false,
  error: null,
  refresh: async () => {
    set({ loading: true, error: null });
    try {
      const items = await listKeys();
      set({ items });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : String(error) });
    } finally {
      set({ loading: false });
    }
  },
}));
