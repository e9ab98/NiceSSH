import { create } from 'zustand';
import { refreshBus } from '../lib/refreshBus';
import { useProjectsStore } from './projects';
import { useGlobalDefaultStore } from './globalDefault';
import { useIdentitiesStore } from './identities';
import { getRepoGitConfig, isGitRepo, type RepoGitConfig } from '../ipc/git';
import { mergeWithGlobalDefault } from '../lib/mergeWithGlobalDefault';

/**
 * Cross-mount cache for the per-project state that ProjectsView
 * needs to render its list and detail panel.
 *
 * Why a dedicated store and not `useState` inside ProjectsView:
 *   - React unmounts the view when the user navigates between
 *     tabs (`/projects` → `/identities` → back). All `useState`
 *     is wiped on unmount, so the user would land on a "no
 *     identity" / "not initialised" list every time they came
 *     back, even though the data hasn't changed.
 *   - Putting the cache here means it survives the unmount, and
 *     the view re-renders instantly with the previous values.
 *     An initial fetch on mount then runs in the background to
 *     pick up any external changes.
 *
 * What lives here:
 *   - `repoConfigs`: per-project `.git/config` snapshots, keyed
 *     by project id. Refreshed by the bus on global-default /
 *     identity / project mutations, plus an explicit `refresh*`
 *     call from the manual refresh button.
 *   - `isRepoMap`: per-project `isGitRepo(path)` result. Same
 *     lifecycle as `repoConfigs`.
 *   - `selectedId`: which project is highlighted in the list.
 *     Kept here so the selection persists across tab switches.
 *
 * What does NOT live here:
 *   - Dialog open/close state (`auditOpen`, `passOpen`, etc.) —
 *     these are per-session UX state and should reset on tab
 *     switch.
 *   - `statusForSelected` (git porcelain status). The Quick
 *     Actions row is expected to go stale quickly (ahead/behind,
 *     dirty files), so we re-fetch it on every selection change
 *     rather than caching.
 */

/// Fallback config returned by `fetchOneRepoConfig` when the
/// Rust side throws (e.g. the path is not a git repo, or
/// `get_repo_git_config` can't read `.git/config`). We still
/// want a record in the map so React keys stay stable and the
/// "no config" branch in the UI renders cleanly.
const emptyRepoConfig: RepoGitConfig = {
  hasConfig: false,
  userName: null,
  userEmail: null,
  userNameSource: 'none',
  userEmailSource: 'none',
  sshKeyPath: null,
  managedByNicessh: false,
  sshCommandCount: 0,
  remoteUrl: null,
  remoteProtocol: null,
};

/// Reads the global default + its backing identity straight from
/// the live stores (instead of from a closure) so this helper
/// stays pure with respect to React state and can be called from
/// any context — store actions, bus handlers, manual refresh.
async function fetchOneRepoConfig(path: string): Promise<RepoGitConfig> {
  const globalDefaultId = useGlobalDefaultStore.getState().id;
  const identities = useIdentitiesStore.getState().items;
  const globalDefault = globalDefaultId
    ? identities.find((i) => i.id === globalDefaultId) ?? null
    : null;
  let cfg: RepoGitConfig;
  try {
    cfg = await getRepoGitConfig(path);
  } catch {
    cfg = emptyRepoConfig;
  }
  // The git-side fallback inside `get_repo_git_config` only
  // sees `~/.gitconfig` + includeIf; it does NOT know about
  // NiceSSH's UI-level pointer, so we layer that on here.
  const merged = mergeWithGlobalDefault(cfg, {
    globalDefaultId: globalDefaultId ?? null,
    globalDefault: globalDefault
      ? { userName: globalDefault.userName, userEmail: globalDefault.userEmail }
      : null,
  });
  if (merged !== cfg) {
    return {
      ...cfg,
      userName: merged.userName,
      userEmail: merged.userEmail,
      userNameSource: merged.userNameSource,
      userEmailSource: merged.userEmailSource,
    };
  }
  return cfg;
}

interface State {
  repoConfigs: Record<string, RepoGitConfig>;
  isRepoMap: Record<string, boolean>;
  selectedId: string | null;
  setSelectedId: (id: string | null) => void;
  /// Full re-read across every project. Call from the manual
  /// refresh button and from the `global-default-changed` bus
  /// event.
  refreshRepoConfigs: () => Promise<void>;
  /// Partial re-read for a specific set of paths. Used by the
  /// bus events that only touch a handful of projects (identity
  /// update for a non-global-default, project add/assign).
  refreshRepoConfigsForPaths: (paths: string[]) => Promise<void>;
  /// Re-scans `isGitRepo` for every project. Used on initial
  /// mount and after the bus's `project-added` event so the new
  /// project's badge updates immediately.
  refreshIsRepo: () => Promise<void>;
  /// Drops the cached entries for a removed project. Called from
  /// the bus's `project-removed` event. Also clears `selectedId`
  /// if it was pointing at the removed project.
  evictProject: (projectId: string) => void;
}

