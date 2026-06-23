import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Input, Label } from '../../components/ui/input';
import { Button } from '../../components/ui/button';
import type { Identity } from '../../ipc/identities';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  initial?: Identity;
  // Prefill key path when creating a new identity (used by the SSH Keys
  // view's "Create identity for this key" button). Ignored when `initial`
  // is set.
  defaultKeyPath?: string;
  onSubmit: (values: Omit<Identity, 'id'>) => Promise<void>;
}

export function IdentityFormDialog({ open, onOpenChange, initial, defaultKeyPath, onSubmit }: Props) {
  const { t } = useTranslation();
  const [label, setLabel] = useState(initial?.label ?? '');
  const [userName, setUserName] = useState(initial?.userName ?? '');
  const [userEmail, setUserEmail] = useState(initial?.userEmail ?? '');
  const [keyPath, setKeyPath] = useState(initial?.keyPath ?? defaultKeyPath ?? '~/.ssh/id_ed25519');
  const [matchPath, setMatchPath] = useState(initial?.matchPath ?? '');
  const [hostAlias, setHostAlias] = useState(initial?.hostAlias ?? 'github.com');
  const [gitHost, setGitHost] = useState(initial?.gitHost ?? 'github.com');
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    try {
      await onSubmit({
        label,
        userName,
        userEmail,
        keyPath,
        matchPath: matchPath || null,
        hostAlias: hostAlias || null,
        gitHost: gitHost || null,
      });
      onOpenChange(false);
    } finally {
      setBusy(false);
    }
  };

  const handleOpenChange = (v: boolean) => {
    if (!v) {
      setLabel(''); setUserName(''); setUserEmail(''); setMatchPath('');
      setKeyPath(defaultKeyPath ?? '~/.ssh/id_ed25519');
    }
    onOpenChange(v);
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent>
        <DialogHeader><DialogTitle>{initial ? t('identityForm.editTitle') : t('identityForm.newTitle')}</DialogTitle></DialogHeader>
        <form onSubmit={submit} className="space-y-3">
          <div>
            <Label htmlFor="label">{t('identityForm.label')}</Label>
            <Input id="label" value={label} onChange={(e) => setLabel(e.target.value)} required placeholder="Work" />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <Label htmlFor="userName">{t('identityForm.userName')}</Label>
              <Input id="userName" value={userName} onChange={(e) => setUserName(e.target.value)} required />
            </div>
            <div>
              <Label htmlFor="userEmail">{t('identityForm.userEmail')}</Label>
              <Input id="userEmail" type="email" value={userEmail} onChange={(e) => setUserEmail(e.target.value)} required />
            </div>
          </div>
          <div>
            <Label htmlFor="keyPath">{t('identityForm.keyPath')}</Label>
            <Input id="keyPath" value={keyPath} onChange={(e) => setKeyPath(e.target.value)} required />
          </div>
          <div>
            <Label htmlFor="matchPath">{t('identityForm.matchPath')}</Label>
            <Input id="matchPath" value={matchPath} onChange={(e) => setMatchPath(e.target.value)} placeholder={t('identityForm.matchPathPlaceholder')} />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <Label htmlFor="hostAlias">{t('identityForm.hostAlias')}</Label>
              <Input id="hostAlias" value={hostAlias} onChange={(e) => setHostAlias(e.target.value)} placeholder="github.com" />
            </div>
            <div>
              <Label htmlFor="gitHost">{t('identityForm.gitHost')}</Label>
              <Input id="gitHost" value={gitHost} onChange={(e) => setGitHost(e.target.value)} placeholder="github.com" />
            </div>
          </div>
          <DialogFooter className="gap-2">
            <Button type="button" variant="ghost" onClick={() => handleOpenChange(false)}>{t('common.cancel')}</Button>
            <Button type="submit" disabled={busy}>{busy ? t('common.saving') : t('common.save')}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
