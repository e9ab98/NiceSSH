import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Card } from '../components/ui/card';
import { Button } from '../components/ui/button';
import { listHistory, rollback, HistoryIndexEntry } from '../ipc/history';
import { toast } from 'sonner';

export function HistoryView() {
  const { t } = useTranslation();
  const [entries, setEntries] = useState<HistoryIndexEntry[]>([]);
  const refresh = () => listHistory(50).then(setEntries).catch(() => setEntries([]));
  useEffect(() => { refresh(); }, []);
  const onRollback = async (id: string) => {
    if (!confirm(t('history.rollbackConfirm'))) return;
    try { await rollback(id); toast.success(t('history.rolledBack')); refresh(); }
    catch (e) { toast.error(String(e)); }
  };
  return (
    <div className="p-6 max-w-4xl">
      <h1 className="text-2xl font-semibold mb-4">{t('history.title')}</h1>
      <div className="space-y-2">
        {entries.map((e) => (
          <Card key={e.id} className="p-3">
            <div className="flex items-center justify-between">
              <div className="min-w-0 flex-1">
                <div className="text-sm truncate">{e.summary}</div>
                <div className="text-text-2 text-xs mt-0.5">{e.operation} · {e.fileCount} file(s) · {new Date(e.timestamp).toLocaleString()}</div>
              </div>
              <Button variant="outline" size="sm" onClick={() => onRollback(e.id)}>{t('history.revert')}</Button>
            </div>
          </Card>
        ))}
        {entries.length === 0 && <div className="text-text-1 text-center py-12">{t('history.empty')}</div>}
      </div>
    </div>
  );
}
