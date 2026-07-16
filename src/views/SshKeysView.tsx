import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Button } from '../components/ui/button';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../components/ui/dialog';
import {
  type SshKeyInfo,
  getPublicKey,
  copyPublicKey,
  deleteKey,
} from '../ipc/sshKeys';
import { useIdentitiesStore, useKeysStore } from '../store/identities';
import { IdentityFormDialog } from '../features/identityForm/IdentityFormDialog';
import { Copy, Eye, Trash2, Pencil, Plus } from 'lucide-react';
import { toast } from 'sonner';
import type { Identity } from '../ipc/identities';

export function SshKeysView() {
  const { t } = useTranslation();
  const keys = useKeysStore((s) => s.items);
  const refreshKeys = useKeysStore((s) => s.refresh);
  const { items: identities, refresh: refreshIdentities, create, update } = useIdentitiesStore();
  const [editing, setEditing] = useState<Identity | null>(null);
  const [creating, setCreating] = useState<SshKeyInfo | null>(null);
  const [viewingPub, setViewingPub] = useState<{ name: string; pub: string } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  // Back-compat alias so the existing call sites (e.g. onDelete)
  // continue to work unchanged.
  const refresh = refreshKeys;

  useEffect(() => {
    refreshKeys();
    refreshIdentities();
  }, [refreshIdentities, refreshKeys]);

  const matchesByKey = useMemo(() => {
    const map = new Map<string, Identity[]>();
    for (const k of keys) {
      const matches = identities.filter((id) => id.sshKeyId === k.id);
      map.set(k.privatePath, matches);
    }
    return map;
  }, [keys, identities]);

  const onCopy = async (name: string) => {
    if (busy) return;
    setBusy(name);
    try {
      await copyPublicKey(name);
      toast.success(t('sshKeys.copied'));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(null);
    }
  };

  const onView = async (name: string) => {
    if (busy) return;
    setBusy(name);
    try {
      const pub = await getPublicKey(name);
      setViewingPub({ name, pub });
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(null);
    }
  };

  const onDelete = async (name: string) => {
    if (busy) return;
    if (!confirm(t('sshKeys.deleteConfirm', { name }))) return;
    setBusy(name);
    try {
      await deleteKey(name);
      toast.success(t('sshKeys.deleted'));
      await refresh();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="p-6 max-w-4xl">
      <h1 className="text-2xl font-semibold mb-4">{t('sshKeys.title')}</h1>
      <div className="grid gap-3">
        {keys.map((k) => {
          const matches = matchesByKey.get(k.privatePath) ?? [];
          const primary = matches[0];
          const isBusy = busy === k.name;
          return (
            <Card key={k.name} className="p-4 space-y-2">
              <div className="flex items-center gap-2">
                <span className="font-medium font-mono">{k.name}</span>
                {k.keyType && <Badge variant="outline">{k.keyType}</Badge>}
              </div>
              <div className="text-text-1 text-xs font-mono">{k.privatePath}</div>
              {k.fingerprint && <div className="text-text-2 text-xs font-mono">{k.fingerprint}</div>}

              <div className="mt-2 flex items-center justify-between gap-3 border-t border-border pt-3">
                <div className="min-w-0 flex-1">
                  {primary ? (
                    <>
                      <div className="text-text-1 text-sm truncate">
                        {primary.userName} &lt;{primary.userEmail}&gt;
                      </div>
                      {matches.length > 1 && (
                        <div className="text-text-2 text-xs mt-0.5">
                          {t('sshKeys.linkedIdentities', { count: matches.length })}
                        </div>
                      )}
                    </>
                  ) : (
                    <div className="text-text-2 text-sm">{t('sshKeys.unlinked')}</div>
                  )}
                </div>
                <div className="flex gap-1.5 shrink-0 flex-wrap justify-end">
                  {primary ? (
                    <Button variant="outline" size="sm" onClick={() => setEditing(primary)} disabled={isBusy}>
                      <Pencil className="h-3.5 w-3.5 mr-1" />
                      {t('common.edit')}
                    </Button>
                  ) : (
                    <Button variant="outline" size="sm" onClick={() => setCreating(k)} disabled={isBusy}>
                      <Plus className="h-3.5 w-3.5 mr-1" />
                      {t('sshKeys.createIdentity')}
                    </Button>
                  )}
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => onView(k.name)}
                    disabled={isBusy || !k.publicPath}
                    title={!k.publicPath ? t('sshKeys.noPubKey') : t('sshKeys.view')}
                    aria-label={t('sshKeys.view')}
                  >
                    <Eye className="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => onCopy(k.name)}
                    disabled={isBusy || !k.publicPath}
                    title={!k.publicPath ? t('sshKeys.noPubKey') : t('sshKeys.copy')}
                    aria-label={t('sshKeys.copy')}
                  >
                    <Copy className="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => onDelete(k.name)}
                    disabled={isBusy}
                    title={t('sshKeys.delete')}
                    aria-label={t('sshKeys.delete')}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>
            </Card>
          );
        })}
        {keys.length === 0 && <div className="text-text-1 text-center py-12">{t('sshKeys.empty')}</div>}
      </div>

      {editing && (
        <IdentityFormDialog
          open={!!editing}
          onOpenChange={(v) => !v && setEditing(null)}
          initial={editing}
          onSubmit={async (values) => {
            await update(editing.id, { ...editing, ...values });
            setEditing(null);
          }}
        />
      )}

      {creating && (() => {
            // The scanner gave us a *file* path for an existing SSH key
        // The existing key path is binding context, not a path to edit.
        return (
          <IdentityFormDialog
            key={creating.privatePath}
            open={!!creating}
            onOpenChange={(v) => !v && setCreating(null)}
            defaultLabel={creating.name}
            onSubmit={async (values) => {
              await create({ ...values, sshKeyId: creating.id });
              setCreating(null);
            }}
          />
        );
      })()}

      <Dialog open={!!viewingPub} onOpenChange={(v) => !v && setViewingPub(null)}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t('sshKeys.viewTitle', { name: viewingPub?.name ?? '' })}</DialogTitle>
          </DialogHeader>
          <pre className="text-xs bg-bg-0 p-3 rounded-md border border-border overflow-x-auto whitespace-pre-wrap break-all max-h-80 overflow-y-auto font-mono">
            {viewingPub?.pub ?? ''}
          </pre>
          <DialogFooter className="gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={async () => {
                if (!viewingPub) return;
                try {
                  await copyPublicKey(viewingPub.name);
                  toast.success(t('sshKeys.copied'));
                } catch (e) {
                  toast.error(String(e));
                }
              }}
            >
              <Copy className="h-3.5 w-3.5 mr-1" />
              {t('sshKeys.copy')}
            </Button>
            <Button onClick={() => setViewingPub(null)}>{t('common.close')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
