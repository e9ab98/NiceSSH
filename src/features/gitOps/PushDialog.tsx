import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter, DialogDescription } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { Input } from '../../components/ui/input';
import {
  gitPush,
  preflightPush,
  type OverrideAck,
  type PreflightReport,
  type RiskReason,
  type Severity,
} from '../../ipc/git';
import { toast } from 'sonner';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  projectId: string;
  projectPath: string;
  projectName: string;
  onPushed: () => void;
}

/// Push confirmation dialog with preflight. The default flow:
///   1. User clicks "Push" in the dialog (or we can also auto-run
///      preflight on open).
///   2. We call `preflight_push` and get back a tier (Safe / Verify / Warn).
///   3. Safe: call `git_push` with `tier="safe"` ack, no extra UI.
///   4. Verify: render the reason list + "Push anyway" button.
///   5. Warn: same as Verify, but if `requiresTypedConfirmation` is
///      true, the user must type the `overrideTarget` into the input
///      before the "Push anyway" button enables.
export function PushDialog({ open, onOpenChange, projectId, projectPath, projectName, onPushed }: Props) {
  const { t } = useTranslation();
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [force, setForce] = useState(false);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<PreflightReport | null>(null);
  const [typedOverride, setTypedOverride] = useState('');

  const reset = () => {
    setShowAdvanced(false);
    setForce(false);
    setReport(null);
    setTypedOverride('');
  };

  const handleOpenChange = (v: boolean) => {
    if (!v) reset();
    onOpenChange(v);
  };

  const runPreflight = async (): Promise<PreflightReport | null> => {
    try {
      const r = await preflightPush(projectId, projectPath, force, false);
      setReport(r);
      return r;
    } catch (e) {
      toast.error(String(e));
      return null;
    }
  };

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    try {
      // First pass: compute the preflight. We always call this
      // — even for what looks like a Safe push — so we get a
      // coherent history record with `tier` populated.
      const r = report ?? (await runPreflight());
      if (!r) return;
      const ack: OverrideAck = {
        tier: r.tier,
        riskCodes: r.reasons.map((x) => x.code),
        typedOverride:
          r.requiresTypedConfirmation && typedOverride ? typedOverride : null,
      };
      await gitPush(projectPath, force, ack);
      toast.success(t('gitOps.push.success', { name: projectName }));
      onPushed();
      onOpenChange(false);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const sevColor = (s: Severity) =>
    s === 'danger'
      ? 'border-danger/40 bg-danger/10 text-danger'
      : s === 'warning'
      ? 'border-warning/40 bg-warning/10 text-warning'
      : 'border-border bg-bg-0 text-text-1';

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('gitOps.push.title')}</DialogTitle>
          <DialogDescription>{t('gitOps.push.subtitle', { name: projectName })}</DialogDescription>
        </DialogHeader>

        <div className="space-y-3">
          {/* Preflight report — shown once we've computed it. */}
          {report && report.reasons.length > 0 && (
            <div className={'rounded-md border p-3 space-y-2 ' + sevColor(report.tier === 'warn' ? 'danger' : report.tier === 'verify' ? 'warning' : 'info')}>
              <div className="font-medium text-sm">
                {report.tier === 'warn'
                  ? t('gitOps.push.preflight.warnTitle')
                  : t('gitOps.push.preflight.verifyTitle')}
              </div>
              <ul className="text-xs space-y-1 list-disc pl-5">
                {report.reasons.map((r: RiskReason, i) => (
                  <li key={i}>{t(r.messageKey)}</li>
                ))}
              </ul>
            </div>
          )}

          {report?.requiresTypedConfirmation && (
            <div>
              <label className="text-xs text-text-1">
                {t('gitOps.push.preflight.typeToConfirm', { target: report.overrideTarget ?? '' })}
              </label>
              <Input
                value={typedOverride}
                onChange={(e) => setTypedOverride(e.target.value)}
                placeholder={report.overrideTarget ?? ''}
                className="mt-1"
              />
            </div>
          )}

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
                onChange={(e) => {
                  setForce(e.target.checked);
                  // Force toggle changes the preflight — reset
                  // so the next submit re-computes.
                  setReport(null);
                  setTypedOverride('');
                }}
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
          <Button
            type="button"
            onClick={submit}
            disabled={
              busy ||
              (report?.requiresTypedConfirmation === true &&
                typedOverride !== report.overrideTarget)
            }
            variant={report?.tier === 'warn' || force ? 'danger' : 'default'}
          >
            {busy
              ? t('common.pushing')
              : report?.tier === 'warn'
              ? t('gitOps.push.preflight.submitWarn')
              : force
              ? t('gitOps.push.submitForce')
              : t('gitOps.push.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
