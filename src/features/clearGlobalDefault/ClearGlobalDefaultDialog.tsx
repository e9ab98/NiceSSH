import { useTranslation } from 'react-i18next';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  /// Current effective user.name / user.email that the unset
  /// action will remove from `~/.gitconfig`. May be null if git
  /// itself has no name/email resolved. Used to render the
  /// "values that will be removed" preview so the user can see
  /// what they're about to drop.
  userName: string | null;
  userEmail: string | null;
  /// True while the parent is performing the unset.
  busy: boolean;
  onConfirm: () => Promise<void>;
}

/// Confirmation dialog shown before `unset_global_default_identity`
/// because that command also strips the `[user]` section from
/// `~/.gitconfig` — including values the user may have set by
/// hand. History rollback can restore them, but only if the user
/// is aware of the change.
export function ClearGlobalDefaultDialog({
  open,
  onOpenChange,
  userName,
  userEmail,
  busy,
  onConfirm,
}: Props) {
  const { t } = useTranslation();
  const hasUser = !!(userName || userEmail);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('clearGlobalDefault.title')}</DialogTitle>
          <DialogDescription>
            {t('clearGlobalDefault.description')}
          </DialogDescription>
        </DialogHeader>
        {hasUser && (
          <div className="rounded-md border border-border bg-bg-0 p-3 text-xs font-mono text-text-1 whitespace-pre-wrap">
            {[userName && `name  = ${userName}`, userEmail && `email = ${userEmail}`]
              .filter(Boolean)
              .join('\n')}
          </div>
        )}
        <p className="text-xs text-text-2">
          {t('clearGlobalDefault.rollbackHint')}
        </p>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
            {t('common.cancel')}
          </Button>
          <Button
            variant="danger"
            onClick={() => { void onConfirm(); }}
            disabled={busy}
          >
            {t('clearGlobalDefault.confirm')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
