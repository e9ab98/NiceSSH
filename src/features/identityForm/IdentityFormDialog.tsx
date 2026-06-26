import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Input, Label } from '../../components/ui/input';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../../components/ui/tooltip';
import { Button } from '../../components/ui/button';
import type { Identity } from '../../ipc/identities';
import { basename, dirname, joinKeyPath, looksLikeKeyFile, sanitizeLabel, splitKeyPath } from '../../lib/keyPath';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  initial?: Identity;
  // Prefill key directory when creating a new identity (used by the SSH
  // Keys view's "Create identity for this key" button). Ignored when
  // `initial` is set.
  defaultKeyPath?: string;
  // Prefill the label when the caller already knows it (e.g. SshKeysView
  // has the real key's basename). When provided we skip the basename
  // heuristic in splitKeyPath so a key whose filename doesn't start with
  // `id_` is not silently treated as a directory. Ignored when `initial`
  // is set.
  defaultLabel?: string;
  onSubmit: (values: Omit<Identity, 'id'>) => Promise<void>;
}


export function IdentityFormDialog({ open, onOpenChange, initial, defaultKeyPath, defaultLabel, onSubmit }: Props) {
  const { t } = useTranslation();
  const { dir: initialDir, label: splitLabel } = splitKeyPath(
    initial?.keyPath ?? defaultKeyPath ?? '~/.ssh/id_ed25519',
    'id_ed25519',
  );
  const initialLabel = initial?.label ?? defaultLabel ?? splitLabel;
  const [label, setLabel] = useState<string>(initial?.label ?? initialLabel);
  const [labelDirty, setLabelDirty] = useState<boolean>(!!initial);
  const [userName, setUserName] = useState(initial?.userName ?? '');
  const [userEmail, setUserEmail] = useState(initial?.userEmail ?? '');
  // The stored value of keyPath is *just the directory* (no filename).
  // We always normalise to end with `/` for clarity and to match what
  // KeyGeneratorDialog writes back.
  const [keyPath, setKeyPath] = useState<string>(initialDir);
  const [hostAlias, setHostAlias] = useState(initial?.hostAlias ?? 'github.com');
  const [gitHost, setGitHost] = useState(initial?.gitHost ?? 'github.com');
  const [busy, setBusy] = useState(false);
  // Base label auto-derived from current keyPath. Used to keep the
  // label in sync until the user types into it.
  const labelBaseRef = useRef<string>(initialLabel);

  // Re-seed the local state when the dialog opens. This handles the
  // case where the user opens the dialog, changes their mind, cancels,
  // and re-opens — without this we would keep stale values.
  useEffect(() => {
    if (!open) return;
    const seed = splitKeyPath(
      initial?.keyPath ?? defaultKeyPath ?? '~/.ssh/id_ed25519',
      'id_ed25519',
    );
    setKeyPath(seed.dir);
    setLabel(initial?.label ?? defaultLabel ?? seed.label);
    setLabelDirty(!!initial);
    labelBaseRef.current = initial?.label ?? defaultLabel ?? seed.label;
    setUserName(initial?.userName ?? '');
    setUserEmail(initial?.userEmail ?? '');
    setHostAlias(initial?.hostAlias ?? 'github.com');
    setGitHost(initial?.gitHost ?? 'github.com');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const updateKeyPath = (next: string) => {
    // We treat the user input as a directory; if it ends with what
    // looks like an SSH key filename (e.g. `id_ed25519`, `id_work.pub`),
    // strip that filename and keep only the directory portion.
    //
    // Note: do NOT add a trailing slash unconditionally — the user
    // may have intentionally omitted it. See splitKeyPath doc.
    const trimmed = next.replace(/[\\/]+$/, '');
    const lastSeg = basename(trimmed);
    let dirOnly: string;
    if (lastSeg && looksLikeKeyFile(lastSeg)) {
      // Looks like a file path — keep only the directory part.
      const d = dirname(trimmed);
      dirOnly = d === '' ? '~/.ssh/' : d;
    } else {
      // Treat the whole path as a directory.
      dirOnly = trimmed;
    }
    if (dirOnly === '') dirOnly = '~/.ssh/';
    // Preserve user-provided trailing slash: if they typed a slash, keep
    // one; otherwise leave it off. We only normalise empty / single-
    // slash to "~/.ssh/".
    const wantsTrailing = /[\\/]$/.test(next) && dirOnly !== '~/.ssh/';
    if (wantsTrailing && !dirOnly.endsWith('/')) dirOnly += '/';
    setKeyPath(dirOnly);
    // If the user hasn't manually edited the label, refresh it from
    // whatever the original full path ended with.
    if (!labelDirty && lastSeg && looksLikeKeyFile(lastSeg)) {
      const derived = sanitizeLabel(lastSeg.endsWith('.pub') ? lastSeg.slice(0, -4) : lastSeg);
      if (derived) {
        setLabel(derived);
        labelBaseRef.current = derived;
      }
    }
  };

  const browseKeyDir = async () => {
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        defaultPath: await defaultSshPath(),
      });
      if (typeof picked === 'string' && picked.length > 0) {
        updateKeyPath(picked);
      }
    } catch {
      // user cancelled or dialog unavailable — ignore
    }
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    const cleanLabel = sanitizeLabel(label.trim());
    if (!cleanLabel) return;
    setBusy(true);
    try {
      await onSubmit({
        label: cleanLabel,
        userName,
        userEmail,
        // Persist the keyPath as-typed (no trailing-slash normalisation).
        // Join with the label happens at *display* time via joinKeyPath.
        keyPath,
        // matchPath is no longer edited in this form — preserve whatever
        // was already stored. IdentitySwitcherDialog is the only place
        // that mutates it now.
        matchPath: initial?.matchPath ?? null,
        hostAlias: hostAlias || null,
        gitHost: gitHost || null,
      });
      onOpenChange(false);
    } finally {
      setBusy(false);
    }
  };

  const reset = () => {
    const seed = splitKeyPath(defaultKeyPath ?? '~/.ssh/id_ed25519', 'id_ed25519');
    setLabel(defaultLabel ?? seed.label);
    setLabelDirty(false);
    setUserName('');
    setUserEmail('');
    setKeyPath(seed.dir);
    labelBaseRef.current = defaultLabel ?? seed.label;
    setHostAlias('github.com');
    setGitHost('github.com');
  };

  const handleOpenChange = (v: boolean) => {
    if (!v) reset();
    onOpenChange(v);
  };

  const displayKeyPath = joinKeyPath(keyPath, label || 'id_ed25519');

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent>
        <DialogHeader><DialogTitle>{initial ? t('identityForm.editTitle') : t('identityForm.newTitle')}</DialogTitle></DialogHeader>
        <form onSubmit={submit} className="space-y-3">
          <div>
            <Label htmlFor="label">{t('identityForm.label')}</Label>
            {/* In create mode we want the input + optional reset button on
                the same row (matches the original layout). In edit mode
                the input is disabled, so we drop the reset button and
                wrap the input in a span to give Radix a hover target. */}
            {initial ? (
              <TooltipProvider delayDuration={150}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span className="block flex-1">
                      <Input
                        id="label"
                        value={label}
                        required
                        readOnly
                        disabled
                        placeholder="id_ed25519"
                        className="flex-1"
                      />
                    </span>
                  </TooltipTrigger>
                  <TooltipContent>
                    {t('identityForm.labelImmutable')}
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
            ) : (
              <div className="flex gap-2 items-start">
                <Input
                  id="label"
                  value={label}
                  onChange={(e) => { setLabel(e.target.value); setLabelDirty(true); }}
                  required
                  placeholder="id_ed25519"
                  className="flex-1"
                />
                {labelDirty && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      const derived = sanitizeLabel(basename(keyPath) || 'id_ed25519');
                      setLabel(derived);
                      setLabelDirty(false);
                    }}
                  >
                    {t('identityForm.resetLabel')}
                  </Button>
                )}
              </div>
            )}
            <div className="text-text-2 text-xs mt-1">{t('identityForm.labelHint')}</div>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <Label htmlFor="userName">{t('identityForm.userName')}</Label>
              <Input id="userName" value={userName} onChange={(e) => setUserName(e.target.value)} required />
            </div>
            <div>
              <Label htmlFor="userEmail">{t('identityForm.userEmail')}</Label>
              <Input id="userEmail" type="email" value={userEmail} onChange={(e) => setUserEmail(e.target.value)} required />
            </div>
          </div>

          <div>
            <Label htmlFor="keyPath">{t('identityForm.keyPath')}</Label>
            {initial ? (
              <TooltipProvider delayDuration={150}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span className="block flex-1">
                      <Input
                        id="keyPath"
                        value={keyPath}
                        required
                        readOnly
                        disabled
                        className="flex-1"
                      />
                    </span>
                  </TooltipTrigger>
                  <TooltipContent>
                    {t('identityForm.keyPathImmutable')}
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
            ) : (
              <div className="flex gap-2">
                <Input
                  id="keyPath"
                  value={keyPath}
                  onChange={(e) => updateKeyPath(e.target.value)}
                  required
                  className="flex-1"
                />
                <Button type="button" variant="outline" onClick={browseKeyDir}>
                  {t('identityForm.browseKey')}
                </Button>
              </div>
            )}
            <div className="text-text-2 text-xs mt-1">{t('identityForm.keyPathHint')}</div>
            <div className="text-text-2 text-xs mt-0.5 font-mono break-all">
              → {displayKeyPath}
            </div>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <Label htmlFor="hostAlias">{t('identityForm.hostAlias')}</Label>
              <Input id="hostAlias" value={hostAlias} onChange={(e) => setHostAlias(e.target.value)} placeholder="github.com" />
            </div>
            <div>
              <Label htmlFor="gitHost">{t('identityForm.gitHost')}</Label>
              <Input id="gitHost" value={gitHost} onChange={(e) => setGitHost(e.target.value)} placeholder="github.com" />
            </div>
          </div>
          <DialogFooter className="gap-2">
            <Button type="button" variant="ghost" onClick={() => handleOpenChange(false)}>{t('common.cancel')}</Button>
            <Button type="submit" disabled={busy}>{busy ? t('common.saving') : t('common.save')}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

// Helpers — best-effort default paths for the dialog. We try to give the
// picker a sensible starting location (e.g. ~/.ssh) without failing if
// the path can't be resolved.
async function defaultSshPath(): Promise<string | undefined> {
  try {
    const { homeDir } = await import('@tauri-apps/api/path');
    const home = await homeDir();
    return `${home}/.ssh`;
  } catch {
    return undefined;
  }
}

