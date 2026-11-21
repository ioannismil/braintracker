// Helpers for driving the real BrainTracker app through tauri-driver (WebDriver for Tauri).
import { Builder, By, Key } from "selenium-webdriver";
import { execFileSync, spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";

export { By, Key };

/** This folder (the .ps1 helpers live here). */
export const HERE = path.dirname(fileURLToPath(import.meta.url));

/** Filled in by run.mjs: where the portable test copy lives, where to write results, etc. */
export const cfg = {
  work: "", // temporary folder for this run
  appDir: "", // portable copy of the app (its data goes in appDir/data)
  exe: "",
  bt: "",
  out: "", // screenshots of failures + results.json
  tauriDriver: "",
  edgeDriver: "",
  /** Skip tests that need a real desktop (system-wide shortcuts, other apps' windows, OCR). */
  ci: false,
};

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Windows PowerShell 5.1, with its own module path (PowerShell 7, e.g. in GitHub Actions, sets
// one that breaks 5.1's modules).
const psEnv = { ...process.env };
delete psEnv.PSModulePath;

/**
 * Runs a PowerShell script and returns its output. Only a real error fails it: a query that
 * finds nothing (e.g. Get-Process … -ErrorAction SilentlyContinue) still succeeds.
 */
export function ps(script) {
  const wrapped = `$ErrorActionPreference = 'Stop'\ntry {\n${script}\n} catch { [Console]::Error.WriteLine($_); exit 1 }\nexit 0`;
  return execFileSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", wrapped], { encoding: "utf8", env: psEnv }).trim();
}

/** Presses a key combo system-wide, like a real keyboard (for global shortcuts). */
export function press(combo) {
  execFileSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(HERE, "keys.ps1"), combo], { env: psEnv });
}

/** Runs the `bt` command line of the test copy. */
export function bt(...args) {
  try {
    return { code: 0, out: execFileSync(cfg.bt, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }) };
  } catch (e) {
    return { code: e.status, out: (e.stdout || "") + (e.stderr || "") };
  }
}

let driverProc;
export async function start() {
  driverProc = spawn(cfg.tauriDriver, ["--native-driver", cfg.edgeDriver], { stdio: "ignore" });
  let last;
  for (let i = 0; i < 20; i++) {
    await sleep(500);
    try {
      const driver = await new Builder()
        .usingServer("http://127.0.0.1:4444/")
        .withCapabilities({ browserName: "wry", "tauri:options": { application: cfg.exe } })
        .build();
      await driver.manage().setTimeouts({ implicit: 0, script: 20000 });
      return driver;
    } catch (e) {
      last = e;
      if (!/ECONNREFUSED/.test(String(e))) break;
    }
  }
  driverProc.kill();
  throw new Error(`couldn't start the app under WebDriver: ${last?.message ?? last}`);
}

export async function stop(driver) {
  try {
    await driver?.quit();
  } catch {}
  driverProc?.kill();
}

/** Calls a backend command from inside the page (for what a native file dialog would ask). */
export async function invoke(driver, cmd, args = {}) {
  const r = await driver.executeAsyncScript(
    `const [cmd, args, done] = arguments;
     window.__TAURI_INTERNALS__.invoke(cmd, args).then(v => done({ ok: v }), e => done({ err: String(e) }));`,
    cmd,
    args,
  );
  if ("err" in r) throw new Error(`${cmd}: ${r.err}`);
  return r.ok;
}

/** Switches to the window whose URL contains `part`; false if there's none. */
export async function toWindow(driver, part, timeout = 5000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    for (const h of await driver.getAllWindowHandles()) {
      try {
        await driver.switchTo().window(h);
        if ((await driver.getCurrentUrl()).includes(part)) return true;
      } catch {}
    }
    await sleep(250);
  }
  return false;
}

export async function waitFor(fn, timeout = 5000, what = "condition") {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    try {
      const v = await fn();
      if (v) return v;
    } catch (e) {
      last = e;
    }
    await sleep(150);
  }
  throw new Error(`timed out waiting for ${what}${last ? `: ${last.message}` : ""}`);
}

export const $ = (driver, css) => driver.findElement(By.css(css));
export const $$ = (driver, css) => driver.findElements(By.css(css));
export const text = async (driver, css) => (await $(driver, css)).getText();
export const exists = async (driver, css) => (await $$(driver, css)).length > 0;

