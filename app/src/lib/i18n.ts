// Translates backend codes and UI keys with the language files. Only English
// for now; each language is a JSON file keyed like en.json.
import en from "../locales/en.json";

export type Params = Record<string, string | number>;

const messages: Record<string, string> = en;
const warned = new Set<string>();

/** The text for `key`, with `{name}` placeholders filled from `params`. A
 *  missing key shows the key itself, so nothing is lost, and is reported
 *  once in the console. */
export function t(key: string, params: Params = {}): string {
  const template = messages[key];
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
