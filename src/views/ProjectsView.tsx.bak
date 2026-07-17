import { useEffect, useMemo, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { open as openDirDialog } from '@tauri-apps/plugin-dialog';
import { CircleSlash, AlertTriangle, FolderGit2, UserCircle2, MousePointer, type LucideIcon } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Badge } from '../components/ui/badge';
import { ProtocolBadge } from '../components/protocolBadge';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../components/ui/tooltip';
import { useProjectsStore } from '../store/projects';
import { useIdentitiesStore, useKeysStore, type SshKeyInfo } from '../store/identities';
import { useSettingsStore } from '../store/settings';
import { applyIdentityToRepo, getRecentCommits, getRepoGitConfig, getGlobalGitConfig, gitStatus, initRepo, isGitRepo, type RepoGitConfig, type GlobalGitConfig, type RepoStatus } from '../ipc/git';
import { tryUnlockKey, isKeyEncrypted } from '../ipc/sshAdd';
import { IdentitySwitcherDialog } from '../features/identitySwitcher/IdentitySwitcherDialog';
import { RepoAuditDialog } from '../features/repoAudit/RepoAuditDialog';
import { PassphraseDialog } from '../features/passphraseDialog/PassphraseDialog';
import { RemoteUrlPromptDialog } from '../features/remotePrompt/RemoteUrlPromptDialog';
import { HttpsBindConfirmDialog, type HttpsBindChoice } from '../features/httpsBindConfirm/HttpsBindConfirmDialog';
import { CommitDialog } from '../features/gitOps/CommitDialog';
import { PullDialog } from '../features/gitOps/PullDialog';
import { PushDialog } from '../features/gitOps/PushDialog';
import { ConnectionTesterDialog } from '../features/connectionTester/ConnectionTesterDialog';
import { ContextMenu } from '../components/ContextMenu';
import { toast } from 'sonner';
import { cn } from '../lib/utils';
import type { Identity } from '../ipc/identities';
// SshKeyInfo type re-exported via identities store
// (no direct ipc import needed here)

type DetectedIdentity =
  | { kind: 'none' }
  | { kind: 'untracked'; keyPath: string }
  | { kind: 'tracked'; identity: Identity; source: 'config' | 'git' };

function detectIdentity(
  project: { id: string; name: string; path: string; identityId: string | null },
  identities: Identity[],
  repoConfig: RepoGitConfig | null,
  keys: SshKeyInfo[],
): DetectedIdentity {
  if (project.identityId) {
    const found = identities.find((i) => i.id === project.identityId);
    if (found) return { kind: 'tracked', identity: found, source: 'config' };
  }
  if (repoConfig?.sshKeyPath) {
    const match = identities.find((i) => keys.find((key) => key.id === i.sshKeyId)?.privatePath === repoConfig.sshKeyPath);
    if (match) return { kind: 'tracked', identity: match, source: 'git' };
    return { kind: 'untracked', keyPath: repoConfig.sshKeyPath };
  }
  return { kind: 'none' };
}

/// Best-effort transformation of an HTTPS URL into its SSH form.
/// Returns null if the URL shape is too exotic to safely auto-convert
/// (in which case we leave the user to type a URL themselves). The
/// rule: replace `https?://HOST/` with `git@HOST:`, dropping any
/// leading `www.`, and keeping the path verbatim. Trailing `.git`
/// is preserved.
function deriveSshUrlFromHttps(remoteUrl: string | null | undefined): string | null {
  if (!remoteUrl) return null;
  const m = remoteUrl.trim().match(/^https?:\/\/([^\/]+)\/(.+?)(?:\.git)?$/);
  if (!m) return null;
  const host = m[1].replace(/^www\./, '');
  const path = m[2];
  if (!host.includes('.')) return null; // bare hostname is suspicious
  return `git@${host}:${path}.git`;
}

function deriveName(p: string): string {
  const trimmed = p.replace(/[\\/]+$/, '');
  const seg = trimmed.split(/[\\/]/).pop();
  return seg && seg.length > 0 ? seg : trimmed;
}

type Tone = 'brand' | 'success' | 'warning' | 'danger';

const toneStyles: Record<Tone, string> = {
  brand: 'bg-brand-soft text-brand-strong [[data-theme=dark]_&]:text-[#93c5fd]',
  success: 'bg-[rgba(34,197,94,0.1)] text-[#16a34a] [[data-theme=dark]_&]:text-[#86efac]',
  warning: 'bg-[rgba(245,158,11,0.12)] text-[#b45309] [[data-theme=dark]_&]:text-[#fcd34d]',
  danger: 'bg-[rgba(239,68,68,0.1)] text-[#dc2626] [[data-theme=dark]_&]:text-[#fca5a5]',
};

function StatCard({ icon: Icon, tone, value, label }: { icon: LucideIcon; tone: Tone; value: number; label: string }) {
  return (
    <div className="rounded-2xl border border-border bg-bg-1 shadow-card p-4 flex items-center gap-3">
      <div className={cn('h-10 w-10 shrink-0 rounded-xl flex items-center justify-center', toneStyles[tone])}>
        <Icon className="h-5 w-5" />
      </div>
      <div className="min-w-0">
        <div className="text-2xl font-extrabold text-text-0 leading-tight">{value}</div>
        <div className="text-xs text-text-1 font-semibold truncate">{label}</div>
      </div>
    </div>
  );
}

