import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Input, Label } from '../../components/ui/input';
import { Button } from '../../components/ui/button';
import { testHttpsConnection, testHttpsConnectionWithCredentials, testSshConnection, type ConnectionTestResult } from '../../ipc/git';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  mode: 'ssh' | 'https';
  projectPath: string;
  identityId?: string;
  identityLabel?: string;
}

export function ConnectionTesterDialog({ open, onOpenChange, mode, projectPath, identityId, identityLabel }: Props) {
  const { t } = useTranslation();
  const [result, setResult] = useState<ConnectionTestResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [retry, setRetry] = useState(0);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');

  useEffect(() => {
    if (!open || (mode === 'ssh' && !identityId)) return;
    setResult(null);
    setUsername('');
    setPassword('');
    setLoading(true);
    const request = mode === 'https' ? testHttpsConnection(projectPath) : testSshConnection(identityId!);
    request
      .then(setResult)
      .catch((error) => setResult({ ok: false, message: String(error), timedOut: false, needsCredentials: false, credentialsSaved: false }))
      .finally(() => setLoading(false));
  }, [open, mode, projectPath, identityId, retry]);

  const submitCredentials = async (event: React.FormEvent) => {
    event.preventDefault();
    setLoading(true);
    try {
      setResult(await testHttpsConnectionWithCredentials(projectPath, username, password));
    } catch (error) {
      setResult({ ok: false, message: String(error), timedOut: false, needsCredentials: false, credentialsSaved: false });
    } finally {
      setPassword('');
      setLoading(false);
    }
  };

  const title = mode === 'https'
    ? t('connectionTester.httpsTitle')
    : identityLabel
      ? t('connectionTester.titleWithLabel', { label: identityLabel })
      : t('connectionTester.title');

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader><DialogTitle>{title}</DialogTitle></DialogHeader>
        {loading && <div className="text-text-1 text-sm">{t('connectionTester.testing')}</div>}
        {result && !loading && (
          <div className="space-y-3">
            <div className={result.ok ? 'text-success font-medium' : 'text-danger font-medium'}>
              {result.ok
                ? mode === 'https' ? t('connectionTester.httpsAuthenticated') : t('connectionTester.authenticated')
                : result.timedOut ? t('connectionTester.timedOut') : t('connectionTester.failed')}
            </div>
            {result.credentialsSaved && <div className="text-text-1 text-sm">{t('connectionTester.credentialsSaved')}</div>}
            {result.message && (
              <pre className="text-xs bg-bg-0 p-3 rounded-md border border-border overflow-x-auto whitespace-pre-wrap break-all max-h-48 overflow-y-auto">
                {result.message}
              </pre>
            )}
            {mode === 'https' && result.needsCredentials && (
              <form onSubmit={submitCredentials} className="space-y-3 rounded-md border border-border p-3">
                <div className="text-sm text-text-1">{t('connectionTester.credentialsPrompt')}</div>
                <div>
                  <Label htmlFor="https-username">{t('connectionTester.username')}</Label>
                  <Input id="https-username" value={username} onChange={(event) => setUsername(event.target.value)} autoFocus required />
                </div>
                <div>
                  <Label htmlFor="https-password">{t('connectionTester.passwordOrToken')}</Label>
                  <Input id="https-password" type="password" value={password} onChange={(event) => setPassword(event.target.value)} required />
                </div>
                <div className="text-xs text-text-2">{t('connectionTester.credentialsSaveHint')}</div>
                <Button type="submit" disabled={loading}>{t('connectionTester.testAndSave')}</Button>
              </form>
            )}
          </div>
        )}
        <DialogFooter className="gap-2">
          {result && !result.ok && !result.needsCredentials && (
            <Button variant="outline" onClick={() => setRetry((value) => value + 1)}>{t('common.retry')}</Button>
          )}
          <Button onClick={() => onOpenChange(false)}>{t('common.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
