import { create } from 'zustand';
import { Project, listProjects, addProject as apiAdd, removeProject as apiRemove, assignIdentity as apiAssign } from '../ipc/projects';
import { refreshBus } from '../lib/refreshBus';

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
    // Tell the rest of the UI to fetch THIS project's repo
    // config (so badges / bind UI render immediately). We
    // intentionally do NOT fire a full refresh — the other
    // projects haven't changed.
    refreshBus.emit({ kind: 'project-added', projectId: created.id, path: created.path });
    return created;
  },
  remove: async (id) => {
    await apiRemove(id);
    set((s) => ({ items: s.items.filter((x) => x.id !== id) }));
    // Tell subscribers to evict the cached repoConfig entry
    // for this id. No IPC needed — the project is gone.
    refreshBus.emit({ kind: 'project-removed', projectId: id });
  },
  assign: async (projectId, identityId) => {
    const updated = await apiAssign(projectId, identityId);
    set((s) => ({ items: s.items.map((x) => (x.id === projectId ? updated : x)) }));
    // The new identity may pull in a different SSH key path or
    // user/email, so refresh that single project's repo config.
    refreshBus.emit({ kind: 'project-assigned', projectId });
  },
}));
