import { create } from 'zustand';
import {
  User,
  UserInput,
  listUsers,
  createUser as apiCreate,
  updateUser as apiUpdate,
  deleteUser as apiDelete,
  importUsersFromGitconfig as apiImport,
} from '../ipc/users';
import { refreshBus } from '../lib/refreshBus';

interface State {
  items: User[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  create: (i: UserInput) => Promise<User>;
  update: (id: string, i: UserInput) => Promise<User>;
  remove: (id: string) => Promise<void>;
  importFromGitconfig: () => Promise<User[]>;
}

/**
 * Centralised store for the git-user pool. Mirrors the shape of
 * `useIdentitiesStore` so a single AppShell refresh call covers
 * both. Mutations emit `refreshBus` events so other views
 * (Identities tab — which reads identity.userName/userEmail
 * which may have just been updated — and the scan dialog — which
 * sources its combobox from this store) can react.
 */
export const useUsersStore = create<State>((set) => ({
  items: [],
  loading: false,
  error: null,
  refresh: async () => {
    set({ loading: true, error: null });
    try {
      const items = await listUsers();
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
    // The backend also propagated this change to any identity whose
    // (userName, userEmail) matched the OLD pair. Tell the identities
    // store to re-fetch so the UI shows the new state.
    refreshBus.emit({ kind: 'user-changed', userId: id });
    return updated;
  },
  remove: async (id) => {
    await apiDelete(id);
    set((s) => ({ items: s.items.filter((x) => x.id !== id) }));
  },
  importFromGitconfig: async () => {
    const created = await apiImport();
    if (created.length > 0) {
      set((s) => ({ items: [...s.items, ...created] }));
    }
    return created;
  },
}));
