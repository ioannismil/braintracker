// Tests for the {{blank}} rules in src/blanks.ts.
// Run: node --experimental-strip-types scripts/test-blanks.mjs
import assert from "node:assert/strict";
import { findBlanks, fillBlanks } from "../src/blanks.ts";

const names = (code) => findBlanks(code).map((b) => `${b.name}=${b.fallback}`);

assert.deepEqual(names("ssh {{user}}@{{host}} -p {{port:22}}"), ["user=", "host=", "port=22"]);
assert.deepEqual(names("{{a}}{{b}}"), ["a=", "b="], "blanks right next to each other");
assert.deepEqual(names("echo {{ name }} {{name:fallback}}"), ["name=fallback"], "same blank twice, default from either");
assert.deepEqual(names("run: echo ${{ secrets.TOKEN }}"), [], "GitHub Actions expressions are left alone");
assert.deepEqual(names("{{ .Values.image }} and {{}}"), [], "Helm templates and empty braces aren't blanks");

assert.equal(fillBlanks("ssh {{user}}@{{host}} -p {{port:22}}", { user: "me", host: "db01" }), "ssh me@db01 -p 22");
assert.equal(fillBlanks("{{a}}{{b}}", { a: "1", b: "2" }), "12");
assert.equal(fillBlanks("x={{x:5}}", { x: "" }), "x=5", "empty value falls back to the default");
assert.equal(fillBlanks("keep ${{ github.sha }} {{v}}", { v: "ok" }), "keep ${{ github.sha }} ok");
assert.equal(fillBlanks("$ {{cmd}}", { cmd: "ls" }), "$ ls", "a $ followed by a space is fine");

console.log("blanks: all tests passed");
