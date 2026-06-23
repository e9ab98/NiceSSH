import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Button } from '../components/ui/button';
import { listKeys, SshKeyInfo } from '../ipc/sshKeys';
import { useIdentitiesStore } from '../store/identities';
import { IdentityFormDialog } from '../features/identityForm/IdentityFormDialog';
import type { Identity } from '../ipc/identities';

// Match a key file (absolute path, e.g. /Users/x/.ssh/id_ed25519) against
// an Identity.keyPath (which the user may have stored as "~/.ssh/id_ed25519"
// or "/Users/x/.ssh/id_ed25519"). Compares filenames plus the parent dir.
function sameKeyFile(privatePath: string, identityKeyPath: string): boolean {
  if (!privatePath || !identityKeyPath) return false;
  const a = privatePath.replace(/\\/g, '/');
  let b = identityKeyPath.replace(/\\/g, '/');
  if (b.startsWith('~/')) b = b.slice(2);
  if (a === b) return true;
  return a.split('/').slice(-2).join('/') === b.split('/').slice(-2).join('/');
}

export function SshKeysView() {
  const { t } = useTranslation();
  const [keys, setKeys] = useState<SshKeyInfo[]>([]);
  const { items: identities, refresh: refreshIdentities, create, update } = useIdentitiesStore();
  const [editing, setEditing] = useState<Identity | null>(null);
  const [creating, setCreating] = useState<SshKeyInfo | null>(null);

  useEffect(() => {
    listKeys().then(setKeys).catch(() => setKeys([]));
    refreshIdentities();
  }, [refreshIdentities]);

  const matchesByKey = useMemo(() => {
    const map = new Map<string, Identity[]>();
    for (const k of keys) {
      const matches = identities.filter((id) => sameKeyFile(k.privatePath, id.keyPath));
      map.set(k.privatePath, matches);
    }
    return map;
  }, [keys, identities]);

  return (
    <div className="p-6 max-w-4xl">
      <h1 className="text-2xl font-semibold mb-4">{t('sshKeys.title')}</h1>
      <div className="grid gap-3">
        {keys.map((k) => {
          const matches = matchesByKey.get(k.privatePath) ?? [];
          const primary = matches[0];
          return (
            <Card key={k.name} className="p-4">
              <div className="flex items-center gap-2">
                <span className="font-medium font-mono">{k.name}</span>
                {k.keyType && <Badge variant="outline">{k.keyType}</Badge>}
              </div>
              <div className="text-text-1 text-xs mt-1 font-mono">{k.privatePath}</div>
              {k.fingerprint && <div className="text-text-2 text-xs mt-0.5 font-mono">{k.fingerprint}</div>}

              <div className="mt-3 flex items-center justify-between gap-3 border-t border-border pt-3">
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
                <div className="flex gap-2 shrink-0">
                  {primary ? (
                    <Button variant="outline" size="sm" onClick={() => setEditing(primary)}>
                      {t('common.edit')}
                    </Button>
                  ) : (
                    <Button variant="outline" size="sm" onClick={() => setCreating(k)}>
                      {t('sshKeys.createIdentity')}
                    </Button>
                  )}
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

      {creating && (
        <IdentityFormDialog
          key={creating.privatePath}
          open={!!creating}
          onOpenChange={(v) => !v && setCreating(null)}
          defaultKeyPath={creating.privatePath}
          onSubmit={async (values) => {
            await create({ ...values, keyPath: creating.privatePath });
            setCreating(null);
          }}
        />
      )}
    </div>
  );
}
