/**
 * AnkiTov i18n Module — Frontend Internationalization
 *
 * Lightweight, zero-dependency i18n for the Management Console.
 * Supports RTL languages (Arabic, Hebrew) out of the box.
 *
 * ## Quick Start
 *
 * ```html
 * <script src="/dashboard/i18n.js"></script>
 * <script>
 *   (async () => {
 *     await I18n.init();
 *     console.log(I18n.t('app.title')); // → "AnkiTov Management Console"
 *   })();
 * </script>
 * ```
 *
 * ## How it works
 *
 * 1. On init(), loads the current locale JSON from `locales/{lang}.json`
 * 2. Sets `dir` and `lang` attributes on `<html>`
 * 3. All translatable elements use `data-i18n` or `data-i18n-placeholder`
 * 4. `I18n.t(key, params)` resolves messages with `${param}` interpolation
 * 5. Language preference persists in `localStorage`
 * 6. On locale switch, re-renders all `data-i18n` elements
 *
 * ## Usage in HTML
 *
 * ```html
 * <span data-i18n="sidebar.dashboard">Dashboard</span>
 * <input data-i18n-placeholder="deck.uploadPrompt" placeholder="Upload...">
 * <button data-i18n="btn.generate">Generate</button>
 * ```
 *
 * The value between the tags is treated as a fallback — it's displayed
 * before i18n loads and if the translation key is missing.
 *
 * @module I18n
 */
