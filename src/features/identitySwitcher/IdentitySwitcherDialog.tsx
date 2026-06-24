import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { Badge } from '../../components/ui/badge';
import { Tabs, TabsList, TabsTrigger } from '../../components/ui/tabs';
import { setGlobalGitConfig, getGlobalGitConfig } from '../../ipc/git';
import { toast } from 'sonner';
import { cn } from '../../lib/utils';
import type { Identity } from '../../ipc/identities';

type Scope = 'project' | 'global';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  identities: Identity[];
  currentId: string | null;
  onSelect: (id: string) => Promise<void> | void;
}

export function IdentitySwitcherDialog({ open, onOpenChange, identities, currentId, onSelect }: Props) {
  const { t } = useTranslation();
  const [scope, setScope] = useState<Scope>('project');
  const [busy, setBusy] = useState(false);
  const [globalIdentityId, setGlobalIdentityId] = useState<string | null>(null);

  // When the dialog opens, fetch the current global identity so we can
  // highlight it in the list under "Global" scope.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    (async () => {
      try {
        const cfg = await getGlobalGitConfig();
        if (cancelled || !cfg.sshKeyPath) return;
        // Match global identity by key path (heuristic — we don't have a direct id)
        const match = identities.find((i) => i.keyPath === cfg.sshKeyPath);
        if (match) setGlobalIdentityId(match.id);
      } catch {
        // ignore — best-effort
      }
    })();
    return () => { cancelled = true; };
  }, [open, identities]);

  const handleSelect = async (id: string) => {
    if (busy) return;
    setBusy(true);
    try {
      if (scope === 'project') {
        await onSelect(id);
        onOpenChange(false);
      } else {
        const result = await setGlobalGitConfig(id);
        const target = identities.find((i) => i.id === id);
        toast.success(
          t('identitySwitcher.globalApplied', {
            label: target?.label ?? '',
            email: result.userEmail,
          })
        );
        setGlobalIdentityId(id);
        onOpenChange(false);
      }
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader><DialogTitle>{t('identitySwitcher.title')}</DialogTitle></DialogHeader>

        <Tabs value={scope} onValueChange={(v) => setScope(v as Scope)}>
          <TabsList className="w-full grid grid-cols-2">
            <TabsTrigger value="project">{t('identitySwitcher.scope.project')}</TabsTrigger>
            <TabsTrigger value="global">{t('identitySwitcher.scope.global')}</TabsTrigger>
          </TabsList>
        </Tabs>

        <p className="text-xs text-text-1 -mt-1">
          {scope === 'project'
            ? t('identitySwitcher.scopeHint.project')
            : t('identitySwitcher.scopeHint.global')}
        </p>

        <div className="space-y-2 max-h-[50vh] overflow-y-auto">
          {identities.length === 0 && (
            <div className="text-text-1 text-sm py-4 text-center">{t('identitySwitcher.empty')}</div>
          )}
          {identities.map((id) => {
            const isCurrent = scope === 'project' ? id.id === currentId : id.id === globalIdentityId;
            return (
              <button
                key={id.id}
                onClick={() => !isCurrent && handleSelect(id.id)}
                disabled={isCurrent || busy}
                className={cn(
                  'w-full text-left p-3 rounded-md border transition-colors',
                  isCurrent
                    ? 'border-brand bg-brand-soft cursor-default'
                    : 'border-border hover:bg-bg-2 hover:border-border-strong'
                )}
              >
                <div className="flex items-center justify-between">
                  <span className="font-semibold">{id.label}</span>
                  {isCurrent && <Badge variant="outline">{t('common.current')}</Badge>}
                </div>
                <div className="text-text-1 text-xs mt-1">{id.userEmail}</div>
                <div className="text-text-2 text-xs mt-0.5 font-mono">{id.keyPath}</div>
              </button>
            );
          })}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={busy}>
            {t('common.cancel')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
