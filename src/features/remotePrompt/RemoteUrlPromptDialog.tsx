import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { Input } from '../../components/ui/input';
import { writeRepoRemote } from '../../ipc/git';
import { toast } from 'sonner';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  projectPath: string;
  /// Optional URL prefilled into the input. The dialog also flips
  /// the protocol dropdown to the inferred value of the prefill.
  /// Used by the "switch remote to SSH…" flow so the user just
  /// confirms a sensible default and clicks Save.
  initialUrl?: string;
  /// Optional label override for the Save button. When omitted the
  /// i18n key `projects.remotePrompt.save` is used.
  saveLabel?: string;
  /// Called with the protocol string once the user successfully
  /// saves a remote URL — the caller decides what to do next (e.g.
  /// retry `applyIdentityToRepo`).
  onSaved: (info: { url: string; protocol: string }) => void;
}

type Proto = 'ssh' | 'https' | 'git' | 'unknown';

function inferProtocol(url: string): Proto {
  const u = url.trim();
  if (u.startsWith('git@') || u.startsWith('ssh://') || u.startsWith('ssh+git://')) return 'ssh';
  if (u.startsWith('https://') || u.startsWith('http://')) return 'https';
  if (u.startsWith('git://')) return 'git';
  return 'unknown';
}

export function RemoteUrlPromptDialog({
  open,
  onOpenChange,
  projectPath,
  initialUrl,
  saveLabel,
  onSaved,
}: Props) {
  const { t } = useTranslation();
  const [url, setUrl] = useState('');
  const [busy, setBusy] = useState(false);

  const inferred = inferProtocol(url);
  const canSave = inferred !== 'unknown';

  // Reset state whenever the dialog is reopened so a previous failed
  // attempt does not leak text in — unless the caller explicitly
  // passed `initialUrl`, in which case we seed the input with it.
  // We intentionally re-run on `initialUrl` too so a parent that
  // rebuilds the dialog with a fresh URL gets the new prefill.
  useEffect(() => {
    if (open) setUrl(initialUrl ?? '');
  }, [open, initialUrl]);

  const handleSave = async () => {
    if (!canSave || busy) return;
    setBusy(true);
    try {
      const protocol = await writeRepoRemote(projectPath, url.trim());
      toast.success(t('projects.remotePrompt.saved'));
      onSaved({ url: url.trim(), protocol });
      onOpenChange(false);
    } catch (e) {
      // Backend rejects unknown URLs with a Validation error; surface
      // the original message so the user knows *why* it failed.
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('projects.remotePrompt.title')}</DialogTitle>
        </DialogHeader>

        <p className="text-sm text-text-2">{t('projects.remotePrompt.body')}</p>

        <div className="flex flex-col gap-1.5">
          <label className="text-xs font-medium text-text-1">
            {t('projects.remotePrompt.urlLabel')}
          </label>
          <Input
            type="text"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="git@github.com:user/repo.git"
            autoFocus
            disabled={busy}
          />
          <div className="flex items-center gap-2 text-xs text-text-2">
            <span>{t('projects.remotePrompt.protocolLabel')}:</span>
            <span className="font-mono">
              {inferred === 'unknown'
                ? '—'
                : inferred.toUpperCase()}
            </span>
          </div>
          {inferred === 'unknown' && url.trim() !== '' && (
            <p className="text-xs text-danger">{t('projects.remotePrompt.invalidUrl')}</p>
          )}
        </div>

        <DialogFooter className="gap-2">
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
            {t('projects.remotePrompt.cancel')}
          </Button>
          <Button onClick={handleSave} disabled={!canSave || busy}>
            {busy ? t('common.loading') : (saveLabel ?? t('projects.remotePrompt.save'))}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
