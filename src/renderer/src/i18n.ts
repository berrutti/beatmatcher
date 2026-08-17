import { createI18n } from 'vue-i18n';
import { storageGet, STORAGE_KEYS } from './utils/storage';
import en from './locales/en.json';
import de from './locales/de.json';
import es from './locales/es.json';

export const SUPPORTED_LOCALES = ['en', 'de', 'es'] as const;

function savedLocale(): string {
  const stored = storageGet<string>(STORAGE_KEYS.locale, 'en');
  return SUPPORTED_LOCALES.some((locale) => locale === stored) ? stored : 'en';
}

export const i18n = createI18n({
  legacy: false,
  locale: savedLocale(),
  fallbackLocale: 'en',
  messages: { en, de, es }
});
