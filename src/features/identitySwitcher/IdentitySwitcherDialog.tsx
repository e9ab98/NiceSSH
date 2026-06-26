import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { Input, Label } from '../../components/ui/input';
import { Badge } from '../../components/ui/badge';
import { Tabs, TabsList, TabsTrigger } from '../../components/ui/tabs';
import { setGlobalGitConfig, getGlobalGitConfig } from '../../ipc/git';
import { updateIdentity } from '../../ipc/identities';
import { toast } from 'sonner';
import { cn } from '../../lib/utils';
import type { Identity } from '../../ipc/identities';

type Scope = 'project' | 'global';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  identities: Identity[];
  currentId: string | null;
  // Path of the project the user is binding. Used as the default value
  // for the matchPath input (so by default the project path itself
  // becomes the includeIf gitdir prefix).
  projectPath?: string | null;
  onSelect: (id: string) => Promise<void> | void;
}

function dirname(p: string): string {
  if (!p) return '';
  const trimmed = p.replace(/\/+$/, '');
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  return idx >= 0 ? trimmed.slice(0, idx + 1) : '';
}

/**
 * Compute the initial value of the "match path" input shown in the
 * IdentitySwitcherDialog.
 *
 * Priority:
 *   1. `currentMatchPath` — the matchPath of the currently-bound identity,
 *      if non-empty. Editing it lets the user re-bind an existing
 *      identity to a different directory prefix.
 *   2. `projectPath` — the path of the repo the user just opened. We
 *      default to the repo itself (not its parent) so the resulting
 *      `includeIf "gitdir:<repo>/"` is a valid single-repo binding.
 *      We ensure a trailing `/` to mirror what the Rust
 *      `git_config::append_include_if` writes.
 *   3. Empty string — nothing to seed.
 *
 * Exported separately so a unit test can pin the rule.
 */
export function computeMatchPathSeed(
  currentMatchPath: string | null | undefined,
  projectPath: string | null | undefined,
): string {
  const trimmed = currentMatchPath?.trim();
  if (trimmed) return trimmed;
  if (projectPath) {
    return projectPath.endsWith('/') ? projectPath : projectPath + '/';
  }
  return '';
}

