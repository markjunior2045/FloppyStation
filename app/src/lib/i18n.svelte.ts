// Translates backend codes and UI keys with the language files: one JSON
// file per language, keyed like en.json. The backend says which language to
// use; changing it updates every text on screen.
import en from "../locales/en.json";
import es from "../locales/es.json";
import pt from "../locales/pt.json";

export type Params = Record<string, string | number>;

const LANGUAGES: Record<string, Record<string, string>> = { en, pt, es };

let current = $state("en");
const warned = new Set<string>();

/** Switches the language (`en`, `pt` or `es`; others are ignored). */
export function setLanguage(language: string) {
  if (language in LANGUAGES && language !== current) {
    current = language;
    document.documentElement.lang = language;
  }
}

/** The text for `key` in the current language (English if it has none),
 *  with `{name}` placeholders filled from `params`. A missing key shows the
 *  key itself, so nothing is lost, and is reported once in the console. */
export function t(key: string, params: Params = {}): string {
  const template = LANGUAGES[current][key] ?? en[key as keyof typeof en];
  if (template === undefined) {
    if (!warned.has(key)) {
      warned.add(key);
      console.warn(`missing translation: ${key}`);
    }
    return key;
  }
  return template.replace(/\{(\w+)\}/g, (placeholder, name: string) =>
    name in params ? String(params[name]) : placeholder,
  );
}

/** The text for a failed backend call: a `Problem` ({ code, params }) or a
 *  bare code. A field error reads like "Name is empty." */
export function problemText(problem: unknown): string {
  if (typeof problem === "string") return t(problem, { detail: "" });
  const { code, params = {} } = (problem ?? {}) as { code?: string; params?: Record<string, string> };
  if (!code) return String(problem);
  if (code === "config_invalid_value" && params.field) {
    return t("ui.form.invalid", {
      field: t(`ui.field.${params.field}`),
      reason: t(`ui.reason.${params.reason}`),
    });
  }
  const field: Params = params.field ? { field: t(`ui.field.${params.field}`) } : {};
  return t(code, { detail: "", ...params, ...field });
}
