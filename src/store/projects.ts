import { create } from 'zustand';
import { Project, listProjects, addProject as apiAdd, removeProject as apiRemove, assignIdentity as apiAssign } from '../ipc/projects';

interface State {
  items: Project[];
  loading: boolean;
  refresh: () => Promise<void>;
  add: (p: { name: string; path: string; identityId: string | null }) => Promise<Project>;
  remove: (id: string) => Promise<void>;
  assign: (projectId: string, identityId: string) => Promise<void>;
}

export const useProjectsStore = create<State>((set) => ({
  items: [],
  loading: false,
  refresh: async () => {
    set({ loading: true });
    const items = await listProjects();
    set({ items, loading: false });
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
