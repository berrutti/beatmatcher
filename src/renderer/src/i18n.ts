import { createI18n } from 'vue-i18n';
import { storageGet, STORAGE_KEYS } from './utils/storage';
import en from './locales/en.json';
import de from './locales/de.json';
import es from './locales/es.json';

export const SUPPORTED_LOCALES = ['en', 'de', 'es'] as const;
export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

function savedLocale(): string {
  const v = storageGet<string>(STORAGE_KEYS.locale, 'en');
  return SUPPORTED_LOCALES.some((locale) => locale === v) ? v : 'en';
}

export const i18n = createI18n({
  legacy: false,
  locale: savedLocale(),
  fallbackLocale: 'en',
  messages: { en, de, es }
});
