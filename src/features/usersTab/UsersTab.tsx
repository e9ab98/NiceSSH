import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '../../components/ui/button';
import { Card } from '../../components/ui/card';
import { useUsersStore } from '../../store/users';
import { useIdentitiesStore } from '../../store/identities';
import { importUsersFromGitconfig } from '../../ipc/users';
import { UserFormDialog } from '../userForm/UserFormDialog';
import { UserDeleteDialog } from '../userDelete/UserDeleteDialog';
import { toast } from 'sonner';
import type { User } from '../../ipc/users';
import type { Identity } from '../../ipc/identities';

/**
 * Users tab body — list of git users in the pool, with edit /
 * delete / import actions.
 *
 * Layout follows the project identities list (Card-per-row +
 * inline edit/delete buttons) so the tab-switching UX between
 * [Users] and [Identities] feels consistent.
 */
export function UsersTab() {
  const { t } = useTranslation();
  const items = useUsersStore((s) => s.items);
  const refresh = useUsersStore((s) => s.refresh);
  const identities = useIdentitiesStore((s) => s.items);
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<User | null>(null);
  const [deleting, setDeleting] = useState<User | null>(null);
  const [importing, setImporting] = useState(false);

  const handleImport = async () => {
    if (importing) return;
    setImporting(true);
    try {
      const created = await importUsersFromGitconfig();
      // Refresh the local store so the new users show up; pass the
      // created list directly so the toast can quote the count
      // without an extra round-trip.
      await refresh();
      if (created.length === 0) {
        toast.info(t('usersTab.importNothing'));
      } else {
        toast.success(
          t('usersTab.importSuccess', { count: created.length }),
        );
      }
    } catch (e) {
      toast.error(t('usersTab.importFailed', { message: String(e) }));
    } finally {
      setImporting(false);
    }
  };

  const openCreate = () => {
    setEditing(null);
    setFormOpen(true);
  };

  const openEdit = (u: User) => {
    setEditing(u);
    setFormOpen(true);
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-2">
        <div className="text-text-1 text-sm">{t('usersTab.subtitle')}</div>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={handleImport}
            disabled={importing}
          >
            {importing ? t('usersTab.importing') : t('usersTab.importFromGitconfig')}
          </Button>
          <Button size="sm" onClick={openCreate}>
            {t('usersTab.newUser')}
          </Button>
        </div>
      </div>

      {items.length === 0 ? (
        <Card className="p-6 text-center text-text-2 text-sm">
          {t('usersTab.empty')}
        </Card>
      ) : (
        <div className="space-y-2">
          {items.map((u) => {
            const linkedCount = identities.filter(
              (i: Identity) => i.userName === u.name && i.userEmail === u.email,
            ).length;
            return (
              <Card key={u.id} className="p-3">
                <div className="flex items-center justify-between gap-2">
                  <div className="min-w-0 flex-1">
                    <div className="font-medium truncate">
                      {u.name} <span className="text-text-2 font-mono text-xs">&lt;{u.email}&gt;</span>
                    </div>
                    <div className="text-text-2 text-xs mt-1">
                      {linkedCount === 0
                        ? t('usersTab.linkedNone')
                        : t('usersTab.linkedCount', { count: linkedCount })}
                    </div>
                  </div>
                  <div className="flex items-center gap-2 shrink-0">
                    <Button variant="ghost" size="sm" onClick={() => openEdit(u)}>
                      {t('common.edit')}
                    </Button>
                    <Button variant="ghost" size="sm" onClick={() => setDeleting(u)}>
                      {t('common.delete')}
                    </Button>
                  </div>
                </div>
              </Card>
            );
          })}
        </div>
      )}

      <UserFormDialog
        open={formOpen}
        onOpenChange={setFormOpen}
        initial={editing}
      />
      <UserDeleteDialog
        open={deleting !== null}
        onOpenChange={(v) => !v && setDeleting(null)}
        user={deleting}
      />
    </div>
  );
}
