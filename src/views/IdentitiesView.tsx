import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Badge } from '../components/ui/badge';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../components/ui/tooltip';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '../components/ui/dialog';
import { useIdentitiesStore, useKeysStore } from '../store/identities';
import { useUsersStore } from '../store/users';
import { useGlobalDefaultStore } from '../store/globalDefault';
import { IdentityFormDialog } from '../features/identityForm/IdentityFormDialog';
import { KeyGeneratorDialog } from '../features/keyGenerator/KeyGeneratorDialog';
import { ScanResultsDialog } from '../features/scanResults/ScanResultsDialog';
import { ClearGlobalDefaultDialog } from '../features/clearGlobalDefault/ClearGlobalDefaultDialog';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '../components/ui/tabs';
import { UsersTab } from '../features/usersTab/UsersTab';
import { scanExistingIdentities, type ScannedIdentity } from '../ipc/identities';
import { getGlobalGitConfig, type GlobalGitConfig } from '../ipc/git';
import { toast } from 'sonner';
import type { Identity } from '../ipc/identities';
import { importKey } from '../ipc/sshKeys';
import { homeDir } from '@tauri-apps/api/path';

type DeleteMode = 'record' | 'withFiles';

/**
 * Global-default picker shown at the top of the Identities view.
 *
 * Renders as a native `<select>` styled with the project's tokens
 * (same pattern as SettingsView's theme / language / key-type
 * pickers). Choosing an identity from the dropdown calls
 * `useGlobalDefaultStore.set(id)`, which writes BOTH `~/.gitconfig`
 * and the cfg pointer via the Rust `set_global_default_identity`
 * command, then updates the store. The "Clear" button calls
 * `clear()`, which drops the pointer only and leaves `~/.gitconfig`
 * alone (the user's hand-edits are never silently undone).
 *
 * When the cfg-stored id points at a deleted identity
 * (`globalDefaultId !== null && identity === undefined`) the
 * dropdown falls back to the "no default" option rather than
 * highlight a dangling record.
 */
