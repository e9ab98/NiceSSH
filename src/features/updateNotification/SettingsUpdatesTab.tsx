import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Card } from '../../components/ui/card';
import { Button } from '../../components/ui/button';
import { marked } from 'marked';
import {
  checkForUpdate,
  downloadAndInstall,
  getCurrentVersion,
  isNewer,
  shouldNotify,
  LS_KEYS,
  type UpdateInfo,
} from '../../lib/update';

type Phase = 'idle' | 'downloading' | 'ready';

export function SettingsUpdatesTab() {
  const { t } = useTranslation();
  const [current, setCurrent] = useState<string>('');
  const [latest, setLatest] = useState<UpdateInfo | null>(null);
  const [hasChecked, setHasChecked] = useState(false);
  const [notify, setNotify] = useState<boolean>(shouldNotify());
  const [phase, setPhase] = useState<Phase>('idle');
  const [progress, setProgress] = useState(0);

  // Changelog states
  const [changelog, setChangelog] = useState<string>('');
  const [parsedHtml, setParsedHtml] = useState<string>('');
  const [loadingLog, setLoadingLog] = useState<boolean>(false);
  const [logError, setLogError] = useState<boolean>(false);
  const [showLatestLogs, setShowLatestLogs] = useState<boolean>(false);

  useEffect(() => {
    getCurrentVersion().then(setCurrent).catch(() => setCurrent('?'));
  }, []);

  const onCheck = async () => {
    // "Check now" bypasses the 24h cache: clear the timestamp, then call.
    localStorage.removeItem(LS_KEYS.checked);
    setHasChecked(true);
    try {
      const info = await checkForUpdate();
      setLatest(info);
    } catch {
      toast.error(t('settings.updates.checkFailed'));
    }
  };

  const onUpdate = async () => {
    setPhase('downloading');
    setProgress(0);
    try {
      await downloadAndInstall((pct) => setProgress(pct));
      setPhase('ready');
    } catch {
      toast.error(t('update.toast.failed'));
      setPhase('idle');
    }
  };

  // Tauri 2 relaunch() lives in @tauri-apps/plugin-process (not bundled
  // here); the user closes the app to apply the installed binary.
  const onRestart = () => {
    toast.success(t('settings.updates.restartHint'));
  };

  const onNotifyToggle = (next: boolean) => {
    setNotify(next);
    localStorage.setItem(LS_KEYS.notifyOnUpdate, next ? 'true' : 'false');
  };

  const updateAvailable = !!latest && !!current && isNewer(latest.version, current);

  const fetchChangelogData = async (version: string) => {
    if (!version) return;
    const cleanVersion = version.replace(/^v/, '');
    const tag = `v${cleanVersion}`;
    const cacheKey = `changelog-${tag}`;
    const cached = sessionStorage.getItem(cacheKey);

    if (cached) {
      setChangelog(cached);
      setLogError(false);
      return;
    }

    setLoadingLog(true);
    setLogError(false);
    try {
      const res = await fetch(`https://api.github.com/repos/e9ab98/NiceSSH/releases/tags/${tag}`);
      if (!res.ok) {
        throw new Error(`Failed to fetch release logs: ${res.status}`);
      }
      const data = await res.json();
      const body = data.body || '';
      sessionStorage.setItem(cacheKey, body);
      setChangelog(body);
    } catch (err) {
      console.error(err);
      setLogError(true);
      setChangelog('');
    } finally {
      setLoadingLog(false);
    }
  };

  // 1. When updates are found, default to showing the latest logs. Otherwise, current logs.
  useEffect(() => {
    if (updateAvailable && latest) {
      setShowLatestLogs(true);
    } else {
      setShowLatestLogs(false);
    }
  }, [updateAvailable, latest]);

  // 2. Fetch the changelog when target version changes
  useEffect(() => {
    if (showLatestLogs && latest) {
      fetchChangelogData(latest.version);
    } else if (current) {
      fetchChangelogData(current);
    }
  }, [showLatestLogs, latest, current]);

  // 3. Parse markdown using marked
  useEffect(() => {
    if (changelog) {
      try {
        const html = marked.parse(changelog, { async: false }) as string;
        setParsedHtml(html);
      } catch (err) {
        console.error('Failed to parse markdown', err);
        setParsedHtml('');
      }
    } else {
      setParsedHtml('');
    }
  }, [changelog]);

  return (
    <div className="space-y-3">
      <Card className="p-4 space-y-3">
        <div className="flex justify-between text-sm">
          <span className="text-text-1">{t('settings.updates.current')}</span>
          <span data-testid="current-version">
            {t('settings.updates.currentValue', { version: current || '…' })}
          </span>
        </div>

        <div className="flex justify-between text-sm">
          <span className="text-text-1">{t('settings.updates.latest')}</span>
          <span data-testid="latest-version">
            {!hasChecked
              ? t('settings.updates.notChecked')
              : latest
                ? t('settings.updates.latestValue', { version: latest.version })
                : t('settings.updates.upToDate')}
          </span>
        </div>

        <div className="flex gap-2 pt-2">
          <Button size="sm" variant="outline" onClick={onCheck}>
            {t('settings.updates.check')}
          </Button>
          {updateAvailable && phase === 'idle' && (
            <Button size="sm" onClick={onUpdate}>
              {t('settings.updates.update')}
            </Button>
          )}
          {phase === 'downloading' && (
            <Button size="sm" disabled>
              {t('update.toast.progress', { pct: progress })}
            </Button>
          )}
          {phase === 'ready' && (
            <Button size="sm" onClick={onRestart}>
              {t('update.toast.restart')}
            </Button>
          )}
        </div>
      </Card>

      {/* Changelog Card */}
      <Card className="p-4 flex flex-col gap-2">
        <div className="flex justify-between items-center border-b border-border pb-2 mb-1">
          <h2 className="text-sm font-semibold text-text-0">
            {t('settings.updates.changelog')} - {showLatestLogs && latest ? `v${latest.version}` : `v${current}`}
          </h2>
          {updateAvailable && latest && (
            <div className="flex gap-1.5 bg-bg-1 p-0.5 rounded border border-border">
              <button
                onClick={() => setShowLatestLogs(false)}
                className={`px-2 py-0.5 text-xs rounded transition-colors ${
                  !showLatestLogs
                    ? 'bg-bg-0 text-text-0 font-medium shadow-sm'
                    : 'text-text-2 hover:text-text-0'
                }`}
              >
                {t('settings.updates.viewCurrentChangelog')}
              </button>
              <button
                onClick={() => setShowLatestLogs(true)}
                className={`px-2 py-0.5 text-xs rounded transition-colors ${
                  showLatestLogs
                    ? 'bg-bg-0 text-text-0 font-medium shadow-sm'
                    : 'text-text-2 hover:text-text-0'
                }`}
              >
                {t('settings.updates.viewLatestChangelog')}
              </button>
            </div>
          )}
        </div>

        {loadingLog && (
          <div className="text-text-2 text-sm py-6 flex items-center justify-center gap-2">
            <span className="w-4 h-4 rounded-full border-2 border-text-2 border-t-transparent animate-spin" />
            <span>{t('settings.updates.loadingChangelog')}</span>
          </div>
        )}

        {logError && !loadingLog && (
          <div className="text-text-2 text-sm py-4 flex flex-col gap-3 items-center justify-center text-center">
            <span>{t('settings.updates.changelogLoadFailed')}</span>
            <div className="flex gap-2">
              <Button
                size="sm"
                variant="outline"
                onClick={() => fetchChangelogData(showLatestLogs && latest ? latest.version : current)}
              >
                {t('common.retry')}
              </Button>
              <a
                href="https://github.com/e9ab98/NiceSSH/releases"
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center justify-center rounded-md text-sm font-medium transition-colors border border-border h-8 px-3 text-text-1 hover:bg-bg-1 hover:text-text-0"
              >
                GitHub Releases
              </a>
            </div>
          </div>
        )}

        {!loadingLog && !logError && parsedHtml && (
          <div
            className="text-xs text-text-1 leading-relaxed max-h-72 overflow-y-auto pr-2
              [&_h2]:text-sm [&_h2]:font-semibold [&_h2]:mt-3 [&_h2]:mb-1.5 [&_h2]:border-b [&_h2]:border-border [&_h2]:pb-0.5 [&_h2]:text-text-0
              [&_h3]:text-xs [&_h3]:font-semibold [&_h3]:mt-2.5 [&_h3]:mb-1 [&_h3]:text-text-0
              [&_ul]:list-disc [&_ul]:pl-4 [&_ul]:space-y-1 [&_ul]:my-1.5
              [&_ol]:list-decimal [&_ol]:pl-4 [&_ol]:space-y-1 [&_ol]:my-1.5
              [&_li]:text-text-1
              [&_a]:text-blue-500 [&_a]:hover:underline [&_a]:cursor-pointer
              [&_code]:bg-bg-1 [&_code]:px-1 [&_code]:py-0.5 [&_code]:rounded [&_code]:text-xs [&_code]:font-mono [&_code]:mx-0.5 [&_code]:text-text-0
              [&_strong]:font-semibold [&_strong]:text-text-0"
            dangerouslySetInnerHTML={{ __html: parsedHtml }}
          />
        )}

        {!loadingLog && !logError && !parsedHtml && !changelog && (
          <div className="text-text-2 text-xs py-6 text-center">
            暂无日志内容
          </div>
        )}
      </Card>

      <Card className="p-4">
        <label className="flex items-center justify-between text-sm">
          <span>{t('settings.updates.notify')}</span>
          <input
            type="checkbox"
            role="checkbox"
            checked={notify}
            onChange={(e) => onNotifyToggle(e.target.checked)}
          />
        </label>
      </Card>
    </div>
  );
}
