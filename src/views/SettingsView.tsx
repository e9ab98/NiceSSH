import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '../components/ui/tabs';
import { Card } from '../components/ui/card';
import { Button } from '../components/ui/button';
import { useSettingsStore, type KeyType } from '../store/settings';
import type { Locale } from '../i18n';
import { checkEnvironment, EnvCheck, clearHistory, readLogTail, clearLog, testSigningSetup, type SigningTestResult } from '../ipc/settings';
import { listKeys, type SshKeyInfo } from '../ipc/sshKeys';
import { useIdentitiesStore } from '../store/identities';
import { toast } from 'sonner';
import { SettingsUpdatesTab } from '../features/updateNotification/SettingsUpdatesTab';

const LOCALES: { value: Locale; labelKey: string }[] = [
  { value: 'en', labelKey: 'settings.language.english' },
  { value: 'zh-CN', labelKey: 'settings.language.chinese' },
];

function classifyKeyType(keyType: string | null | undefined): 'ed25519' | 'rsa' | 'other' {
  const kt = (keyType ?? '').toLowerCase();
  if (kt.includes('ed25519')) return 'ed25519';
  if (kt.includes('rsa')) return 'rsa';
  return 'other';
}

export function SettingsView() {
  const { t } = useTranslation();
  const { theme, setTheme, locale, setLocale, defaultKeyType, setDefaultKeyType } = useSettingsStore();
  const [env, setEnv] = useState<EnvCheck[]>([]);
  const [keys, setKeys] = useState<SshKeyInfo[]>([]);
  const [keysLoadError, setKeysLoadError] = useState(false);
  const [logText, setLogText] = useState<string>('');
  const [logError, setLogError] = useState<string | null>(null);
  const [logAutoRefresh, setLogAutoRefresh] = useState(false);

  // v3: signing diagnostic — list of identities that have
  // requireSignedCommits=true AND a signingKeyId. The Settings →
  // Signing tab lets the user pick one and run `test_signing_setup`
  // to verify the full pipeline actually works on their machine.
  const [signingTestIdentityId, setSigningTestIdentityId] = useState<string>('');
  const [signingTestBusy, setSigningTestBusy] = useState(false);
  const [signingTestResult, setSigningTestResult] = useState<SigningTestResult | null>(null);
  // Read the identities list from the store so this tab stays in
  // sync with edits in the Identities view.
  const identities = useIdentitiesStore((s) => s.items);
  const signingIdentities = useMemo(
    () => identities.filter((id) => id.requireSignedCommits && id.signingKeyId !== null),
    [identities],
  );
  // Default to the first eligible identity when the tab first
  // mounts or the current selection becomes invalid.
  useEffect(() => {
    if (!signingTestIdentityId && signingIdentities.length > 0) {
      setSigningTestIdentityId(signingIdentities[0].id);
    } else if (
      signingTestIdentityId &&
      !signingIdentities.some((id) => id.id === signingTestIdentityId)
    ) {
      setSigningTestIdentityId(signingIdentities[0]?.id ?? '');
    }
  }, [signingIdentities, signingTestIdentityId]);
  const onRunSigningTest = useCallback(async () => {
    const id = identities.find((x) => x.id === signingTestIdentityId);
    if (!id || !id.signingKeyId) return;
    setSigningTestBusy(true);
    setSigningTestResult(null);
    try {
      const r = await testSigningSetup(id.id, id.signingKeyId, id.signingKeyKind);
      setSigningTestResult(r);
      if (r.ok) toast.success(t('settings.signing.testPassed'));
      else toast.error(t('settings.signing.testFailed'));
    } catch (e) {
      setSigningTestResult({
        ok: false,
        status: 'E',
        signerEmail: null,
        signerKeyFpr: null,
        message: String(e),
      });
      toast.error(String(e));
    } finally {
      setSigningTestBusy(false);
    }
  }, [identities, signingTestIdentityId, t]);

  useEffect(() => {
    checkEnvironment().then(setEnv).catch(() => setEnv([]));
  }, []);

  useEffect(() => {
    listKeys()
      .then((ks) => { setKeys(ks); setKeysLoadError(false); })
      .catch(() => { setKeys([]); setKeysLoadError(true); });
  }, []);

  const refreshLog = useCallback(() => {
    readLogTail(500)
      .then((text) => { setLogText(text); setLogError(null); })
      .catch((e) => { setLogText(''); setLogError(String(e)); });
  }, []);

  useEffect(() => {
    refreshLog();
  }, [refreshLog]);

  useEffect(() => {
    if (!logAutoRefresh) return;
    const id = setInterval(refreshLog, 3000);
    return () => clearInterval(id);
  }, [logAutoRefresh, refreshLog]);

  const modKey = navigator.platform.toLowerCase().includes('mac') ? '⌘' : 'Ctrl';

  const stats = useMemo(() => {
    const result = { ed25519: 0, rsa: 0, other: 0, total: 0 };
    for (const k of keys) {
      const bucket = classifyKeyType(k.keyType);
      result[bucket]++;
      result.total++;
    }
    return result;
  }, [keys]);

  return (
    <div className="p-6 max-w-3xl">
      <h1 className="text-2xl font-semibold mb-4">{t('settings.title')}</h1>
      <Tabs defaultValue="theme">
        <TabsList>
          <TabsTrigger value="theme">{t('settings.tabs.theme')}</TabsTrigger>
          <TabsTrigger value="language">{t('settings.tabs.language')}</TabsTrigger>
          <TabsTrigger value="keys">{t('settings.tabs.keys')}</TabsTrigger>
          <TabsTrigger value="signing">{t('settings.tabs.signing')}</TabsTrigger>
          <TabsTrigger value="logs">{t('settings.tabs.logs')}</TabsTrigger>
          <TabsTrigger value="env">{t('settings.tabs.env')}</TabsTrigger>
          <TabsTrigger value="updates">{t('settings.tabs.updates')}</TabsTrigger>
        </TabsList>

        <TabsContent value="theme" className="space-y-3">
          <Card className="p-4 space-y-3">
            <label className="flex items-center justify-between">
              <span>{t('settings.theme.mode')}</span>
              <select
                value={theme}
                onChange={(e) => setTheme(e.target.value as 'light' | 'dark' | 'system')}
                className="h-9 rounded-md border border-border bg-bg-0 px-3 text-sm text-text-0"
              >
                <option value="system">{t('settings.theme.modeSystem')}</option>
                <option value="light">{t('settings.theme.modeLight')}</option>
                <option value="dark">{t('settings.theme.modeDark')}</option>
              </select>
            </label>
          </Card>
        </TabsContent>

        <TabsContent value="language" className="space-y-3">
          <Card className="p-4 space-y-3">
            <label className="flex items-center justify-between">
              <span>{t('settings.language.label')}</span>
              <select
                value={locale}
                onChange={(e) => setLocale(e.target.value as Locale)}
                className="h-9 rounded-md border border-border bg-bg-0 px-3 text-sm text-text-0"
              >
                {LOCALES.map((l) => (
                  <option key={l.value} value={l.value}>{t(l.labelKey)}</option>
                ))}
              </select>
            </label>
          </Card>
        </TabsContent>

        <TabsContent value="keys" className="space-y-3">
          <Card className="p-4 space-y-3">
            <label className="flex items-center justify-between">
              <span>{t('settings.keys.defaultType')}</span>
              <select
                value={defaultKeyType}
                onChange={(e) => setDefaultKeyType(e.target.value as KeyType)}
                className="h-9 rounded-md border border-border bg-bg-0 px-3 text-sm text-text-0"
              >
                <option value="ed25519">Ed25519</option>
                <option value="rsa">RSA</option>
              </select>
            </label>
            <p className="text-text-2 text-xs">{t('settings.keys.defaultTypeHint')}</p>
          </Card>

          <Card className="p-4 space-y-3">
            <div className="font-medium">{t('settings.keys.statsTitle')}</div>
            {keysLoadError ? (
              <div className="text-text-2 text-sm">{t('settings.keys.statsLoadError')}</div>
            ) : stats.total === 0 ? (
              <div className="text-text-2 text-sm">{t('settings.keys.statsEmpty')}</div>
            ) : (
              <div className="grid grid-cols-2 gap-2 text-sm">
                <div className="flex items-center justify-between rounded-md border border-border bg-bg-0 px-3 py-2">
                  <span className="text-text-1">{t('settings.keys.statsEd25519')}</span>
                  <span className="font-mono font-semibold">{stats.ed25519}</span>
                </div>
                <div className="flex items-center justify-between rounded-md border border-border bg-bg-0 px-3 py-2">
                  <span className="text-text-1">{t('settings.keys.statsRsa')}</span>
                  <span className="font-mono font-semibold">{stats.rsa}</span>
                </div>
                <div className="flex items-center justify-between rounded-md border border-border bg-bg-0 px-3 py-2">
                  <span className="text-text-1">{t('settings.keys.statsOther')}</span>
                  <span className="font-mono font-semibold">{stats.other}</span>
                </div>
                <div className="flex items-center justify-between rounded-md border border-border bg-bg-0 px-3 py-2">
                  <span className="text-text-1">{t('settings.keys.statsTotal')}</span>
                  <span className="font-mono font-semibold">{stats.total}</span>
                </div>
              </div>
            )}
          </Card>
        </TabsContent>

        <TabsContent value="signing" className="space-y-3">
          <Card className="p-4 space-y-3">
            <div>
              <div className="font-medium">{t('settings.signing.testTitle')}</div>
              <p className="text-text-2 text-xs mt-1">{t('settings.signing.testHint')}</p>
            </div>
            <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
              <select
                value={signingTestIdentityId}
                onChange={(e) => setSigningTestIdentityId(e.target.value)}
                className="h-9 rounded-md border border-border bg-bg-0 px-3 text-sm text-text-0 min-w-[14rem]"
                disabled={signingIdentities.length === 0 || signingTestBusy}
                aria-label={t('settings.signing.testIdentityLabel')}
              >
                {signingIdentities.length === 0 ? (
                  <option value="">{t('settings.signing.noEligibleIdentity')}</option>
                ) : (
                  signingIdentities.map((id) => (
                    <option key={id.id} value={id.id}>
                      {id.label} ({id.userEmail})
                    </option>
                  ))
                )}
              </select>
              <Button
                variant="outline"
                size="sm"
                disabled={signingIdentities.length === 0 || signingTestBusy || !signingTestIdentityId}
                onClick={onRunSigningTest}
              >
                {signingTestBusy ? t('settings.signing.testRunning') : t('settings.signing.testButton')}
              </Button>
            </div>
            {signingTestResult && (
              <div
                className={
                  'rounded-md border p-3 text-sm ' +
                  (signingTestResult.ok
                    ? 'border-success/40 bg-success/10 text-text-0'
                    : 'border-danger/40 bg-danger/10 text-text-0')
                }
                data-testid="signing-test-result"
              >
                    <div className="font-medium">
                      {signingTestResult.ok
                        ? t('settings.signing.testPassed')
                        : t('settings.signing.testFailed')}
                    </div>
                    <div className="text-xs text-text-1 mt-1">
                      {signingTestResult.message}
                    </div>
                    {signingTestResult.signerEmail && (
                      <div className="text-xs text-text-2 mt-1">
                        {t('settings.signing.signedBy', { email: signingTestResult.signerEmail })}
                      </div>
                    )}
                    {signingTestResult.signerKeyFpr && (
                      <div className="text-xs text-text-2 mt-1 font-mono break-all">
                        {t('settings.signing.signerFpr', { fpr: signingTestResult.signerKeyFpr })}
                      </div>
                    )}
                  </div>
            )}
          </Card>
        </TabsContent>

        <TabsContent value="logs" className="space-y-3">
          <Card className="p-4 space-y-3">
            <div className="flex items-center justify-between gap-2">
              <span className="font-medium">{t('settings.logs.viewerTitle')}</span>
              <div className="flex items-center gap-2">
                <label className="flex items-center gap-1 text-xs text-text-1">
                  <input
                    type="checkbox"
                    checked={logAutoRefresh}
                    onChange={(e) => setLogAutoRefresh(e.target.checked)}
                  />
                  {t('settings.logs.autoRefresh')}
                </label>
                <Button variant="outline" size="sm" onClick={refreshLog}>
                  {t('settings.logs.refresh')}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={async () => {
                    if (confirm(t('settings.logs.clearLogConfirm'))) {
                      try {
                        await clearLog();
                        await refreshLog();
                        toast.success(t('settings.logs.logCleared'));
                      } catch (e) {
                        toast.error(String(e));
                      }
                    }
                  }}
                >
                  {t('settings.logs.clearLogButton')}
                </Button>
              </div>
            </div>
            {logError ? (
              <div className="text-danger text-sm">{logError}</div>
            ) : (
              <pre className="text-xs bg-bg-0 p-3 rounded-md border border-border overflow-x-auto whitespace-pre-wrap break-all max-h-96 overflow-y-auto font-mono">
                {logText || t('settings.logs.empty')}
              </pre>
            )}
          </Card>
        </TabsContent>

        <TabsContent value="env" className="space-y-2">
          <Card className="p-4 space-y-2">
            <div className="font-medium">{t('settings.shortcuts.title')}</div>
            <div className="text-text-1 text-sm space-y-1">
              <div><kbd className="font-mono text-xs px-1.5 py-0.5 rounded border border-border bg-bg-0">{modKey}</kbd> + 1 — {t('settings.shortcuts.projects')}</div>
              <div><kbd className="font-mono text-xs px-1.5 py-0.5 rounded border border-border bg-bg-0">{modKey}</kbd> + 2 — {t('settings.shortcuts.identities')}</div>
              <div><kbd className="font-mono text-xs px-1.5 py-0.5 rounded border border-border bg-bg-0">{modKey}</kbd> + 3 — {t('settings.shortcuts.sshConfig')}</div>
              <div><kbd className="font-mono text-xs px-1.5 py-0.5 rounded border border-border bg-bg-0">{modKey}</kbd> + 4 — {t('settings.shortcuts.history')}</div>
              <div><kbd className="font-mono text-xs px-1.5 py-0.5 rounded border border-border bg-bg-0">{modKey}</kbd> + , — {t('settings.shortcuts.settings')}</div>
            </div>
          </Card>
          <Card className="p-4">
            <div className="flex items-center justify-between">
              <span>{t('history.clearTitle')}</span>
              <Button
                variant="danger"
                onClick={async () => {
                  if (confirm(t('history.clearConfirm'))) {
                    await clearHistory();
                    toast.success(t('history.cleared'));
                  }
                }}
              >
                {t('history.clearButton')}
              </Button>
            </div>
          </Card>
          {env.map((e) => (
            <Card key={e.tool} className="p-3 flex items-center justify-between">
              <div>
                <div className="font-mono text-sm">{e.tool}</div>
                <div className="text-text-2 text-xs mt-0.5">{e.detail}</div>
              </div>
              <span
                className={
                  e.status === 'ok'
                    ? 'text-success text-sm'
                    : e.status === 'warning'
                      ? 'text-warning text-sm'
                      : 'text-danger text-sm'
                }
              >
                {e.status === 'ok' ? '✓' : e.status === 'warning' ? '⚠' : '✗'}
              </span>
            </Card>
          ))}
          {env.length === 0 && <div className="text-text-1 text-center py-8">{t('settings.env.loading')}</div>}
        </TabsContent>

        <TabsContent value="updates" className="space-y-3">
          <SettingsUpdatesTab />
        </TabsContent>
      </Tabs>
    </div>
  );
}
