import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { Badge } from '../../components/ui/badge';
import { Label } from '../../components/ui/input';
import { useUsersStore } from '../../store/users';
import { UserFormDialog } from '../userForm/UserFormDialog';
import type { ScannedIdentity } from '../../ipc/identities';
import type { User } from '../../ipc/users';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  candidates: ScannedIdentity[];
  onImport: (selected: ScannedIdentity[]) => Promise<void>;
}

/**
 * Per-row pick state. The scanner returns `userName` / `userEmail`
 * as `string | null`; we use those to **auto-pick** a matching User
 * from the pool on dialog open (value-based linkage). After that
 * the user is in charge: each row must have a User picked before
 * import is allowed, and the only way to change the picked user
 * is via the dropdown — there are no longer inline name/email
 * inputs to edit ad-hoc.
 */
type RowPick = { userId: string | null };

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
  // userId picked per row. `null` means "no User bound yet" and
  // blocks import for that row. We seed this on dialog open from
  // the scanner's (userName, userEmail) so rows that already match
  // an existing User don't need any user action.
  const [picks, setPicks] = useState<Record<string, RowPick>>({});
  // When a row picks "+ New user..." from its dropdown, we open
  // the shared UserFormDialog scoped to that row key. On save,
  // the new User is auto-picked for that row.
  const [creatingFor, setCreatingFor] = useState<string | null>(null);

  // Pull users from the global store. IdentitiesView mounts this
  // store on section load so the list is already populated by the
  // time the dialog opens. We still re-read on every render so a
  // "+ New user..." save (which mutates the store from this dialog)
  // shows up immediately in the dropdown.
  const users = useUsersStore((s) => s.items);

  // Reset per-row picks + creation flag whenever the dialog re-opens
  // with a fresh scan, otherwise stale picks leak across scans.
  //
  // Bug fix: previously this unconditionally overwrote every row
  // with the scanner's auto-pick whenever `users` changed. That
  // meant the moment a "+ New user..." save mutated the user pool,
  // the row whose pick we just set (via onSaved) was reset to
  // `null` because the new User's (name, email) often differs
  // from the scanner's parsed (userName, userEmail). The user
  // would see their pick briefly appear then vanish, then the
  // import would either fail (no User) or pull the wrong user
  // (the scanner's leftover values). Now we preserve any row
  // that already has a pick — only rows that have not been
  // touched are auto-picked on first dialog open.
  useEffect(() => {
    if (!open) return;
    setPicks((prev) => {
      const next: Record<string, RowPick> = { ...prev };
      for (const c of candidates) {
        const k = keyFor(c);
        if (next[k] === undefined) {
          // Auto-pick a User whose (name, email) value-equals the
          // scanner's (userName, userEmail). When the scanner
          // returned null on either side, we leave the row
          // un-picked and the user must explicitly pick or
          // create a User.
          const matched = users.find(
            (u) =>
              u.name === (c.userName ?? '') &&
              u.email === (c.userEmail ?? ''),
          );
          next[k] = { userId: matched?.id ?? null };
        }
      }
      return next;
    });
    setCreatingFor(null);
  }, [open, candidates, users]);

  const toggle = (c: ScannedIdentity) => {
    const k = keyFor(c);
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(k)) next.delete(k);
      else next.add(k);
      return next;
    });
  };

  // The set of keys that are eligible to be selected (i.e. non-conflicting).
  const eligibleKeys = candidates
    .filter((c) => !c.conflictsWithExisting && !c.conflictsWithExistingKey)
    .map((c) => keyFor(c));

  const allSelected = eligibleKeys.length > 0 && eligibleKeys.every((k) => selected.has(k));
  const noneSelected = eligibleKeys.every((k) => !selected.has(k));

  const selectAll = () => setSelected(new Set(eligibleKeys));
  const deselectAll = () => setSelected(new Set());

  const setPick = (k: string, userId: string | null) => {
    setPicks((prev) => ({ ...prev, [k]: { userId } }));
  };

  // The User the row is currently bound to. Returns null when the
  // row hasn't been picked yet OR the picked id no longer resolves
  // (e.g. the user was deleted from the pool while this dialog was
  // open — unlikely but defensive).
  const effectiveUser = (c: ScannedIdentity): User | null => {
    const k = keyFor(c);
    const id = picks[k]?.userId;
    if (!id) return null;
    return users.find((u) => u.id === id) ?? null;
  };

  // Block import if any selected row is missing a User. The
  // backend's `create_identity` requires a non-empty email; since
  // every User has a non-empty email (validated server-side),
  // picking a User is sufficient to guarantee that.
  const missingUserLabels = useMemo(
    () => candidates
      .filter((c) => selected.has(keyFor(c)))
      .filter((c) => !effectiveUser(c))
      .map((c) => c.label),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [candidates, selected, picks, users],
  );
  const canImport = selected.size > 0 && missingUserLabels.length === 0 && !busy;

  const handleImport = async () => {
    if (!canImport) return;
    setBusy(true);
    try {
      // Build the merged candidate list: scanner output overlaid
      // with the picked User's (name, email). The dialog-level
      // guard above guarantees every selected row has a User, so
      // the non-null assertion is safe.
      const toImport = candidates
        .filter((c) => selected.has(keyFor(c)))
        .map((c) => {
          const u = effectiveUser(c);
          if (!u) {
            // Defensive: canImport is false in this case so this
            // branch is unreachable, but TS doesn't know that.
            throw new Error(`row ${c.label} has no bound user`);
          }
          return {
            ...c,
            userName: u.name,
            userEmail: u.email,
          };
        });
      await onImport(toImport);
      onOpenChange(false);
    } finally {
      setBusy(false);
    }
  };

  // Pre-fill values for the "+ New user..." inline create dialog:
  // when the user opens it from a row, seed with whichever (name,
  // email) we'd otherwise fall back to — the currently-picked
  // User, or the scanner's values. Avoids retyping when the user
  // is just confirming the scanner's detection as a new pool entry.
  const creatingForCandidate =
    creatingFor !== null
      ? candidates.find((c) => keyFor(c) === creatingFor) ?? null
      : null;
  const creatingForDefaultValues = (() => {
    if (!creatingForCandidate) return undefined;
    const u = effectiveUser(creatingForCandidate);
    if (u) return { name: u.name, email: u.email };
    return {
      name: creatingForCandidate.userName ?? '',
      email: creatingForCandidate.userEmail ?? '',
    };
  })();

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
          <>
            {/* Toolbar: select all / deselect all + count */}
            <div className="flex items-center justify-between gap-2 text-xs">
              <div className="text-text-1">
                {t('scanResults.selectedCount', { count: selected.size })}
              </div>
              {eligibleKeys.length > 0 && (
                allSelected ? (
                  <Button variant="ghost" size="sm" onClick={deselectAll} disabled={busy}>
                    {t('scanResults.deselectAll')}
                  </Button>
                ) : (
                  <Button variant="ghost" size="sm" onClick={selectAll} disabled={busy || noneSelected}>
                    {t('scanResults.selectAll')}
                  </Button>
                )
              )}
            </div>

            <div className="space-y-2 max-h-[55vh] overflow-y-auto">
              {candidates.map((c) => {
                const k = keyFor(c);
                const isSelected = selected.has(k);
                const conflict = c.conflictsWithExisting || c.conflictsWithExistingKey;
                const bound = effectiveUser(c);
                // Three display states:
                //   1. bound to a User → show "→ name <email>" in muted text
                //   2. not bound + scanner detected values → warn the user
                //      those values will be DROPPED unless they pick/create
                //   3. not bound + scanner had nothing → generic "pick or
                //      create a User to import"
                const scannerHasValues = !!(c.userName || c.userEmail);
                return (
                  <div
                    key={k}
                    role="group"
                    aria-label={c.label}
                    onClick={() => !conflict && toggle(c)}
                    className={`block p-3 rounded-md border transition-colors ${
                      conflict ? 'cursor-default' : 'cursor-pointer'
                    } ${
                      isSelected ? 'border-brand bg-brand-soft' : 'border-border hover:bg-bg-2'
                    }`}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <div className="flex items-center gap-2 min-w-0 flex-1">
                        <input
                          type="checkbox"
                          checked={isSelected}
                          disabled={conflict}
                          onChange={() => toggle(c)}
                          onClick={(e) => e.stopPropagation()}
                          className="shrink-0"
                        />
                        <span className="font-medium truncate">{c.label}</span>
                        {(c.provenance.sources ?? []).map((src) => (
                          <Badge key={src.kind} variant="outline">
                            {src.kind === 'gitconfig_include_if'
                              ? t('scanResults.fromGitconfig')
                              : t('scanResults.fromSsh')}
                          </Badge>
                        ))}
                        {conflict && <Badge variant="warning">{t('scanResults.conflict')}</Badge>}
                      </div>
                    </div>
                    <div className="text-text-1 text-xs mt-1 grid grid-cols-2 gap-x-3">
                      {c.keyPath && <div className="col-span-2">key: <span className="font-mono break-all">{c.keyPath}</span></div>}
                      {c.matchPath && <div className="col-span-2">match: <span className="font-mono">{c.matchPath}</span></div>}
                    </div>
                    {/* The dropdown is the SOLE binding mechanism
                        now. No inline name/email inputs — picking a
                        User is the only way to import this row. */}
                    <div className="mt-2">
                      <Label htmlFor={`${k}-user`} className="text-text-2 text-xs">
                        {t('scanResults.bindUser')}
                      </Label>
                      <select
                        id={`${k}-user`}
                        value={picks[k]?.userId ?? ''}
                        onChange={(e) => {
                          const v = e.target.value;
                          if (v === '__new__') {
                            setCreatingFor(k);
                          } else if (v) {
                            setPick(k, v);
                          } else {
                            setPick(k, null);
                          }
                        }}
                        onClick={(e) => e.stopPropagation()}
                        className="h-8 text-xs w-full rounded-md border border-border bg-bg-0 px-2"
                      >
                        <option value="">{t('scanResults.pickUserPlaceholder')}</option>
                        {users.map((u) => (
                          <option key={u.id} value={u.id}>
                            {`${u.name} <${u.email}>`}
                          </option>
                        ))}
                        <option value="__new__">{t('scanResults.createNewUser')}</option>
                      </select>
                    </div>
                    {bound ? (
                      <div className="text-text-2 text-xs mt-1">
                        {t('scanResults.boundAs', {
                          name: bound.name,
                          email: bound.email,
                        })}
                      </div>
                    ) : isSelected && !conflict ? (
                      <div className="text-warning text-xs mt-1">
                        {scannerHasValues
                          ? t('scanResults.scannerValuesDropped', {
                              name: c.userName ?? '',
                              email: c.userEmail ?? '',
                            })
                          : t('scanResults.userRequired')}
                      </div>
                    ) : null}
                    <div className="text-text-2 text-xs mt-1">{c.provenance.detail}</div>
                    {conflict && (
                      <div className="text-warning text-xs mt-1">
                        {c.conflictsWithExisting && t('scanResults.conflictLabel')}
                        {c.conflictsWithExisting && c.conflictsWithExistingKey && ' · '}
                        {c.conflictsWithExistingKey && t('scanResults.conflictKey')}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </>
        )}

        <DialogFooter className="gap-2">
          <Button variant="ghost" onClick={() => onOpenChange(false)}>{t('common.cancel')}</Button>
          <Button onClick={handleImport} disabled={!canImport}>
            {busy ? t('scanResults.importing') : t('scanResults.import', { count: selected.size })}
          </Button>
        </DialogFooter>
      </DialogContent>

      {/* Inline "+ new user..." flow: scoped to whichever row
          triggered it. On save, the new User's id flows back into
          that row's pick state and the dropdown reflects it. */}
      <UserFormDialog
        open={creatingFor !== null}
        onOpenChange={(v) => !v && setCreatingFor(null)}
        onSaved={(u) => {
          if (creatingFor) setPick(creatingFor, u.id);
          setCreatingFor(null);
        }}
        defaultValues={creatingForDefaultValues}
      />
    </Dialog>
  );
}

function keyFor(c: ScannedIdentity): string {
  // Combine label + key path to give each candidate a stable identity
  // (two candidates could share a label from different sources).
  return `${c.label}::${c.keyPath ?? ''}::${c.matchPath ?? ''}`;
}
