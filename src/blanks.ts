//! Fill-in blanks in saved commands: `{{host}}`, or `{{port:22}}` with a default.
//! No imports, so it can be tested on its own (see scripts/test-blanks.mjs).

/**
 * `{{name}}` or `{{name:default}}` in a code block is a blank to fill in when copying.
 * `${{ … }}` (GitHub Actions) is left alone.
 */
export const PLACEHOLDER = /(?<!\$)\{\{\s*([A-Za-z_][\w.-]*)\s*(?::([^}\n]*))?\}\}/g;

export interface Blank {
  name: string;
  fallback: string;
}

export function findBlanks(code: string): Blank[] {
  const seen = new Map<string, Blank>();
  for (const m of code.matchAll(PLACEHOLDER)) {
    const name = m[1];
    const fallback = (m[2] ?? "").trim();
    const known = seen.get(name);
    if (!known) seen.set(name, { name, fallback });
    else if (!known.fallback && fallback) known.fallback = fallback;
  }
  return [...seen.values()];
}

/** Puts the values in; a blank left empty uses its default (or nothing). */
export function fillBlanks(code: string, values: Record<string, string>): string {
  return code.replace(PLACEHOLDER, (_m, name: string, fallback?: string) => {
    const v = values[name];
    return v !== undefined && v !== "" ? v : (fallback ?? "").trim();
  });
}
