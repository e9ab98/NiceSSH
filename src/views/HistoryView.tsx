import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Card } from '../components/ui/card';
import { Button } from '../components/ui/button';
import { Input } from '../components/ui/input';
import { listHistory, rollback, HistoryIndexEntry } from '../ipc/history';
import { toast } from 'sonner';
import { filterEntries, FILTER_GROUPS, FilterGroup } from '../lib/historyFilter';

export function HistoryView() {
  const { t } = useTranslation();
  const [entries, setEntries] = useState<HistoryIndexEntry[]>([]);
  const [query, setQuery] = useState('');
  const [group, setGroup] = useState<FilterGroup>('all');

  const refresh = () =>
    listHistory(50).then(setEntries).catch(() => setEntries([]));
  useEffect(() => {
    refresh();
  }, []);

  const filtered = useMemo(
    () => filterEntries(entries, query, group),
    [entries, query, group],
  );

  const onRollback = async (id: string) => {
    if (!confirm(t('history.rollbackConfirm'))) return;
    try {
      await rollback(id);
      toast.success(t('history.rolledBack'));
      refresh();
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <div className="p-6 max-w-4xl">
      <h1 className="text-2xl font-semibold mb-4">{t('history.title')}</h1>

      {/* Search box */}
      <div className="mb-3">
        <Input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t('history.searchPlaceholder')}
          aria-label={t('history.searchPlaceholder')}
        />
      </div>

      {/* Filter chips + count */}
      <div className="flex items-center gap-2 mb-4 flex-wrap">
        {FILTER_GROUPS.map((g) => {
          const isActive = g === group;
          return (
            <button
              key={g}
              type="button"
              onClick={() => setGroup(g)}
              className={
                'rounded-full px-3 py-1 text-xs font-medium border transition-colors ' +
                (isActive
                  ? 'bg-brand text-white border-brand'
                  : 'bg-bg-0 text-text-1 border-border hover:bg-bg-2 hover:text-text-0')
              }
              aria-pressed={isActive}
            >
              {t(`history.filter${g.charAt(0).toUpperCase()}${g.slice(1)}`)}
            </button>
          );
        })}
        <span className="ml-auto text-xs text-text-2">
          {group === 'all' && query.trim() === ''
            ? t('history.count', { count: entries.length })
            : t('history.filteredCount', {
                shown: filtered.length,
                total: entries.length,
              })}
        </span>
      </div>

      {/* List */}
      <div className="space-y-2">
        {filtered.map((e) => (
          <Card key={e.id} className="p-3">
            <div className="flex items-center justify-between">
              <div className="min-w-0 flex-1">
                <div className="text-sm truncate">{e.summary}</div>
                <div className="text-text-2 text-xs mt-0.5">
                  {e.operation} · {e.fileCount} file(s) ·{' '}
                  {new Date(e.timestamp).toLocaleString()}
                </div>
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={() => onRollback(e.id)}
              >
                {t('history.revert')}
              </Button>
            </div>
          </Card>
        ))}
      </div>

      {/* Empty states */}
      {entries.length === 0 && (
        <div className="text-text-1 text-center py-12">{t('history.empty')}</div>
      )}
      {entries.length > 0 && filtered.length === 0 && (
        <div className="text-text-1 text-center py-12">{t('history.noMatches')}</div>
      )}
    </div>
  );
}
