/**
 * Sterngate Client-side Internationalization (i18n) Engine
 * Handles dynamic multilingual translations across en, de, sv
 */
class I18nManager {
  constructor() {
    this.currentLang = this.detectLanguage();
    this.translations = {};
    this.loaded = false;
  }

  detectLanguage() {
    const saved = localStorage.getItem('sterngate_lang');
    if (saved && ['en', 'de', 'sv'].includes(saved)) {
      return saved;
    }
    const nav = (navigator.language || navigator.userLanguage || 'en').toLowerCase();
    if (nav.startsWith('de')) return 'de';
    if (nav.startsWith('sv')) return 'sv';
    return 'en';
  }

  async init() {
    await this.loadLanguage(this.currentLang);
    // Also preload English as fallback if current is different
    if (this.currentLang !== 'en') {
      this.loadLanguage('en').catch(() => {});
    }
    this.applyTranslations();
    const select = document.getElementById('lang-select');
    if (select) {
      select.value = this.currentLang;
    }
    document.documentElement.lang = this.currentLang;
    this.loaded = true;
  }

  async loadLanguage(lang) {
    if (this.translations[lang]) return this.translations[lang];
    try {
      const res = await fetch(`/static/locales/${lang}.json`);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      this.translations[lang] = data;
      return data;
    } catch (e) {
      console.warn(`Could not load locale ${lang}:`, e);
      return null;
    }
  }

  async setLanguage(lang) {
    if (!['en', 'de', 'sv'].includes(lang)) lang = 'en';
    await this.loadLanguage(lang);
    this.currentLang = lang;
    localStorage.setItem('sterngate_lang', lang);
    document.documentElement.lang = lang;
    const select = document.getElementById('lang-select');
    if (select) select.value = lang;
    this.applyTranslations();
    window.dispatchEvent(new CustomEvent('languageChanged', { detail: { lang } }));
  }

  t(key, fallback = '') {
    const resolve = (obj, path) => {
      if (!obj) return null;
      return path.split('.').reduce((acc, part) => (acc && acc[part] !== undefined) ? acc[part] : null, obj);
    };

    let val = resolve(this.translations[this.currentLang], key);
    if (val === null && this.currentLang !== 'en') {
      val = resolve(this.translations['en'], key);
    }
    return val !== null ? val : (fallback || key);
  }

  applyTranslations() {
    // Translate textContent
    document.querySelectorAll('[data-i18n]').forEach(el => {
      const key = el.getAttribute('data-i18n');
      el.textContent = this.t(key, el.textContent);
    });

    // Translate input placeholders
    document.querySelectorAll('[data-i18n-placeholder]').forEach(el => {
      const key = el.getAttribute('data-i18n-placeholder');
      el.placeholder = this.t(key, el.placeholder);
    });

    // Translate title attributes
    document.querySelectorAll('[data-i18n-title]').forEach(el => {
      const key = el.getAttribute('data-i18n-title');
      el.title = this.t(key, el.title);
    });
  }
}

const i18n = new I18nManager();
document.addEventListener('DOMContentLoaded', () => {
  i18n.init();
});
