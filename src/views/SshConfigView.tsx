import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Button } from '../components/ui/button';
import {
  getSshConfig,
  validateSshConfig,
  deleteManagedHostBlock,
  type HostBlock,
} from '../ipc/sshConfig';
import { HostBlockEditorDialog } from '../features/hostBlockEditor/HostBlockEditorDialog';
import { Pencil, Trash2, Plus } from 'lucide-react';
import { toast } from 'sonner';

interface ValidateReport {
  ok: boolean;
  summary: string;
  details: string;
}

export function SshConfigView() {
  const { t } = useTranslation();
  const [hosts, setHosts] = useState<HostBlock[]>([]);
  const [report, setReport] = useState<ValidateReport | null>(null);
  const [editing, setEditing] = useState<HostBlock | null>(null);
  const [creating, setCreating] = useState(false);
  const [busyLabel, setBusyLabel] = useState<string | null>(null);

  const refresh = () => getSshConfig().then(setHosts).catch(() => setHosts([]));
  useEffect(() => { refresh(); }, []);

  const onValidate = async () => {
    setReport(null);
    try {
      const r = await validateSshConfig();
      setReport(r);
      toast[r.ok ? 'success' : 'error'](r.ok ? t('sshConfig.valid') : t('sshConfig.invalid'));
    } catch (e) {
      const r = { ok: false, summary: String(e), details: String(e) };
      setReport(r);
      toast.error(t('sshConfig.invalid'));
    }
  };

  const onDelete = async (label: string) => {
    if (busyLabel) return;
    if (!confirm(t('sshConfig.deleteConfirm', { label }))) return;
    setBusyLabel(label);
    try {
      await deleteManagedHostBlock(label);
      toast.success(t('sshConfig.deleted'));
      await refresh();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusyLabel(null);
    }
  };

  return (
    <div className="p-6 max-w-4xl space-y-4">
      <div className="flex items-center justify-between gap-2">
        <h1 className="text-2xl font-semibold">{t('sshConfig.title')}</h1>
        <div className="flex gap-2">
          <Button variant="outline" onClick={onValidate}>{t('sshConfig.validate')}</Button>
          <Button onClick={() => setCreating(true)}>
            <Plus className="h-4 w-4 mr-1" />
            {t('sshConfig.newBlock')}
          </Button>
        </div>
      </div>

      {report && (
        <Card className="p-4 space-y-2">
          <div className={report.ok ? 'text-success text-sm font-medium' : 'text-danger text-sm font-medium'}>
            {report.ok ? t('sshConfig.valid') : t('sshConfig.invalid')}
          </div>
          <div className="text-text-1 text-sm">{report.summary}</div>
          {report.details && (
            <pre className="text-xs bg-bg-0 p-3 rounded-md border border-border overflow-x-auto whitespace-pre-wrap break-all max-h-48 overflow-y-auto font-mono">
              {report.details}
            </pre>
          )}
        </Card>
      )}

      <div className="grid gap-3">
        {hosts.map((h, i) => (
          <Card key={`${h.label}-${i}`} className="p-4 space-y-2">
            <div className="flex items-center justify-between gap-2">
              <div className="flex items-center gap-2 min-w-0 flex-1">
                <span className="font-mono font-medium truncate">
                  {h.isMatch ? 'Match' : 'Host'} {h.label}
                </span>
                {h.managed && <Badge variant="outline">{t('sshConfig.managed')}</Badge>}
              </div>
              {h.managed && (
                <div className="flex gap-1 shrink-0">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setEditing(h)}
                    aria-label={t('sshConfig.editBlock')}
                  >
                    <Pencil className="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => onDelete(h.label)}
                    disabled={busyLabel === h.label}
                    aria-label={t('sshConfig.deleteBlock')}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </div>
              )}
            </div>
            <div className="font-mono text-xs space-y-0.5 pl-4">
              {h.directives.map(([k, v], j) => (
                <div key={j}><span className="text-text-2">{k}</span> <span className="text-text-0">{v}</span></div>
              ))}
            </div>
          </Card>
        ))}
        {hosts.length === 0 && <div className="text-text-1 text-center py-12">{t('sshConfig.empty')}</div>}
      </div>

      <HostBlockEditorDialog
        open={creating}
        onOpenChange={setCreating}
        onSaved={refresh}
      />
      <HostBlockEditorDialog
        open={!!editing}
        onOpenChange={(v) => !v && setEditing(null)}
        initial={editing ?? undefined}
        onSaved={refresh}
      />
    </div>
  );
}
