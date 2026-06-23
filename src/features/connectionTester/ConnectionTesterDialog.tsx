import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { testSshConnection } from '../../ipc/git';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  identityId: string;
  identityLabel?: string;
}

export function ConnectionTesterDialog({ open, onOpenChange, identityId, identityLabel }: Props) {
  const { t } = useTranslation();
  const [result, setResult] = useState<{ ok: boolean; message: string; timedOut: boolean } | null>(null);
  const [loading, setLoading] = useState(false);
  const [retry, setRetry] = useState(0);

  useEffect(() => {
    if (!open || !identityId) return;
    setResult(null);
    setLoading(true);
    testSshConnection(identityId)
      .then(setResult)
      .catch((e) => setResult({ ok: false, message: String(e), timedOut: false }))
      .finally(() => setLoading(false));
  }, [open, identityId, retry]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader><DialogTitle>{identityLabel ? t('connectionTester.titleWithLabel', { label: identityLabel }) : t('connectionTester.title')}</DialogTitle></DialogHeader>
        {loading && (
          <div className="text-text-1 text-sm">{t('connectionTester.testing')}</div>
        )}
        {result && (
          <div className="space-y-2">
            <div className={result.ok ? 'text-success font-medium' : 'text-danger font-medium'}>
              {result.ok ? t('connectionTester.authenticated') : result.timedOut ? t('connectionTester.timedOut') : t('connectionTester.failed')}
            </div>
            <pre className="text-xs bg-bg-0 p-3 rounded-md border border-border overflow-x-auto whitespace-pre-wrap break-all max-h-48 overflow-y-auto">
              {result.message || t('connectionTester.noOutput')}
            </pre>
          </div>
        )}
        <DialogFooter className="gap-2">
          {result && !result.ok && (
            <Button variant="outline" onClick={() => setRetry((n) => n + 1)}>{t('common.retry')}</Button>
          )}
          <Button onClick={() => onOpenChange(false)}>{t('common.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