const I18n = (() => {
  'use strict';

  // ── Private state ──────────────────────────────────────────────
  let _currentLocale = 'en-US';
  let _messages = {};
  let _isRTL = false;
  let _initialized = false;
  const _listeners = [];

  // ── Constants ──────────────────────────────────────────────────
  const STORAGE_KEY = 'ankitov-locale';
  const SUPPORTED_LOCALES = [
    { code: 'en-US', nativeName: 'English (US)', englishName: 'English (US)' },
    { code: 'ar',    nativeName: 'العربية',       englishName: 'Arabic'           },
    { code: 'he',    nativeName: 'עברית',         englishName: 'Hebrew'           },
    { code: 'fr',    nativeName: 'Français',      englishName: 'French'           },
    { code: 'es',    nativeName: 'Español',        englishName: 'Spanish'          },
    { code: 'de',    nativeName: 'Deutsch',       englishName: 'German'           },
    { code: 'it',    nativeName: 'Italiano',      englishName: 'Italian'          },
    { code: 'ru',    nativeName: 'Русский',       englishName: 'Russian'          },
    { code: 'zh',    nativeName: '简体中文',       englishName: 'Chinese'          },
    { code: 'ko',    nativeName: '한국어',         englishName: 'Korean'           },
    { code: 'ja',    nativeName: '日本語',         englishName: 'Japanese'         },
    { code: 'pt',    nativeName: 'Português (Brasil)', englishName: 'Portuguese (Brazil)' },
    { code: 'hi',    nativeName: 'हिन्दी',            englishName: 'Hindi'                },
  ];

  // ── Public API ─────────────────────────────────────────────────

  /**
   * Initialize i18n: detect language, load messages, update DOM.
   * Call once at startup. Safe to call multiple times (no-op after first).
   *
   * @param {string} [localePath="/dashboard/locales"] — path to locale JSON files
   * @returns {Promise<void>}
   */
  async function init(localePath = '/dashboard/locales') {
    if (_initialized) return;

    // 1. Detect preferred locale
    _currentLocale = _detectLocale();

    // 2. Load messages
    try {
      const resp = await fetch(`${localePath}/${_currentLocale}.json`);
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      const data = await resp.json();
      _messages = data;
      _isRTL = (data._meta && data._meta.direction === 'rtl');
    } catch (err) {
      console.warn(`[i18n] Could not load locale "${_currentLocale}", falling back to en-US`, err);
      _currentLocale = 'en-US';
      _isRTL = false;
      try {
        const fallback = await fetch(`${localePath}/en-US.json`);
        _messages = await fallback.json();
      } catch (e2) {
        console.error('[i18n] Fatal: cannot load fallback locale', e2);
        _messages = {};
      }
    }

    // 3. Apply to DOM
    _applyDirection();
    _refreshDOM();
    _initialized = true;

    // 4. Notify listeners
    _notifyListeners();
  }

  /**
   * Translate a key, optionally interpolating `${param}` placeholders.
   *
   * @param {string} key — dot-separated message ID (e.g., "btn.generate")
   * @param {Object} [params] — optional key-value map for interpolation
   * @returns {string} translated string, or the key itself if not found
   *
   * @example
   *   I18n.t('page.of', { page: 3 })  // → "Page 3" (en) or "صفحة 3" (ar)
   */
  function t(key, params = {}) {
    let msg = _messages[key];
    if (msg === undefined) return key;

    // Template interpolation: ${param} → value
    for (const [k, v] of Object.entries(params)) {
      msg = msg.replaceAll(`\$\{${k}\}`, v);
    }
    return msg;
  }

  /**
   * Check if a translation key exists.
   * @param {string} key
   * @returns {boolean}
   */
  function has(key) {
    return _messages[key] !== undefined;
  }

  /**
   * Get current locale code (e.g., "en-US", "ar").
   * @returns {string}
   */
  function getLocale() {
    return _currentLocale;
  }

  /**
   * Check if current locale is RTL.
   * @returns {boolean}
   */
  function isRTL() {
    return _isRTL;
  }

  /**
   * Get text direction: "ltr" or "rtl".
   * @returns {string}
   */
  function getDirection() {
    return _isRTL ? 'rtl' : 'ltr';
  }

  /**
   * Get the list of supported locales.
   * @returns {Array<{code: string, nativeName: string, englishName: string}>}
   */
  function getSupportedLocales() {
    return SUPPORTED_LOCALES;
  }

  /**
   * Switch to a different locale.
   *
   * @param {string} localeCode — e.g. "ar", "he", "en-US"
   * @param {string} [localePath="/dashboard/locales"]
   * @returns {Promise<void>}
   */
  async function setLocale(localeCode, localePath = '/dashboard/locales') {
    try {
      const resp = await fetch(`${localePath}/${localeCode}.json`);
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      const data = await resp.json();

      _currentLocale = localeCode;
      _messages = data;
      _isRTL = (data._meta && data._meta.direction === 'rtl');

      localStorage.setItem(STORAGE_KEY, localeCode);
      _applyDirection();
      _refreshDOM();
      _notifyListeners();

      // Also notify the backend
      try {
        await fetch('/api/v1/locale/set', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ locale: localeCode }),
        });
      } catch (_) { /* non-critical */ }
    } catch (err) {
      console.error(`[i18n] Failed to load locale "${localeCode}"`, err);
    }
  }

  /**
   * Register a callback that fires when the locale changes.
   * Useful for dynamic UI elements that need manual re-render.
   *
   * @param {Function} fn — called with no arguments after locale switch
   */
  function onChange(fn) {
    _listeners.push(fn);
    if (_initialized) fn(); // fire immediately if already loaded
  }

  /**
   * Refresh all DOM elements with `data-i18n` and `data-i18n-*` attributes.
   * Call after dynamically adding new content that uses these attributes.
   */
  function refresh() {
    _refreshDOM();
  }

  // ── Private helpers ────────────────────────────────────────────

  function _detectLocale() {
    // Priority: localStorage > browser languages > default
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored && SUPPORTED_LOCALES.some(l => l.code === stored)) return stored;

    // Try browser's navigator.languages
    for (const lang of navigator.languages || []) {
      const match = SUPPORTED_LOCALES.find(l => l.code === lang || l.code.startsWith(lang.split('-')[0]));
      if (match) return match.code;
    }

    return 'en-US';
  }

  function _applyDirection() {
    document.documentElement.lang = _currentLocale;
    document.documentElement.dir = _isRTL ? 'rtl' : 'ltr';
    document.body.classList.toggle('rtl', _isRTL);

    // Toggle RTL CSS stylesheet if present
    const rtlCSS = document.getElementById('rtl-css');
    if (rtlCSS) rtlCSS.disabled = !_isRTL;

    // Sync locale dropdown if present
    const localeSelect = document.getElementById('locale-select');
    if (localeSelect) localeSelect.value = _currentLocale;
  }

  function _refreshDOM() {
    // Update text content
    document.querySelectorAll('[data-i18n]').forEach(el => {
      const key = el.getAttribute('data-i18n');
      el.textContent = t(key);
    });

    // Update HTML content (use sparingly, XSS risk)
    document.querySelectorAll('[data-i18n-html]').forEach(el => {
      const key = el.getAttribute('data-i18n-html');
      el.innerHTML = t(key);
    });

    // Update placeholder attributes
    document.querySelectorAll('[data-i18n-placeholder]').forEach(el => {
      const key = el.getAttribute('data-i18n-placeholder');
      el.placeholder = t(key);
    });

    // Update title attributes
    document.querySelectorAll('[data-i18n-title]').forEach(el => {
      const key = el.getAttribute('data-i18n-title');
      el.title = t(key);
    });

    // Update aria-label attributes
    document.querySelectorAll('[data-i18n-aria-label]').forEach(el => {
      const key = el.getAttribute('data-i18n-aria-label');
      el.setAttribute('aria-label', t(key));
    });
  }

  function _notifyListeners() {
    for (const fn of _listeners) {
      try { fn(); } catch (e) { console.error('[i18n] Listener error:', e); }
    }
  }

  // ── Export ─────────────────────────────────────────────────────
  return {
    init,
    t,
    has,
    getLocale,
    isRTL,
    getDirection,
    getSupportedLocales,
    setLocale,
    onChange,
    refresh,
  };
})();
