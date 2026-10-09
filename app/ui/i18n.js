// Minimalistische Übersetzung ohne Abhängigkeiten.
// Sprachdateien registrieren sich in window.LB_LOCALES (siehe locales/*.js).
(function () {
  const locales = (window.LB_LOCALES = window.LB_LOCALES || {});
  const FALLBACK = "en";
  let current = FALLBACK;

  function resolve(setting) {
    const wanted = !setting || setting === "auto"
      ? (navigator.languages && navigator.languages[0]) || navigator.language || FALLBACK
      : setting;
    const base = String(wanted).toLowerCase().split(/[-_]/)[0];
    return locales[base] ? base : FALLBACK;
  }

  function t(key, vars) {
    const dict = locales[current] || {};
    let s = dict[key] ?? (locales[FALLBACK] || {})[key] ?? key;
    if (vars) {
      for (const [k, v] of Object.entries(vars)) s = s.split(`{${k}}`).join(String(v));
    }
    return s;
  }

  window.i18n = {
    t,
    get lang() { return current; },
    set(setting) {
      current = resolve(setting);
      document.documentElement.lang = current;
      return current;
    },
    /** [{code, name}] aller eingebundenen Sprachen */
    available() {
      return Object.keys(locales).map((code) => ({ code, name: locales[code]._name || code }));
    },
    num(n) { return new Intl.NumberFormat(current).format(n || 0); },
    compact(n) { return new Intl.NumberFormat(current, { notation: "compact", maximumFractionDigits: 1 }).format(n || 0); },
    dateTime(ms) {
      return new Intl.DateTimeFormat(current, { dateStyle: "short", timeStyle: "medium" }).format(new Date(ms));
    },
  };
})();
