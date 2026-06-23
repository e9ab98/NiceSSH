import { useEffect } from 'react';
import { useSettingsStore } from '../store/settings';

type Mode = 'light' | 'dark' | 'system';

function applyTheme(mode: Mode) {
  const sys = window.matchMedia('(prefers-color-scheme: dark)').matches;
  const effective = mode === 'system' ? (sys ? 'dark' : 'light') : mode;
  document.documentElement.dataset.theme = effective;
}

export function useTheme() {
  const mode = useSettingsStore((s) => s.theme);
  useEffect(() => {
    applyTheme(mode);
    if (mode === 'system') {
      const mq = window.matchMedia('(prefers-color-scheme: dark)');
      const handler = () => applyTheme('system');
      mq.addEventListener('change', handler);
      return () => mq.removeEventListener('change', handler);
    }
  }, [mode]);
  return { mode, setMode: (m: Mode) => useSettingsStore.getState().setTheme(m) };
}
