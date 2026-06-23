import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Button } from '../components/ui/button';
import { getSshConfig, validateSshConfig, HostBlock } from '../ipc/sshConfig';
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
  useEffect(() => { getSshConfig().then(setHosts).catch(() => setHosts([])); }, []);

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

  return (
    <div className="p-6 max-w-4xl space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t('sshConfig.title')}</h1>
        <Button variant="outline" onClick={onValidate}>{t('sshConfig.validate')}</Button>
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
          <Card key={i} className="p-4">
            <div className="flex items-center gap-2 mb-2">
              <span className="font-mono font-medium">{h.isMatch ? 'Match' : 'Host'} {h.label}</span>
              {h.managed && <Badge variant="outline">{t('sshConfig.managed')}</Badge>}
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
    </div>
  );
}
