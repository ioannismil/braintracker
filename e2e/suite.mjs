// The end-to-end test suite (run it with `npm run e2e`, see run.mjs).
// Tests run in order and build on each other's notes, like one person using the app.
import fs from "node:fs";
import { spawn } from "node:child_process";
import path from "node:path";
import {
  $, $$, HERE, Key, bt, buttonByText, cfg, clearInput, exists, foreground, invoke, makeImage, ps, press, sleep, start,
  stop, summary, test, text, toWindow, waitFor, windowVisible,
} from "./lib.mjs";

const DESKTOP = { desktop: true };

export async function run() {
  const d = await start();
  const ctrl = (k) => d.actions().keyDown(Key.CONTROL).sendKeys(k).keyUp(Key.CONTROL).perform();
  const keys = (...k) => d.actions().sendKeys(...k).perform();
  const rows = async () => Promise.all((await $$(d, ".list .row .row-title")).map(async (r) => (await r.getText()).replace(/^●/, "")));
  const status = () => text(d, ".toolbar .status");
  const saved = () => waitFor(async () => (await status()) === "Saved", 4000, "Saved");
  const click = async (css) => (await $(d, css)).click();
  const clipboard = () => ps("Get-Clipboard -Raw");
  const typeInto = async (css, s) => {
    const e = await $(d, css);
    await e.click();
    await e.sendKeys(s);
  };
  const entryByTitle = async (t) => {
    const list = await invoke(d, "list_entries", { query: null, tag: null });
    return list.find((e) => e.title === t);
  };

  let mainHandle;
  let fatal = false;
  let selProc;
  const selectIn = (t) => {
    closeSelect();
    selProc = spawn("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(HERE, "seltext.ps1"), t], { stdio: "ignore" });
  };
  const closeSelect = () => {
    try { selProc?.kill(); } catch {}
    selProc = undefined;
  };
  try {
    await waitFor(() => exists(d, ".app"), 15000, "app to load");
    mainHandle = await d.getWindowHandle();
    const info = await invoke(d, "app_info");
    if (!info.portable || !info.dataDir.toLowerCase().startsWith(cfg.appDir.toLowerCase())) throw new Error(`not isolated: ${info.dataDir}`);
    const shot = path.join(cfg.work, "screenshot.png");
    makeImage(shot, "Deploy failed with error 0x80070005 here", 16); // a size Windows OCR reads codes at
    const photo = path.join(cfg.work, "photo.png");
    makeImage(photo, "whiteboard photo");
    console.log(`BrainTracker ${info.version}, data: ${info.dataDir}\n`);

    console.log("Main window");
    await test("starts on the welcome screen with an empty log", async () => {
      await waitFor(() => exists(d, ".welcome"), 5000, "welcome");
      if (!(await text(d, ".list-empty")).includes("Your log is empty")) throw new Error("no empty-log hint");
    });

    await test("＋ New entry button: creates an entry, title focused", async () => {
      await click(".new-btn");
      await waitFor(() => exists(d, "input.title"), 3000, "editor");
      const focused = await d.switchTo().activeElement();
      if ((await focused.getAttribute("class")) !== "title") throw new Error("title not focused");
    });

    await test("typing a title, tags and body auto-saves", async () => {
      await d.switchTo().activeElement().then((e) => e.sendKeys("Reset the docker network"));
      await keys(Key.ENTER); // Enter in the title jumps to the body
      const f = await d.switchTo().activeElement();
      if ((await f.getAttribute("class")) !== "body") throw new Error("Enter in title didn't move to body");
      await f.sendKeys("Ran `docker network prune` then restarted.\n\n```\nssh {{user:admin}}@{{host}}\n```");
      await typeInto(".tags-field input", "docker, ops");
      await saved();
      const e = await entryByTitle("Reset the docker network");
      if (!e || e.tags.join() !== "docker,ops") throw new Error(`stored: ${JSON.stringify(e)}`);
    });

    await test("Tab in the body indents instead of leaving it", async () => {
      const b = await $(d, "textarea.body");
      await b.click();
      await b.sendKeys(Key.END, Key.ENTER, Key.TAB, "indented");
      const v = await b.getAttribute("value");
      if (!v.endsWith("\n  indented")) throw new Error(JSON.stringify(v.slice(-20)));
      await saved();
    });

    await test("Ctrl+S saves immediately", async () => {
      const b = await $(d, "textarea.body");
      await b.sendKeys(" now");
      await ctrl("s");
      await waitFor(async () => (await status()) === "Saved", 1500, "save within 1.5s");
    });

    await test("Ctrl+E toggles preview; Markdown renders with copy buttons", async () => {
      await ctrl("e");
      await waitFor(() => exists(d, "article.markdown"), 2000, "preview");
      if (!(await exists(d, "article.markdown .copy-btn"))) throw new Error("no copy button");
      if (!(await exists(d, "article.markdown code"))) throw new Error("inline code not rendered");
      if (!(await exists(d, "article.markdown .ph"))) throw new Error("{{blanks}} not highlighted");
    });

    await test("Copy on a command with blanks asks for them, fills them in, remembers non-secret ones", async () => {
      await click("article.markdown .copy-btn");
      await waitFor(() => exists(d, ".ph-form"), 2000, "fill-in form");
      const inputs = await $$(d, ".ph-form input");
      if (inputs.length !== 2) throw new Error(`${inputs.length} inputs`);
      await (await $(d, '.ph-form input[name="host"]')).sendKeys("db01");
      const prev = await text(d, ".ph-preview");
      if (prev !== "ssh admin@db01") throw new Error(`preview "${prev}"`);
      await (await $(d, '.ph-form input[name="host"]')).sendKeys(Key.ENTER);
      await waitFor(async () => (await clipboard()) === "ssh admin@db01", 3000, "clipboard");
      await waitFor(async () => (await invoke(d, "get_placeholders")).values.host === "db01", 3000, "remembered host");
    });

    await test("fill-in form: Copy as is, Cancel and Esc", async () => {
      await click("article.markdown .copy-btn");
      await waitFor(() => exists(d, ".ph-form"), 2000, "form");
      const host = await (await $(d, '.ph-form input[name="host"]')).getAttribute("value");
      if (host !== "db01") throw new Error(`remembered value not prefilled: "${host}"`);
      await (await buttonByText(d, "Copy as is", ".ph-form")).click();
      await waitFor(async () => (await clipboard()) === "ssh {{user:admin}}@{{host}}", 3000, "raw copy");
      await click("article.markdown .copy-btn");
      await waitFor(() => exists(d, ".ph-form"), 2000, "form");
      await (await buttonByText(d, "Cancel", ".ph-form")).click();
      if (await exists(d, ".ph-form")) throw new Error("Cancel didn't close");
      await click("article.markdown .copy-btn");
      await waitFor(() => exists(d, ".ph-form"), 2000, "form");
      await (await $(d, ".ph-form input")).sendKeys(Key.ESCAPE);
      if (await exists(d, ".ph-form")) throw new Error("Esc didn't close");
      if (!(await exists(d, "article.markdown"))) throw new Error("Esc in the form also left preview");
    });

    await test("double-clicking the preview switches to editing", async () => {
      await d.actions().doubleClick(await $(d, "article.markdown p")).perform();
      await waitFor(() => exists(d, "textarea.body"), 2000, "editor");
    });

    await test("Ctrl+T new entry; leaving an empty one discards it", async () => {
      const before = (await invoke(d, "list_entries", { query: null, tag: null })).length;
      await ctrl("t");
      await waitFor(async () => (await (await $(d, "input.title")).getAttribute("value")) === "", 2000, "blank entry");
      await (await $$(d, ".list .row"))[1].click(); // back to the first note
      await sleep(500);
      const after = (await invoke(d, "list_entries", { query: null, tag: null })).length;
      if (after !== before) throw new Error(`${before} → ${after}: the empty entry wasn't discarded`);
    });

    await test("clicking a note with text opens it in preview", async () => {
      await (await $$(d, ".list .row"))[0].click();
      await waitFor(() => exists(d, "article.markdown"), 2000, "preview");
    });

    await test("more notes for searching", async () => {
      for (const [t, tags, body] of [
        ["Renew the TLS certificate", ["ops", "tls"], "certbot renew --nginx"],
        ["Fix printer queue", ["windows"], "net stop spooler && net start spooler"],
        ["Postgres vacuum", ["db"], "VACUUM ANALYZE; error 0x80070005 seen once"],
      ])
        await invoke(d, "create_entry", { title: t, tags, body });
      await d.executeScript("window.dispatchEvent(new Event('focus'))"); // app refreshes on focus
      await waitFor(async () => (await rows()).length === 4, 3000, "4 rows");
    });

    await test("Ctrl+K focuses search; typing filters; result count shows", async () => {
      await ctrl("k");
      const s = await d.switchTo().activeElement();
      if ((await s.getAttribute("placeholder")) !== "Search everything…") throw new Error("search not focused");
      await s.sendKeys("certbot");
      await waitFor(async () => (await rows()).join() === "Renew the TLS certificate", 3000, "one result");
      if (!(await text(d, ".group")).toLowerCase().includes("1 result")) throw new Error("no count");
    });

    await test("search: ↓/↑ browse, Enter opens for editing, Esc clears", async () => {
      const s = await $(d, ".search input");
      await s.sendKeys(Key.ARROW_DOWN);
      await waitFor(async () => (await (await $(d, "input.title")).getAttribute("value")) === "Renew the TLS certificate", 2000, "opened");
      await s.sendKeys(Key.ARROW_DOWN, Key.ARROW_UP);
      await s.sendKeys(Key.ENTER);
      await waitFor(async () => (await (await $(d, "input.title")).getAttribute("value")) === "Renew the TLS certificate", 2000, "still that note");
      await s.click();
      await s.sendKeys(Key.ESCAPE);
      if ((await s.getAttribute("value")) !== "") throw new Error("Esc didn't clear");
      await waitFor(async () => (await rows()).length === 4, 2000, "all rows back");
    });

    await test("search filters: tag:, has:code, is:pinned, after:", async () => {
      const s = await $(d, ".search input");
      const check = async (q, n) => {
        await clearInput(s);
        await s.sendKeys(q);
        await waitFor(async () => (await rows()).length === n, 3000, `${q} → ${n}`);
      };
      await check("tag:ops", 2);
      await check("has:code", 1);
      await check("is:pinned", 0);
      await check("after:2020-01", 4);
      await check("spooler", 1);
      await clearInput(s);
      await s.sendKeys(Key.ESCAPE);
    });

    await test("search filter chips appear on focus and fill the box", async () => {
      const s = await $(d, ".search input");
      await s.click();
      await waitFor(() => exists(d, ".search-tips .chip"), 2000, "chips");
      await (await buttonByText(d, "has:image", ".search-tips")).click();
      if ((await s.getAttribute("value")) !== "has:image ") throw new Error(await s.getAttribute("value"));
      await clearInput(s);
      await s.sendKeys(Key.ESCAPE);
    });

    await test("search with no match offers to create; Enter creates it", async () => {
      const s = await $(d, ".search input");
      await s.click();
      await s.sendKeys("Rotate the backup keys");
      await waitFor(() => exists(d, ".list-empty .link"), 3000, "create offer");
      await s.sendKeys(Key.ENTER);
      await waitFor(async () => (await (await $(d, "input.title")).getAttribute("value")) === "Rotate the backup keys", 2000, "created");
      const f = await d.switchTo().activeElement();
      await f.sendKeys("gpg --gen-key");
      await saved();
    });

    await test("tag chips filter the list", async () => {
      await (await buttonByText(d, "#tls", ".tag-bar")).click();
      await waitFor(async () => (await rows()).join() === "Renew the TLS certificate", 2000, "tls only");
      await (await buttonByText(d, "All", ".tag-bar")).click();
      await waitFor(async () => (await rows()).length === 5, 2000, "all");
    });

    await test("tag suggestions complete a tag", async () => {
      const t = await $(d, ".tags-field input");
      await t.click();
      await t.sendKeys("do");
      await waitFor(() => exists(d, ".suggest .chip"), 2000, "suggestion");
      await (await buttonByText(d, "#docker", ".suggest")).click();
      if ((await t.getAttribute("value")) !== "docker, ") throw new Error(await t.getAttribute("value"));
      await saved();
    });

    await test("☆ Pin puts the note in Pinned at the top; unpin", async () => {
      await (await buttonByText(d, "Pin", ".toolbar")).click();
      await waitFor(async () => (await text(d, ".list .group")).toLowerCase() === "pinned", 2000, "Pinned group");
      if ((await rows())[0] !== "Rotate the backup keys") throw new Error(`rows: ${await rows()} open: ${await (await $(d, "input.title")).getAttribute("value")}`);
      await (await buttonByText(d, "Pinned", ".toolbar")).click();
      await waitFor(async () => (await text(d, ".list .group")).toLowerCase() !== "pinned", 2000, "unpinned");
    });

    await test("🗑 delete shows Undo; Undo brings it back", async () => {
      await (await $(d, '.toolbar button[title^="Move to trash"]')).click();
      await waitFor(() => exists(d, ".toast"), 2000, "toast");
      if ((await rows()).includes("Rotate the backup keys")) throw new Error("still listed");
      await (await buttonByText(d, "Undo", ".toast")).click();
      await waitFor(async () => (await rows()).includes("Rotate the backup keys"), 2000, "restored").catch(async (e) => { throw new Error(`${e.message} rows: ${await rows()}`); });
    });

    await test("Trash: restore, delete forever (with confirm), empty trash", async () => {
      await (await $(d, '.toolbar button[title^="Move to trash"]')).click();
      await waitFor(() => exists(d, ".toast"), 2000, "toast");
      await (await $(d, ".toast .icon-btn")).click();
      await waitFor(async () => (await text(d, ".sidebar-foot")).includes("1"), 2000, "trash count 1");
      await click(".sidebar-foot .tool");
      await waitFor(() => exists(d, ".trash-list li"), 2000, "trash list");
      await (await buttonByText(d, "Restore", ".trash-list")).click();
      await waitFor(async () => (await (await $(d, "input.title")).getAttribute("value")) === "Rotate the backup keys", 2000, "reopened");
      // delete again, then delete forever
      await (await $(d, '.toolbar button[title^="Move to trash"]')).click();
      await click(".sidebar-foot .tool");
      await waitFor(() => exists(d, ".trash-list li"), 2000, "trash list");
      await (await $(d, '.trash-actions button[title="Delete forever"]')).click();
      await (await buttonByText(d, "Delete forever", ".trash-actions")).click();
      await waitFor(() => exists(d, ".empty-note"), 2000, "empty");
      await click('.dialog .icon-btn[aria-label="Close"]');
      // empty trash with two items
      for (const t of ["Trash me 1", "Trash me 2"]) {
        const e = await invoke(d, "create_entry", { title: t, tags: [], body: "x" });
        await invoke(d, "delete_entry", { id: e.id });
      }
      await d.executeScript("window.dispatchEvent(new Event('focus'))");
      await click(".sidebar-foot .tool");
      await waitFor(async () => (await $$(d, ".trash-list li")).length === 2, 2000, "2 in trash");
      await (await buttonByText(d, "Empty trash", ".dialog")).click();
      await (await buttonByText(d, "Delete all 2 for good", ".dialog")).click();
      await waitFor(() => exists(d, ".empty-note"), 2000, "emptied");
      await keys(Key.ESCAPE);
      if (await exists(d, ".dialog")) throw new Error("Esc didn't close the trash");
    });

    await test("History lists earlier versions and restores one", async () => {
      const e = await entryByTitle("Fix printer queue");
      // A version is kept when the text is replaced (bt does this; so does a big edit later on).
      bt("edit", String(e.id), "--body", "the new way: Restart-Service Spooler");
      await (await $$(d, ".list .row")).find; // noop
      await d.executeScript("window.dispatchEvent(new Event('focus'))");
      await waitFor(async () => (await rows()).includes("Fix printer queue"), 2000, "row");
      for (const r of await $$(d, ".list .row")) if ((await r.getText()).includes("Fix printer queue")) await r.click();
      await waitFor(async () => (await text(d, "article.markdown")).includes("Restart-Service"), 3000, "new text");
      await (await buttonByText(d, "History", ".toolbar")).click();
      await waitFor(() => exists(d, ".history-item"), 3000, "a version");
      await click(".history-item");
      await waitFor(async () => (await text(d, ".history-preview")).includes("net stop spooler"), 2000, "old text");
      await (await buttonByText(d, "Restore this version", ".dialog")).click();
      await waitFor(async () => !(await exists(d, ".dialog")), 2000, "closed");
      await waitFor(async () => (await text(d, "article.markdown")).includes("net stop spooler"), 2000, "restored text");
    });

    console.log("\nbt command line while the app is open");
    await test("bt add shows up in the open app", async () => {
      const r = bt("add", "Added from bt", "--tag", "cli", "--body", "hello from the terminal");
      if (r.code) throw new Error(r.out);
      await waitFor(async () => (await rows()).includes("Added from bt"), 4000, "row appears");
    });

    await test("bt edit --append on the open note updates the window", async () => {
      for (const r of await $$(d, ".list .row")) if ((await r.getText()).includes("Added from bt")) await r.click();
      await waitFor(async () => (await text(d, "article.markdown")).includes("hello from the terminal"), 2000, "open");
      const id = (await entryByTitle("Added from bt")).id;
      bt("edit", String(id), "--append", "appended line");
      await waitFor(async () => (await text(d, "article.markdown")).includes("appended line"), 4000, "window updates");
    });

    await test("bt append while you're typing: both changes kept (merged)", async () => {
      const id = (await entryByTitle("Added from bt")).id;
      await ctrl("e"); // edit
      const b = await waitFor(() => $(d, "textarea.body"), 2000, "editor");
      await b.click();
      await b.sendKeys(Key.CONTROL, Key.HOME, Key.NULL, "TYPED HERE ");
      bt("edit", String(id), "--append", "from bt meanwhile"); // before the window's save
      await waitFor(async () => {
        const e = await invoke(d, "get_entry", { id });
        return e.body.includes("TYPED HERE") && e.body.includes("from bt meanwhile");
      }, 5000, "both in the database");
      const v = await (await $(d, "textarea.body")).getAttribute("value");
      if (!v.includes("from bt meanwhile") || !v.includes("TYPED HERE")) throw new Error(`window: ${v}`);
    });

    await test("bt replacing text you're editing: conflict banner → Keep mine", async () => {
      const id = (await entryByTitle("Added from bt")).id;
      const b = await $(d, "textarea.body");
      await b.click();
      await b.sendKeys(Key.CONTROL, Key.HOME, Key.NULL, "MINE ");
      bt("edit", String(id), "--body", "THEIRS completely different");
      await waitFor(() => exists(d, ".conflict"), 5000, "conflict banner");
      if ((await status()) !== "Changed elsewhere — not saved") throw new Error(await status());
      await (await buttonByText(d, "Keep mine", ".conflict")).click();
      await waitFor(async () => (await invoke(d, "get_entry", { id })).body.startsWith("MINE "), 3000, "mine saved");
      const versions = await invoke(d, "list_versions", { entryId: id }).catch(() => invoke(d, "list_versions", { id }));
      if (!versions.some((v) => v.snippet?.includes("THEIRS"))) throw new Error("theirs not in History");
    });

    await test("conflict → Use the other version", async () => {
      const id = (await entryByTitle("Added from bt")).id;
      const b = await $(d, "textarea.body");
      await b.click();
      await b.sendKeys(Key.CONTROL, Key.HOME, Key.NULL, "MINE AGAIN ");
      bt("edit", String(id), "--body", "OTHER VERSION");
      await waitFor(() => exists(d, ".conflict"), 5000, "conflict banner");
      await (await buttonByText(d, "Use the other version", ".conflict")).click();
      await waitFor(async () => (await (await $(d, "textarea.body")).getAttribute("value")) === "OTHER VERSION", 3000, "theirs shown");
      if ((await invoke(d, "get_entry", { id })).body !== "OTHER VERSION") throw new Error("db");
    });

    await test("bt delete of the open note closes it", async () => {
      const id = (await entryByTitle("Added from bt")).id;
      await ctrl("e");
      bt("delete", String(id));
      await waitFor(async () => !(await rows()).includes("Added from bt"), 4000, "gone from list");
    });

    console.log("\nPop-out window");
    let popId;
    await test("⧉ Pop out opens the note in its own window", async () => {
      for (const r of await $$(d, ".list .row")) if ((await r.getText()).includes("Reset the docker")) await r.click();
      popId = (await entryByTitle("Reset the docker network")).id;
      await (await buttonByText(d, "Pop out", ".toolbar")).click();
      if (!(await toWindow(d, `note.html?id=${popId}`))) throw new Error("no pop-out window");
      await waitFor(async () => (await text(d, ".note-pop-title")) === "Reset the docker network", 3000, "title");
      await waitFor(() => windowVisible("Reset the docker network"), 3000, "window titled after the note");
    });

    await test("pop-out follows edits made in the main window", async () => {
      await d.switchTo().window(mainHandle);
      await ctrl("e");
      const b = await waitFor(() => $(d, "textarea.body"), 2000, "editor");
      await b.sendKeys(Key.CONTROL, Key.END, Key.NULL, "\nLIVE EDIT");
      await saved();
      await toWindow(d, `note.html?id=${popId}`);
      await waitFor(async () => (await text(d, ".note-pop-body")).includes("LIVE EDIT"), 3000, "pop-out updated");
    });

    await test("pop-out: On top toggle, copy button with blanks, ✎ Edit, Esc closes", async () => {
      await (await buttonByText(d, "On top", ".note-pop-tools")).click();
      await waitFor(async () => (await text(d, ".note-pop-tools")).includes("Not on top"), 2000, "toggled");
      await click(".note-pop-body .copy-btn");
      await waitFor(() => exists(d, ".ph-form"), 2000, "fill-in form in pop-out");
      await (await $(d, ".ph-form input")).sendKeys(Key.ESCAPE);
      await (await buttonByText(d, "Edit", ".note-pop-tools")).click();
      await d.switchTo().window(mainHandle);
      await waitFor(async () => (await (await $(d, "input.title")).getAttribute("value")) === "Reset the docker network", 2000, "main shows it");
      await toWindow(d, `note.html?id=${popId}`);
      await d.actions().sendKeys(Key.ESCAPE).perform();
      await sleep(800);
      await d.switchTo().window(mainHandle);
      if (await toWindow(d, `note.html?id=${popId}`, 1000)) throw new Error("pop-out still open");
      await d.switchTo().window(mainHandle);
    });

    console.log("\nQuick capture (global shortcut)");
    await test("Ctrl+Shift+E opens quick capture", async () => {
      press("ctrl+shift+e");
      if (!(await toWindow(d, "capture.html"))) throw new Error("no capture window");
      await waitFor(() => windowVisible("BrainTracker — Quick capture") || windowVisible("Quick capture"), 3000, "visible").catch(async () => {
        if (!(await d.executeScript("return document.visibilityState")) === "visible") throw new Error("not visible");
      });
      await waitFor(async () => (await (await d.switchTo().activeElement()).getAttribute("class")) === "capture-title", 2000, "title focused");
    }, DESKTOP);

    await test("capture: Enter saves and stays open for the next one", async () => {
      const t = await d.switchTo().activeElement();
      await t.sendKeys("Captured one", Key.SHIFT, Key.ENTER, Key.NULL);
      const f = await d.switchTo().activeElement();
      if ((await f.getAttribute("class")) !== "capture-body-input") throw new Error("Shift+Enter didn't jump to notes");
      await f.sendKeys("line one", Key.ENTER, "line two"); // Enter in notes = new line
      await (await $(d, ".capture-tags input")).sendKeys("capt");
      await (await $(d, ".capture-title")).click();
      await (await $(d, ".capture-title")).sendKeys(Key.ENTER);
      await waitFor(async () => (await text(d, ".capture-foot")).includes("Saved “Captured one”"), 3000, "saved msg");
      const e = await entryByTitle("Captured one");
      if (!e || !e.snippet?.includes("line two")) throw new Error(JSON.stringify(e));
    }, DESKTOP);

    await test("capture: Ctrl+Enter from notes saves; title made from the notes", async () => {
      const b = await $(d, ".capture-body-input");
      await b.click();
      await b.sendKeys("only notes here", Key.CONTROL, Key.ENTER, Key.NULL);
      await waitFor(async () => !!(await entryByTitle("only notes here")), 3000, "saved with derived title");
    }, DESKTOP);

    await test("capture: tag suggestion completes on Enter", async () => {
      const t = await $(d, ".capture-tags input");
      await t.click();
      await t.sendKeys("dock");
      await waitFor(() => exists(d, ".capture-tags .chip"), 2000, "suggestion");
      await t.sendKeys(Key.ENTER);
      if ((await t.getAttribute("value")) !== "docker, ") throw new Error(await t.getAttribute("value"));
      await clearInput(t);
    }, DESKTOP);

    await test("capture: Esc hides it and keeps unsaved text", async () => {
      await (await $(d, ".capture-title")).sendKeys("half written");
      await keys(Key.ESCAPE);
      await sleep(500);
      if (await d.executeScript("return document.hasFocus()")) {
        /* hidden windows can still report focus; check below instead */
      }
      press("ctrl+shift+e");
      await sleep(800);
      await toWindow(d, "capture.html");
      const v = await (await $(d, ".capture-title")).getAttribute("value");
      if (v !== "half written") throw new Error(`"${v}"`);
    }, DESKTOP);

    await test("capture: Ctrl+Shift+Enter saves and closes", async () => {
      await (await $(d, ".capture-title")).sendKeys(Key.CONTROL, Key.SHIFT, Key.ENTER, Key.NULL);
      await waitFor(async () => !!(await entryByTitle("half written")), 3000, "saved");
    }, DESKTOP);

    await test("capture: Open app ↗ and Ctrl+O bring up the main window", async () => {
      press("ctrl+shift+e");
      await toWindow(d, "capture.html");
      await sleep(400);
      await (await buttonByText(d, "Open app", ".capture-head")).click();
      await sleep(800);
      if (!windowVisible("BrainTracker")) throw new Error("main window not visible");
      press("ctrl+shift+e");
      await toWindow(d, "capture.html");
      await sleep(400);
      await (await $(d, ".capture-title")).sendKeys(Key.CONTROL, "o", Key.NULL);
      await sleep(800);
      if (!windowVisible("BrainTracker")) throw new Error("main window not visible after Ctrl+O");
    }, DESKTOP);

    await test("capture: ✕ closes; Save buttons disabled while empty", async () => {
      press("ctrl+shift+e");
      await toWindow(d, "capture.html");
      await sleep(400);
      for (const b of await $$(d, ".capture-actions button")) if (await b.isEnabled()) throw new Error("enabled while empty");
      await click('.capture-head .icon-btn');
      await sleep(500);
    }, DESKTOP);

    await test("Ctrl+Shift+D captures the text selected in another app", async () => {
      selectIn("kubectl rollout restart deploy/api");
      await waitFor(() => foreground() === "BT selection test", 8000, "test window in front");
      await sleep(400);
      press("ctrl+shift+d");
      await toWindow(d, "capture.html");
      await waitFor(async () => (await (await $(d, ".capture-body-input")).getAttribute("value")).includes("kubectl rollout restart"), 4000, "selection in notes");
      await clearInput(await $(d, ".capture-body-input"));
      await keys(Key.ESCAPE);
    }, DESKTOP);

    console.log("\nQuick search (global shortcut)");
    await test("Ctrl+Alt+Space opens quick search with recent notes", async () => {
      await d.switchTo().window(mainHandle);
      press("ctrl+alt+space");
      if (!(await toWindow(d, "spotlight.html"))) throw new Error("no spotlight window");
      await waitFor(async () => (await $$(d, ".spot-row")).length > 0, 3000, "recent rows");
      if (!(await text(d, ".spot-group")).toLowerCase().includes("recent")) throw new Error("no Recent header");
    }, DESKTOP);

    await test("quick search: type, ↓ moves, Enter shows the note, Esc goes back", async () => {
      const i = await $(d, ".spot-bar input");
      await i.sendKeys("spooler");
      // (the recent list already shows this note, so wait for the search's single result)
      await waitFor(async () => (await $$(d, ".spot-row")).length === 1 && (await text(d, ".spot-list")).includes("Fix printer queue"), 3000, "spooler result");
      await i.sendKeys(Key.ENTER);
      await waitFor(() => exists(d, ".spot-note"), 2000, "note shown");
      if (!(await text(d, ".spot-note-title")).includes("Fix printer queue")) throw new Error("wrong note");
      await i.sendKeys(Key.ESCAPE);
      await waitFor(async () => !(await exists(d, ".spot-note")), 2000, "back");
      await i.sendKeys(Key.ESCAPE);
      if ((await i.getAttribute("value")) !== "") throw new Error("Esc didn't clear");
    }, DESKTOP);

    await test("quick search: whole error message finds closest matches", async () => {
      const i = await $(d, ".spot-bar input");
      await i.sendKeys("Access denied 0x80070005 while running vacuum");
      await waitFor(async () => (await text(d, ".spot-list")).toLowerCase().includes("closest matches"), 3000, "closest");
      if (!(await text(d, ".spot-list")).includes("Postgres vacuum")) throw new Error("not found");
      await clearInput(i);
    }, DESKTOP);

    await test("quick search: Ctrl+Enter opens the note in the app", async () => {
      const i = await $(d, ".spot-bar input");
      await i.sendKeys("certbot");
      await waitFor(async () => (await text(d, ".spot-list")).includes("Renew the TLS"), 3000, "certbot result");
      await i.sendKeys(Key.CONTROL, Key.ENTER, Key.NULL);
      await d.switchTo().window(mainHandle);
      await waitFor(async () => (await (await $(d, "input.title")).getAttribute("value")) === "Renew the TLS certificate", 3000, "opened in app").catch(async (e) => { throw new Error(`${e.message}; main shows "${await (await $(d, "input.title")).getAttribute("value").catch(() => "nothing")}"`); });
    }, DESKTOP);

    await test("quick search: Ctrl+P pops the note out", async () => {
      press("ctrl+alt+space");
      await toWindow(d, "spotlight.html");
      const i = await $(d, ".spot-bar input");
      await clearInput(i);
      await i.sendKeys("VACUUM");
      await waitFor(async () => (await text(d, ".spot-list")).includes("Postgres vacuum"), 3000, "vacuum result");
      await i.sendKeys(Key.CONTROL, "p", Key.NULL);
      const id = (await entryByTitle("Postgres vacuum")).id;
      if (!(await toWindow(d, `note.html?id=${id}`))) throw new Error("no pop-out");
      await d.actions().sendKeys(Key.ESCAPE).perform();
      await sleep(500);
    }, DESKTOP);

    await test("quick search: clicking a row, ← Back, Open in BrainTracker ↗ button", async () => {
      press("ctrl+alt+space");
      await toWindow(d, "spotlight.html");
      const i = await $(d, ".spot-bar input");
      await clearInput(i);
      await i.sendKeys("docker");
      await waitFor(async () => (await text(d, ".spot-list")).includes("Reset the docker"), 3000, "docker result");
      await click(".spot-row");
      await waitFor(() => exists(d, ".spot-note"), 2000, "shown");
      await (await buttonByText(d, "Back", ".spot-bar")).click();
      await waitFor(async () => !(await exists(d, ".spot-note")), 2000, "back");
      await click(".spot-row");
      await waitFor(() => exists(d, ".spot-note"), 2000, "shown");
      await (await buttonByText(d, "Open in BrainTracker", ".spot-foot")).click();
      await d.switchTo().window(mainHandle);
      await waitFor(async () => (await (await $(d, "input.title")).getAttribute("value")).includes("docker"), 3000, "opened");
    }, DESKTOP);

    await test("Ctrl+Alt+Shift+Space searches for the text selected in another app", async () => {
      selectIn("certbot renew");
      await waitFor(() => foreground() === "BT selection test", 8000, "test window in front");
      await sleep(400);
      press("ctrl+alt+shift+space");
      await toWindow(d, "spotlight.html");
      await waitFor(async () => (await (await $(d, ".spot-bar input")).getAttribute("value")) === "certbot renew", 4000, "query from selection");
      await waitFor(async () => (await text(d, ".spot-list")).includes("Renew the TLS"), 3000, "result");
      await keys(Key.ESCAPE);
      await keys(Key.ESCAPE);
    }, DESKTOP);

    await test("clipboard is put back after grabbing a selection (files + formatted text)", async () => {
      // Explorer-style file copy on the clipboard first
      fs.writeFileSync(path.join(cfg.work, "sel.txt"), "x");
      ps(`Set-Clipboard -Path "${path.join(cfg.work, "sel.txt")}"`);
      selectIn("certbot renew");
      await waitFor(() => foreground() === "BT selection test", 8000, "test window in front");
      await sleep(400);
      press("ctrl+alt+shift+space");
      await toWindow(d, "spotlight.html");
      await sleep(1500);
      await keys(Key.ESCAPE);
      await keys(Key.ESCAPE);
      const files = ps(`Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.Clipboard]::GetFileDropList() -join ';'`);
      if (!files.endsWith("sel.txt")) throw new Error(`clipboard now: "${files}"`);
      // HTML (formatted text)
      ps(`Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.DataObject; $d.SetData('HTML Format', "Version:0.9\`r\`nStartHTML:0\`r\`nEndHTML:0\`r\`nStartFragment:0\`r\`nEndFragment:0\`r\`n<b>bold</b>"); $d.SetText('bold'); [System.Windows.Forms.Clipboard]::SetDataObject($d, $true)`);
      selectIn("certbot renew");
      await waitFor(() => foreground() === "BT selection test", 8000, "test window in front");
      await sleep(400);
      press("ctrl+alt+shift+space");
      await toWindow(d, "spotlight.html");
      await sleep(1500);
      await keys(Key.ESCAPE);
      await keys(Key.ESCAPE);
      const html = ps(`Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.Clipboard]::GetData('HTML Format')`);
      if (!html.includes("<b>bold</b>")) throw new Error(`HTML lost: "${html.slice(0, 80)}"`);
      closeSelect();
    }, DESKTOP);

    console.log("\nShow / hide main window");
    await test("Ctrl+Shift+Space hides and shows the main window", async () => {
      await d.switchTo().window(mainHandle);
      press("shift+ctrl+space"); // show (focus) — or hide if already in front
      await sleep(700);
      const a = windowVisible("BrainTracker");
      press("shift+ctrl+space");
      await sleep(700);
      const b = windowVisible("BrainTracker");
      if (a === b) throw new Error(`visible stayed ${a}`);
      if (!b) {
        press("shift+ctrl+space");
        await sleep(700);
      }
      if (!windowVisible("BrainTracker")) throw new Error("couldn't bring it back");
    }, DESKTOP);

    console.log("\nImages and screenshot text");
    await test("pasting a screenshot adds it to the note; its text becomes searchable", async () => {
      await d.switchTo().window(mainHandle);
      await ctrl("t");
      const t = await waitFor(() => $(d, "input.title"), 2000, "new");
      await t.sendKeys("Screenshot note", Key.ENTER);
      ps(`Add-Type -AssemblyName System.Windows.Forms,System.Drawing; $i=[System.Drawing.Image]::FromFile("${shot}"); [System.Windows.Forms.Clipboard]::SetImage($i)`);
      const b = await $(d, "textarea.body");
      await b.click();
      await b.sendKeys(Key.CONTROL, "v", Key.NULL);
      await waitFor(async () => (await b.getAttribute("value")).includes("![](") || (await b.getAttribute("value")).includes("!["), 4000, "image markdown");
      await waitFor(() => exists(d, ".thumbs img"), 3000, "thumbnail");
      await saved();
      await waitFor(async () => (await invoke(d, "list_entries", { query: "0x80070005", tag: null })).some((e) => e.title === "Screenshot note"), 15000, "OCR text searchable");
      const r = await invoke(d, "list_entries", { query: "has:image", tag: null });
      if (!r.some((e) => e.title === "Screenshot note")) throw new Error("has:image");
    }, DESKTOP);

    console.log("\nSettings");
    await test("Ctrl+, opens Settings; tabs switch; Esc closes", async () => {
      await d.switchTo().window(mainHandle);
      await (await $(d, ".search input")).click();
      await (await $(d, ".search input")).sendKeys(Key.ESCAPE);
      await ctrl(",");
      await waitFor(() => exists(d, '.dialog[aria-label="Settings"]'), 2000, "settings");
      for (const t of await $$(d, ".tabs .tab")) {
        await t.click();
        if ((await t.getAttribute("aria-selected")) !== "true") throw new Error("tab");
      }
      await keys(Key.ESCAPE);
      await waitFor(async () => !(await exists(d, ".dialog")), 2000, "closed");
      await click(".brand .icon-btn"); // ⚙ opens it too
      await waitFor(() => exists(d, ".dialog"), 2000, "settings via ⚙");
    });

    await test("Offline mode switch: on disables update checks; off again", async () => {
      const sw = await $(d, 'button[aria-label="Offline mode"]');
      await sw.click();
      await waitFor(async () => (await sw.getAttribute("aria-checked")) === "true", 2000, "on");
      if (await (await buttonByText(d, "Check now", ".setting.block")).isEnabled()) throw new Error("Check now still enabled");
      if (!(await invoke(d, "get_offline"))) throw new Error("not stored");
      let refused = false;
      await invoke(d, "check_for_update").catch(() => (refused = true));
      if (!refused) throw new Error("update check ran while offline");
      await sw.click();
      await waitFor(async () => (await sw.getAttribute("aria-checked")) === "false", 2000, "off");
    });

    await test("Updates: Check now runs", async () => {
      const b = await buttonByText(d, "Check now", ".setting.block");
      await b.click();
      await waitFor(async () => !(await b.getText()).includes("Checking"), 20000, "check finished");
      const msg = await text(d, ".dialog");
      if (!/up to date|available|Couldn|newer|Version/.test(msg)) throw new Error("no result shown");
    });

    await test("Launch at startup switch toggles (and is put back)", async () => {
      const sw = await $(d, '.setting button.switch[role="switch"]:not([aria-label])');
      const was = await sw.getAttribute("aria-checked");
      await sw.click();
      await waitFor(async () => (await sw.getAttribute("aria-checked")) !== was, 3000, "toggled");
      await sw.click();
      await waitFor(async () => (await sw.getAttribute("aria-checked")) === was, 3000, "back");
    });

    await test("Backups: Back up now, then restore it (with confirm)", async () => {
      await (await buttonByText(d, "Back up now", ".dialog")).click();
      await waitFor(() => exists(d, ".backup-list li"), 5000, "backup listed");
      const before = (await invoke(d, "list_entries", { query: null, tag: null })).length;
      await invoke(d, "create_entry", { title: "made after the backup", tags: [], body: "x" });
      await (await buttonByText(d, "Restore", ".backup-list")).click();
      await (await buttonByText(d, "Replace current data", ".backup-list")).click();
      await waitFor(async () => (await invoke(d, "list_entries", { query: null, tag: null })).length === before, 5000, "restored");
      await waitFor(async () => (await text(d, ".backup-list")).includes("before restore"), 3000, "safety copy kept");
    });

    await test("Inbox: notes and images dropped in the folder are imported and moved", async () => {
      const dir = path.join(cfg.work, "inbox");
      fs.rmSync(dir, { recursive: true, force: true });
      fs.mkdirSync(dir);
      await invoke(d, "set_inbox_config", { config: { enabled: true, dir } });
      fs.writeFileSync(path.join(dir, "from phone.md"), "# Phone note\nbuy cables");
      fs.copyFileSync(photo, path.join(dir, "photo.png"));
      fs.writeFileSync(path.join(dir, "half.md.tmp"), "ignore me");
      await sleep(3500);
      // re-render the dialog with the new folder
      await keys(Key.ESCAPE);
      await ctrl(",");
      await invoke(d, "inbox_check_now");
      await waitFor(async () => fs.existsSync(path.join(dir, "Imported", "from phone.md")), 6000, "moved to Imported");
      if (!fs.existsSync(path.join(dir, "Imported", "photo.png"))) throw new Error("image not imported");
      if (!fs.existsSync(path.join(dir, "half.md.tmp"))) throw new Error("temp file touched");
      const r = await invoke(d, "list_entries", { query: "tag:inbox", tag: null });
      if (r.length !== 2) throw new Error(`${r.length} inbox notes: ${r.map((e) => e.title)}`);
      await invoke(d, "set_inbox_config", { config: { enabled: false, dir: null } });
    });

    await test("Import notes (files) and clean up unused images", async () => {
      const f = path.join(cfg.work, "import-me.md");
      fs.writeFileSync(f, "---\ntitle: Imported note\ntags: [old]\n---\nfrom another app");
      const r = await invoke(d, "import_markdown", { paths: [f] });
      if (!JSON.stringify(r).includes("1")) throw new Error(JSON.stringify(r));
      if (!(await entryByTitle("Imported note"))) throw new Error("not imported");
      await (await buttonByText(d, "Check", ".dialog", true)).click();
      await waitFor(async () => /unused|No unused|nothing/i.test(await text(d, ".dialog")), 3000, "result");
    });

    await test("Phone sync: turn on, show a pairing QR code, cancel, turn off", async () => {
      // (The sync protocol itself is tested in Rust: src-tauri/src/sync/tests.rs.)
      await keys(Key.ESCAPE);
      await ctrl(",");
      await waitFor(() => exists(d, ".dialog"), 2000, "settings");
      await waitFor(async () => /Sync with your phone/.test(await text(d, ".dialog")), 3000, "phone sync section");
      await (await $(d, '.dialog button[aria-label="Sync with your phone"]')).click();
      await waitFor(async () => {
        try {
          return !!(await buttonByText(d, "Pair a phone", ".dialog"));
        } catch {
          return false;
        }
      }, 4000, "sync switched on");
      await (await buttonByText(d, "Pair a phone", ".dialog")).click();
      await waitFor(() => exists(d, ".dialog .pair-qr svg"), 4000, "QR code");
      const st = await invoke(d, "sync_status");
      if (!st.enabled || !st.running || !st.pairing?.code.startsWith("BTSYNC1:"))
        throw new Error(JSON.stringify({ ...st, pairing: !!st.pairing }));
      await (await buttonByText(d, "Cancel", ".dialog", true)).click();
      await waitFor(async () => !(await exists(d, ".dialog .pair-qr")), 3000, "QR code gone");
      const off = await invoke(d, "set_sync_enabled", { enabled: false });
      if (off.running) throw new Error("still listening after turning sync off");
    });

    await test("Export writes every note as Markdown", async () => {
      const dir = await invoke(d, "export_markdown");
      const files = fs.readdirSync(dir).filter((f) => f.endsWith(".md"));
      const n = (await invoke(d, "list_entries", { query: null, tag: null })).length;
      if (files.length !== n) throw new Error(`${files.length} files for ${n} notes`);
      fs.rmSync(dir, { recursive: true, force: true });
      const parent = path.dirname(dir);
      if (fs.readdirSync(parent).length === 0) fs.rmdirSync(parent);
    });

    await test("Shortcuts tab: record a new shortcut, it works, reset to defaults", async () => {
      await (await buttonByText(d, "Keyboard shortcuts", ".tabs")).click();
      const rowsSc = await $$(d, ".sc-row");
      let btn;
      for (const r of rowsSc) if ((await r.getText()).startsWith("New entry")) btn = await r.findElement({ css: ".sc-key" });
      await btn.click();
      await waitFor(async () => (await btn.getText()) === "Press keys…", 1000, "recording");
      await d.actions().keyDown(Key.CONTROL).keyDown(Key.SHIFT).sendKeys("y").keyUp(Key.SHIFT).keyUp(Key.CONTROL).perform();
      await waitFor(async () => (await btn.getText()).includes("Y"), 2000, "recorded");
      if ((await invoke(d, "get_shortcuts")).shortcuts.newEntry !== "Ctrl+Shift+Y") throw new Error("not saved");
      await keys(Key.ESCAPE);
      await waitFor(async () => !(await exists(d, ".dialog")), 2000, "closed");
      const n = (await rows()).length;
      await d.actions().keyDown(Key.CONTROL).keyDown(Key.SHIFT).sendKeys("y").keyUp(Key.SHIFT).keyUp(Key.CONTROL).perform();
      await waitFor(async () => (await (await $(d, "input.title")).getAttribute("value")) === "", 2000, "new entry via new keys");
      await ctrl(",");
      await (await buttonByText(d, "Keyboard shortcuts", ".tabs")).click();
      await (await buttonByText(d, "Reset to defaults", ".dialog")).click();
      await waitFor(async () => (await invoke(d, "get_shortcuts")).shortcuts.newEntry === "Ctrl+T", 2000, "reset");
      void n;
    });

    await test("Commands tab: Ask and Remember switches, forget a value", async () => {
      await (await buttonByText(d, "Commands", ".tabs")).click();
      await waitFor(async () => (await $$(d, ".dialog button.switch")).length === 2, 3000, "2 switches");
      const sws = await $$(d, ".dialog button.switch");
      await sws[0].click();
      await waitFor(async () => (await invoke(d, "get_placeholders")).ask === false, 2000, "ask off");
      await sws[0].click();
      await waitFor(async () => (await invoke(d, "get_placeholders")).ask === true, 2000, "ask on");
      await sws[1].click();
      await waitFor(async () => (await invoke(d, "get_placeholders")).remember === false, 2000, "remember off");
      await sws[1].click();
      await click('.dialog .icon-btn[aria-label="Forget host"]');
      await waitFor(async () => !(await invoke(d, "get_placeholders")).values.host, 2000, "forgot host");
      await keys(Key.ESCAPE);
    });

    await test("offline mode: web images in notes aren't loaded (pasted screenshots are); they load again when it's off", async () => {
      await d.switchTo().window(mainHandle);
      if (await exists(d, ".dialog")) await keys(Key.ESCAPE);
      await invoke(d, "set_offline", { enabled: true });
      const e = await invoke(d, "create_entry", { title: "Remote image", tags: [], body: "![logo](https://example.com/logo.png)\n\n![screenshot](img:local-shot.png)" });
      await d.executeScript("window.dispatchEvent(new Event('focus'))");
      await waitFor(async () => (await rows()).includes("Remote image"), 3000, "row");
      for (const r of await $$(d, ".list .row")) if ((await r.getText()).includes("Remote image")) await r.click();
      await waitFor(() => exists(d, "article.markdown .blocked-img"), 3000, "offline placeholder");
      if (await d.executeScript("return !!document.querySelector('article.markdown img[src*=\"example.com\"]')")) throw new Error("loaded while offline");
      if (!(await d.executeScript("return !!document.querySelector('article.markdown img[src^=\"http://btimg.localhost/\"]')")))
        throw new Error("pasted screenshot hidden by offline mode");
      await invoke(d, "set_offline", { enabled: false });
      await ctrl("e");
      await ctrl("e");
      await waitFor(async () => (await d.executeScript("return document.querySelector('article.markdown img')?.getAttribute('src')")) === "https://example.com/logo.png", 3000, "loaded when online");
      void e;
    });

    console.log("\nApp lifecycle");
    await test("closing the main window hides it to the tray (app keeps running)", async () => {
      await d.switchTo().window(mainHandle);
      await d.executeScript("window.__TAURI_INTERNALS__.invoke('plugin:window|close', { label: 'main' })").catch(() => {});
      await sleep(800);
      if (windowVisible("BrainTracker")) throw new Error("still visible");
      const n = await invoke(d, "trash_count"); // still answers = still running
      if (typeof n !== "number") throw new Error("not running");
      await invoke(d, "open_in_main", { id: (await entryByTitle("Reset the docker network")).id });
      await sleep(800);
      if (!windowVisible("BrainTracker")) throw new Error("didn't come back");
    });

    await test("starting it again (bt --open) uses the running app, no second copy", async () => {
      const before = Number(ps("(Get-Process braintracker -ErrorAction SilentlyContinue | Measure-Object).Count"));
      bt("search", "spooler", "--open");
      await sleep(1500);
      if (!(await toWindow(d, "spotlight.html"))) throw new Error("no spotlight");
      await waitFor(async () => (await (await $(d, ".spot-bar input")).getAttribute("value")) === "spooler", 3000, "query handed over");
      await keys(Key.ESCAPE);
      await keys(Key.ESCAPE);
      const after = Number(ps("(Get-Process braintracker -ErrorAction SilentlyContinue | Measure-Object).Count"));
      if (after !== before) throw new Error(`${before} → ${after} processes`);
    });
  } catch (e) {
    console.error("FATAL", e);
    fatal = true;
  } finally {
    closeSelect();
    await stop(d);
  }
  return summary() + (fatal ? 1 : 0);
}
