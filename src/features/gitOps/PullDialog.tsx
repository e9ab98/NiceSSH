import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter, DialogDescription } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { gitPull } from '../../ipc/git';
import { toast } from 'sonner';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  projectPath: string;
  projectName: string;
  onPulled: () => void;
}

/// Pull confirmation dialog. Pull is destructive-ish (can
/// produce merge commits or rewrite history with --rebase), so
/// we always show a confirmation step — and bundle the rebase
/// option here rather than as a separate top-level button,
/// because the user typically picks rebase/merge once per
/// project and not per pull.
export function PullDialog({ open, onOpenChange, projectPath, projectName, onPulled }: Props) {
  const { t } = useTranslation();
  const [rebase, setRebase] = useState(false);
  const [busy, setBusy] = useState(false);

  const handleOpenChange = (v: boolean) => {
    if (!v) setRebase(false);
    onOpenChange(v);
  };

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await gitPull(projectPath, rebase);
      toast.success(t('gitOps.pull.success', { name: projectName }));
      onPulled();
      onOpenChange(false);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('gitOps.pull.title')}</DialogTitle>
          <DialogDescription>{t('gitOps.pull.subtitle', { name: projectName })}</DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <label className="flex items-start gap-2 text-sm text-text-1 cursor-pointer select-none">
            <input
              type="checkbox"
              checked={rebase}
              onChange={(e) => setRebase(e.target.checked)}
              className="h-4 w-4 mt-0.5 rounded border-border text-brand focus:ring-brand"
            />
            <span>
              <div className="font-semibold text-text-0">{t('gitOps.pull.rebase')}</div>
              <div className="text-xs text-text-2 mt-0.5">{t('gitOps.pull.rebaseHint')}</div>
            </span>
          </label>
        </div>
        <DialogFooter className="gap-2">
          <Button type="button" variant="ghost" onClick={() => handleOpenChange(false)} disabled={busy}>
            {t('common.cancel')}
          </Button>
          <Button type="button" onClick={submit} disabled={busy}>
            {busy ? t('common.pulling') : t('gitOps.pull.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
