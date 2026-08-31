import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { useUsersStore } from '../../store/users';
import { useIdentitiesStore } from '../../store/identities';
import type { User } from '../../ipc/users';
import type { Identity } from '../../ipc/identities';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  user: User | null;
}

/**
 * Confirms deletion of a user. The backend blocks the operation
 * when any identity's (userName, userEmail) still matches this
 * user, but we proactively detect that here too so we can show
 * a friendlier preview ("still used by N identities: ...")
 * before the user clicks confirm. The store's `remove` will
 * surface the backend error if we miss an edge case.
 */
export function UserDeleteDialog({ open, onOpenChange, user }: Props) {
  const { t } = useTranslation();
  const remove = useUsersStore((s) => s.remove);
  const identities = useIdentitiesStore((s) => s.items);
  const [busy, setBusy] = useState(false);

  // Reset busy state when the dialog re-opens with a different
  // target, otherwise a previous failure leaves the button disabled.
  useEffect(() => {
    if (open) setBusy(false);
  }, [open, user?.id]);

  if (!user) return null;

  // Pre-flight: list identities whose (userName, userEmail) is
  // value-equal to this user. The backend will block the delete
  // if this list is non-empty; the UI shows the same list so
  // the user can fix it before retrying.
  const linked = identities.filter(
    (i: Identity) => i.userName === user.name && i.userEmail === user.email,
  );

  const confirm = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await remove(user.id);
      onOpenChange(false);
    } catch {
      // ipc client already toasted (e.g. "user is still referenced by N identities")
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('userDelete.title')}</DialogTitle>
        </DialogHeader>
        <div className="space-y-2 text-sm">
          <div className="text-text-1">
            {t('userDelete.body', { name: user.name, email: user.email })}
          </div>
          {linked.length > 0 ? (
            <div className="rounded-md border border-warning/40 bg-warning-soft p-3 text-warning text-xs">
              {t('userDelete.blocked', { count: linked.length })}
              <ul className="list-disc list-inside mt-1">
                {linked.map((i) => (
                  <li key={i.id}>{i.label}</li>
                ))}
              </ul>
              <div className="mt-1 text-text-2">{t('userDelete.blockedHint')}</div>
            </div>
          ) : (
            <div className="text-text-2 text-xs">{t('userDelete.unused')}</div>
          )}
        </div>
        <DialogFooter className="gap-2">
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {t('common.cancel')}
          </Button>
          <Button
            onClick={confirm}
            disabled={busy || linked.length > 0}
            variant="danger"
          >
            {busy ? t('common.deleting') : t('userDelete.confirm')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
