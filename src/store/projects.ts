import { create } from 'zustand';
import { Project, listProjects, addProject as apiAdd, removeProject as apiRemove, assignIdentity as apiAssign } from '../ipc/projects';

interface State {
  items: Project[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  add: (p: { name: string; path: string; identityId: string | null }) => Promise<Project>;
  remove: (id: string) => Promise<void>;
  assign: (projectId: string, identityId: string) => Promise<void>;
}

export const useProjectsStore = create<State>((set) => ({
  items: [],
  loading: false,
  error: null,
  refresh: async () => {
    set({ loading: true, error: null });
    try {
      const items = await listProjects();
      set({ items });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : String(error) });
      throw error;
    } finally {
      set({ loading: false });
    }
  },
  add: async (p) => {
    const created = await apiAdd(p);
    set((s) => ({ items: [...s.items, created] }));
    return created;
  },
  remove: async (id) => {
    await apiRemove(id);
    set((s) => ({ items: s.items.filter((x) => x.id !== id) }));
  },
  assign: async (projectId, identityId) => {
    const updated = await apiAssign(projectId, identityId);
    set((s) => ({ items: s.items.map((x) => (x.id === projectId ? updated : x)) }));
  },
}));
