import { Badge } from './ui/badge';

export type RemoteProtocol = 'ssh' | 'https' | 'git' | 'unknown' | null;

/// Shared, localised protocol badge used in both the audit dialog
/// and the right-hand project detail panel. Kept tiny on purpose so
/// it has no opinions about *where* it lives on the page.
///
/// `t` is the i18next translator so callers don't need to thread a
/// hook down here — they capture `t` from their own
/// `useTranslation()` and pass it in.
export function ProtocolBadge({
  protocol,
  t,
  className,
}: {
  protocol: RemoteProtocol;
  t: (key: string) => string;
  className?: string;
}) {
  if (protocol == null) {
    return <Badge variant="default" className={className}>—</Badge>;
  }
  const label = t(`repoAudit.protocol.${protocol}`);
  const variant: 'default' | 'success' | 'warning' | 'danger' =
    protocol === 'ssh' ? 'success'
      : protocol === 'https' ? 'warning'
        : protocol === 'git' ? 'default'
          : 'danger';
  return <Badge variant={variant} className={className}>{label}</Badge>;
}
