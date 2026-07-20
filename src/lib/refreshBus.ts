/**
 * Minimal in-process pub-sub for "please refresh repo configs" hints.
 *
 * Why this exists: the old design wired `refreshRepoConfigs` into a
 * `useEffect` that depended on `[projects, identities]`. Both of
 * those arrays get a fresh reference any time their `refresh()`
 * IPC round-trip finishes, which means the refresh fired on
 * EVERY disk re-read — including ones where nothing actually
 * changed (switching selected project, scrolling, manual refresh).
 *
 * The fix: only fire a refresh when something *semantically*
 * changed. We do that by emitting a typed event from the store
 * mutations themselves (global-default set/clear, identity
 * create/update/remove, project add/remove/assign) and letting
 * ProjectsView subscribe and decide what to refresh.
 *
 * This is intentionally NOT a zustand store and NOT persisted:
 * it's a plain Set of handlers. We don't want re-renders on
 * emit; subscribers schedule their own work.
 */
export type RefreshEvent =
  /// The NiceSSH-recorded global default pointer changed
  /// (either set to an identity or cleared). Every project's
  /// `mergeWithGlobalDefault` output may shift, so subscribers
  /// should refresh all projects.
  | { kind: 'global-default-changed' }
  /// An existing identity was created or its fields were edited.
  /// Subscribers should refresh projects that depend on this
  /// identity — either because they are bound to it
  /// (`project.identityId === identityId`) or because it IS the
  /// global default (in which case refresh everything).
  | { kind: 'identity-changed'; identityId: string }
  /// An identity was deleted. Any project previously bound to
  /// it should re-read its `.git/config` to drop the stale
  /// reference. Subscribers handle the per-project filtering
  /// the same way as `identity-changed`.
  | { kind: 'identity-removed'; identityId: string }
  /// A new project was registered. Subscribers should fetch
  /// that project's repo config so the UI shows it as soon as
  /// the list re-renders. We pass the path so the subscriber
  /// doesn't need to re-read the projects list to find it.
  | { kind: 'project-added'; projectId: string; path: string }
  /// A project was unregistered. Subscribers should evict it
  /// from any cached maps (no IPC needed — the project is
  /// gone, so there is nothing to re-read).
  | { kind: 'project-removed'; projectId: string }
  /// A project's bound identity changed (the `assign` action).
  /// The new identity may pull in a different SSH key path or
  /// `[user] name/email`, so the subscriber should refresh
  /// that single project's repo config.
  | { kind: 'project-assigned'; projectId: string };

type Handler = (event: RefreshEvent) => void;

const handlers = new Set<Handler>();

export const refreshBus = {
  /// Register a handler. Returns an unsubscribe function so
  /// React effects can clean up on unmount. Multiple handlers
  /// can subscribe; they all fire in registration order.
  on(handler: Handler): () => void {
    handlers.add(handler);
    return () => {
      handlers.delete(handler);
    };
  },
  /// Broadcast an event to every registered handler. Handlers
  /// are called synchronously inside a try/catch so one bad
  /// subscriber cannot break the store mutation that fired
  /// the event.
  emit(event: RefreshEvent): void {
    handlers.forEach((h) => {
      try {
        h(event);
      } catch {
        // Swallow: a single broken handler should not block
        // the rest. Errors inside async work bubble out via
        // the subscriber's own try/catch.
      }
    });
  },
};

// Test-only: lets unit tests reset the bus between cases.
// Not used in production code paths.
export function __resetRefreshBus(): void {
  handlers.clear();
}
