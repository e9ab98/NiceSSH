import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Input, Label } from '../../components/ui/input';
import { Button } from '../../components/ui/button';
import { useUsersStore } from '../../store/users';
import type { User, UserInput } from '../../ipc/users';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  /// When set, the dialog edits an existing user. When null,
  /// it creates a new one.
  initial?: User | null;
  /// Pre-fill values for the create flow. Caller passes the
  /// (name, email) it wants as a starting point — e.g. the
  /// scan dialog passes a name+email combo the user typed
  /// inline. Falls back to empty strings when omitted.
  defaultValues?: { name?: string; email?: string };
  onSaved?: (user: User) => void;
}

export function UserFormDialog({ open, onOpenChange, initial, defaultValues, onSaved }: Props) {
  const { t } = useTranslation();
  const create = useUsersStore((s) => s.create);
  const update = useUsersStore((s) => s.update);
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [busy, setBusy] = useState(false);

  // Sync fields when the dialog opens / target changes. We don't
  // bother with a useEffect-driven "dirty" tracking on email/name
  // because the form is short and the user can always clear-and-
  // retype.
  useEffect(() => {
    if (!open) return;
    if (initial) {
      setName(initial.name);
      setEmail(initial.email);
    } else {
      setName(defaultValues?.name ?? '');
      setEmail(defaultValues?.email ?? '');
    }
  }, [open, initial, defaultValues?.name, defaultValues?.email]);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (busy) return;
    const input: UserInput = {
      name: name.trim(),
      email: email.trim(),
    };
    if (!input.name || !input.email) return;
    setBusy(true);
    try {
      const saved = initial
        ? await update(initial.id, input)
        : await create(input);
      onSaved?.(saved);
      onOpenChange(false);
    } catch {
      // ipc client already toasted
    } finally {
      setBusy(false);
    }
  };

  const titleKey = initial ? 'userForm.editTitle' : 'userForm.newTitle';

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t(titleKey)}</DialogTitle>
        </DialogHeader>
        <form onSubmit={submit} className="space-y-3">
          <div>
            <Label htmlFor="user-name">{t('userForm.name')}</Label>
            <Input
              id="user-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('userForm.namePlaceholder')}
              required
              autoFocus
            />
          </div>
          <div>
            <Label htmlFor="user-email">{t('userForm.email')}</Label>
            <Input
              id="user-email"
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder={t('userForm.emailPlaceholder')}
              required
            />
            {initial && (
              <div className="text-text-2 text-xs mt-1">
                {t('userForm.editPropagationHint')}
              </div>
            )}
          </div>
          <DialogFooter className="gap-2">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={busy || !name.trim() || !email.trim()}>
              {busy ? t('common.saving') : t('common.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