export function ProjectsView() {
  const { t } = useTranslation();
  const projects = useProjectsStore((s) => s.items);
  const refreshProjects = useProjectsStore((s) => s.refresh);
  const keys = useKeysStore((s) => s.items);
  const refreshKeys = useKeysStore((s) => s.refresh);
  const add = useProjectsStore((s) => s.add);
  const remove = useProjectsStore((s) => s.remove);
  const assign = useProjectsStore((s) => s.assign);
  const identities = useIdentitiesStore((s) => s.items);
  const refreshIdentities = useIdentitiesStore((s) => s.refresh);
  const markKeyUnlocked = useSettingsStore((s) => s.markKeyUnlocked);
  const recentlyUnlocked = useSettingsStore((s) => s.recentlyUnlockedKeys);
  const privatePathFor = (identity: Identity) => keys.find((key) => key.id === identity.sshKeyId)?.privatePath ?? '';

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [switcherOpen, setSwitcherOpen] = useState(false);
  const [testerOpen, setTesterOpen] = useState(false);
  const [passOpen, setPassOpen] = useState(false);
  const [pendingIdentityId, setPendingIdentityId] = useState<string | null>(null);
  const [repoConfigs, setRepoConfigs] = useState<Record<string, RepoGitConfig>>({});
  const [globalGit, setGlobalGit] = useState<GlobalGitConfig | null>(null);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; projectId: string; projectName: string } | null>(null);
  const [adding, setAdding] = useState(false);
  const [auditOpen, setAuditOpen] = useState(false);
  // Per-project isGitRepo() result. Drives the 'Not initialized' badge
  // and the Initialize Project button. Refreshed whenever the projects
  // list changes or after a successful init_repo call.
  const [isRepoMap, setIsRepoMap] = useState<Record<string, boolean>>({});
  // When set, points at a project that the user wants to initialize.
  // The IdentitySwitcherDialog reuses its `switcherOpen` state to ask
  // for an identity; `initTargetId` carries the project context so we
  // know what to do with the chosen identity.
  const [initTargetId, setInitTargetId] = useState<string | null>(null);
  const [initializing, setInitializing] = useState(false);
  // Quick Actions dialogs. We open them via dedicated state so the
  // dialogs know which project they target (the selected one) and
  // can call back to refresh the status row on success.
  const [commitOpen, setCommitOpen] = useState(false);
  const [pullOpen, setPullOpen] = useState(false);
  const [pushOpen, setPushOpen] = useState(false);
  // Last known status snapshot for the *selected* project. Refreshed
  // on selection change and after every successful git op. Drives
  // the ahead/behind/working-tree-dirty line under the Quick
  // Actions row.
  const [statusForSelected, setStatusForSelected] = useState<RepoStatus | null>(null);
  // Recent commits for the selected project, mirrored from
  // ProjectDetail's local state so dialog success callbacks can
  // trigger a refresh without a prop-drilling rabbit hole.
  const [commitsForSelected, setCommitsForSelected] = useState<{ hash: string; subject: string }[]>([]);
  const refreshIsRepo = useCallback(async () => {
    const out: Record<string, boolean> = {};
    for (const p of projects) {
      try { out[p.id] = await isGitRepo(p.path); } catch { out[p.id] = false; }
    }
    setIsRepoMap(out);
  }, [projects]);

  const refreshRepoConfigs = useCallback(async () => {
    const out: Record<string, RepoGitConfig> = {};
    for (const p of projects) {
      try {
        out[p.id] = await getRepoGitConfig(p.path);
      } catch {
        out[p.id] = { hasConfig: false, userName: null, userEmail: null, sshKeyPath: null, managedByNicessh: false, sshCommandCount: 0, remoteUrl: null, remoteProtocol: null };
      }
    }
    setRepoConfigs(out);
  }, [projects]);

  // Keep the isRepo map in sync with the projects list. We do this in
  // a separate effect (rather than inside refreshRepoConfigs) so that
  // the dependency arrays of useCallbacks above stay accurate.
  useEffect(() => { void refreshIsRepo(); }, [refreshIsRepo]);


  useEffect(() => {
    refreshProjects();
    refreshIdentities();
    void refreshKeys();
  }, [refreshKeys]);

  useEffect(() => {
    getGlobalGitConfig().then(setGlobalGit).catch(() => setGlobalGit(null));
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const out: Record<string, RepoGitConfig> = {};
      for (const p of projects) {
        try {
          out[p.id] = await getRepoGitConfig(p.path);
        } catch {
          out[p.id] = { hasConfig: false, userName: null, userEmail: null, sshKeyPath: null, managedByNicessh: false, sshCommandCount: 0, remoteUrl: null, remoteProtocol: null };
        }
      }
      if (!cancelled) setRepoConfigs(out);
    })();
    return () => { cancelled = true; };
  }, [projects]);

  const selected = projects.find((p) => p.id === selectedId) ?? null;
  // Refresh the Quick Actions status row for the *selected*
  // project. We deliberately do not refresh on every project
  // change — git status is cheap, but porcelain parsing is
  // still O(files), and the left list view does not need
  // per-row status yet.
  // Pull the status deps into local primitives so the
  // `useCallback` doesn't re-create `refreshStatus` on every
  // render. Without this, `useEffect([refreshStatus])` would
  // re-fire endlessly and burn the webview CPU on every project
  // store update.
  const selectedPath = selected?.path ?? null;
  const selectedIsRepo = selected ? isRepoMap[selected.id] : undefined;
  const refreshStatus = useCallback(async () => {
    if (!selectedPath) { setStatusForSelected(null); return; }
    if (selectedIsRepo === false) { setStatusForSelected(null); return; }
    try {
      const s = await gitStatus(selectedPath);
      setStatusForSelected(s);
    } catch {
      setStatusForSelected(null);
    }
  }, [selectedPath, selectedIsRepo]);
  useEffect(() => { void refreshStatus(); }, [refreshStatus, selectedId]);

  const repoConfig = selected ? (repoConfigs[selected.id] ?? null) : null;
  const detected = selected ? detectIdentity(selected, identities, repoConfig, keys) : { kind: 'none' as const };
  const identity = detected.kind === 'tracked' ? detected.identity : null;
  const pendingIdentity = identities.find((i) => i.id === pendingIdentityId) ?? null;

  // Stats: derived from projects + identities + repoConfigs
  const stats = useMemo(() => {
    let bound = 0, unbound = 0, errors = 0;
    for (const p of projects) {
      const cfg = repoConfigs[p.id];
      const d = detectIdentity(p, identities, cfg ?? null, keys);
      if (d.kind === 'tracked') bound++;
      else unbound++;
    }
    // errors: projects whose getRepoGitConfig returned hasConfig=false (loaded as fallback)
    errors = Object.values(repoConfigs).filter((c) => c && !c.hasConfig).length;
    return { total: projects.length, bound, unbound, errors };
  }, [projects, identities, repoConfigs]);

  const defaultIdentityId = (() => {
    if (!globalGit?.sshKeyPath) return null;
    const match = identities.find((i) => privatePathFor(i) === globalGit.sshKeyPath);
    return match?.id ?? null;
  })();

  // Controls the modal that prompts for a missing remote URL.
  // We track *which* project's bind was suspended so the dialog can
  // ask for the URL and then retry the bind with a fresh protocol
  // decision. The pair (projectId, identityId) lives in state so
  // cancel-equivalent (cancel) just clears the state without retrying.
  const [remotePrompt, setRemotePrompt] = useState<{
    projectId: string;
    identityId: string;
    projectPath: string;
    /// When set, the RemoteUrlPromptDialog renders with this URL
    /// pre-filled. Used both by the "this project has no remote"
    /// flow (no prefill) and by the "switch this HTTPS remote to
    /// SSH…" flow (prefilled with `deriveSshUrlFromHttps(...)`).
    prefillUrl?: string;
  } | null>(null);
  // Pending toast i18n key to show *after* a successful retry, so the
  // 'identity applied' message reflects the actual outcome (SSH vs
  // user-only vs needs-remote).
  // Holds the "we just bound an identity and noticed the remote is
  // HTTPS" request so the warning dialog can render with full
  // context. Cleared on dialog close / on the user picking any
  // option. `repoSnapshot` captures the SSH-able URL to use as a
  // suggestion when the user picks "switch remote to SSH".
  const [httpsConfirm, setHttpsConfirm] = useState<{
    projectId: string;
    identityId: string;
    projectName: string;
    projectPath: string;
    protocol: 'https' | 'http' | null;
    remoteUrl: string | null;
  } | null>(null);

  const bindIdentity = async (projectId: string, identityId: string, projectPath: string) => {
    const outcome = await applyIdentityToRepo(projectId, identityId);
    if (outcome === 'needs-remote') {
      toast(t('projects.outcome.needs-remote'));
      setRemotePrompt({ projectId, identityId, projectPath });
      return;
    }
    if (outcome === 'user-only') {
      // Surface the warning dialog instead of a tiny toast. We need
      // the project's *display* name + the current remote URL for
      // context, so peek at the in-memory store + cached config.
      const projectName = projects.find((p) => p.id === projectId)?.name ?? '';
      const cfg = repoConfigs[projectId];
      // The interface for remoteProtocol is 'ssh' | 'https' | 'git'
      // | 'unknown' | null — we treat anything that's not 'ssh'/'git'
      // as "uses git-credential" for the purpose of this warning,
      // and surface the literal 'https' as the heading text (the
      // classifier groups plain http:// and https:// together).
      const protocol: 'https' | null =
        cfg && cfg.remoteProtocol !== 'ssh' && cfg.remoteProtocol !== 'git' && cfg.remoteProtocol !== null
          ? 'https'
          : null;
      setHttpsConfirm({
        projectId,
        identityId,
        projectName,
        projectPath,
        protocol,
        remoteUrl: cfg?.remoteUrl ?? null,
      });
      return;
    }
    // SSH-style: identity fully bound (user + sshCommand). Update
    // the project store so subsequent reloads see this binding, then
    // toast success. The "needs-remote" and "user-only" branches
    // route through dialogs and update the store from inside their
    // confirm handlers — so centralising assign() here keeps the
    // store in sync regardless of which branch the user lands in.
    try { await assign(projectId, identityId); } catch { /* no-op */ }
    toast.success(t('projects.outcome.ssh-style'));
  };

  const handleHttpsConfirmChoice = async (choice: HttpsBindChoice) => {
    if (!httpsConfirm) return;
    const { projectId, identityId, projectPath, remoteUrl } = httpsConfirm;
    if (choice === 'cancel') {
      // Bind has already happened server-side. Closing the dialog
      // is the only honest action — we surface a soft toast so the
      // user knows the bind took effect even though they backed out
      // of the optional SSH switch.
      toast.success(t('projects.outcome.user-only'));
      setHttpsConfirm(null);
      return;
    }
    if (choice === 'continue') {
      // Identity stays as applied (user-only). Mark in store for
      // consistency with the existing switcher flow.
      try { await assign(projectId, identityId); } catch { /* already */ }
      toast.success(t('projects.outcome.user-only'));
      setHttpsConfirm(null);
      return;
    }
    // choice === 'change-remote': open RemoteUrlPromptDialog with a
    // pre-filled SSH URL. Once it saves, we go through the regular
    // needs-remote retry path so the *next* applyIdentityToRepo call
    // has a real URL on disk to read.
    const ssh = deriveSshUrlFromHttps(remoteUrl);
    setHttpsConfirm(null);
    setRemotePrompt({
      projectId,
      identityId,
      projectPath,
      // Carry the suggested SSH URL through the existing
      // RemoteUrlPromptDialog initialUrl plumbing.
      prefillUrl: ssh ?? undefined,
    });
  };

  const handleAdd = async () => {
    if (adding) return;
    setAdding(true);
    try {
      const picked = await openDirDialog({ directory: true, multiple: false });
      if (typeof picked !== 'string') return;
      const project = await add({ name: deriveName(picked), path: picked, identityId: defaultIdentityId });
      if (defaultIdentityId) {
        await bindIdentity(project.id, defaultIdentityId, project.path);
      } else {
        toast.success(t('projects.added'));
      }
      setSelectedId(project.id);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setAdding(false);
    }
  };

  /// Open the identity switcher for a project that is not yet a git
  /// repository. The user picks an identity, we (a) call init_repo on
  /// the project path, (b) refresh isRepo + repoConfig so the UI
  /// shows the freshly-created repo, and (c) chain into the regular
  /// bindIdentity flow so the SSH key is wired up to .git/config.
  const handleInit = async (projectId: string) => {
    const proj = projects.find((p) => p.id === projectId);
    if (!proj || initializing) return;
    setInitTargetId(projectId);
    setSwitcherOpen(true);
  };

  /// Called by the IdentitySwitcherDialog when the user picks an
  /// identity while a project is being initialized. Performs:
  ///   init_repo(path, identityId) -> bindIdentity(project, identityId)
  /// `bindIdentity` already handles HTTPS warnings and needs-remote
  /// retry dialogs, so we just feed it the chosen identity and let
  /// the existing dialog chain play out.
  const handleInitSelect = async (identityId: string) => {
    if (!initTargetId) return;
    const proj = projects.find((p) => p.id === initTargetId);
    if (!proj) {
      setInitTargetId(null);
      return;
    }
    setInitializing(true);
    try {
      await initRepo(proj.path, identityId);
      toast.success(t('projects.initSuccess'));
      // Refresh the isRepo flag so the badge and the button update.
      await refreshIsRepo();
      // Chain into the regular bind flow so the SSH key lands in
      // .git/config (and any HTTPS/needs-remote dialogs surface).
      await bindIdentity(initTargetId, identityId, proj.path);
      setInitTargetId(null);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setInitializing(false);
    }
  };

  const performSwitch = async (targetIdentityId: string) => {
    if (!selected) return;
    // Route through bindIdentity so that the HTTPS warning dialog,
    // remote URL prompt, and success toast all behave the same way
    // regardless of whether the user is binding an identity for the
    // first time, switching an existing one, or retrying after a
    // remote-URL fix. The store update (assign) happens inside
    // bindIdentity — so we must not call assign() here (it would be
    // a redundant optimistic write that could mask dialog-flow bugs).
    await bindIdentity(selected.id, targetIdentityId, selected.path);
  };

  const handleSelect = async (targetIdentityId: string) => {
    if (!selected) {
      setSwitcherOpen(false);
      return;
    }
    if (selected.identityId && targetIdentityId === selected.identityId) {
      setSwitcherOpen(false);
      return;
    }
    const target = identities.find((i) => i.id === targetIdentityId);
    if (!target) return;
    const fullKp = privatePathFor(target);
    if (!fullKp) {
      toast.error(t('identities.noKeyBound'));
      return;
    }
    if (!recentlyUnlocked[fullKp]) {
      setSwitcherOpen(false);
      const encrypted = await isKeyEncrypted(fullKp);
      if (!encrypted) {
        const ok = await tryUnlockKey(fullKp, '');
        if (ok) {
          markKeyUnlocked(fullKp);
          await performSwitch(targetIdentityId);
          return;
        }
      }
      setPendingIdentityId(targetIdentityId);
      setPassOpen(true);
      return;
    }
    setSwitcherOpen(false);
    await performSwitch(targetIdentityId);
  };

  const handleUnlock = async (passphrase: string): Promise<boolean> => {
    if (!pendingIdentity) return false;
    const keyPath = privatePathFor(pendingIdentity);
    const ok = await tryUnlockKey(keyPath, passphrase);
    if (ok) {
      markKeyUnlocked(keyPath);
      if (selected) {
        await performSwitch(pendingIdentity.id);
      }
      setPendingIdentityId(null);
      return true;
    }
    return false;
  };

  const handleRemove = async (projectId: string) => {
    try {
      await remove(projectId);
      if (selectedId === projectId) setSelectedId(null);
      toast.success(t('projects.removed'));
    } catch (e) {
      toast.error(String(e));
    }
  };

  const handleDelete = async () => {
    if (!contextMenu) return;
    if (!confirm(t('projects.deleteConfirm'))) return;
    try {
      await remove(contextMenu.projectId);
      if (selectedId === contextMenu.projectId) setSelectedId(null);
      toast.success(t('projects.removed'));
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <TooltipProvider>
      <div className="h-full p-4 flex flex-col gap-4">
        {/* Top: 4 stat cards */}
        <div className="grid grid-cols-4 gap-3">
          <StatCard icon={FolderGit2} tone="brand" value={stats.total} label={t('projects.stat.total')} />
          <StatCard icon={UserCircle2} tone="success" value={stats.bound} label={t('projects.stat.bound')} />
          <StatCard icon={CircleSlash} tone="warning" value={stats.unbound} label={t('projects.stat.unbound')} />
          <StatCard icon={AlertTriangle} tone="danger" value={stats.errors} label={t('projects.stat.errors')} />
        </div>

        {/* Main: two-column layout */}
        <div
          className="flex-1 grid gap-4 min-h-0"
          style={{ gridTemplateColumns: 'minmax(0,1fr) clamp(380px, 28vw, 460px)' }}
        >
          {/* Left: list */}
          <div className="flex flex-col gap-3 min-h-0">
            <div className="flex items-center gap-2">
              <Button onClick={handleAdd} disabled={adding}>
                {adding ? t('common.adding') : t('projects.addProject')}
              </Button>
          <Button variant="outline" onClick={() => setAuditOpen(true)}>
            {t('repoAudit.title')}
          </Button>
            </div>
            <div className="flex-1 rounded-2xl border border-border bg-bg-1 shadow-card overflow-y-auto">
              {projects.length === 0 ? (
                <div className="p-8 text-center text-text-2 text-sm">{t('projects.empty')}</div>
              ) : (
                <ul className="p-1.5 flex flex-col gap-0.5">
                  {projects.map((p) => {
                    const cfg = repoConfigs[p.id] ?? null;
                    const d = detectIdentity(p, identities, cfg, keys);
                    return (
                      <li
                        key={p.id}
                        onClick={() => setSelectedId(p.id)}
                        onContextMenu={(e) => {
                          e.preventDefault();
                          setContextMenu({ x: e.clientX, y: e.clientY, projectId: p.id, projectName: p.name });
                        }}
                        className={cn(
                          'relative flex items-center gap-3 px-3 py-2.5 rounded-md cursor-pointer transition-colors',
                          p.id === selectedId
                            ? 'bg-brand-soft shadow-[inset_3px_0_0_0_var(--brand)]'
                            : 'hover:bg-bg-2'
                        )}
                      >
                        <div className="h-8 w-8 shrink-0 rounded-md bg-brand-soft text-brand-strong flex items-center justify-center text-sm font-bold [[data-theme=dark]_&]:text-[#93c5fd]">
                          {p.name.charAt(0).toUpperCase()}
                        </div>
                        <div className="flex-1 min-w-0">
                          <div className="text-sm font-semibold text-text-0 truncate">{p.name}</div>
                          <div className="text-xs text-text-2 truncate font-mono">{p.path}</div>
                        </div>
                        <Badge variant={d.kind === 'tracked' ? 'success' : d.kind === 'untracked' ? 'warning' : 'default'}>
                          {d.kind === 'tracked' ? t('projects.badge.bound') : d.kind === 'untracked' ? t('projects.badge.untracked') : t('projects.badge.unbound')}
                        </Badge>
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          </div>

          {/* Right: detail panel */}
          <div className="rounded-2xl border border-border bg-bg-1 shadow-card overflow-y-auto p-5">
            {selected ? (
              <ProjectDetail
                project={selected}
                keyPath={
                  detected.kind === 'tracked'
                    ? privatePathFor(detected.identity)
                    : null
                }
                detected={detected}
                hasIdentities={identities.length > 0}
                repoConfig={repoConfig}
                isRepo={isRepoMap[selected.id]}
                initializing={initializing}
                commits={commitsForSelected}
                setCommits={setCommitsForSelected}
                status={statusForSelected}
                opsBusy={false}
                onCommit={() => setCommitOpen(true)}
                onPull={() => setPullOpen(true)}
                onPush={() => setPushOpen(true)}
                onInit={() => { void handleInit(selected.id); }}
                onSwitch={() => setSwitcherOpen(true)}
                onTest={() => setTesterOpen(true)}
                onRemove={() => { void handleRemove(selected.id); }}
              />
            ) : (
              <div className="h-full min-h-[280px] flex flex-col items-center justify-center text-text-2 text-sm gap-2">
                <MousePointer className="h-8 w-8 opacity-50" />
                <div>{t('projects.detail.empty')}</div>
              </div>
            )}
          </div>
        </div>

        <IdentitySwitcherDialog
          open={switcherOpen}
          onOpenChange={(v) => {
            setSwitcherOpen(v);
            // When the user cancels out of the init flow, drop the
            // target so a later 'switch identity' click does not
            // accidentally re-run init_repo.
            if (!v) setInitTargetId(null);
          }}
          identities={identities}
          currentId={initTargetId ? null : (selected?.identityId ?? null)}
          projectPath={selected?.path ?? null}
          onSelect={initTargetId ? handleInitSelect : handleSelect}
        />
        {selected && (
          <CommitDialog
            open={commitOpen}
            onOpenChange={setCommitOpen}
            projectPath={selected.path}
            projectName={selected.name}
            onCommitted={async () => {
              await refreshStatus();
              try {
                const list = await getRecentCommits(selected.path, 10);
                setCommitsForSelected(list);
              } catch { /* ignore */ }
            }}
          />
        )}
        {selected && (
          <PullDialog
            open={pullOpen}
            onOpenChange={setPullOpen}
            projectPath={selected.path}
            projectName={selected.name}
            onPulled={refreshStatus}
          />
        )}
        {selected && (
          <PushDialog
            open={pushOpen}
            onOpenChange={setPushOpen}
            projectPath={selected.path}
            projectName={selected.name}
            onPushed={refreshStatus}
          />
        )}
        <RepoAuditDialog
          open={auditOpen}
          onOpenChange={setAuditOpen}
          onChanged={refreshRepoConfigs}
        />
        {pendingIdentity && (
          <PassphraseDialog
            open={passOpen}
            onOpenChange={(v) => { if (!v) setPendingIdentityId(null); }}
            keyPath={privatePathFor(pendingIdentity)}
            onUnlock={handleUnlock}
          />
        )}
        {selected && (identity || repoConfig?.remoteProtocol === 'https') && (
          <ConnectionTesterDialog
            open={testerOpen}
            onOpenChange={setTesterOpen}
            mode={repoConfigs[selected?.id ?? '']?.remoteProtocol === 'https' ? 'https' : 'ssh'}
            projectPath={selected?.path ?? ''}
            identityId={identity?.id}
            identityLabel={identity?.label}
          />
        )}
        {contextMenu && (
          <ContextMenu
            x={contextMenu.x}
            y={contextMenu.y}
            onClose={() => setContextMenu(null)}
            items={[
              {
                label: t('projects.removeMenu'),
                onSelect: () => { void handleRemove(contextMenu.projectId); },
              },
              {
                label: t('projects.deleteMenu'),
                onSelect: () => { void handleDelete(); },
                destructive: true,
              },
            ]}
          />
        )}
      </div>

      {/* Remote URL prompt: shown when applyIdentityToRepo detected
          that the repo has no `[remote ...] url`, so it could not
          decide between ssh-style and user-only binding. After the
          user provides a URL, we retry the bind. */}
      <RemoteUrlPromptDialog
        open={!!remotePrompt}
        onOpenChange={(v) => {
          if (!v) setRemotePrompt(null);
        }}
        projectPath={remotePrompt?.projectPath ?? ''}
        initialUrl={remotePrompt?.prefillUrl}
        onSaved={async () => {
          if (!remotePrompt) return;
          const { projectId, identityId } = remotePrompt;
          setRemotePrompt(null);
          // Re-fetch repo config so the rest of the UI sees the
          // freshly-written remote URL immediately.
          await refreshRepoConfigs();
          // bindIdentity handles the store update internally on the
          // happy (ssh-style) path via assign(). If it returns
          // needs-remote again we surface that toast — though that
          // should not happen now that we just persisted a URL.
          await bindIdentity(projectId, identityId, remotePrompt.projectPath);
        }}
      />

      {/* HTTPS bind warning. Shown when bindIdentity noticed the
          project’s remote is HTTP(S) and wrote only the [user]
          block. The user can pick: continue as-is, switch the
          remote to SSH (which opens RemoteUrlPromptDialog above
          with a pre-filled URL), or dismiss. */}
      <HttpsBindConfirmDialog
        open={!!httpsConfirm}
        onOpenChange={(v) => { if (!v) setHttpsConfirm(null); }}
        projectName={httpsConfirm?.projectName}
        protocol={httpsConfirm?.protocol ?? undefined}
        remoteUrl={httpsConfirm?.remoteUrl ?? null}
        onChoose={handleHttpsConfirmChoice}
      />
    </TooltipProvider>
  );
}

function ProjectDetail({ project, detected, keyPath, hasIdentities, repoConfig, isRepo, initializing, status, opsBusy, onInit, onCommit, onPull, onPush, onSwitch, onTest, onRemove, commits, setCommits }: {
  project: { id: string; name: string; path: string };
  detected: DetectedIdentity;
  /// Full private-key path for the bound identity, pre-resolved
  /// by the parent. We accept a plain string instead of the
  /// identity + keys pair because ProjectDetail is a standalone
  /// component (no closure over its parent's `privatePathFor`).
  keyPath: string | null;
  hasIdentities: boolean;
  repoConfig: RepoGitConfig | null;
  /// Result of isGitRepo() for this project. `undefined` means the
  /// check is still in flight (project was just added and the IPC
  /// has not returned yet). `false` triggers the 'Not initialized'
  /// call-to-action below the header.
  isRepo?: boolean;
  initializing?: boolean;
  /// Latest status snapshot for the Quick Actions row. `null` while
  /// loading or when the project is not a git repository.
  status: RepoStatus | null;
  /// Single shared "a git op is in flight" flag, used to disable
  /// the 4 Quick Actions buttons.
  opsBusy: boolean;
  onInit: () => void;
  onCommit: () => void;
  onPull: () => void;
  onPush: () => void;
  onSwitch: () => void;
  onTest: () => void;
  onRemove: () => void;
  /// Recent commits for this project. Lifted to the parent so the
  /// CommitDialog success callback can refresh it.
  commits: { hash: string; subject: string }[];
  setCommits: (list: { hash: string; subject: string }[]) => void;
}) {
  const { t } = useTranslation();
  // Commits are now lifted to the parent (so the CommitDialog can
  // refresh them on success). The local effect still owns the
  // initial fetch — the parent re-fetches on commit success.
  useEffect(() => {
    let cancelled = false;
    getRecentCommits(project.path, 10)
      .then((list) => { if (!cancelled) setCommits(list); })
      .catch(() => { if (!cancelled) setCommits([]); });
    return () => { cancelled = true; };
  }, [project.path]);

  const hasIdentity = detected.kind === 'tracked';
  const identity = hasIdentity ? detected.identity : null;
  const untrackedKey = detected.kind === 'untracked' ? detected.keyPath : null;

  // Whether the repo's `.git/config` has both [user] name and
  // email. `git commit` refuses to run without these, and the
  // raw error from git is a wall of English the user does not
  // want to see. The Quick Actions row uses this to disable the
  // Commit button (and surface a tooltip) up front.
  //   - `repoConfig` null means the IPC is still loading; we
  //     treat that as "not known yet" and let the click through
  //     (the backend will refuse if needed).
  //   - hasConfig=false (no .git/config at all) is also "not
  //     known", same treatment.
  const hasUserIdentity =
    !!repoConfig &&
    repoConfig.hasConfig &&
    !!repoConfig.userName?.trim() &&
    !!repoConfig.userEmail?.trim();

  return (
    <div className="flex flex-col gap-5">
      {/* Header */}
      <div>
        <h1 className="text-xl font-extrabold text-text-0">{project.name}</h1>
        <p className="text-xs text-text-2 font-mono truncate mt-0.5">{project.path}</p>
      </div>

      {/* Not-a-git-repo call-to-action. Shown when isGitRepo() has
          resolved to false for this project. Skipped while the check
          is still pending (isRepo === undefined) so we do not flash
          the warning during the initial load. */}
      {isRepo === false && (
        <div className="rounded-xl border border-warning/40 bg-[rgba(245,158,11,0.08)] p-3 flex items-center gap-3">
          <AlertTriangle className="h-5 w-5 text-warning shrink-0 [[data-theme=dark]_&]:text-[#fcd34d]" />
          <div className="flex-1 min-w-0">
            <div className="text-sm font-semibold text-text-0">{t('projects.detail.notARepoTitle')}</div>
            <div className="text-xs text-text-1 mt-0.5">{t('projects.detail.notARepoBody')}</div>
          </div>
          <Button
            variant="default"
            size="sm"
            disabled={initializing || !hasIdentities}
            onClick={onInit}
            className="shrink-0"
          >
            {t('projects.initProject')}
          </Button>
        </div>
      )}

      {/* Remote */}
      <div>
        <SectionLabel>{t('projects.detail.remote')}</SectionLabel>
        <div className="rounded-xl border border-border bg-bg-0 p-3 flex flex-col gap-1.5 text-sm">
          {repoConfig?.remoteUrl ? (
            <>
              <div className="flex items-center gap-2">
                <ProtocolBadge protocol={repoConfig.remoteProtocol} t={t} />
                <span className="text-xs font-mono text-text-1 truncate flex-1 min-w-0">{repoConfig.remoteUrl}</span>
              </div>
              {repoConfig.remoteProtocol === 'unknown' && (
                <p className="text-xs text-warning mt-0.5">
                  {t('projects.detail.remoteUnknownNote')}
                </p>
              )}
            </>
          ) : (
            <div className="flex items-center gap-2">
              <ProtocolBadge protocol={null} t={t} />
              <span className="text-xs text-text-2">
                {t('projects.detail.remoteNone')}
              </span>
            </div>
          )}
        </div>
      </div>

      {/* Git Identity */}
      <div>
        <SectionLabel>{t('projects.detail.identity')}</SectionLabel>
        <div className="rounded-xl border border-border bg-bg-0 p-3 flex flex-col gap-1.5 text-sm">
          {identity ? (
            <>
              <KV label={t('projects.detail.name')} value={identity.label} />
              <KV label={t('projects.detail.email')} value={identity.userEmail || '—'} mono />
              {keyPath && <KV label={t('projects.detail.key')} value={keyPath} mono />}
              {identity.matchPath && <KV label={t('projects.match')} value={identity.matchPath} mono />}
            </>
          ) : untrackedKey ? (
            <div className="text-text-1 text-xs">
              {t('projects.gitUsingUnknownKey', { path: untrackedKey })}
            </div>
          ) : (
            <div className="text-text-2 text-sm">{t('projects.detail.noIdentity')}</div>
          )}
          <div className="flex flex-wrap items-center gap-1.5 pt-1">
            {detected.kind === 'tracked' && detected.source === 'git' && (
              <Badge variant="outline">{t('projects.detectedFromGit')}</Badge>
            )}
            {(detected.kind === 'untracked' || detected.kind === 'none') && (
              <Badge variant="warning">{t('projects.noIdentityBadge')}</Badge>
            )}
          </div>
        </div>
      </div>

      {/* Git Config */}
      {repoConfig && (
        <div>
          <SectionLabel>{t('projects.detail.gitConfig')}</SectionLabel>
          <div className="rounded-xl border border-border bg-bg-0 p-3 flex flex-col gap-1.5 text-sm">
            <KV label={t('projects.detail.userName')} value={repoConfig.userName} />
            <KV label={t('projects.detail.userEmail')} value={repoConfig.userEmail} mono />
            <KV label={t('projects.detail.sshKey')} value={repoConfig.sshKeyPath} mono />
          </div>
        </div>
      )}

      {/* Quick Actions: fetch / pull / commit / push. Only shown
          for projects that ARE git repositories. Buttons are
          disabled when no remote is configured (push/pull) or
          while a git op is already running. */}
      {isRepo !== false && (
        <div>
          <SectionLabel>{t('gitOps.quickActions')}</SectionLabel>
          <div className="rounded-xl border border-border bg-bg-0 p-3 flex flex-col gap-2">
            {/* 3-column grid for the 3 remaining actions (commit /
                pull / push). Keeping it as a single row visually
                groups "git operations" against the separate
                identity / SSH block below. */}
            <div className="grid grid-cols-3 gap-2">
              <Button variant="outline" size="sm" onClick={onPull} disabled={opsBusy || !repoConfig?.remoteUrl}>
                {t('gitOps.pull.label')}
              </Button>
              {/* Commit needs [user] name + email in the repo's
                  .git/config. `repoConfig` is null only when the
                  IPC has not returned yet (e.g. project just
                  added), in which case we still allow the click —
                  the backend will surface a clearer error if it
                  turns out to be missing. The common case (a
                  cloned repo with no per-repo identity) is
                  covered by `!hasUserIdentity`. */}
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="default"
                    size="sm"
                    onClick={onCommit}
                    disabled={opsBusy || !hasUserIdentity}
                  >
                    {t('gitOps.commit.label')}
                  </Button>
                </TooltipTrigger>
                {!hasUserIdentity && (
                  <TooltipContent>{t('gitOps.commit.needIdentity')}</TooltipContent>
                )}
              </Tooltip>
              <Button variant="default" size="sm" onClick={onPush} disabled={opsBusy || !repoConfig?.remoteUrl}>
                {t('gitOps.push.label')}
              </Button>
            </div>
            {status && (
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-text-1">
                {status.porcelain.trim() ? (
                  <span className="text-warning font-semibold [[data-theme=dark]_&]:text-[#fcd34d]">
                    {t('gitOps.status.uncommitted', { count: status.porcelain.split('\n').filter(Boolean).length })}
                  </span>
                ) : (
                  <span>{t('gitOps.status.clean')}</span>
                )}
                {status.hasUpstream && status.ahead != null && status.ahead > 0 && (
                  <span className="text-brand-strong font-semibold [[data-theme=dark]_&]:text-[#93c5fd]">
                    {t('gitOps.status.ahead', { count: status.ahead })}
                  </span>
                )}
                {status.hasUpstream && status.behind != null && status.behind > 0 && (
                  <span className="text-danger font-semibold [[data-theme=dark]_&]:text-[#fca5a5]">
                    {t('gitOps.status.behind', { count: status.behind })}
                  </span>
                )}
                {!repoConfig?.remoteUrl && (
                  <span className="text-text-2">{t('gitOps.status.noRemote')}</span>
                )}
              </div>
            )}
          </div>
        </div>
      )}

      {/* Actions */}
      <div className="flex flex-wrap gap-2">
        {!hasIdentity && (
          <Button variant="default" onClick={onSwitch} disabled={!hasIdentities} className="flex-1">
            {untrackedKey ? t('projects.rebindIdentity') : t('projects.bindIdentity')}
          </Button>
        )}
        {hasIdentity && (
          <Button variant="default" onClick={onSwitch} className="flex-1">
            {t('projects.switchIdentity')}
          </Button>
        )}
        {(repoConfig?.remoteProtocol === 'https' || hasIdentity) && (
          <Button variant="outline" onClick={onTest} className="flex-1">
            {repoConfig?.remoteProtocol === 'https' ? t('projects.testHttps') : t('projects.testSsh')}
          </Button>
        )}
        <Button variant="danger" onClick={onRemove}>{t('projects.removeMenu')}</Button>
      </div>

      {/* Recent commits */}
      <div>
        <SectionLabel>{t('projects.recentCommits')}</SectionLabel>
        <div className="rounded-xl border border-border bg-bg-0 overflow-hidden">
          {commits.length === 0 ? (
            <div className="p-3 text-text-2 text-sm">{t('projects.noCommits')}</div>
          ) : (
            <ul className="divide-y divide-border">
              {commits.map((c) => (
                <li key={c.hash} className="p-2 text-sm font-mono flex gap-3">
                  <span className="text-brand-strong shrink-0 font-semibold [[data-theme=dark]_&]:text-[#93c5fd]">{c.hash.slice(0, 7)}</span>
                  <span className="text-text-1 truncate">{c.subject}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return <div className="text-[11px] font-bold uppercase tracking-wider text-text-2 mb-1.5">{children}</div>;
}

function KV({ label, value, mono = false }: { label: string; value: string | null; mono?: boolean }) {
  return (
    <div className="flex justify-between gap-2 items-baseline">
      <span className="text-text-1 shrink-0 text-xs">{label}</span>
      {value ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <span className={cn('text-text-0 truncate', mono && 'font-mono text-xs')}>{value}</span>
          </TooltipTrigger>
          <TooltipContent className="max-w-md break-all">{value}</TooltipContent>
        </Tooltip>
      ) : (
        <span className="text-text-2">—</span>
      )}
    </div>
  );
}
