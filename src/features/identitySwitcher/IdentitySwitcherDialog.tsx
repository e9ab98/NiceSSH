import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Button } from '../../components/ui/button';
import { Badge } from '../../components/ui/badge';
import type { Identity } from '../../ipc/identities';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  identities: Identity[];
  currentId: string | null;
  onSelect: (id: string) => Promise<void> | void;
}

export function IdentitySwitcherDialog({ open, onOpenChange, identities, currentId, onSelect }: Props) {
  const { t } = useTranslation();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader><DialogTitle>{t('identitySwitcher.title')}</DialogTitle></DialogHeader>
        <div className="space-y-2 max-h-[60vh] overflow-y-auto">
          {identities.length === 0 && (
            <div className="text-text-1 text-sm py-4 text-center">{t('identitySwitcher.empty')}</div>
          )}
          {identities.map((id) => {
            const isCurrent = id.id === currentId;
            return (
              <button
                key={id.id}
                onClick={() => !isCurrent && onSelect(id.id)}
                disabled={isCurrent}
                className={`w-full text-left p-3 rounded-md border transition-colors ${
                  isCurrent
                    ? 'border-accent bg-bg-2 cursor-default'
                    : 'border-border hover:bg-bg-2 hover:border-border-strong'
                }`}
              >
                <div className="flex items-center justify-between">
                  <span className="font-medium">{id.label}</span>
                  {isCurrent && <Badge variant="outline">{t('common.current')}</Badge>}
                </div>
                <div className="text-text-1 text-xs mt-1">{id.userEmail}</div>
                <div className="text-text-2 text-xs mt-0.5 font-mono">{id.keyPath}</div>
                {id.matchPath && <div className="text-text-2 text-xs mt-0.5">{t('identities.match')}: {id.matchPath}</div>}
              </button>
            );
          })}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>{t('common.cancel')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
