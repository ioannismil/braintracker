// End-to-end tests: drives the real, built BrainTracker app like a person would.
//
//   npm run e2e                 everything (needs your desktop: it presses real keys)
//   npm run e2e -- --ci         skip tests that need a desktop (system-wide shortcuts etc.)
//   npm run e2e -- --stop-app   quit your running BrainTracker first, start it again after
//   npm run e2e -- --exe <path> test another build (default: src-tauri/target/release)
//   npm run e2e -- --demo       record the README screenshots + video into docs/media (see demo.mjs)
//
// Needs: a release build (npx tauri build --no-bundle) and tauri-driver
// (cargo install tauri-driver --locked). The matching Edge WebDriver is downloaded by itself.
//
// Your notes are never touched: the app runs as a portable copy in a temporary folder with
// its own empty data. Settings that live outside that folder (start with Windows) are put back.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { cfg, HERE, ps, sleep } from "./lib.mjs";

const args = process.argv.slice(2);
const flag = (f) => args.includes(f);
const opt = (f) => (args.includes(f) ? args[args.indexOf(f) + 1] : undefined);
const fail = (msg) => {
  console.error(`e2e: ${msg}`);
  process.exit(2);
};

const ROOT = path.resolve(HERE, "..");
const release = opt("--exe") ? path.dirname(path.resolve(opt("--exe"))) : path.join(ROOT, "src-tauri", "target", "release");
for (const f of ["braintracker.exe", "bt.exe"])
  if (!fs.existsSync(path.join(release, f))) fail(`${path.join(release, f)} not found — build first: npx tauri build --no-bundle`);

cfg.ci = flag("--ci");
cfg.out = path.join(HERE, "out");
fs.rmSync(cfg.out, { recursive: true, force: true });
fs.mkdirSync(cfg.out, { recursive: true });

// ---------- drivers ----------

const onPath = (exe) => {
  try {
    return execFileSync("where.exe", [exe], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).split(/\r?\n/)[0].trim();
  } catch {
    return "";
  }
};
cfg.tauriDriver =
  process.env.TAURI_DRIVER ||
  onPath("tauri-driver.exe") ||
  [path.join(os.homedir(), ".cargo", "bin", "tauri-driver.exe")].find((p) => fs.existsSync(p)) ||
  fail("tauri-driver not found — install it: cargo install tauri-driver --locked");

/** Edge WebDriver must match the WebView2 version exactly; fetched once into e2e/.cache. */
function edgeDriver() {
  if (process.env.MSEDGEDRIVER) return process.env.MSEDGEDRIVER;
  const v = ps(
    `$k='SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'; ` +
      `@("HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}","HKLM:\\$k","HKCU:\\$k") | ` +
      `% { (Get-ItemProperty $_ -ErrorAction SilentlyContinue).pv } | ? { $_ } | Select-Object -First 1`,
  );
  if (!v) fail("WebView2 runtime not found");
  const dir = path.join(HERE, ".cache", `msedgedriver-${v}`);
  const exe = path.join(dir, "msedgedriver.exe");
  if (!fs.existsSync(exe)) {
    console.log(`Downloading Edge WebDriver ${v}…`);
    fs.mkdirSync(dir, { recursive: true });
    const zip = path.join(dir, "d.zip");
    ps(`$ProgressPreference='SilentlyContinue'; Invoke-WebRequest 'https://msedgedriver.microsoft.com/${v}/edgedriver_win64.zip' -OutFile '${zip}'`);
    execFileSync(path.join(process.env.SystemRoot || "C:\\Windows", "System32", "tar.exe"), ["-xf", zip, "-C", dir]); // Windows' own tar unpacks zips
    fs.unlinkSync(zip);
    if (!fs.existsSync(exe)) fail(`downloaded Edge WebDriver ${v}, but there's no msedgedriver.exe in it`);
  }
  return exe;
}
cfg.edgeDriver = edgeDriver();

// ---------- the portable test copy ----------

cfg.work = fs.mkdtempSync(path.join(os.tmpdir(), "braintracker-e2e-"));
cfg.appDir = path.join(cfg.work, "app");
fs.mkdirSync(cfg.appDir);
for (const f of ["braintracker.exe", "bt.exe"]) fs.copyFileSync(path.join(release, f), path.join(cfg.appDir, f));
fs.writeFileSync(path.join(cfg.appDir, "portable.txt"), "e2e test copy");
cfg.exe = path.join(cfg.appDir, "braintracker.exe");
cfg.bt = path.join(cfg.appDir, "bt.exe");

// Only one BrainTracker can run at a time (and the test copy must not talk to yours).
const running = ps(
  `Get-Process braintracker -ErrorAction SilentlyContinue | ? { $_.Path -and $_.Path -notlike '${cfg.work}*' } | % { $_.Path } | Select-Object -First 1`,
);
let restartApp = "";
if (running) {
  if (!flag("--stop-app")) fail(`BrainTracker is running (${running}). Quit it from the tray, or run with --stop-app.`);
  console.log(`Stopping your BrainTracker for the test (it's started again afterwards)…`);
  ps(`Get-Process braintracker -ErrorAction SilentlyContinue | Stop-Process -Force`);
  await sleep(1500);
  restartApp = running;
}

// "Start with Windows" is a registry value outside the test copy: remember it, put it back.
const RUN = "HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const autostart = ps(`(Get-ItemProperty '${RUN}' -ErrorAction SilentlyContinue).BrainTracker`);

function cleanUp() {
  try {
    ps(
      autostart
        ? `Set-ItemProperty '${RUN}' -Name BrainTracker -Value '${autostart.replace(/'/g, "''")}'`
        : `Remove-ItemProperty '${RUN}' -Name BrainTracker -ErrorAction SilentlyContinue`,
    );
  } catch (e) {
    console.error(`e2e: couldn't restore the start-with-Windows setting: ${e.message}`);
  }
  try {
    ps(`Get-Process braintracker, tauri-driver, msedgedriver -ErrorAction SilentlyContinue | ? { -not $_.Path -or $_.Path -like '${cfg.work}*' -or $_.Name -ne 'braintracker' } | Stop-Process -Force`);
  } catch {}
  if (restartApp) {
    try {
      ps(`Start-Process '${restartApp}' -ArgumentList '--minimized'`);
      console.log("Your BrainTracker was started again.");
    } catch (e) {
      console.error(`e2e: start BrainTracker again yourself (${e.message})`);
    }
  }
  try {
    fs.rmSync(cfg.work, { recursive: true, force: true });
  } catch {}
}
process.on("SIGINT", () => {
  cleanUp();
  process.exit(130);
});

console.log(`Testing ${release}${cfg.ci ? " (CI: desktop-only tests skipped)" : ""}\n`);
let failed = 1;
try {
  const { run } = await import(flag("--demo") ? "./demo.mjs" : "./suite.mjs"); // --demo: README media
  failed = await run();
} catch (e) {
  console.error("e2e:", e);
} finally {
  cleanUp();
}
if (failed) console.log(`\nScreenshots of failures: ${cfg.out}`);
process.exit(failed ? 1 : 0);