export const useRepoConfigsStore = create<State>((set, get) => {
  // Wire the bus once at module load. The store is a singleton
  // and lives for the entire app session, so a permanent
  // subscription is fine. We translate semantic events into the
  // appropriate per-store state updates.
  refreshBus.on((event) => {
    const projects = useProjectsStore.getState().items;
    const globalDefaultId = useGlobalDefaultStore.getState().id;
    if (event.kind === 'global-default-changed') {
      // Pointer flipped (set or clear). Every project's
      // `mergeWithGlobalDefault` output may shift, so refresh all.
      void get().refreshRepoConfigs();
    } else if (event.kind === 'identity-changed' || event.kind === 'identity-removed') {
      // If this identity IS the global default, the fallback
      // merge for every project changed → refresh all.
      // Otherwise, only projects bound to this identity need
      // their repo config re-read.
      if (event.identityId === globalDefaultId) {
        void get().refreshRepoConfigs();
        return;
      }
      const paths = projects
        .filter((p) => p.identityId === event.identityId)
        .map((p) => p.path);
      void get().refreshRepoConfigsForPaths(paths);
    } else if (event.kind === 'project-added') {
      // The new project hasn't been fetched yet. Refresh its
      // repo config + isGitRepo status.
      void get().refreshRepoConfigsForPaths([event.path]);
      void get().refreshIsRepo();
    } else if (event.kind === 'project-removed') {
      get().evictProject(event.projectId);
    } else if (event.kind === 'project-assigned') {
      // The bound identity flipped — re-read that single project.
      const proj = projects.find((p) => p.id === event.projectId);
      if (proj) void get().refreshRepoConfigsForPaths([proj.path]);
    } else if (event.kind === 'user-changed') {
      // The backend propagated the (name, email) change to every
      // identity whose previous values matched the OLD pair.
      // We don't know which identities were affected from the
      // event alone, so refresh everything — repo-config reads
      // are cheap relative to a user-driven UI cycle.
      void get().refreshRepoConfigs();
    }
  });

  return {
    repoConfigs: {},
    isRepoMap: {},
    selectedId: null,
    setSelectedId: (id) => set({ selectedId: id }),
    refreshRepoConfigs: async () => {
      const projects = useProjectsStore.getState().items;
      const out: Record<string, RepoGitConfig> = {};
      for (const p of projects) {
        out[p.id] = await fetchOneRepoConfig(p.path);
      }
      set({ repoConfigs: out });
    },
    refreshRepoConfigsForPaths: async (paths) => {
      if (paths.length === 0) return;
      const unique = Array.from(new Set(paths));
      // Parallel: each call is independent (no shared mutable
      // state in `getRepoGitConfig`). Merging happens after.
      const fetched = await Promise.all(unique.map((p) => fetchOneRepoConfig(p)));
      const projects = useProjectsStore.getState().items;
      set((state) => {
        const next = { ...state.repoConfigs };
        unique.forEach((path, i) => {
          const proj = projects.find((p) => p.path === path);
          if (proj) next[proj.id] = fetched[i];
        });
        return { repoConfigs: next };
      });
    },
    refreshIsRepo: async () => {
      const projects = useProjectsStore.getState().items;
      const out: Record<string, boolean> = {};
      for (const p of projects) {
        try {
          out[p.id] = await isGitRepo(p.path);
        } catch {
          out[p.id] = false;
        }
      }
      set({ isRepoMap: out });
    },
    evictProject: (projectId) => {
      set((state) => {
        const nextRepoConfigs =
          projectId in state.repoConfigs
            ? Object.fromEntries(
                Object.entries(state.repoConfigs).filter(([k]) => k !== projectId),
              )
            : state.repoConfigs;
        const nextIsRepoMap =
          projectId in state.isRepoMap
            ? Object.fromEntries(
                Object.entries(state.isRepoMap).filter(([k]) => k !== projectId),
              )
            : state.isRepoMap;
        const nextSelectedId =
          state.selectedId === projectId ? null : state.selectedId;
        return {
          repoConfigs: nextRepoConfigs,
          isRepoMap: nextIsRepoMap,
          selectedId: nextSelectedId,
        };
      });
    },
  };
});
