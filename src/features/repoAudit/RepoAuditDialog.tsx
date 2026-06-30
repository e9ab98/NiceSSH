import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { Badge } from '../../components/ui/badge';
import { auditRepos, cleanRepoGitconfig, type RepoAudit, type RepoAuditStatus } from '../../ipc/git';
import { toast } from 'sonner';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  // Called after a project is cleaned so the parent can refresh its
  // per-project repo config / identity detection.
  onChanged?: () => void;
}

function statusVariant(s: RepoAuditStatus): 'default' | 'success' | 'warning' | 'danger' {
  switch (s) {
    case 'clean': return 'success';
    case 'dirty': return 'warning';
    case 'no-config':
    case 'no-identity': return 'danger';
  }
}

export function RepoAuditDialog({ open, onOpenChange, onChanged }: Props) {
  const { t } = useTranslation();
  const [rows, setRows] = useState<RepoAudit[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [cleaning, setCleaning] = useState<string | null>(null);
  const [runSsh, setRunSsh] = useState(false);

  const refresh = async (withSsh: boolean) => {
    if (busy) return;
    setBusy(true);
    try {
      const result = await auditRepos(withSsh);
      setRows(result);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (!open) return;
    refresh(runSsh);
    // Only auto-refresh on dialog open. Toggling the SSH checkbox
    // does NOT trigger a refresh — the user must click Refresh
    // (or reopen the dialog) to actually run the SSH tests.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const dirty = rows?.filter((r) => r.status === 'dirty') ?? [];
  const clean = rows?.filter((r) => r.status === 'clean') ?? [];
  const other = rows?.filter((r) => r.status !== 'clean' && r.status !== 'dirty') ?? [];

  const handleClean = async (projectId: string) => {
    if (cleaning) return;
    setCleaning(projectId);
    try {
      await cleanRepoGitconfig(projectId);
      toast.success(t('repoAudit.cleaned'));
      await refresh(runSsh);
      onChanged?.();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setCleaning(null);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl">
        <DialogHeader>
          <DialogTitle>{t('repoAudit.title')}</DialogTitle>
        </DialogHeader>

        <div className="flex items-center gap-3 text-sm">
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={runSsh}
              onChange={(e) => setRunSsh(e.target.checked)}
            />
            {t('repoAudit.runSshTests')}
          </label>
          <Button size="sm" variant="outline" onClick={() => refresh(runSsh)} disabled={busy}>
            {busy ? t('common.loading') : t('common.refresh')}
          </Button>
        </div>

        <div className="max-h-[60vh] overflow-y-auto rounded-md border border-border">
          {rows === null ? (
            <div className="p-6 text-center text-text-2 text-sm">{t('common.loading')}</div>
          ) : rows.length === 0 ? (
            <div className="p-6 text-center text-text-2 text-sm">{t('repoAudit.empty')}</div>
          ) : (
            <table className="w-full text-sm table-fixed">
              <thead className="bg-bg-2 text-text-2 text-xs">
                <tr>
                  <th className="text-left p-2 font-medium whitespace-nowrap w-[34%]">{t('repoAudit.project')}</th>
                  <th className="text-left p-2 font-medium whitespace-nowrap w-32">{t('repoAudit.status')}</th>
                  <th className="text-left p-2 font-medium whitespace-nowrap w-40">{t('repoAudit.identity')}</th>
                  <th className="text-left p-2 font-medium whitespace-nowrap w-28" title={t('repoAudit.sshCommands')}>{t('repoAudit.sshCommands')}</th>
                  <th className="text-left p-2 font-medium whitespace-nowrap w-28">{t('repoAudit.sshTest')}</th>
                  <th className="text-right p-2 font-medium whitespace-nowrap w-24">{t('repoAudit.action')}</th>
                </tr>
              </thead>
              <tbody>
                {[...dirty, ...other, ...clean].map((r) => (
                  <tr key={r.projectId} className="border-t border-border align-top">
                    <td className="p-2">
                      <div className="font-medium">{r.projectName}</div>
                      <div className="text-text-2 text-xs font-mono truncate max-w-xs">{r.projectPath}</div>
                    </td>
                    <td className="p-2">
                      <Badge variant={statusVariant(r.status)}>{t(`repoAudit.status.${r.status}`)}</Badge>
                    </td>
                    <td className="p-2 text-text-1">
                      {r.identityLabel ?? (
                        <Badge variant="warning">{t('repoAudit.status.no-identity')}</Badge>
                      )}
                    </td>
                    <td className="p-2 font-mono text-xs">{r.sshCommandCount}</td>
                    <td className="p-2 text-text-1 text-xs">
                      {r.sshTestOk === null ? (
                        <span className="text-text-2">—</span>
                      ) : r.sshTestOk ? (
                        <span className="text-success">✓ {t('repoAudit.sshOk')}</span>
                      ) : (
                        <span className="text-danger">✗ {r.sshTestMessage ?? ''}</span>
                      )}
                    </td>
                    <td className="p-2 text-right">
                      {r.status === 'dirty' || r.sshCommandCount > 1 ? (
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => handleClean(r.projectId)}
                          disabled={cleaning === r.projectId || r.identityId === null}
                          title={r.identityId === null ? t('repoAudit.cleanNeedsIdentity') : ''}
                        >
                          {cleaning === r.projectId ? t('common.loading') : t('repoAudit.clean')}
                        </Button>
                      ) : (
                        <span className="text-text-2 text-xs">—</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        <DialogFooter className="gap-2">
          <span className="text-text-2 text-xs mr-auto">
            {rows && (
              <>
                {t('repoAudit.summary', { dirty: dirty.length, clean: clean.length, other: other.length })}
              </>
            )}
          </span>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>{t('common.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
