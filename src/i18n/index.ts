import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import LanguageDetector from 'i18next-browser-languagedetector';

import en from './locales/en.json';
import zhCN from './locales/zh-CN.json';

export const SUPPORTED_LOCALES = ['en', 'zh-CN'] as const;
export type Locale = (typeof SUPPORTED_LOCALES)[number];

export const LOCALE_LABELS: Record<Locale, string> = {
  en: 'English',
  'zh-CN': '简体中文',
};

const STORAGE_KEY = 'nicessh-locale';

void i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      'zh-CN': { translation: zhCN },
    },
    fallbackLng: 'en',
    // Treat 'zh-CN' as an opaque, atomic language code. Without these,
    // i18next splits it into ['zh-CN', 'zh', 'en'] and tries to load
    // 'zh' first — but our resources only define 'zh-CN', so it
    // silently falls back to 'en' and the user sees English forever.
    load: 'currentOnly',
    cleanCode: true,
    supportedLngs: SUPPORTED_LOCALES as unknown as string[],
    nonExplicitSupportedLngs: false,
    detection: {
      order: ['querystring', 'localStorage', 'navigator'],
      lookupLocalStorage: STORAGE_KEY,
      caches: ['localStorage'],
    },
    interpolation: { escapeValue: false },
    returnEmptyString: false,
  })
  .then(() => {
    // If the detector picked 'en' from localStorage (e.g. because the user
    // switched back), apply the value zustand is going to read.
    const persisted = localStorage.getItem(STORAGE_KEY);
    if (persisted === 'en' || persisted === 'zh-CN') {
      if (i18n.language !== persisted) {
        void i18n.changeLanguage(persisted);
      }
    }
  });

export default i18n;
