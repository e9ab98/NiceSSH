import { create } from 'zustand';
import { Identity, listIdentities, createIdentity as apiCreate, updateIdentity as apiUpdate, deleteIdentity as apiDelete } from '../ipc/identities';

interface State {
  items: Identity[];
  loading: boolean;
  refresh: () => Promise<void>;
  create: (i: Omit<Identity, 'id'>) => Promise<Identity>;
  update: (id: string, i: Identity) => Promise<void>;
  remove: (id: string) => Promise<void>;
}

export const useIdentitiesStore = create<State>((set) => ({
  items: [],
  loading: false,
  refresh: async () => {
    set({ loading: true });
    const items = await listIdentities();
    set({ items, loading: false });
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
  remove: async (id) => {
    await apiDelete(id);
    set((s) => ({ items: s.items.filter((x) => x.id !== id) }));
  },
}));
