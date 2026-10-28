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