export async function buttonByText(driver, t, scope = "", exact = false) {
  for (const b of await $$(driver, `${scope} button`)) {
    try {
      const s = (await b.getText()).trim();
      if (exact ? s === t : s.includes(t)) return b;
    } catch {}
  }
  throw new Error(`no button "${t}"`);
}

/** Empties a React-controlled input (WebDriver's clear() doesn't tell React). */
export async function clearInput(el) {
  await el.sendKeys(Key.CONTROL, "a", Key.NULL, Key.BACK_SPACE);
}

const WIN32 = `
using System; using System.Text; using System.Runtime.InteropServices;
public static class BtWin {
  delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(P p, IntPtr l);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  static string Title(IntPtr h) { var s = new StringBuilder(256); GetWindowText(h, s, 256); return s.ToString(); }
  public static bool Visible(string t) {
    bool found = false;
    EnumWindows((h, l) => { if (Title(h) == t && IsWindowVisible(h) && !IsIconic(h)) found = true; return true; }, IntPtr.Zero);
    return found;
  }
  public static string Front() { return Title(GetForegroundWindow()); }
}`;
const win32 = (call) => ps(`Add-Type -TypeDefinition @'\n${WIN32}\n'@; ${call}`);

/** Is a window with exactly this title on screen (not hidden in the tray, not minimised)? */
export const windowVisible = (title) => win32(`[BtWin]::Visible('${title.replace(/'/g, "''")}')`) === "True";
/** Title of the window in front. */
export const foreground = () => win32("[BtWin]::Front()");

export function screenshot(file) {
  ps(
    `Add-Type -AssemblyName System.Windows.Forms,System.Drawing; $b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds; ` +
      `$i=New-Object System.Drawing.Bitmap $b.Width,$b.Height; $g=[System.Drawing.Graphics]::FromImage($i); ` +
      `$g.CopyFromScreen($b.Location,[System.Drawing.Point]::Empty,$b.Size); $i.Save('${file}')`,
  );
}

/** Draws `text` into a PNG (for screenshot/OCR and inbox tests). */
export function makeImage(file, text, px = 19) {
  ps(
    `Add-Type -AssemblyName System.Drawing; $b=New-Object System.Drawing.Bitmap 900,200; $g=[System.Drawing.Graphics]::FromImage($b); ` +
      `$g.Clear([System.Drawing.Color]::White); $g.TextRenderingHint='AntiAlias'; ` +
      `$g.DrawString('${text}', (New-Object System.Drawing.Font('Segoe UI', ${px}, [System.Drawing.GraphicsUnit]::Pixel)), [System.Drawing.Brushes]::Black, 20, 80); ` +
      `$b.Save('${file}'); $g.Dispose(); $b.Dispose()`,
  );
}

// ---------- a tiny test runner ----------

export const results = [];

/**
 * Runs one test. `desktop: true` marks tests that need a real, interactive desktop
 * (system-wide shortcuts, other apps' windows, the clipboard, Windows OCR); they're skipped with --ci.
 */
export async function test(name, fn, { desktop = false } = {}) {
  if (desktop && cfg.ci) {
    results.push({ name, skipped: true });
    console.log(`  – ${name} (skipped: needs a desktop)`);
    return;
  }
  const t0 = Date.now();
  try {
    await fn();
    results.push({ name, ok: true, ms: Date.now() - t0 });
    console.log(`  ✓ ${name}`);
  } catch (e) {
    const n = results.length + 1;
    results.push({ name, ok: false, err: e.message.split("\n")[0] });
    try {
      screenshot(path.join(cfg.out, `fail-${String(n).padStart(2, "0")}.png`));
    } catch {}
    console.log(`  ✗ ${name}\n      ${e.message.split("\n").slice(0, 3).join("\n      ")}`);
  }
}

export function summary() {
  const ran = results.filter((r) => !r.skipped);
  const bad = ran.filter((r) => !r.ok);
  const skipped = results.length - ran.length;
  console.log(`\n${ran.length - bad.length}/${ran.length} passed${skipped ? `, ${skipped} skipped` : ""}`);
  fs.writeFileSync(path.join(cfg.out, "results.json"), JSON.stringify(results, null, 2));
  return bad.length;
}
