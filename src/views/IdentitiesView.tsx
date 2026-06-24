import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../components/ui/tooltip';
import { useIdentitiesStore } from '../store/identities';
import { IdentityFormDialog } from '../features/identityForm/IdentityFormDialog';
import { KeyGeneratorDialog } from '../features/keyGenerator/KeyGeneratorDialog';
import { ScanResultsDialog } from '../features/scanResults/ScanResultsDialog';
import { scanExistingIdentities, type ScannedIdentity } from '../ipc/identities';
import { toast } from 'sonner';
import type { Identity } from '../ipc/identities';

export function IdentitiesView() {
  const { t } = useTranslation();
  const { items, loading, refresh, create, update, remove } = useIdentitiesStore();
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Identity | null>(null);
  const [genFor, setGenFor] = useState<Identity | null>(null);
  const [scanOpen, setScanOpen] = useState(false);
  const [candidates, setCandidates] = useState<ScannedIdentity[]>([]);
  const [scanning, setScanning] = useState(false);

  useEffect(() => { refresh(); }, [refresh]);

  const runScan = async () => {
    if (scanning) return;
    setScanning(true);
    try {
      const result = await scanExistingIdentities();
      setCandidates(result);
      setScanOpen(true);
    } catch (e) {
      toast.error(t('scanResults.scanFailed', { message: String(e) }));
    } finally {
      setScanning(false);
    }
  };

  const handleImport = async (selected: ScannedIdentity[]) => {
    let imported = 0;
    for (const c of selected) {
      // Skip anything that conflicts (defensive: UI already pre-deselects them)
      if (c.conflictsWithExisting || c.conflictsWithExistingKey) continue;
      try {
        await create({
          label: c.label,
          userName: c.userName ?? '',
          userEmail: c.userEmail ?? '',
          keyPath: c.keyPath ?? '',
          matchPath: c.matchPath,
          hostAlias: null,
          gitHost: null,
        });
        imported++;
      } catch {
        // already toasted by ipc client
      }
    }
    if (imported > 0) {
      toast.success(t('scanResults.scanSuccess_other', { count: imported }));
    }
  };

  return (
    <TooltipProvider>
      <div className="p-6 max-w-4xl">
        <div className="flex items-center justify-between mb-4 gap-2">
          <h1 className="text-2xl font-semibold">{t('identities.title')}</h1>
          <div className="flex gap-2">
            <Button variant="outline" onClick={runScan} disabled={scanning}>
              {scanning ? t('scanResults.scanning') : t('scanResults.scan')}
            </Button>
            <Button onClick={() => setFormOpen(true)}>{t('identities.newIdentity')}</Button>
          </div>
        </div>
        {loading && <div className="text-text-1">{t('common.loading')}</div>}
        <div className="grid gap-3">
          {items.map((id) => (
            <Card key={id.id} className="p-4">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="font-medium">{id.label}</span>
                    <Badge variant="outline">{id.hostAlias ?? 'github.com'}</Badge>
                  </div>
                  <div className="text-text-1 text-sm mt-1">{id.userName} &lt;{id.userEmail}&gt;</div>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <div className="text-text-2 text-xs mt-1 font-mono truncate max-w-md">{id.keyPath}</div>
                    </TooltipTrigger>
                    <TooltipContent>{id.keyPath}</TooltipContent>
                  </Tooltip>
                  {id.matchPath && <div className="text-text-2 text-xs mt-0.5">{t('identities.match')}: {id.matchPath}</div>}
                </div>
                <div className="flex gap-2 shrink-0">
                  <Button variant="outline" size="sm" onClick={() => setEditing(id)}>
                    {t('common.edit')}
                  </Button>
                  <Button variant="outline" size="sm" onClick={() => setGenFor(id)}>
                    {id.keyPath.includes('id_') ? t('identities.regenerateKey') : t('identities.generateKey')}
                  </Button>
                  <Button variant="danger" size="sm" onClick={() => { if (confirm(t('identities.deleteConfirm'))) remove(id.id); }}>{t('common.delete')}</Button>
                </div>
              </div>
            </Card>
          ))}
          {items.length === 0 && !loading && (
            <div className="text-text-1 text-center py-12">{t('identities.empty')}</div>
          )}
        </div>

        <IdentityFormDialog
          open={formOpen}
          onOpenChange={setFormOpen}
          onSubmit={async (values) => {
            await create(values);
            toast.success(t('identities.created'));
          }}
        />
        {editing && (
          <IdentityFormDialog
            open={!!editing}
            onOpenChange={(v) => !v && setEditing(null)}
            initial={editing}
            onSubmit={async (values) => {
              await update(editing.id, { ...editing, ...values });
              toast.success(t('identities.updated'));
              setEditing(null);
            }}
          />
        )}
        {genFor && (
          <KeyGeneratorDialog
            open={!!genFor}
            onOpenChange={(v) => !v && setGenFor(null)}
            defaultName={genFor.keyPath.split('/').pop() || 'id_ed25519'}
            defaultComment={genFor.userEmail}
            onGenerated={async (keyPath) => {
              await update(genFor.id, { ...genFor, keyPath });
              toast.success(t('identities.keyGenerated'));
            }}
          />
        )}
        <ScanResultsDialog
          open={scanOpen}
          onOpenChange={setScanOpen}
          candidates={candidates}
          onImport={handleImport}
        />
      </div>
    </TooltipProvider>
  );
}
