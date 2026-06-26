import { create } from 'zustand';
import i18n from '../i18n';
import type { Locale } from '../i18n';

type Mode = 'light' | 'dark' | 'system';
export type KeyType = 'ed25519' | 'rsa';

interface SettingsState {
  theme: Mode;
  setTheme: (m: Mode) => void;
  locale: Locale;
  setLocale: (l: Locale) => void;
  recentlyUnlockedKeys: Record<string, true>;
  markKeyUnlocked: (keyPath: string) => void;
  clearUnlocked: () => void;
  defaultKeyType: KeyType;
  setDefaultKeyType: (t: KeyType) => void;
}

function readPersistedTheme(): Mode {
  const v = localStorage.getItem('nicessh-theme');
  if (v === 'light' || v === 'dark' || v === 'system') return v;
  return 'system';
}

function readPersistedLocale(): Locale {
  const v = localStorage.getItem('nicessh-locale');
  if (v === 'en' || v === 'zh-CN') return v;
  return 'en';
}

function readPersistedDefaultKeyType(): KeyType {
  const v = localStorage.getItem('nicessh-default-key-type');
  if (v === 'rsa') return 'rsa';
  return 'ed25519';
}

export const useSettingsStore = create<SettingsState>((set) => ({
  theme: readPersistedTheme(),
  setTheme: (m) => {
    localStorage.setItem('nicessh-theme', m);
    set({ theme: m });
  },
  locale: readPersistedLocale(),
  setLocale: (l) => {
    localStorage.setItem('nicessh-locale', l);
    // Sync i18next synchronously here so the language is updated *before*
    // we update the store. This way any component that subscribes to
    // store.locale already sees the new language by the time it re-renders.
    if (i18n.language !== l) {
      void i18n.changeLanguage(l);
    }
    set({ locale: l });
  },
  recentlyUnlockedKeys: {},
  markKeyUnlocked: (keyPath) =>
    set((s) => ({ recentlyUnlockedKeys: { ...s.recentlyUnlockedKeys, [keyPath]: true } })),
  clearUnlocked: () => set({ recentlyUnlockedKeys: {} }),
  defaultKeyType: readPersistedDefaultKeyType(),
  setDefaultKeyType: (t) => {
    localStorage.setItem('nicessh-default-key-type', t);
    set({ defaultKeyType: t });
  },
}));
