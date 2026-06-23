import { useState, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { downloadAndInstall, markDismissed } from '../../lib/update';
import { Button } from '../../components/ui/button';

type Phase = 'idle' | 'downloading' | 'ready' | 'error';

interface Props {
  version: string;
  notes?: string | null;
}

const TOAST_ID = 'nicessh-update';
const PROGRESS_THROTTLE_MS = 100;

export function UpdateToast({ version, notes }: Props) {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<Phase>('idle');
  const [progress, setProgress] = useState(0);
  const lastProgressAt = useRef(0);

  const onUpdateNow = async () => {
    setPhase('downloading');
    setProgress(0);
    try {
      await downloadAndInstall((pct) => {
        const now = Date.now();
        if (now - lastProgressAt.current < PROGRESS_THROTTLE_MS && pct < 100) return;
        lastProgressAt.current = now;
        setProgress(pct);
      });
      setPhase('ready');
    } catch {
      setPhase('error');
      toast.error(t('update.toast.failed'));
      setTimeout(() => setPhase('idle'), 200);
    }
  };

  const onDismiss = () => {
    markDismissed(version);
    toast.dismiss(TOAST_ID);
  };

  // Tauri 2's app relaunch() lives in @tauri-apps/plugin-process, which
  // is not bundled here. The installed-by-Tauri-updater binary replaces
  // the running one on relaunch; the user closes & reopens the app to
  // pick it up. We surface this as a final-state instruction.
  const onRestart = () => {
    toast.dismiss(TOAST_ID);
  };

  return (
    <div
      data-testid="update-toast"
      className="flex flex-col gap-2 p-3 min-w-[320px]"
    >
      <div className="flex items-center justify-between">
        <div className="font-medium text-sm">
          {t('update.toast.title', { version })}
        </div>
        <button
          aria-label="Close"
          onClick={onDismiss}
          className="text-text-1 hover:text-text-0 text-lg leading-none"
          data-testid="update-toast-close"
        >
          ×
        </button>
      </div>

      {notes && (
        <div className="text-xs text-text-1 max-h-20 overflow-auto whitespace-pre-wrap">
          {notes}
        </div>
      )}

      {phase === 'downloading' && (
        <div
          role="progressbar"
          aria-valuenow={progress}
          aria-valuemin={0}
          aria-valuemax={100}
          className="h-1.5 w-full rounded bg-bg-2 overflow-hidden"
        >
          <div
            className="h-full bg-accent transition-[width] duration-200"
            style={{ width: `${progress}%` }}
          />
        </div>
      )}

      <div className="flex gap-2 justify-end">
        {phase === 'ready' ? (
          <Button size="sm" onClick={onRestart}>
            {t('update.toast.restart')}
          </Button>
        ) : (
          <>
            <Button
              size="sm"
              variant="ghost"
              onClick={onDismiss}
              disabled={phase === 'downloading'}
            >
              {t('update.toast.later')}
            </Button>
            <Button
              size="sm"
              onClick={onUpdateNow}
              disabled={phase === 'downloading' || phase === 'error'}
            >
              {phase === 'downloading'
                ? t('update.toast.progress', { pct: progress })
                : t('update.toast.update')}
            </Button>
          </>
        )}
      </div>
    </div>
  );
}
