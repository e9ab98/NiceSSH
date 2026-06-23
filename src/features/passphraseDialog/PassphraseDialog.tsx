import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Input, Label } from '../../components/ui/input';
import { Button } from '../../components/ui/button';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  keyPath: string;
  onUnlock: (passphrase: string) => Promise<boolean>;
}

export function PassphraseDialog({ open, onOpenChange, keyPath, onUnlock }: Props) {
  const { t } = useTranslation();
  const [passphrase, setPassphrase] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      const ok = await onUnlock(passphrase);
      if (ok) {
        onOpenChange(false);
        setPassphrase('');
      } else {
        setError(t('passphrase.wrong'));
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  const handleOpenChange = (v: boolean) => {
    if (!v) {
      setPassphrase('');
      setError(null);
    }
    onOpenChange(v);
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader><DialogTitle>{t('passphrase.title')}</DialogTitle></DialogHeader>
        <form onSubmit={submit} className="space-y-3">
          <div className="text-text-1 text-sm font-mono break-all">{keyPath}</div>
          <div>
            <Label htmlFor="pass">{t('passphrase.passphrase')}</Label>
            <Input
              id="pass"
              type="password"
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              autoFocus
              required
            />
          </div>
          {error && <div className="text-danger text-sm">{error}</div>}
          <DialogFooter className="gap-2">
            <Button type="button" variant="ghost" onClick={() => handleOpenChange(false)}>{t('common.cancel')}</Button>
            <Button type="submit" disabled={submitting}>{submitting ? t('common.unlocking') : t('common.unlock')}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
