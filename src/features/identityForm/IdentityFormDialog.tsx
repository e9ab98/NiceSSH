import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Input, Label } from '../../components/ui/input';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../../components/ui/tooltip';
import { Button } from '../../components/ui/button';
import { useKeysStore, type SshKeyInfo } from '../../store/identities';
import { useUsersStore } from '../../store/users';
import type { Identity, SigningKeyKind } from '../../ipc/identities';
import type { User } from '../../ipc/users';
function parseSshPort(raw: string): number | null {
  // Empty / whitespace → "use default" (matches the `null` payload).
  const trimmed = raw.trim();
  if (trimmed === '') return null;
  // Reject anything that isn't a clean 1–65535 integer. We avoid
  // silently clamping (e.g. "0" → null, "70000" → null) because that
  // would mask user typos; bouncing the value back via HTML form
  // validation is the friendlier UX.
  if (!/^\d+$/.test(trimmed)) return null;
  const n = Number(trimmed);
  if (!Number.isInteger(n) || n < 1 || n > 65535) return null;
  return n;
}

function sanitizeLabel(value: string): string {
  return value.replace(/[\\/]+/g, '_');
}

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  initial?: Identity;
  defaultLabel?: string;
  onSubmit: (values: Omit<Identity, 'id'>) => Promise<void>;
}

