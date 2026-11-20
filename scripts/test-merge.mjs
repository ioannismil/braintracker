// Tests for combining window edits with `bt` edits (src/merge.ts).
// Run: node --experimental-strip-types scripts/test-merge.mjs
import assert from "node:assert/strict";
import { mergeNote } from "../src/merge.ts";

const v = (body, title = "Note", tags = []) => ({ title, body, tags });
const base = v("Step 1");

// bt appended while I edited the start: both kept.
assert.deepEqual(mergeNote(base, v("Step 1, done"), v("Step 1\n\nStep 2")), v("Step 1, done\n\nStep 2"));
// bt appended while I appended too: both additions kept, mine first.
assert.deepEqual(mergeNote(base, v("Step 1\n\nmine"), v("Step 1\n\ntheirs")), v("Step 1\n\nmine\n\ntheirs"));
// I appended while bt rewrote the note: my addition goes on the end of theirs.
assert.deepEqual(mergeNote(base, v("Step 1\n\nmine"), v("Rewritten")), v("Rewritten\n\nmine"));
// Appending to an empty note.
assert.deepEqual(mergeNote(v(""), v("typed here"), v("from bt")), v("typed here\n\nfrom bt"));
// Only one side changed: that side wins.
assert.deepEqual(mergeNote(base, v("Step 1"), v("bt version")), v("bt version"));
assert.deepEqual(mergeNote(base, v("my version"), v("Step 1")), v("my version"));
// Same addition on both sides isn't doubled.
assert.deepEqual(mergeNote(base, v("Step 1\n\nsame"), v("Step 1\n\nsame")), v("Step 1\n\nsame"));
// Both rewrote it differently: can't merge.
assert.equal(mergeNote(base, v("mine entirely"), v("theirs entirely")), null);

// Title and tags: whoever changed them wins; both changing differently can't merge.
assert.deepEqual(mergeNote(base, v("Step 1", "My title"), v("Step 1\n\nmore")), v("Step 1\n\nmore", "My title"));
assert.deepEqual(mergeNote(v("x", "T", ["a"]), v("x", "T", ["a"]), v("x", "T", ["b", "a"])), v("x", "T", ["b", "a"]));
assert.equal(mergeNote(v("x", "T"), v("x", "Mine"), v("x", "Theirs")), null);

console.log("merge: all tests passed");
