import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open as openDirDialog } from '@tauri-apps/plugin-dialog';
import { Card } from '../components/ui/card';
import { Button } from '../components/ui/button';
import { Badge } from '../components/ui/badge';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../components/ui/tooltip';
import { useProjectsStore } from '../store/projects';
import { useIdentitiesStore } from '../store/identities';
import { useSettingsStore } from '../store/settings';
import { applyIdentityToRepo, getRecentCommits, getRepoGitConfig, getGlobalGitConfig, isGitRepo, type RepoGitConfig, type GlobalGitConfig } from '../ipc/git';
import { tryUnlockKey, isKeyEncrypted } from '../ipc/sshAdd';
import { IdentitySwitcherDialog } from '../features/identitySwitcher/IdentitySwitcherDialog';
import { PassphraseDialog } from '../features/passphraseDialog/PassphraseDialog';
import { ConnectionTesterDialog } from '../features/connectionTester/ConnectionTesterDialog';
import { ContextMenu } from '../components/ContextMenu';
import { toast } from 'sonner';
import type { Identity } from '../ipc/identities';

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
    const match = identities.find((i) => i.keyPath === repoConfig.sshKeyPath);
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
          out[p.id] = { hasConfig: false, userName: null, userEmail: null, sshKeyPath: null, managedByNicessh: false };
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

  // Default identity: match the global gitconfig key path against our
  // identities. Fallback: nothing (no auto-bind).
  const defaultIdentityId = (() => {
    if (!globalGit?.sshKeyPath) return null;
    const match = identities.find((i) => i.keyPath === globalGit.sshKeyPath);
    return match?.id ?? null;
  })();

  // One-click "Add Project": open system dir picker, validate git repo,
  // write to config.json, and apply the default identity if any.
  const handleAdd = async () => {
    if (adding) return;
    setAdding(true);
    try {
      const picked = await openDirDialog({ directory: true, multiple: false });
      if (typeof picked !== 'string') return; // user cancelled
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
    if (!recentlyUnlocked[target.keyPath]) {
      setSwitcherOpen(false);
      // For unencrypted keys, add to agent without prompting — ssh-add
      // accepts them outright with no passphrase.
      const encrypted = await isKeyEncrypted(target.keyPath);
      if (!encrypted) {
        const ok = await tryUnlockKey(target.keyPath, '');
        if (ok) {
          markKeyUnlocked(target.keyPath);
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
    const ok = await tryUnlockKey(pendingIdentity.keyPath, passphrase);
    if (ok) {
      markKeyUnlocked(pendingIdentity.keyPath);
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
      <div className="flex h-full">
        {/* Left tree */}
        <div className="w-64 border-r border-border bg-bg-1 p-3 overflow-y-auto shrink-0">
          <Button onClick={handleAdd} disabled={adding} className="w-full mb-3">
            {adding ? t('common.adding') : t('projects.addProject')}
          </Button>
          {projects.map((p) => {
            const d = detectIdentity(p, identities, repoConfigs[p.id] ?? null);
            return (
              <button
                key={p.id}
                onClick={() => setSelectedId(p.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setContextMenu({ x: e.clientX, y: e.clientY, projectId: p.id, projectName: p.name });
                }}
                className={`w-full text-left p-2 rounded-md mb-1 ${
                  p.id === selectedId ? 'bg-bg-2 text-text-0' : 'text-text-1 hover:bg-bg-2'
                }`}
              >
                <div className="flex items-center gap-2">
                  <div className="text-sm font-medium truncate flex-1">{p.name}</div>
                  {d.kind === 'tracked' && <span className="w-1.5 h-1.5 rounded-full bg-accent shrink-0" aria-label="identity bound" />}
                  {d.kind === 'untracked' && <span className="w-1.5 h-1.5 rounded-full bg-warning shrink-0" aria-label="untracked key" />}
                </div>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <div className="text-xs text-text-2 truncate font-mono">{p.path}</div>
                  </TooltipTrigger>
                  <TooltipContent>{p.path}</TooltipContent>
                </Tooltip>
              </button>
            );
          })}
        </div>

        {/* Main panel */}
        <div className="flex-1 p-6 overflow-y-auto">
          {!selected ? (
            <div className="text-text-1">{t('projects.selectPrompt')}</div>
          ) : (
            <ProjectDetail
              project={selected}
              detected={detected}
              hasIdentities={identities.length > 0}
              repoConfig={repoConfig}
              onSwitch={() => setSwitcherOpen(true)}
              onTest={() => setTesterOpen(true)}
            />
          )}
        </div>

        <IdentitySwitcherDialog
          open={switcherOpen}
          onOpenChange={setSwitcherOpen}
          identities={identities}
          currentId={selected?.identityId ?? null}
          onSelect={handleSelect}
        />
        {pendingIdentity && (
          <PassphraseDialog
            open={passOpen}
            onOpenChange={(v) => { if (!v) setPendingIdentityId(null); }}
            keyPath={pendingIdentity.keyPath}
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

function ProjectDetail({ project, detected, hasIdentities, repoConfig, onSwitch, onTest }: {
  project: { id: string; name: string; path: string };
  detected: DetectedIdentity;
  hasIdentities: boolean;
  repoConfig: RepoGitConfig | null;
  onSwitch: () => void;
  onTest: () => void;
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
    <div className="space-y-4 max-w-3xl">
      <h1 className="text-2xl font-semibold">{project.name}</h1>
      <Card className="p-4 space-y-2">
        <Row label={t('projects.path')} value={project.path} />
        <Row
          label={t('projects.identity')}
          value={
            identity
              ? `${identity.label} <${identity.userEmail || '—'}>`
              : untrackedKey
                ? t('projects.gitUsingUnknownKey', { path: untrackedKey })
                : t('projects.noIdentity')
          }
        />
        <Row label={t('projects.key')} value={identity?.keyPath ?? untrackedKey ?? '—'} />
        {identity?.matchPath && <Row label={t('projects.match')} value={identity.matchPath} />}
        <div className="flex flex-wrap items-center gap-2 pt-1">
          {detected.kind === 'tracked' && detected.source === 'git' && (
            <Badge variant="outline">{t('projects.detectedFromGit')}</Badge>
          )}
          {(detected.kind === 'untracked' || detected.kind === 'none') && (
            <Badge variant="warning">{t('projects.noIdentityBadge')}</Badge>
          )}
        </div>
      </Card>
      <div className="flex gap-2 items-center flex-wrap">
        {!hasIdentity ? (
          <Button variant="default" onClick={onSwitch} disabled={!hasIdentities}>
            {untrackedKey ? t('projects.rebindIdentity') : t('projects.bindIdentity')}
          </Button>
        ) : (
          <>
            <Button variant="outline" onClick={onSwitch}>{t('projects.switchIdentity')}</Button>
            <Button variant="outline" onClick={onTest}>{t('projects.testSsh')}</Button>
          </>
        )}
      </div>
      {repoConfig && (repoConfig.userName || repoConfig.userEmail) && (
        <div className="text-text-2 text-xs">
          {repoConfig.userName && <span>name = {repoConfig.userName}</span>}
          {repoConfig.userName && repoConfig.userEmail && <span className="mx-2">·</span>}
          {repoConfig.userEmail && <span>email = {repoConfig.userEmail}</span>}
        </div>
      )}
      <div>
        <h2 className="text-sm font-medium text-text-1 mb-2">{t('projects.recentCommits')}</h2>
        <div className="border border-border rounded-md divide-y divide-border max-h-96 overflow-y-auto">
          {commits.length === 0 && <div className="p-3 text-text-2 text-sm">{t('projects.noCommits')}</div>}
          {commits.map((c) => (
            <div key={c.hash} className="p-2 text-sm font-mono flex gap-3">
              <span className="text-accent shrink-0">{c.hash.slice(0, 7)}</span>
              <span className="text-text-1 truncate">{c.subject}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-start gap-3 text-sm">
      <span className="text-text-2 w-20 shrink-0">{label}</span>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="text-text-0 font-mono break-all">{value}</span>
        </TooltipTrigger>
        <TooltipContent className="max-w-md break-all">{value}</TooltipContent>
      </Tooltip>
    </div>
  );
}
