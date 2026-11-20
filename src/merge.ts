//! Combining an edit made in the window with one made elsewhere (the `bt` command line)
//! at the same time. No imports, so it can be tested on its own (scripts/test-merge.mjs).

export interface Version {
  title: string;
  body: string;
  tags: string[];
}

const sameTags = (a: string[], b: string[]) => a.length === b.length && [...a].sort().join(",") === [...b].sort().join(",");

/** Whichever side changed a field wins; if both changed it differently, null. */
function pick<T>(base: T, mine: T, theirs: T, same: (a: T, b: T) => boolean): T | null {
  if (same(mine, theirs) || same(theirs, base)) return mine;
  if (same(mine, base)) return theirs;
  return null;
}

/** `text` with something added at the end of `base` (as `bt edit --append` does): the addition. */
function addedAtEnd(base: string, text: string): string | null {
  const b = base.trimEnd();
  if (!b) return text.trim() ? text.trim() : null;
  if (!text.startsWith(b) || text.length <= b.length) return null;
  const added = text.slice(b.length);
  return added.trim() ? added.trim() : null;
}

function appendTo(text: string, added: string): string {
  if (text.trimEnd().endsWith(added)) return text; // already there
  return text.trim() ? `${text.trimEnd()}\n\n${added}` : added;
}

function mergeBody(base: string, mine: string, theirs: string): string | null {
  const simple = pick(base, mine, theirs, (a, b) => a === b);
  if (simple !== null) return simple;
  // Both changed it. Fine if one side only added to the end: put that addition after the
  // other side's text.
  const theirAddition = addedAtEnd(base, theirs);
  if (theirAddition !== null) return appendTo(mine, theirAddition);
  const myAddition = addedAtEnd(base, mine);
  if (myAddition !== null) return appendTo(theirs, myAddition);
  return null;
}

/**
 * Three-way merge of a note. `base` is the note as the window loaded it, `mine` the window's
 * text now, `theirs` what was saved elsewhere since. Returns null when both sides changed the
 * same thing in ways that can't be combined (the user then chooses).
 */
export function mergeNote(base: Version, mine: Version, theirs: Version): Version | null {
  const title = pick(base.title, mine.title, theirs.title, (a, b) => a.trim() === b.trim());
  const tags = pick(base.tags, mine.tags, theirs.tags, sameTags);
  const body = mergeBody(base.body, mine.body, theirs.body);
  if (title === null || tags === null || body === null) return null;
  return { title, body, tags };
}
