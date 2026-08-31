import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { Input, Label } from '../../components/ui/input';
import { Badge } from '../../components/ui/badge';
import { updateIdentity } from '../../ipc/identities';
import { toast } from 'sonner';
import { cn } from '../../lib/utils';
import type { Identity } from '../../ipc/identities';
import { useKeysStore, useIdentitiesStore } from '../../store/identities';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  identities: Identity[];
  currentId: string | null;
  // Path of the project the user is binding. Used as the default value
  // for the matchPath input (so by default the project path itself
  // becomes the includeIf gitdir prefix).
  projectPath?: string | null;
  /// Remote protocol of the project, if known. When `'https'` (or
  /// `'http'`) the switcher treats sshKeyId as optional — HTTPS
  /// remotes use git-credential, not SSH, so a key binding is moot
  /// for `git push`/`pull` and would only add noise. When `'ssh'`
  /// (or anything else, including `null`/`unknown`) we keep the old
  /// behaviour: reject identities without a key path and show it
  /// in the row.
  projectProtocol?: string | null;
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

export function IdentitySwitcherDialog({ open, onOpenChange, identities, currentId, projectPath, projectProtocol, onSelect }: Props) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  // The match path input mirrors `currentId`'s identity.matchPath when
  // the dialog opens so the user can edit it before confirming the bind.
  // We store it as raw user input (not normalized) and only normalize
  // at submit time.
  const [matchPathInput, setMatchPathInput] = useState<string>('');
  const keys = useKeysStore((s) => s.items);
  const refreshKeys = useKeysStore((s) => s.refresh);

  // Protocol state machine. Drives both the list filter and the
  // dialog title / scope hint. Three cases instead of two because
  // "no remote URL" is meaningfully different from "SSH remote":
  //
  //   * `'https'` / `'http'` — show only user-only identities
  //     (`sshKeyId === null`). HTTPS remotes use git-credential, not
  //     SSH, so the SSH key bound to the identity is irrelevant for
  //     `git push`/`pull` and we hide it from the list to keep
  //     the user's mental model clean.
  //   * `'ssh'` / `'git'` / `'unknown'` — show only SSH-keyed
  //     identities (`sshKeyId !== null`). Rust conservatively writes
  //     `[core] sshCommand` for all three (unknown-with-URL gets
  //     the same ssh-style treatment), so a user-only identity
  //     would produce an sshCommand pointing at a missing key path
  //     and the push would silently break.
  //   * `null` / `undefined` — project has no `[remote "origin"]
  //     url` yet, or `repoConfig` hasn't loaded. Show all
  //     identities. Whatever the user picks, the Rust backend's
  //     `apply_identity_to_repo` returns `BindOutcome::NeedsRemote`
  //     and the `RemoteUrlPromptDialog` asks for a URL before the
  //     bind completes. Filtering here to SSH-only would silently
  //     hide user-only identities the user has been using
  //     everywhere else and leave the dialog empty with no
  //     recovery path — the bug we're avoiding.
  type SwitcherMode = 'https' | 'ssh' | 'any';
  const mode: SwitcherMode =
    projectProtocol === 'https' || projectProtocol === 'http'
      ? 'https'
      : projectProtocol === 'ssh' ||
          projectProtocol === 'git' ||
          projectProtocol === 'unknown'
        ? 'ssh'
        : 'any';

  const filteredIdentities =
    mode === 'https'
      ? identities.filter((id) => !id.sshKeyId)
      : mode === 'ssh'
        ? identities.filter((id) => !!id.sshKeyId)
        : identities; // 'any': no filter — Rust needs-remote will gate the bind

  // The dialog now only handles the *project* scope: the global
  // default is set in the Identities view via GlobalDefaultSection.
  // We still seed the matchPath input from the currently-bound
  // identity's matchPath, falling back to the project path. The
  // effect deliberately does NOT depend on `identities` or `keys`
  // directly: both come from zustand stores that replace their
  // items array on every refresh, so depending on them would cause
  // this effect to re-fire on every store update and trigger an
  // unbounded refresh -> set -> re-render loop that pegs the
  // webview CPU (the original cause of the white-screen crash).
  useEffect(() => {
    if (!open) return;
    // Read straight from the store so this effect's deps stay small.
    const current = useIdentitiesStore.getState().items.find((i) => i.id === currentId);
    setMatchPathInput(computeMatchPathSeed(current?.matchPath, projectPath));
    void refreshKeys();
  }, [open, currentId, projectPath, refreshKeys]);

  const handleSelect = async (id: string) => {
    if (busy) return;
    setBusy(true);
    try {
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
        <DialogHeader>
          <DialogTitle>
            {mode === 'https'
              ? t('identitySwitcher.titleHttps')
              : mode === 'ssh'
                ? t('identitySwitcher.titleSsh')
                : t('identitySwitcher.title')}
          </DialogTitle>
        </DialogHeader>

        <p className="text-xs text-text-1 -mt-1">
          {mode === 'https'
            ? t('identitySwitcher.scopeHint.https')
            : mode === 'ssh'
              ? t('identitySwitcher.scopeHint.ssh')
              : t('identitySwitcher.scopeHint.project')}
        </p>

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

        <div className="space-y-2 max-h-[40vh] overflow-y-auto">
          {filteredIdentities.length === 0 && (
            <div className="text-text-1 text-sm py-4 text-center">
              {identities.length === 0
                ? t('identitySwitcher.empty')
                : mode === 'https'
                  ? t('identitySwitcher.emptyHttps')
                  : mode === 'ssh'
                    ? t('identitySwitcher.emptySsh')
                    : t('identitySwitcher.empty')}
            </div>
          )}
          {filteredIdentities.map((id) => {
            const isCurrent = id.id === currentId;
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
                {mode !== 'https' && (
                  <div className="text-text-2 text-xs mt-0.5 font-mono truncate">
                    {keys.find((key) => key.id === id.sshKeyId)?.privatePath || t('identities.noKeyBound')}
                  </div>
                )}
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