export function IdentityFormDialog({ open, onOpenChange, initial, defaultLabel, onSubmit }: Props) {
  const { t } = useTranslation();
  const initialLabel = initial?.label ?? defaultLabel ?? '';
  const [label, setLabel] = useState(initialLabel);
  const [labelDirty, setLabelDirty] = useState(!!initial);
  const [userName, setUserName] = useState(initial?.userName ?? '');
  const [userEmail, setUserEmail] = useState(initial?.userEmail ?? '');
  const [hostAlias, setHostAlias] = useState(initial?.hostAlias ?? 'github.com');
  const [gitHost, setGitHost] = useState(initial?.gitHost ?? 'github.com');
  // Non-default SSH port (e.g. 2222 for self-hosted GitLab, 30003
  // for Synology Git Server). `null` → no `-p` arg → default port 22.
  // Stored as a string so the input can hold an empty field while
  // the user is typing; we coerce to number | null at submit time.
  const [sshPortInput, setSshPortInput] = useState<string>(
    initial?.sshPort != null ? String(initial.sshPort) : '',
  );
  // v3: commit signing fields. Defaults: signing off; if the
  // user enables it, default to reusing the identity's push
  // SSH key (signingKeyId = sshKeyId), since "same key for push
  // + sign" is what 95% of users want.
  const [requireSignedCommits, setRequireSignedCommits] = useState(
    initial?.requireSignedCommits ?? false,
  );
  const [signingKeyId, setSigningKeyId] = useState<string | null>(
    initial?.signingKeyId ?? initial?.sshKeyId ?? null,
  );
  const [signingKeyKind, setSigningKeyKind] = useState<SigningKeyKind>(
    initial?.signingKeyKind ?? 'ssh',
  );
  const [busy, setBusy] = useState(false);
  const labelBaseRef = useRef(initialLabel);
  const keys = useKeysStore((s) => s.items);
  // Make sure we have a fresh list for the signing-key dropdown.
  const keysRefresh = useKeysStore((s) => s.refresh);
  useEffect(() => {
    void keysRefresh();
  }, [keysRefresh]);
  // Pool of saved git users. Used by the "pick from user pool"
  // combobox above userName/userEmail so the user can reuse an
  // existing (name, email) pair instead of retyping it.
  const users = useUsersStore((s) => s.items);
  const usersRefresh = useUsersStore((s) => s.refresh);
  useEffect(() => {
    void usersRefresh();
  }, [usersRefresh]);
  const signingKeyOptions = useMemo(() => keys, [keys]);
  // (no defaultDropdownId needed — the signingKeyId state is
  //  initialized to initial?.signingKeyId ?? initial?.sshKeyId
  //  above, and the dropdown's selected value falls back to the
  //  first option when neither is set.)
  // Show "Advanced (GPG)" selector only when the user opts in via
  // an explicit toggle — keeps the form compact for the 95% case.
  const [showSigningKindAdvanced, setShowSigningKindAdvanced] = useState(
    initial?.signingKeyKind === 'gpg',
  );

  useEffect(() => {
    if (!open) return;
    setLabel(initial?.label ?? defaultLabel ?? '');
    setLabelDirty(!!initial);
    labelBaseRef.current = initial?.label ?? defaultLabel ?? '';
    setUserName(initial?.userName ?? '');
    setUserEmail(initial?.userEmail ?? '');
    setHostAlias(initial?.hostAlias ?? 'github.com');
    setGitHost(initial?.gitHost ?? 'github.com');
    setSshPortInput(initial?.sshPort != null ? String(initial.sshPort) : '');
    setRequireSignedCommits(initial?.requireSignedCommits ?? false);
    setSigningKeyId(initial?.signingKeyId ?? initial?.sshKeyId ?? null);
    setSigningKeyKind(initial?.signingKeyKind ?? 'ssh');
    setShowSigningKindAdvanced(initial?.signingKeyKind === 'gpg');
  }, [open, initial, defaultLabel]);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (busy) return;
    const cleanLabel = sanitizeLabel(label.trim());
    if (!cleanLabel) return;
    setBusy(true);
    try {
      // Consistent-state guard. Mirrors the Rust
      // `signing_config_is_inconsistent` check: turning the toggle
      // on without picking a key would fail backend validation.
      const finalSigningKeyId = requireSignedCommits
        ? (signingKeyId ?? initial?.sshKeyId ?? null)
        : null;
      await onSubmit({
        label: cleanLabel,
        userName,
        userEmail,
        sshKeyId: initial?.sshKeyId ?? null,
        matchPath: initial?.matchPath ?? null,
        hostAlias: hostAlias || null,
        gitHost: gitHost || null,
        sshPort: parseSshPort(sshPortInput),
        requireSignedCommits,
        signingKeyId: finalSigningKeyId,
        signingKeyKind,
      });
      onOpenChange(false);
    } finally {
      setBusy(false);
    }
  };

  const reset = () => {
    setLabel(defaultLabel ?? '');
    setLabelDirty(false);
    setUserName('');
    setUserEmail('');
    setHostAlias('github.com');
    setGitHost('github.com');
    setSshPortInput('');
    setRequireSignedCommits(false);
    setSigningKeyId(null);
    setSigningKeyKind('ssh');
    setShowSigningKindAdvanced(false);
    labelBaseRef.current = defaultLabel ?? '';
  };

  const handleOpenChange = (value: boolean) => {
    if (!value) reset();
    onOpenChange(value);
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent>
        <DialogHeader><DialogTitle>{initial ? t('identityForm.editTitle') : t('identityForm.newTitle')}</DialogTitle></DialogHeader>
        <form onSubmit={submit} className="space-y-3">
          <div>
            <Label htmlFor="label">{t('identityForm.label')}</Label>
            {initial ? (
              <TooltipProvider delayDuration={150}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span className="block"><Input id="label" value={label} required readOnly disabled /></span>
                  </TooltipTrigger>
                  <TooltipContent>{t('identityForm.labelImmutable')}</TooltipContent>
                </Tooltip>
              </TooltipProvider>
            ) : (
              <div className="flex gap-2 items-start">
                <Input id="label" value={label} onChange={(event) => { setLabel(event.target.value); setLabelDirty(true); }} required placeholder="work" className="flex-1" />
                {labelDirty && <Button type="button" variant="ghost" size="sm" onClick={() => { setLabel(labelBaseRef.current); setLabelDirty(false); }}>{t('identityForm.resetLabel')}</Button>}
              </div>
            )}
            <div className="text-text-2 text-xs mt-1">{t('identityForm.labelHint')}</div>
          </div>
          {/* Reuse an existing user from the pool. Picking one
              copies its (name, email) into the inputs below. The
              inputs themselves remain freely editable — editing
              after a pick "unlinks" the identity from the user
              (value-based linkage, no FK). */}
          <div>
            <Label htmlFor="user-pool">{t('identityForm.pickUser')}</Label>
            <select
              id="user-pool"
              value=""
              onChange={(e) => {
                const u: User | undefined = users.find((x) => x.id === e.target.value);
                if (u) {
                  setUserName(u.name);
                  setUserEmail(u.email);
                }
              }}
              className="h-9 w-full rounded-md border border-border bg-bg-0 px-3 text-sm"
            >
              <option value="">{t('identityForm.pickUserPlaceholder')}</option>
              {users.map((u) => (
                <option key={u.id} value={u.id}>
                  {`${u.name} <${u.email}>`}
                </option>
              ))}
            </select>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div><Label htmlFor="userName">{t('identityForm.userName')}</Label><Input id="userName" value={userName} onChange={(event) => setUserName(event.target.value)} required /></div>
            <div><Label htmlFor="userEmail">{t('identityForm.userEmail')}</Label><Input id="userEmail" type="email" value={userEmail} onChange={(event) => setUserEmail(event.target.value)} required /></div>
          </div>
          <div className="grid grid-cols-3 gap-3">
            <div><Label htmlFor="hostAlias">{t('identityForm.hostAlias')}</Label><Input id="hostAlias" value={hostAlias} onChange={(event) => setHostAlias(event.target.value)} placeholder="github.com" /></div>
            <div><Label htmlFor="gitHost">{t('identityForm.gitHost')}</Label><Input id="gitHost" value={gitHost} onChange={(event) => setGitHost(event.target.value)} placeholder="github.com" /></div>
            <div>
              <Label htmlFor="sshPort">{t('identityForm.sshPort')}</Label>
              <Input
                id="sshPort"
                type="number"
                inputMode="numeric"
                min={1}
                max={65535}
                step={1}
                value={sshPortInput}
                onChange={(event) => setSshPortInput(event.target.value)}
                placeholder={t('identityForm.sshPortPlaceholder')}
              />
            </div>
          </div>
          <p className="text-text-2 text-xs -mt-2">{t('identityForm.sshPortHint')}</p>

          {/* ── v3: commit signing ─────────────────────────────── */}
          <div className="rounded-md border border-border bg-bg-0 p-3 space-y-2">
            <label className="flex items-start gap-2 cursor-pointer select-none">
              <input
                type="checkbox"
                checked={requireSignedCommits}
                onChange={(e) => setRequireSignedCommits(e.target.checked)}
                className="h-4 w-4 mt-0.5 rounded border-border text-brand focus:ring-brand"
                aria-label={t('identityForm.signing.enableLabel')}
              />
              <span>
                <div className="text-sm font-medium text-text-0">
                  {t('identityForm.signing.enableLabel')}
                </div>
                <div className="text-xs text-text-2 mt-0.5">
                  {t('identityForm.signing.enableHint')}
                </div>
              </span>
            </label>
            {requireSignedCommits && (
              <div className="space-y-2 pl-6">
                <div>
                  <Label htmlFor="signingKeyId">{t('identityForm.signing.keyLabel')}</Label>
                  <select
                    id="signingKeyId"
                    value={signingKeyId ?? ''}
                    onChange={(e) => setSigningKeyId(e.target.value || null)}
                    className="h-9 w-full rounded-md border border-border bg-bg-0 px-3 text-sm text-text-0"
                  >
                    <option value="">{t('identityForm.signing.noKeyOption')}</option>
                    {signingKeyOptions.map((key: SshKeyInfo) => (
                      <option key={key.id} value={key.id}>
                        {key.name} ({key.privatePath})
                      </option>
                    ))}
                  </select>
                  <p className="text-xs text-text-2 mt-1">{t('identityForm.signing.keyHint')}</p>
                </div>
                <button
                  type="button"
                  onClick={() => setShowSigningKindAdvanced((s) => !s)}
                  className="text-xs text-text-1 hover:text-text-0 underline underline-offset-2"
                >
                  {showSigningKindAdvanced
                    ? t('identityForm.signing.hideAdvanced')
                    : t('identityForm.signing.showAdvanced')}
                </button>
                {showSigningKindAdvanced && (
                  <div>
                    <Label htmlFor="signingKeyKind">{t('identityForm.signing.kindLabel')}</Label>
                    <select
                      id="signingKeyKind"
                      value={signingKeyKind}
                      onChange={(e) => setSigningKeyKind(e.target.value as SigningKeyKind)}
                      className="h-9 w-full rounded-md border border-border bg-bg-0 px-3 text-sm text-text-0"
                    >
                      <option value="ssh">{t('identityForm.signing.kindSsh')}</option>
                      <option value="gpg">{t('identityForm.signing.kindGpg')}</option>
                    </select>
                    <p className="text-xs text-text-2 mt-1">{t('identityForm.signing.kindHint')}</p>
                  </div>
                )}
              </div>
            )}
          </div>

          <DialogFooter className="gap-2">
            <Button type="button" variant="ghost" onClick={() => handleOpenChange(false)}>{t('common.cancel')}</Button>
            <Button type="submit" disabled={busy}>{busy ? t('common.saving') : t('common.save')}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