export function IdentitySwitcherDialog({ open, onOpenChange, identities, currentId, projectPath, onSelect }: Props) {
  const { t } = useTranslation();
  const [scope, setScope] = useState<Scope>('project');
  const [busy, setBusy] = useState(false);
  const [globalIdentityId, setGlobalIdentityId] = useState<string | null>(null);
  // The match path input mirrors `currentId`'s identity.matchPath when
  // the dialog opens so the user can edit it before confirming the bind.
  // We store it as raw user input (not normalized) and only normalize
  // at submit time.
  const [matchPathInput, setMatchPathInput] = useState<string>('');

  // When the dialog opens, fetch the current global identity so we can
  // highlight it in the list under "Global" scope. Also seed the
  // matchPath input from the currently-bound identity (or project path).
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    (async () => {
      try {
        const cfg = await getGlobalGitConfig();
        if (cancelled || !cfg.sshKeyPath) return;
        const match = identities.find((i) => i.keyPath === cfg.sshKeyPath);
        if (match) setGlobalIdentityId(match.id);
      } catch {
        // ignore — best-effort
      }
    })();
    // Seed the match path input from the currently-bound identity's
    // matchPath, or fall back to the project's own path. See
    // `computeMatchPathSeed` for the rule.
    const current = identities.find((i) => i.id === currentId);
    setMatchPathInput(computeMatchPathSeed(current?.matchPath, projectPath));
    return () => { cancelled = true; };
  }, [open, currentId, identities, projectPath]);

  const handleSelect = async (id: string) => {
    if (busy) return;
    setBusy(true);
    try {
      if (scope === 'project') {
        // Persist the matchPath back to the identity *before* applying
        // it, so applyIdentityToRepo can read the latest value.
        const current = identities.find((i) => i.id === id);
        const normalized = matchPathInput.trim() || null;
        if (current && (current.matchPath ?? null) !== normalized) {
          try {
            const updated = await updateIdentity(id, { ...current, matchPath: normalized });
            // Reflect locally so handleSelect's onSelect sees the new value
            // and so the list re-renders. (The store will refresh on the
            // caller's next listIdentities.)
            current.matchPath = normalized;
            // Surface the change in case the caller doesn't toast it.
            toast.success(t('identitySwitcher.matchPathUpdated'));
            // Use the updated identity in case the parent cares
            void updated;
          } catch (e) {
            toast.error(String(e));
            return; // don't proceed with applyIdentityToRepo if the write failed
          }
        }
        await onSelect(id);
        onOpenChange(false);
      } else {
        const result = await setGlobalGitConfig(id);
        const target = identities.find((i) => i.id === id);
        toast.success(
          t('identitySwitcher.globalApplied', {
            label: target?.label ?? '',
            email: result.userEmail,
          })
        );
        setGlobalIdentityId(id);
        onOpenChange(false);
      }
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const browseMatchDir = async () => {
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        defaultPath: projectPath || undefined,
      });
      if (typeof picked === 'string' && picked.length > 0) {
        // Use the directory itself (strip filename if any) as the match path.
        const dir = dirname(picked) || (picked.endsWith('/') ? picked : picked + '/');
        setMatchPathInput(dir);
      }
    } catch {
      // user cancelled — ignore
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader><DialogTitle>{t('identitySwitcher.title')}</DialogTitle></DialogHeader>

        <Tabs value={scope} onValueChange={(v) => setScope(v as Scope)}>
          <TabsList className="w-full grid grid-cols-2">
            <TabsTrigger value="project">{t('identitySwitcher.scope.project')}</TabsTrigger>
            <TabsTrigger value="global">{t('identitySwitcher.scope.global')}</TabsTrigger>
          </TabsList>
        </Tabs>

        <p className="text-xs text-text-1 -mt-1">
          {scope === 'project'
            ? t('identitySwitcher.scopeHint.project')
            : t('identitySwitcher.scopeHint.global')}
        </p>

        {scope === 'project' && (
          <div className="space-y-1">
            <Label htmlFor="matchPath">{t('identitySwitcher.matchPathLabel')}</Label>
            <div className="flex gap-2">
              <Input
                id="matchPath"
                value={matchPathInput}
                onChange={(e) => setMatchPathInput(e.target.value)}
                placeholder={t('identitySwitcher.matchPathPlaceholder')}
                className="flex-1"
              />
              <Button type="button" variant="outline" onClick={browseMatchDir}>
                {t('identitySwitcher.matchPathBrowse')}
              </Button>
            </div>
            <div className="text-text-2 text-xs">{t('identitySwitcher.matchPathHint')}</div>
          </div>
        )}

        <div className="space-y-2 max-h-[40vh] overflow-y-auto">
          {identities.length === 0 && (
            <div className="text-text-1 text-sm py-4 text-center">{t('identitySwitcher.empty')}</div>
          )}
          {identities.map((id) => {
            const isCurrent = scope === 'project' ? id.id === currentId : id.id === globalIdentityId;
            return (
              <button
                key={id.id}
                onClick={() => !isCurrent && handleSelect(id.id)}
                disabled={isCurrent || busy}
                className={cn(
                  'w-full text-left p-3 rounded-md border transition-colors',
                  isCurrent
                    ? 'border-brand bg-brand-soft cursor-default'
                    : 'border-border hover:bg-bg-2 hover:border-border-strong'
                )}
              >
                <div className="flex items-center justify-between">
                  <span className="font-semibold">{id.label}</span>
                  {isCurrent && <Badge variant="outline">{t('common.current')}</Badge>}
                </div>
                <div className="text-text-1 text-xs mt-1">{id.userEmail}</div>
                <div className="text-text-2 text-xs mt-0.5 font-mono truncate">{id.keyPath || ''}</div>
              </button>
            );
          })}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
            {t('common.cancel')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
