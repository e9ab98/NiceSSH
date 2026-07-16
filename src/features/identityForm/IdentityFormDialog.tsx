import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Input, Label } from '../../components/ui/input';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../../components/ui/tooltip';
import { Button } from '../../components/ui/button';
import type { Identity } from '../../ipc/identities';
function sanitizeLabel(value: string): string {
  return value.replace(/[\\/]+/g, '_');
}

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  initial?: Identity;
  defaultLabel?: string;
  onSubmit: (values: Omit<Identity, 'id'>) => Promise<void>;
}

export function IdentityFormDialog({ open, onOpenChange, initial, defaultLabel, onSubmit }: Props) {
  const { t } = useTranslation();
  const initialLabel = initial?.label ?? defaultLabel ?? '';
  const [label, setLabel] = useState(initialLabel);
  const [labelDirty, setLabelDirty] = useState(!!initial);
  const [userName, setUserName] = useState(initial?.userName ?? '');
  const [userEmail, setUserEmail] = useState(initial?.userEmail ?? '');
  const [hostAlias, setHostAlias] = useState(initial?.hostAlias ?? 'github.com');
  const [gitHost, setGitHost] = useState(initial?.gitHost ?? 'github.com');
  const [busy, setBusy] = useState(false);
  const labelBaseRef = useRef(initialLabel);

  useEffect(() => {
    if (!open) return;
    setLabel(initial?.label ?? defaultLabel ?? '');
    setLabelDirty(!!initial);
    labelBaseRef.current = initial?.label ?? defaultLabel ?? '';
    setUserName(initial?.userName ?? '');
    setUserEmail(initial?.userEmail ?? '');
    setHostAlias(initial?.hostAlias ?? 'github.com');
    setGitHost(initial?.gitHost ?? 'github.com');
  }, [open, initial, defaultLabel]);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (busy) return;
    const cleanLabel = sanitizeLabel(label.trim());
    if (!cleanLabel) return;
    setBusy(true);
    try {
      await onSubmit({
        label: cleanLabel,
        userName,
        userEmail,
        sshKeyId: initial?.sshKeyId ?? null,
        matchPath: initial?.matchPath ?? null,
        hostAlias: hostAlias || null,
        gitHost: gitHost || null,
      });
      onOpenChange(false);
    } finally {
      setBusy(false);
    }
  };

  const reset = () => {
    setLabel(defaultLabel ?? '');
    setLabelDirty(false);
    setUserName('');
    setUserEmail('');
    setHostAlias('github.com');
    setGitHost('github.com');
    labelBaseRef.current = defaultLabel ?? '';
  };

  const handleOpenChange = (value: boolean) => {
    if (!value) reset();
    onOpenChange(value);
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent>
        <DialogHeader><DialogTitle>{initial ? t('identityForm.editTitle') : t('identityForm.newTitle')}</DialogTitle></DialogHeader>
        <form onSubmit={submit} className="space-y-3">
          <div>
            <Label htmlFor="label">{t('identityForm.label')}</Label>
            {initial ? (
              <TooltipProvider delayDuration={150}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span className="block"><Input id="label" value={label} required readOnly disabled /></span>
                  </TooltipTrigger>
                  <TooltipContent>{t('identityForm.labelImmutable')}</TooltipContent>
                </Tooltip>
              </TooltipProvider>
            ) : (
              <div className="flex gap-2 items-start">
                <Input id="label" value={label} onChange={(event) => { setLabel(event.target.value); setLabelDirty(true); }} required placeholder="work" className="flex-1" />
                {labelDirty && <Button type="button" variant="ghost" size="sm" onClick={() => { setLabel(labelBaseRef.current); setLabelDirty(false); }}>{t('identityForm.resetLabel')}</Button>}
              </div>
            )}
            <div className="text-text-2 text-xs mt-1">{t('identityForm.labelHint')}</div>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div><Label htmlFor="userName">{t('identityForm.userName')}</Label><Input id="userName" value={userName} onChange={(event) => setUserName(event.target.value)} required /></div>
            <div><Label htmlFor="userEmail">{t('identityForm.userEmail')}</Label><Input id="userEmail" type="email" value={userEmail} onChange={(event) => setUserEmail(event.target.value)} required /></div>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div><Label htmlFor="hostAlias">{t('identityForm.hostAlias')}</Label><Input id="hostAlias" value={hostAlias} onChange={(event) => setHostAlias(event.target.value)} placeholder="github.com" /></div>
            <div><Label htmlFor="gitHost">{t('identityForm.gitHost')}</Label><Input id="gitHost" value={gitHost} onChange={(event) => setGitHost(event.target.value)} placeholder="github.com" /></div>
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
