import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter, DialogDescription } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { Label, Textarea } from '../../components/ui/input';
import { gitCommit } from '../../ipc/git';
import { toast } from 'sonner';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  projectPath: string;
  projectName: string;
  /// Called with the new HEAD hash on a successful commit. The
  /// caller refreshes the commit list / status row.
  onCommitted: (hash: string) => void;
}

export function CommitDialog({ open, onOpenChange, projectPath, projectName, onCommitted }: Props) {
  const { t } = useTranslation();
  const [message, setMessage] = useState('');
  const [addAll, setAddAll] = useState(true);
  const [busy, setBusy] = useState(false);

  // Reset state when the dialog opens so a previous failed
  // attempt's text does not leak in.
  if (open && message === '' && !busy) {
    // no-op: this branch is only taken once on open; the
    // handleOpenChange wrapper does the real reset on close.
  }

  const handleOpenChange = (v: boolean) => {
    if (!v) {
      setMessage('');
      setAddAll(true);
    }
    onOpenChange(v);
  };

  const submit = async () => {
    const msg = message.trim();
    if (!msg || busy) return;
    setBusy(true);
    try {
      // \`silent: true\` so the global ipc wrapper does not
      // double-toast on failure: we already have a catch below
      // that decides between a soft info toast ("nothing to
      // commit") and a domain-specific error message.
      const hash = await gitCommit(projectPath, msg, addAll, { silent: true });
      toast.success(t('gitOps.commit.success', { hash: hash.slice(0, 7) || '·' }));
      onCommitted(hash);
      onOpenChange(false);
    } catch (e) {
      // "nothing to commit" surfaces as a Validation error
      // from Rust. We use a softer toast (info, not error)
      // to distinguish "user did nothing" from "git blew up".
      const text = String(e);
      if (text.includes('nothing to commit')) {
        toast(t('gitOps.commit.nothingToCommit'));
      } else {
        toast.error(text);
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('gitOps.commit.title')}</DialogTitle>
          <DialogDescription>{t('gitOps.commit.subtitle', { name: projectName })}</DialogDescription>
        </DialogHeader>
        <form
          onSubmit={(e) => { e.preventDefault(); void submit(); }}
          className="space-y-3"
        >
          <div>
            <Label htmlFor="commitMsg">{t('gitOps.commit.message')}</Label>
            <Textarea
              id="commitMsg"
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              placeholder={t('gitOps.commit.messagePlaceholder')}
              rows={4}
              autoFocus
              required
            />
          </div>
          <label className="flex items-center gap-2 text-sm text-text-1 cursor-pointer select-none">
            <input
              type="checkbox"
              checked={addAll}
              onChange={(e) => setAddAll(e.target.checked)}
              className="h-4 w-4 rounded border-border text-brand focus:ring-brand"
            />
            <span>{t('gitOps.commit.addAll')}</span>
          </label>
          <DialogFooter className="gap-2">
            <Button type="button" variant="ghost" onClick={() => handleOpenChange(false)} disabled={busy}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={busy || !message.trim()}>
              {busy ? t('common.committing') : t('gitOps.commit.submit')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
