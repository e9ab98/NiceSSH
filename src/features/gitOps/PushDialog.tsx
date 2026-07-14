import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter, DialogDescription } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { gitPush } from '../../ipc/git';
import { toast } from 'sonner';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  projectPath: string;
  projectName: string;
  onPushed: () => void;
}

/// Push confirmation dialog. The default flow is a plain
/// `git push`; a force option is hidden behind an "Advanced"
/// disclosure so casual users do not see it but power users can
/// still get to it. The force flag maps to `--force-with-lease`
/// on the Rust side, so even when enabled git itself refuses
/// if the remote has diverged in ways we have not seen.
export function PushDialog({ open, onOpenChange, projectPath, projectName, onPushed }: Props) {
  const { t } = useTranslation();
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [force, setForce] = useState(false);
  const [busy, setBusy] = useState(false);

  const handleOpenChange = (v: boolean) => {
    if (!v) {
      setShowAdvanced(false);
      setForce(false);
    }
    onOpenChange(v);
  };

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await gitPush(projectPath, force);
      toast.success(t('gitOps.push.success', { name: projectName }));
      onPushed();
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
          <DialogTitle>{t('gitOps.push.title')}</DialogTitle>
          <DialogDescription>{t('gitOps.push.subtitle', { name: projectName })}</DialogDescription>
        </DialogHeader>
        <div className="space-y-2">
          <button
            type="button"
            onClick={() => setShowAdvanced((s) => !s)}
            className="text-xs text-text-1 hover:text-text-0 underline underline-offset-2"
          >
            {showAdvanced ? t('gitOps.push.hideAdvanced') : t('gitOps.push.showAdvanced')}
          </button>
          {showAdvanced && (
            <label className="flex items-start gap-2 text-sm text-text-1 cursor-pointer select-none rounded-md border border-border bg-bg-0 p-2">
              <input
                type="checkbox"
                checked={force}
                onChange={(e) => setForce(e.target.checked)}
                className="h-4 w-4 mt-0.5 rounded border-border text-brand focus:ring-brand"
              />
              <span>
                <div className="font-semibold text-text-0">{t('gitOps.push.force')}</div>
                <div className="text-xs text-text-2 mt-0.5">{t('gitOps.push.forceHint')}</div>
              </span>
            </label>
          )}
        </div>
        <DialogFooter className="gap-2">
          <Button type="button" variant="ghost" onClick={() => handleOpenChange(false)} disabled={busy}>
            {t('common.cancel')}
          </Button>
          <Button type="button" onClick={submit} disabled={busy} variant={force ? 'danger' : 'default'}>
            {busy ? t('common.pushing') : (force ? t('gitOps.push.submitForce') : t('gitOps.push.submit'))}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
