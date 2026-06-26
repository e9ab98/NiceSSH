import { useEffect, useMemo, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { open as openDirDialog } from '@tauri-apps/plugin-dialog';
import { CircleSlash, AlertTriangle, FolderGit2, UserCircle2, MousePointer, type LucideIcon } from 'lucide-react';
import { Button } from '../components/ui/button';
import { Badge } from '../components/ui/badge';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../components/ui/tooltip';
import { useProjectsStore } from '../store/projects';
import { useIdentitiesStore } from '../store/identities';
import { useSettingsStore } from '../store/settings';
import { applyIdentityToRepo, getRecentCommits, getRepoGitConfig, getGlobalGitConfig, isGitRepo, setGlobalGitConfig, type RepoGitConfig, type GlobalGitConfig } from '../ipc/git';
import { tryUnlockKey, isKeyEncrypted } from '../ipc/sshAdd';
import { IdentitySwitcherDialog } from '../features/identitySwitcher/IdentitySwitcherDialog';
import { RepoAuditDialog } from '../features/repoAudit/RepoAuditDialog';
import { PassphraseDialog } from '../features/passphraseDialog/PassphraseDialog';
import { ConnectionTesterDialog } from '../features/connectionTester/ConnectionTesterDialog';
import { ContextMenu } from '../components/ContextMenu';
import { toast } from 'sonner';
import { cn } from '../lib/utils';
import type { Identity } from '../ipc/identities';
import { fullKeyPath } from '../lib/keyPath';

type DetectedIdentity =
  | { kind: 'none' }
  | { kind: 'untracked'; keyPath: string }
  | { kind: 'tracked'; identity: Identity; source: 'config' | 'git' };

function detectIdentity(
  project: { id: string; name: string; path: string; identityId: string | null },
  identities: Identity[],
  repoConfig: RepoGitConfig | null,
): DetectedIdentity {
  if (project.identityId) {
    const found = identities.find((i) => i.id === project.identityId);
    if (found) return { kind: 'tracked', identity: found, source: 'config' };
  }
  if (repoConfig?.sshKeyPath) {
    const match = identities.find((i) => fullKeyPath(i) === repoConfig.sshKeyPath);
    if (match) return { kind: 'tracked', identity: match, source: 'git' };
    return { kind: 'untracked', keyPath: repoConfig.sshKeyPath };
  }
  return { kind: 'none' };
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
  const add = useProjectsStore((s) => s.add);
  const remove = useProjectsStore((s) => s.remove);
  const assign = useProjectsStore((s) => s.assign);
  const identities = useIdentitiesStore((s) => s.items);
  const refreshIdentities = useIdentitiesStore((s) => s.refresh);
  const markKeyUnlocked = useSettingsStore((s) => s.markKeyUnlocked);
  const recentlyUnlocked = useSettingsStore((s) => s.recentlyUnlockedKeys);

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
  const refreshRepoConfigs = useCallback(async () => {
    const out: Record<string, RepoGitConfig> = {};
    for (const p of projects) {
      try {
        out[p.id] = await getRepoGitConfig(p.path);
      } catch {
        out[p.id] = { hasConfig: false, userName: null, userEmail: null, sshKeyPath: null, managedByNicessh: false, sshCommandCount: 0 };
      }
    }
    setRepoConfigs(out);
  }, [projects]);

  useEffect(() => { refreshProjects(); refreshIdentities(); }, []);

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
          out[p.id] = { hasConfig: false, userName: null, userEmail: null, sshKeyPath: null, managedByNicessh: false, sshCommandCount: 0 };
        }
      }
      if (!cancelled) setRepoConfigs(out);
    })();
    return () => { cancelled = true; };
  }, [projects]);

  const selected = projects.find((p) => p.id === selectedId) ?? null;
  const repoConfig = selected ? (repoConfigs[selected.id] ?? null) : null;
  const detected = selected ? detectIdentity(selected, identities, repoConfig) : { kind: 'none' as const };
  const identity = detected.kind === 'tracked' ? detected.identity : null;
  const pendingIdentity = identities.find((i) => i.id === pendingIdentityId) ?? null;

  // Stats: derived from projects + identities + repoConfigs
  const stats = useMemo(() => {
    let bound = 0, unbound = 0, errors = 0;
    for (const p of projects) {
      const cfg = repoConfigs[p.id];
      const d = detectIdentity(p, identities, cfg ?? null);
      if (d.kind === 'tracked') bound++;
      else unbound++;
    }
    // errors: projects whose getRepoGitConfig returned hasConfig=false (loaded as fallback)
    errors = Object.values(repoConfigs).filter((c) => c && !c.hasConfig).length;
    return { total: projects.length, bound, unbound, errors };
  }, [projects, identities, repoConfigs]);

  const defaultIdentityId = (() => {
    if (!globalGit?.sshKeyPath) return null;
    const match = identities.find((i) => i.keyPath === globalGit.sshKeyPath);
    return match?.id ?? null;
  })();

  const handleAdd = async () => {
    if (adding) return;
    setAdding(true);
    try {
      const picked = await openDirDialog({ directory: true, multiple: false });
      if (typeof picked !== 'string') return;
      if (!(await isGitRepo(picked))) {
        toast.error(t('addProject.notGitRepo'));
        return;
      }
      const project = await add({ name: deriveName(picked), path: picked, identityId: defaultIdentityId });
      if (defaultIdentityId) {
        await applyIdentityToRepo(project.id, defaultIdentityId);
        toast.success(t('projects.addedWithIdentity'));
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

  const performSwitch = async (targetIdentityId: string) => {
    if (!selected) return;
    await applyIdentityToRepo(selected.id, targetIdentityId);
    await assign(selected.id, targetIdentityId);
    const t2 = identities.find((i) => i.id === targetIdentityId);
    if (t2) toast.success(t('projects.switchedTo', { label: t2.label }));
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
    const fullKp = fullKeyPath(target); if (!recentlyUnlocked[fullKp]) {
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
    const ok = await tryUnlockKey(fullKeyPath(pendingIdentity), passphrase);
    if (ok) {
      markKeyUnlocked(fullKeyPath(pendingIdentity));
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

  const handleSetAsGlobal = async () => {
    if (!identity) return;
    const target = identities.find((i) => i.id === identity.id);
    if (!target) return;
    if (!confirm(t('projects.setAsGlobalConfirm', { label: target.label }))) return;
    try {
      const result = await setGlobalGitConfig(target.id);
      toast.success(
        t('projects.setAsGlobalApplied', {
          label: target.label,
          email: result.userEmail,
        })
      );
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
                    const d = detectIdentity(p, identities, cfg);
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
                detected={detected}
                hasIdentities={identities.length > 0}
                repoConfig={repoConfig}
                onSwitch={() => setSwitcherOpen(true)}
                onTest={() => setTesterOpen(true)}
                onRemove={() => { void handleRemove(selected.id); }}
                onSetAsGlobal={() => { void handleSetAsGlobal(); }}
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
          onOpenChange={setSwitcherOpen}
          identities={identities}
          currentId={selected?.identityId ?? null}
          projectPath={selected?.path ?? null}
          onSelect={handleSelect}
        />
        <RepoAuditDialog
          open={auditOpen}
          onOpenChange={setAuditOpen}
          onChanged={refreshRepoConfigs}
        />
        {pendingIdentity && (
          <PassphraseDialog
            open={passOpen}
            onOpenChange={(v) => { if (!v) setPendingIdentityId(null); }}
            keyPath={fullKeyPath(pendingIdentity)}
            onUnlock={handleUnlock}
          />
        )}
        {identity && (
          <ConnectionTesterDialog
            open={testerOpen}
            onOpenChange={setTesterOpen}
            identityId={identity.id}
            identityLabel={identity.label}
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
    </TooltipProvider>
  );
}

function ProjectDetail({ project, detected, hasIdentities, repoConfig, onSwitch, onTest, onRemove, onSetAsGlobal }: {
  project: { id: string; name: string; path: string };
  detected: DetectedIdentity;
  hasIdentities: boolean;
  repoConfig: RepoGitConfig | null;
  onSwitch: () => void;
  onTest: () => void;
  onRemove: () => void;
  onSetAsGlobal: () => void;
}) {
  const { t } = useTranslation();
  const [commits, setCommits] = useState<{ hash: string; subject: string }[]>([]);
  useEffect(() => {
    getRecentCommits(project.path, 10).then(setCommits).catch(() => setCommits([]));
  }, [project.path]);

  const hasIdentity = detected.kind === 'tracked';
  const identity = hasIdentity ? detected.identity : null;
  const untrackedKey = detected.kind === 'untracked' ? detected.keyPath : null;

  return (
    <div className="flex flex-col gap-5">
      {/* Header */}
      <div>
        <h1 className="text-xl font-extrabold text-text-0">{project.name}</h1>
        <p className="text-xs text-text-2 font-mono truncate mt-0.5">{project.path}</p>
      </div>

      {/* Git Identity */}
      <div>
        <SectionLabel>{t('projects.detail.identity')}</SectionLabel>
        <div className="rounded-xl border border-border bg-bg-0 p-3 flex flex-col gap-1.5 text-sm">
          {identity ? (
            <>
              <KV label={t('projects.detail.name')} value={identity.label} />
              <KV label={t('projects.detail.email')} value={identity.userEmail || '—'} mono />
              {identity.keyPath && <KV label={t('projects.detail.key')} value={identity.keyPath} mono />}
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

      {/* Actions */}
      <div className="flex flex-wrap gap-2">
        {!hasIdentity ? (
          <Button variant="default" onClick={onSwitch} disabled={!hasIdentities} className="flex-1">
            {untrackedKey ? t('projects.rebindIdentity') : t('projects.bindIdentity')}
          </Button>
        ) : (
          <>
            <Button variant="default" onClick={onSwitch} className="flex-1">
              {t('projects.switchIdentity')}
            </Button>
            <Button variant="outline" onClick={onTest} className="flex-1">
              {t('projects.testSsh')}
            </Button>
          </>
        )}
        {hasIdentity && (
          <Button variant="ghost" onClick={onSetAsGlobal} className="w-full">
            {t('projects.setAsGlobalDefault')}
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
