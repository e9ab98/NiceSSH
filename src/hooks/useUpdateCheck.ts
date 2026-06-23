import { useEffect } from 'react';
import { createElement } from 'react';
import { toast } from 'sonner';
import { checkForUpdate, wasDismissed, shouldNotify, LS_KEYS } from '../lib/update';
import { UpdateToast } from '../features/updateNotification/UpdateToast';

const TOAST_ID = 'nicessh-update';

export function useUpdateCheck(): void {
  useEffect(() => {
    if (!shouldNotify()) return;

    const last = localStorage.getItem(LS_KEYS.checked);
    if (last && Date.now() - Date.parse(last) < 24 * 60 * 60 * 1000) return;

    let cancelled = false;

    // Defer the check by 5 seconds so the app startup/first-paint is completely unaffected.
    // Also set a 5-second network timeout so the check fails fast if the network hangs.
    const timer = setTimeout(() => {
      checkForUpdate({ timeout: 5000 }).then((info) => {
        if (cancelled) return;
        if (!info) return;
        if (wasDismissed(info.version)) return;
        // Use createElement so esbuild's TSX transform doesn't choke on a
        // JSX literal as a function argument.
        toast(
          createElement(UpdateToast, { version: info.version, notes: info.notes }),
          { id: TOAST_ID, duration: Infinity }
        );
      });
    }, 5000);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, []);
}
