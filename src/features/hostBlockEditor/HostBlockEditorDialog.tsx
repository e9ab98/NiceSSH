import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '../../components/ui/dialog';
import { Input, Label } from '../../components/ui/input';
import { Button } from '../../components/ui/button';
import { Switch } from '../../components/ui/switch';
import { addManagedHostBlock, updateManagedHostBlock, type HostBlock } from '../../ipc/sshConfig';
import { Trash2, Plus } from 'lucide-react';
import { toast } from 'sonner';

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  /** If set, the dialog operates on this existing managed block. */
  initial?: HostBlock;
  /** Optional callback after a successful save/delete so the parent can refresh. */
  onSaved?: () => void;
  /** Reserved for future use. */
  onDeleted?: () => void;
}

type Directive = [string, string];

function toDirectives(d: Array<readonly [string, string]>): Directive[] {
  return d.map(([k, v]) => [k, v] as Directive);
}

export function HostBlockEditorDialog({ open, onOpenChange, initial, onSaved, onDeleted: _onDeleted }: Props) {
  const { t } = useTranslation();
  const [label, setLabel] = useState('');
  const [isMatch, setIsMatch] = useState(false);
  const [directives, setDirectives] = useState<Directive[]>([]);
  const [busy, setBusy] = useState(false);

  // Reset form when the dialog opens or the initial block changes.
  useEffect(() => {
    if (!open) return; void initial;
    setLabel(initial?.label ?? '');
    setIsMatch(initial?.isMatch ?? false);
    setDirectives(initial ? toDirectives(initial.directives) : [['HostName', '']]);
  }, [open, initial]); // eslint-disable-line @typescript-eslint/no-unused-vars

  const isEdit = !!initial;

  const setDirective = (idx: number, key: string, value: string) => {
    setDirectives((cur) => cur.map((d, i) => (i === idx ? [key, value] : d)));
  };
  const addDirective = () => setDirectives((cur) => [...cur, ['', '']]);
  const removeDirective = (idx: number) =>
    setDirectives((cur) => cur.filter((_, i) => i !== idx));

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    if (!label.trim()) {
      toast.error(t('hostBlockEditor.labelRequired'));
      return;
    }
    const cleaned = directives.filter(([k]) => k.trim() !== '');
    setBusy(true);
    try {
      if (isEdit && initial) {
        await updateManagedHostBlock({
          currentLabel: initial.label,
          newLabel: label.trim(),
          isMatch,
          directives: cleaned,
        });
        toast.success(t('hostBlockEditor.updated'));
      } else {
        await addManagedHostBlock({
          label: label.trim(),
          isMatch,
          directives: cleaned,
        });
        toast.success(t('hostBlockEditor.added'));
      }
      onSaved?.();
      onOpenChange(false);
    } catch (err) {
      toast.error(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>
            {isEdit ? t('hostBlockEditor.editTitle') : t('hostBlockEditor.newTitle')}
          </DialogTitle>
        </DialogHeader>
        <form onSubmit={submit} className="space-y-3">
          <div className="grid grid-cols-3 gap-3">
            <div className="col-span-2">
              <Label htmlFor="hb-label">{t('hostBlockEditor.label')}</Label>
              <Input
                id="hb-label"
                value={label}
                onChange={(e) => setLabel(e.target.value)}
                required
                placeholder={isMatch ? 'host example.com' : 'github-work'}
                disabled={isEdit}
              />
            </div>
            <div className="flex items-end gap-2 pb-1">
              <Switch
                id="hb-ismatch"
                checked={isMatch}
                onCheckedChange={setIsMatch}
                disabled={isEdit}
              />
              <Label htmlFor="hb-ismatch" className="cursor-pointer">
                {t('hostBlockEditor.isMatch')}
              </Label>
            </div>
          </div>

          <div>
            <div className="flex items-center justify-between mb-1.5">
              <Label>{t('hostBlockEditor.directives')}</Label>
              <Button type="button" variant="outline" size="sm" onClick={addDirective}>
                <Plus className="h-3.5 w-3.5 mr-1" />
                {t('hostBlockEditor.addDirective')}
              </Button>
            </div>
            <div className="space-y-1.5 max-h-64 overflow-y-auto pr-1">
              {directives.map(([k, v], i) => (
                <div key={i} className="flex items-center gap-1.5">
                  <Input
                    value={k}
                    onChange={(e) => setDirective(i, e.target.value, v)}
                    placeholder="HostName"
                    className="flex-1 font-mono"
                  />
                  <Input
                    value={v}
                    onChange={(e) => setDirective(i, k, e.target.value)}
                    placeholder="github.com"
                    className="flex-[2] font-mono"
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => removeDirective(i)}
                    disabled={directives.length === 1}
                    aria-label={t('hostBlockEditor.removeDirective')}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </div>
              ))}
            </div>
          </div>

          <DialogFooter className="gap-2">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={busy}>
              {busy ? t('common.saving') : t('common.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
