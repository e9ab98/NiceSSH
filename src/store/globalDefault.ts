import { create } from 'zustand';
import { subscribeWithSelector } from 'zustand/middleware';
import {
  getGlobalDefaultIdentityId,
  setGlobalDefaultIdentity,
  unsetGlobalDefaultIdentity,
} from '../ipc/git';

/**
 * Centralised store for the **advisory** global-default pointer
 * stored in NiceSSH's config (`globalDefaultIdentityId`).
 *
 * The source of truth for `git` itself is still `~/.gitconfig`;
 * this store only tracks what NiceSSH *thinks* the default is, so
 * the Identities view can render "this is your global default"
 * without re-parsing `~/.gitconfig` on every render.
 *
 * Mirrors the `useIdentitiesStore` / `useKeysStore` pattern:
 *   - `refresh()` re-reads the id from disk (call once on view mount
 *     and after any external mutation)
 *   - `set(id)` writes through the Rust `set_global_default_identity`
 *     command — which updates BOTH `~/.gitconfig` AND the cfg pointer —
 *     then stores the id locally for instant UI feedback
 *   - `clear()` writes through `unset_global_default_identity`,
 *     which only drops the cfg pointer and leaves `~/.gitconfig`
 *     alone (see Rust doc-comment for the rationale)
 */
interface State {
  /// Identity id NiceSSH has recorded as the global default, or
  /// `null` when none is recorded. `undefined` means "haven't
  /// loaded yet" — the UI should treat it as "unknown" rather than
  /// "no default", and typically trigger `refresh()` on mount.
  id: string | null | undefined;
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  set: (id: string) => Promise<void>;
  clear: () => Promise<void>;
}

export const useGlobalDefaultStore = create<State>()(subscribeWithSelector((set, get) => ({
  id: undefined,
  loading: false,
  error: null,
  refresh: async () => {
    set({ loading: true, error: null });
    try {
      const id = await getGlobalDefaultIdentityId();
      set({ id });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : String(error) });
    } finally {
      set({ loading: false });
    }
  },
  set: async (id) => {
    // Optimistic UI: show the new id immediately so the radio
    // flips before the round-trip finishes. The IPC call is the
    // single source of truth for what actually lands on disk; if
    // it throws we revert.
    const previous = get().id;
    set({ id });
    try {
      await setGlobalDefaultIdentity(id);
    } catch (e) {
      set({ id: previous, error: e instanceof Error ? e.message : String(e) });
      throw e;
    }
  },
  clear: async () => {
    const previous = get().id;
    set({ id: null });
    try {
      await unsetGlobalDefaultIdentity();
    } catch (e) {
      set({ id: previous, error: e instanceof Error ? e.message : String(e) });
      throw e;
    }
  },
}));
