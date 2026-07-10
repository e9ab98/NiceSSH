import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { AlertTriangle } from 'lucide-react';

export type HttpsBindChoice = 'continue' | 'change-remote' | 'cancel';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  /// Optional project display name shown in the body so the user
  /// knows which project the dialog refers to (NiceSSH can have
  /// many projects open simultaneously).
  projectName?: string;
  /// Original remote URL, for context. Showcased as code/mono so
  /// long URLs truncate cleanly.
  remoteUrl?: string | null;
  /// Protocol label — `"http"` for plain HTTP, `"https"` for TLS.
  /// Displayed in the heading next to "remote".
  protocol?: string;
  onChoose: (choice: HttpsBindChoice) => void;
}

/// Warning dialog raised when the user is about to bind (or switch to)
/// an identity on a project whose `[remote "origin"]` URL is HTTP(S).
///
/// Why this exists: NiceSSH's [core] sshCommand line is **inert** on
/// HTTPS remotes — `git push`/`pull` go through git-credential, and
/// the user.name/user.email blocks are what actually matter. We still
/// allow the bind (because changing user identity is a legitimate
/// action on HTTPS projects too), but we want the user to know *why*
/// the SSH key they selected won't be the auth credential on this
/// remote, and to offer a one-click "switch the remote to SSH" path.
///
/// Layout notes — this dialog is intentionally compact:
///
///  * `max-w-lg` instead of `max-w-md` so the URL line and the body
///    paragraphs don't wrap awkwardly on macOS / Windows default
///    window sizes.
///  * `p-5 gap-3` instead of `p-6 gap-4` so the chrome doesn't
///    outweigh the content.
///  * Two lines max for the body — explanatory but not a wall of
///    text; the longer "how to fix it" steps are second-priority.
///  * Footer buttons are `size="sm"` so they line up with the
///    inline icon row, not with the heading.
///  * `max-h-[85vh] overflow-y-auto` keeps the modal from extending
///    past the viewport if the body text ever grows on a language
///    with longer strings.
export function HttpsBindConfirmDialog({
  open,
  onOpenChange,
  projectName,
  remoteUrl,
  protocol,
  onChoose,
}: Props) {
  const { t } = useTranslation();

  const handle = (choice: HttpsBindChoice) => {
    onChoose(choice);
    if (choice !== 'continue') onOpenChange(false);
    // For 'continue' we leave the dialog open — the caller will
    // close it via onOpenChange after the toast settles. This keeps
    // the animation smooth on the success path.
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg p-5 gap-3 max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <div className="flex items-start gap-2.5">
            <div className="shrink-0 mt-0.5 inline-flex h-8 w-8 items-center justify-center rounded-lg bg-warning text-warning-strong">
              <AlertTriangle className="h-4 w-4" />
            </div>
            <div className="min-w-0">
              <DialogTitle className="text-base leading-tight">
                {t('httpsBindConfirm.title')}
              </DialogTitle>
              <p className="text-xs text-text-2 mt-0.5 leading-snug">
                {t('httpsBindConfirm.heading', {
                  project: projectName ?? t('httpsBindConfirm.thisProject'),
                  protocol: (protocol ?? 'https').toUpperCase(),
                })}
              </p>
            </div>
          </div>
        </DialogHeader>

        <div className="text-sm text-text-1 space-y-1.5">
          <p className="leading-snug">{t('httpsBindConfirm.body1')}</p>
          {remoteUrl && (
            <code className="block text-xs font-mono text-text-2 bg-bg-0 border border-border rounded-md px-2 py-1 mt-1 truncate">
              {remoteUrl}
            </code>
          )}
          <p className="text-xs text-text-2 leading-snug whitespace-pre-line mt-1">
            {t('httpsBindConfirm.body2')}
          </p>
        </div>

        <DialogFooter className="flex-row justify-end items-center gap-2 !mt-1">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => handle('cancel')}
            className="mr-auto"
          >
            {t('httpsBindConfirm.cancel')}
          </Button>
          <Button variant="outline" size="sm" onClick={() => handle('change-remote')}>
            {t('httpsBindConfirm.changeRemote')}
          </Button>
          <Button size="sm" onClick={() => handle('continue')}>
            {t('httpsBindConfirm.continue')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
