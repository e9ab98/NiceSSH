import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { Badge } from '../../components/ui/badge';
import type { ScannedIdentity } from '../../ipc/identities';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  candidates: ScannedIdentity[];
  onImport: (selected: ScannedIdentity[]) => Promise<void>;
}

export function ScanResultsDialog({ open, onOpenChange, candidates, onImport }: Props) {
  const { t } = useTranslation();
  // Pre-select everything that doesn't conflict. User can uncheck before importing.
  const [selected, setSelected] = useState<Set<string>>(
    () => new Set(
      candidates
        .filter((c) => !c.conflictsWithExisting && !c.conflictsWithExistingKey)
        .map((c) => keyFor(c))
    )
  );
  const [busy, setBusy] = useState(false);

  const toggle = (c: ScannedIdentity) => {
    const k = keyFor(c);
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(k)) next.delete(k);
      else next.add(k);
      return next;
    });
  };

  const handleImport = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const toImport = candidates.filter((c) => selected.has(keyFor(c)));
      await onImport(toImport);
      onOpenChange(false);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>
            {t('scanResults.title')} ({candidates.length})
          </DialogTitle>
        </DialogHeader>

        {candidates.length === 0 ? (
          <div className="text-text-1 text-sm py-8 text-center">{t('scanResults.empty')}</div>
        ) : (
          <div className="space-y-2 max-h-[60vh] overflow-y-auto">
            {candidates.map((c) => {
              const k = keyFor(c);
              const isSelected = selected.has(k);
              const conflict = c.conflictsWithExisting || c.conflictsWithExistingKey;
              return (
                <label
                  key={k}
                  className={`block p-3 rounded-md border transition-colors cursor-pointer ${
                    isSelected ? 'border-accent bg-bg-2' : 'border-border hover:bg-bg-2'
                  }`}
                >
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2 min-w-0 flex-1">
                      <input
                        type="checkbox"
                        checked={isSelected}
                        onChange={() => toggle(c)}
                        className="shrink-0"
                      />
                      <span className="font-medium truncate">{c.label}</span>
                      {c.provenance.kind === 'gitconfig_include_if' ? (
                        <Badge variant="outline">{t('scanResults.fromGitconfig')}</Badge>
                      ) : (
                        <Badge variant="outline">{t('scanResults.fromSsh')}</Badge>
                      )}
                      {conflict && <Badge variant="warning">{t('scanResults.conflict')}</Badge>}
                    </div>
                  </div>
                  <div className="text-text-1 text-xs mt-1 grid grid-cols-2 gap-x-3">
                    {c.userName && <div>name: <span className="font-mono">{c.userName}</span></div>}
                    {c.userEmail && <div>email: <span className="font-mono">{c.userEmail}</span></div>}
                    {c.keyPath && <div className="col-span-2">key: <span className="font-mono break-all">{c.keyPath}</span></div>}
                    {c.matchPath && <div className="col-span-2">match: <span className="font-mono">{c.matchPath}</span></div>}
                  </div>
                  <div className="text-text-2 text-xs mt-1">{c.provenance.detail}</div>
                  {conflict && (
                    <div className="text-warning text-xs mt-1">
                      {c.conflictsWithExisting && t('scanResults.conflictLabel')}
                      {c.conflictsWithExisting && c.conflictsWithExistingKey && ' · '}
                      {c.conflictsWithExistingKey && t('scanResults.conflictKey')}
                    </div>
                  )}
                </label>
              );
            })}
          </div>
        )}

        <DialogFooter className="gap-2">
          <Button variant="ghost" onClick={() => onOpenChange(false)}>{t('common.cancel')}</Button>
          <Button onClick={handleImport} disabled={busy || selected.size === 0}>
            {busy ? t('scanResults.importing') : t('scanResults.import', { count: selected.size })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function keyFor(c: ScannedIdentity): string {
  // Combine label + key path to give each candidate a stable identity
  // (two candidates could share a label from different sources).
  return `${c.label}::${c.keyPath ?? ''}::${c.matchPath ?? ''}`;
}
