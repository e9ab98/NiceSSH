import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '../components/ui/tabs';
import { Card } from '../components/ui/card';
import { Button } from '../components/ui/button';
import { useSettingsStore } from '../store/settings';
import type { Locale } from '../i18n';
import { checkEnvironment, EnvCheck, clearHistory } from '../ipc/settings';
import { toast } from 'sonner';
import { SettingsUpdatesTab } from '../features/updateNotification/SettingsUpdatesTab';

const LOCALES: { value: Locale; labelKey: string }[] = [
  { value: 'en', labelKey: 'settings.language.english' },
  { value: 'zh-CN', labelKey: 'settings.language.chinese' },
];

export function SettingsView() {
  const { t } = useTranslation();
  const { theme, setTheme, locale, setLocale } = useSettingsStore();
  const [env, setEnv] = useState<EnvCheck[]>([]);

  useEffect(() => {
    checkEnvironment().then(setEnv).catch(() => setEnv([]));
  }, []);

  return (
    <div className="p-6 max-w-3xl">
      <h1 className="text-2xl font-semibold mb-4">{t('settings.title')}</h1>
      <Tabs defaultValue="theme">
        <TabsList>
          <TabsTrigger value="theme">{t('settings.tabs.theme')}</TabsTrigger>
          <TabsTrigger value="language">{t('settings.tabs.language')}</TabsTrigger>
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

        <TabsContent value="logs" className="space-y-3">
          <Card className="p-4">
            <div className="flex items-center justify-between">
              <span>{t('settings.logs.clearTitle')}</span>
              <Button
                variant="danger"
                onClick={async () => {
                  if (confirm(t('settings.logs.clearConfirm'))) {
                    await clearHistory();
                    toast.success(t('settings.logs.cleared'));
                  }
                }}
              >
                {t('settings.logs.clearButton')}
              </Button>
            </div>
          </Card>
        </TabsContent>

        <TabsContent value="env" className="space-y-2">
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
