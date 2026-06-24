import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Input, Label } from '../../components/ui/input';
import { Button } from '../../components/ui/button';
import { generateKey, sshKeyExists } from '../../ipc/sshKeys';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { toast } from 'sonner';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  defaultName: string;
  defaultComment: string;
  onGenerated: (keyPath: string, publicKey: string) => void;
}

export function KeyGeneratorDialog({ open, onOpenChange, defaultName, defaultComment, onGenerated }: Props) {
  const { t } = useTranslation();
  const [name, setName] = useState(defaultName);
  const [keyType, setKeyType] = useState('ed25519');
  const [comment, setComment] = useState(defaultComment);
  const [passphrase, setPassphrase] = useState('');
  const [submitting, setSubmitting] = useState(false);

  // Check whether the target key file already exists in ~/.ssh/ whenever
  // the name changes. Used to show an overwrite warning + require confirm.
  useEffect(() => {
    if (!name) { setExists(false); return; }
    let cancelled = false;
    sshKeyExists(name).then((v) => { if (!cancelled) setExists(v); }).catch(() => { if (!cancelled) setExists(false); });
    return () => { cancelled = true; };
  }, [name]);
  const [publicKey, setPublicKey] = useState<string | null>(null);
  const [fingerprint, setFingerprint] = useState<string | null>(null);
  const [exists, setExists] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (submitting) return;
    if (passphrase && passphrase.length < 4) {
      toast.error(t('keyGenerator.passphraseTooShort'));
      return;
    }
    setSubmitting(true);
    try {
      // Re-check existence at submit time and ask for explicit confirmation
      // if the key will overwrite an existing file.
      const willOverwrite = await sshKeyExists(name);
      if (willOverwrite) {
        if (!confirm(t('keyGenerator.overwriteConfirm', { name }))) {
          setSubmitting(false);
          return;
        }
      }
      const result = await generateKey({ name, keyType, comment, passphrase: passphrase || null });
      try { await writeText(result.publicKey); } catch { /* clipboard may not be available in dev */ }
      setPublicKey(result.publicKey);
      setFingerprint(result.fingerprint);
      onGenerated(name.startsWith('~/') ? name : `~/.ssh/${name}`, result.publicKey);
      toast.success(t('keyGenerator.success'));
    } finally {
      setSubmitting(false);
    }
  };

  const handleOpenChange = (v: boolean) => {
    if (!v) {
      setPublicKey(null);
      setFingerprint(null);
      setPassphrase('');
      setExists(false);
    }
    onOpenChange(v);
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent>
        <DialogHeader><DialogTitle>{t('keyGenerator.title')}</DialogTitle></DialogHeader>
        {!publicKey ? (
          <form onSubmit={submit} className="space-y-3">
            <div>
              <Label htmlFor="kname">{t('keyGenerator.keyName')}</Label>
              <Input id="kname" value={name} onChange={(e) => setName(e.target.value)} required />
              {exists && (
                <div className="mt-1.5 text-xs text-danger font-semibold [[data-theme=dark]_&]:text-[#fca5a5]">
                  ⚠ {t('keyGenerator.overwriteWarning', { name })}
                </div>
              )}
            </div>
            <div>
              <Label htmlFor="ktype">{t('keyGenerator.keyType')}</Label>
              <select
                id="ktype"
                value={keyType}
                onChange={(e) => setKeyType(e.target.value)}
                className="w-full h-9 rounded-md border border-border bg-bg-0 px-3 text-sm text-text-0"
              >
                <option value="ed25519">{t('keyGenerator.keyTypeEd25519')}</option>
                <option value="rsa">{t('keyGenerator.keyTypeRsa')}</option>
              </select>
            </div>
            <div>
              <Label htmlFor="kcomment">{t('keyGenerator.comment')}</Label>
              <Input id="kcomment" value={comment} onChange={(e) => setComment(e.target.value)} required />
            </div>
            <div>
              <Label htmlFor="kpass">{t('keyGenerator.passphrase')}</Label>
              <Input id="kpass" type="password" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} />
            </div>
            <DialogFooter className="gap-2">
              <Button type="button" variant="ghost" onClick={() => handleOpenChange(false)}>{t('common.cancel')}</Button>
              <Button type="submit" disabled={submitting}>{submitting ? t('common.generating') : t('common.generate')}</Button>
            </DialogFooter>
          </form>
        ) : (
          <div className="space-y-3">
            <div className="text-success text-sm">✓ {t('keyGenerator.success')}</div>
            {fingerprint && (
              <div className="text-text-1 text-xs font-mono break-all">{fingerprint}</div>
            )}
            <div>
              <Label>{t('keyGenerator.publicKeyLabel')}</Label>
              <textarea
                readOnly
                className="w-full h-32 rounded-md border border-border bg-bg-0 p-2 text-xs font-mono text-text-0"
                value={publicKey}
              />
            </div>
            <div className="text-text-1 text-sm">
              {t('keyGenerator.addToGithub')}{' '}
              <a href="https://github.com/settings/keys" target="_blank" rel="noreferrer" className="text-accent hover:underline">
                {t('keyGenerator.githubKeysLink')}
              </a>
            </div>
            <DialogFooter>
              <Button onClick={() => handleOpenChange(false)}>{t('common.done')}</Button>
            </DialogFooter>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