function GlobalDefaultSection({
  identities,
  globalDefaultId,
  busy,
  onPick,
  onClear,
}: {
  identities: Identity[];
  globalDefaultId: string | null | undefined;
  busy: boolean;
  onPick: (id: string) => Promise<void>;
  onClear: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const selected = identities.find((i) => i.id === globalDefaultId) ?? null;
  // Treat the "loaded but the id no longer resolves" case as no
  // default so the dropdown value matches what the UI promises.
  const effectiveId = selected ? selected.id : '';
  const noDefault = effectiveId === '';

  return (
    <Card className="p-4 mb-3">
      <div className="flex items-start justify-between gap-3 mb-2">
        <div className="min-w-0">
          <p className="text-text-1 text-xs">
            {t('identities.globalDefault.description')}
          </p>
        </div>
        {!noDefault && (
          <Button
            variant="ghost"
            size="sm"
            disabled={busy}
            onClick={() => { void onClear(); }}
          >
            {t('identities.globalDefault.clear')}
          </Button>
        )}
      </div>
      {identities.length === 0 ? (
        <div className="text-text-2 text-sm py-2">
          {t('identities.globalDefault.empty')}
        </div>
      ) : (
        <label className="flex items-center justify-between gap-3">
          <span className="text-text-1 text-sm shrink-0">
            {t('identities.globalDefault.selectLabel')}
          </span>
          <select
            value={effectiveId}
            disabled={busy}
            onChange={(e) => {
              const next = e.target.value;
              if (next === '') {
                void onClear();
              } else if (next !== effectiveId) {
                void onPick(next);
              }
            }}
            className="h-9 min-w-0 flex-1 max-w-md rounded-md border border-border bg-bg-0 px-3 text-sm text-text-0 disabled:opacity-60"
          >
            <option value="">{t('identities.globalDefault.noneOption')}</option>
            {identities.map((id) => (
              <option key={id.id} value={id.id}>
                {`${id.label} — ${id.userName} <${id.userEmail}>`}
              </option>
            ))}
          </select>
        </label>
      )}
    </Card>
  );
}

export function IdentitiesTab() {
  const { t } = useTranslation();
  const { items, loading, refresh, create, update, remove } = useIdentitiesStore();
  // Read keys from the shared store so other views
  // (Projects / IdentitySwitcher) see freshly imported keys
  // without a full page reload.
  const keys = useKeysStore((s) => s.items);
  const refreshKeys = useKeysStore((s) => s.refresh);
  const globalDefaultId = useGlobalDefaultStore((s) => s.id);
  const setGlobalDefault = useGlobalDefaultStore((s) => s.set);
  const clearGlobalDefault = useGlobalDefaultStore((s) => s.clear);
  const refreshGlobalDefault = useGlobalDefaultStore((s) => s.refresh);
  const [globalDefaultBusy, setGlobalDefaultBusy] = useState(false);
  // Confirmation dialog for unset. We snapshot the current effective
  // user.name/email at open-time so the dialog can preview what will
  // be removed from ~/.gitconfig.
  const [clearOpen, setClearOpen] = useState(false);
  const [clearSnapshot, setClearSnapshot] = useState<{
    userName: string | null;
    userEmail: string | null;
  } | null>(null);
  const [home, setHome] = useState('');
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Identity | null>(null);
  const [genFor, setGenFor] = useState<Identity | null>(null);
  const [scanOpen, setScanOpen] = useState(false);
  const [candidates, setCandidates] = useState<ScannedIdentity[]>([]);
  const [scanning, setScanning] = useState(false);

  // Delete dialog state
  const [deleting, setDeleting] = useState<Identity | null>(null);
  const [deleteMode, setDeleteMode] = useState<DeleteMode>('record');
  const [deleteBusy, setDeleteBusy] = useState(false);

  useEffect(() => {
    refresh();
    void refreshKeys();
    void refreshGlobalDefault();
    void homeDir().then(setHome).catch(() => setHome(''));
  }, [refresh, refreshKeys, refreshGlobalDefault]);
  const keyById = (id: string | null) => keys.find((key) => key.id === id) ?? null;

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

  /**
   * As a last-ditch fallback: when a scanned candidate has no
   * `keyPath` (the backend should always backfill this, but
   * guard anyway), look in the current `keys` state for a
   * record whose `privatePath` basename matches `id_<label>`
   * or `<label>`. Returns the absolute path of that record so
   * `importKey` (which is a no-op for already-known keys) can
   * still resolve the import. Mirrors the backend's
   * `backfill_key_paths`.
   */
  async function guessKeyPathForLabel(label: string): Promise<string | null> {
    const names = new Set([`id_${label}`, label]);
    const match = keys.find((k) => {
      const base = k.privatePath.split("/").pop();
      return base !== undefined && names.has(base);
    });
    return match?.privatePath ?? null;
  }

  const handleImport = async (selected: ScannedIdentity[]) => {
    let imported = 0;
    for (const c of selected) {
      // Skip anything that conflicts (defensive: UI already pre-deselects them)
      if (c.conflictsWithExisting || c.conflictsWithExistingKey) continue;
      try {
        // Defensive: after the backend's dedupe+backfill pass,
        // `c.keyPath` should always be set. If it's still null
        // (e.g. gitconfig label has no matching key file in
        // ~/.ssh/), do NOT create an un-bound identity - that
        // would show up as "未绑定 SSH 密钥". Skip with a
        // warning toast instead.
        let resolvedKeyPath = c.keyPath ?? null;
        if (!resolvedKeyPath) {
          const guessed = await guessKeyPathForLabel(c.label);
          if (guessed) resolvedKeyPath = guessed;
        }
        const scannedKey = resolvedKeyPath
          ? (keys.find((key) => normalizePath(key.privatePath) === normalizePath(resolvedKeyPath!)) ?? await importKey(resolvedKeyPath))
          : null;
        if (!scannedKey) {
          toast.warning(
            t('identities.scanNoKeyForLabel', { label: c.label }),
          );
          continue;
        }
        await create({
          label: c.label,
          userName: c.userName ?? '',
          userEmail: c.userEmail ?? '',
          sshKeyId: scannedKey.id,
          matchPath: c.matchPath,
          hostAlias: null,
          gitHost: null,
          // v3: scanned identities have no signing config.
          requireSignedCommits: false,
          signingKeyId: null,
          signingKeyKind: 'ssh',
        });
        // Make the freshly-imported record visible to all
        // views (Projects / IdentitySwitcher) immediately.
        await refreshKeys();
        imported++;
      } catch {
        // already toasted by ipc client
      }
    }
    if (imported > 0) {
      toast.success(t('scanResults.scanSuccess_other', { count: imported }));
    }
  };

  function normalizePath(value: string): string {
    return value.replace(/\\/g, '/').replace(/^~\//, home ? `${home.replace(/\\/g, '/')}/` : '~/');
  }

  const openDelete = (id: Identity) => {
    setDeleting(id);
    setDeleteMode('record'); // safe default
  };

  const closeDelete = () => {
    if (deleteBusy) return;
    setDeleting(null);
  };

  const confirmDelete = async () => {
    if (!deleting || deleteBusy) return;
    setDeleteBusy(true);
    const target = deleting;
    const key = keyById(target.sshKeyId);
    const deleteFiles = deleteMode === 'withFiles' && !!key;
    try {
      await remove(target.id, { deleteFiles });
      // The deleted identity may have been the global default; if so the
      // cfg-stored pointer now references a missing record. Refresh the
      // global-default store so the section re-renders without a stale
      // highlight. Best-effort: ignore failures since the main delete
      // already toast-succeeded.
      void refreshGlobalDefault();
      toast.success(
        deleteFiles ? t('identities.deletedWithFiles') : t('identities.deleted'),
      );
      setDeleting(null);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setDeleteBusy(false);
    }
  };

  const handlePickGlobalDefault = async (id: string) => {
    if (globalDefaultBusy) return;
    setGlobalDefaultBusy(true);
    try {
      await setGlobalDefault(id);
      const target = items.find((x) => x.id === id);
      toast.success(
        t('identities.globalDefault.applied', {
          label: target?.label ?? '',
          email: target?.userEmail ?? '',
        }),
      );
    } catch (e) {
      // The store has already reverted the optimistic update on
      // failure, so the toast here is the only feedback.
      toast.error(String(e));
    } finally {
      setGlobalDefaultBusy(false);
    }
  };

  /// Opens the confirmation dialog. Snapshots the current effective
  /// user.name/email so the user can see what will be removed.
  const handleClearGlobalDefault = async () => {
    if (globalDefaultBusy) return;
    let snapshot: { userName: string | null; userEmail: string | null } = {
      userName: null,
      userEmail: null,
    };
    try {
      const cfg: GlobalGitConfig = await getGlobalGitConfig();
      snapshot = { userName: cfg.userName, userEmail: cfg.userEmail };
    } catch {
      // If we can't read the gitconfig the dialog will just show
      // empty preview; the user can still confirm.
    }
    setClearSnapshot(snapshot);
    setClearOpen(true);
  };

  /// Actual unset, called from the confirmation dialog.
  const handleClearGlobalDefaultConfirm = async () => {
    if (globalDefaultBusy) return;
    setGlobalDefaultBusy(true);
    try {
      await clearGlobalDefault();
      toast.success(t('identities.globalDefault.cleared'));
      setClearOpen(false);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setGlobalDefaultBusy(false);
    }
  };

  return (
    <TooltipProvider>
      <div>
        <h1 className="text-2xl font-semibold mb-4">{t('identities.globalDefault.title')}</h1>
        <GlobalDefaultSection
          identities={items}
          globalDefaultId={globalDefaultId}
          busy={globalDefaultBusy}
          onPick={handlePickGlobalDefault}
          onClear={handleClearGlobalDefault}
        />

        <ClearGlobalDefaultDialog
          open={clearOpen}
          onOpenChange={setClearOpen}
          userName={clearSnapshot?.userName ?? null}
          userEmail={clearSnapshot?.userEmail ?? null}
          busy={globalDefaultBusy}
          onConfirm={handleClearGlobalDefaultConfirm}
        />

        {/* Section break: the global-default picker above is its own
            concern; the cards below are the user's identity registry
            (create / edit / generate / delete). The action buttons
            (scan, + new) live next to the section title because they
            only ever act on this list, not on the global-default
            picker above. */}
        <div className="mt-6 mb-2 flex items-center justify-between gap-2">
          <div className="flex items-baseline gap-2">
            <h2 className="text-2xl font-semibold">
              {t('identities.section.identities')}
            </h2>
            <span className="text-text-2 text-sm">
              {t('identities.section.count', { count: items.length })}
            </span>
          </div>
          <div className="flex gap-2">
            <Button variant="outline" size="sm" onClick={runScan} disabled={scanning}>
              {scanning ? t('scanResults.scanning') : t('scanResults.scan')}
            </Button>
            <Button size="sm" onClick={() => setFormOpen(true)}>{t('identities.newIdentity')}</Button>
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
                  {keyById(id.sshKeyId) ? (
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <div className="text-text-2 text-xs mt-1 font-mono truncate max-w-md">{keyById(id.sshKeyId)?.privatePath}</div>
                      </TooltipTrigger>
                      <TooltipContent>{keyById(id.sshKeyId)?.privatePath}</TooltipContent>
                    </Tooltip>
                  ) : (
                    <div className="text-warning text-xs mt-1">{t('identities.noKeyBound')}</div>
                  )}
                  {id.matchPath && <div className="text-text-2 text-xs mt-0.5">{t('identities.match')}: {id.matchPath}</div>}
                </div>
                <div className="flex gap-2 shrink-0">
                  <Button variant="outline" size="sm" onClick={() => setEditing(id)}>
                    {t('common.edit')}
                  </Button>
                  <Button variant="outline" size="sm" onClick={() => setGenFor(id)}>
                    {id.sshKeyId ? t('identities.regenerateKey') : t('identities.generateAndBindKey')}
                  </Button>
                  <Button variant="danger" size="sm" onClick={() => openDelete(id)}>
                    {t('common.delete')}
                  </Button>
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
            defaultName={genFor.label || 'id_ed25519'}
            defaultDir={keyById(genFor.sshKeyId)?.privatePath?.replace(/[/\\][^/\\]+$/, '') || '~/.ssh/'}
            defaultComment={genFor.userEmail}
            onGenerated={async (privatePath) => {
              // Refresh the shared keys store so ProjectsView
              // and IdentitySwitcherDialog see the new record
              // immediately, then pick it back out by path to
              // bind it to the current identity. `genFor` may
              // have been cleared while this async tick runs
              // (the user closed the dialog), so guard before
              // mutating the store.
              await refreshKeys();
              if (!genFor) return;
              const stored = useKeysStore.getState().items;
              const key = stored.find(
                (item) => item.privatePath === privatePath,
              );
              if (!key) {
                throw new Error('Generated SSH key was not found');
              }
              await update(genFor.id, { ...genFor, sshKeyId: key.id });
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

        {/* Delete confirmation dialog */}
        <Dialog
          open={!!deleting}
          onOpenChange={(v) => { if (!v) closeDelete(); }}
        >
          <DialogContent>
            <DialogHeader>
              <DialogTitle>{t('identities.deleteDialogTitle')}</DialogTitle>
              <DialogDescription>
                {deleting?.label}
              </DialogDescription>
            </DialogHeader>

            <div className="space-y-3">
              <label className="flex items-start gap-2 cursor-pointer rounded-md border border-border p-3 hover:bg-bg-2">
                <input
                  type="radio"
                  className="mt-1"
                  name="delete-mode"
                  value="record"
                  checked={deleteMode === 'record'}
                  onChange={() => setDeleteMode('record')}
                />
                <div className="min-w-0">
                  <div className="font-medium text-text-0">
                    {t('identities.deleteRecordOnly')}
                  </div>
                  <div className="text-text-1 text-xs mt-0.5">
                    {t('identities.deleteRecordOnlyHint')}
                  </div>
                </div>
              </label>

              <label
                className={
                  'flex items-start gap-2 rounded-md border p-3 ' +
                  (deleting && !!keyById(deleting.sshKeyId)
                    ? 'border-border cursor-pointer hover:bg-bg-2'
                    : 'border-border opacity-60 cursor-not-allowed')
                }
              >
                <input
                  type="radio"
                  className="mt-1"
                  name="delete-mode"
                  value="withFiles"
                  checked={deleteMode === 'withFiles'}
                  disabled={!deleting || !keyById(deleting.sshKeyId)}
                  onChange={() => setDeleteMode('withFiles')}
                />
                <div className="min-w-0">
                  <div className="font-medium text-text-0">
                    {t('identities.deleteWithFiles')}
                  </div>
                  <div className="text-text-1 text-xs mt-0.5">
                    {t('identities.deleteWithFilesHint')}
                  </div>
                  {deleting && keyById(deleting.sshKeyId) ? (
                    deleteMode === 'withFiles' ? (
                      <div className="text-danger text-xs mt-2 break-all">
                        {t('identities.deleteFileWarning', { path: keyById(deleting.sshKeyId)?.privatePath })}
                      </div>
                    ) : (
                      <div className="text-text-2 text-xs mt-2 font-mono break-all">
                        {keyById(deleting.sshKeyId)?.privatePath}
                      </div>
                    )
                  ) : (
                    <div className="text-text-2 text-xs mt-2">
                      {t('identities.deleteNoKeyPath')}
                    </div>
                  )}
                </div>
              </label>
            </div>

            <DialogFooter>
              <Button variant="outline" onClick={closeDelete} disabled={deleteBusy}>
                {t('common.cancel')}
              </Button>
              <Button variant="danger" onClick={confirmDelete} disabled={deleteBusy}>
                {deleteBusy ? t('common.loading') : t('common.delete')}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </div>
    </TooltipProvider>
  );
}

/**
 * Top-level identities section. Two tabs:
 *   - Users: pool of git (name, email) pairs that can be reused
 *     across multiple identities / keys. New in this release.
 *   - Identities: existing registry of identities (each tied to
 *     a key, with optional project scoping). Unchanged behaviour.
 *
 *   The default tab is Identities — preserves the historical
 *   landing for existing users.
 */
export function IdentitiesView() {
  const { t } = useTranslation();
  const refreshUsers = useUsersStore((s) => s.refresh);
  const refreshIdentities = useIdentitiesStore((s) => s.refresh);
  useEffect(() => {
    void refreshUsers();
    void refreshIdentities();
  }, [refreshUsers, refreshIdentities]);

  return (
    <Tabs defaultValue="identities" className="p-6 max-w-4xl">
      <TabsList>
        <TabsTrigger value="identities">{t('identities.section.identities')}</TabsTrigger>
        <TabsTrigger value="users">{t('identities.section.users')}</TabsTrigger>
      </TabsList>
      <TabsContent value="identities">
        <IdentitiesTab />
      </TabsContent>
      <TabsContent value="users">
        <UsersTab />
      </TabsContent>
    </Tabs>
  );
}
