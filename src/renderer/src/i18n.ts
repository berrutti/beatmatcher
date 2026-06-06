import { createI18n } from 'vue-i18n';
import { storageGet, STORAGE_KEYS } from './utils/storage';
import en from './locales/en.json';
import de from './locales/de.json';
import es from './locales/es.json';

const SUPPORTED = ['en', 'de', 'es'];

function savedLocale(): string {
  const v = storageGet<string>(STORAGE_KEYS.locale, 'en');
  return SUPPORTED.includes(v) ? v : 'en';
}

export const i18n = createI18n({
  legacy: false,
  locale: savedLocale(),
  fallbackLocale: 'en',
  messages: { en, de, es }
});
