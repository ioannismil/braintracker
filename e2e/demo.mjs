// Records the README media: screenshots + a short feature video of the real app.
//
//   npm run e2e -- --demo [--stop-app]       (needs your desktop for ~1 minute: don't touch it)
//
// Like the tests, it runs a portable copy with its own data (your notes are never touched),
// fills it with realistic example notes, then drives it like a person would while ffmpeg
// records the screen in front of a plain backdrop (e2e/backdrop.ps1), so nothing of your own
// desktop ends up in the recording. Needs ffmpeg on PATH.
//
// Writes docs/media/: *.png screenshots, demo.mp4, demo.gif (the one the README plays).
import fs from "node:fs";
import path from "node:path";
import { execFileSync, spawn } from "node:child_process";
import { $, HERE, Key, cfg, exists, invoke, ps, press, sleep, start, stop, toWindow, waitFor, windowVisible } from "./lib.mjs";

const MEDIA = path.resolve(HERE, "..", "docs", "media");
// Everything happens in this part of the primary screen (the app's windows open centred on it).
const REGION = { x: 680, y: 270, w: 1200, h: 840 };

// ---------- example notes ----------

const now = new Date();
const ago = (days, h = 10, m = 0) => {
  const d = new Date(now);
  d.setDate(d.getDate() - days);
  d.setHours(h, m, 0, 0);
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
};

const NOTES = [
  {
    file: "wsl-dns.md", date: ago(0, 9, 12), tags: "wsl, network", pin: true,
    title: "Fix WSL DNS after the VPN connects",
    body: `The VPN rewrites the Windows DNS servers and WSL keeps the old ones, so nothing resolves.

1. Stop WSL from regenerating the file:

\`\`\`
sudo tee /etc/wsl.conf <<'EOF'
[network]
generateResolvConf = false
EOF
\`\`\`

2. Point it at a resolver that always works:

\`\`\`
echo "nameserver 1.1.1.1" | sudo tee /etc/resolv.conf
\`\`\`

3. \`wsl --shutdown\` from PowerShell, then open WSL again.`,
  },
  {
    file: "ssh.md", date: ago(0, 11, 40), tags: "ssh, ops",
    title: "SSH into a staging box",
    body: `Keys are in 1Password under **Staging SSH**. The jump host is only needed from outside the office.

\`\`\`
ssh -J {{user:deploy}}@bastion.internal {{user:deploy}}@{{host}} -p {{port:22}}
\`\`\`

If it hangs at *Connecting*, the VPN is down.`,
  },
  {
    file: "docker.md", date: ago(1, 16, 5), tags: "docker",
    title: "Reset the Docker network when containers can't reach each other",
    body: `Happened after switching Wi-Fi. Compose services timed out talking to each other.

\`\`\`
docker compose down
docker network prune -f
docker compose up -d
\`\`\`

Restarting Docker Desktop alone did **not** fix it.`,
  },
  {
    file: "postgres.md", date: ago(1, 10, 30), tags: "postgres, sql",
    title: "Postgres: find and kill stuck queries",
    body: `\`\`\`sql
SELECT pid, now() - query_start AS running, query
FROM pg_stat_activity
WHERE state = 'active' AND now() - query_start > interval '5 minutes';

SELECT pg_terminate_backend(<pid>);
\`\`\``,
  },
  {
    file: "winupdate.md", date: ago(3, 14, 20), tags: "windows",
    title: "Windows Update keeps failing",
    body: `Fixed by resetting the update cache (run as admin):

\`\`\`
net stop wuauserv
ren C:\\Windows\\SoftwareDistribution SoftwareDistribution.old
net start wuauserv
\`\`\`

![](winupdate-error.png)`,
  },
  {
    file: "git.md", date: ago(4, 18, 0), tags: "git",
    title: "Git: undo the last commit but keep the changes",
    body: "```\ngit reset --soft HEAD~1\n```\n\nUse `--mixed` to also unstage them. Never on a pushed branch.",
  },
  {
    file: "cert.md", date: ago(9, 9, 0), tags: "nginx, tls",
    title: "Renew the Let's Encrypt certificate by hand",
    body: "The cron job failed because port 80 was blocked by the new firewall rule.\n\n```\nsudo certbot renew --dry-run\nsudo certbot renew && sudo systemctl reload nginx\n```",
  },
  {
    file: "npm.md", date: ago(12, 15, 30), tags: "node",
    title: "npm: EACCES on global install",
    body: "Don't use sudo. Give npm its own folder:\n\n```\nnpm config set prefix ~/.npm-global\n```\n\nThen add `~/.npm-global/bin` to PATH.",
  },
  {
    file: "printer.md", date: ago(16, 11, 0), tags: "office",
    title: "Second-floor printer",
    body: "Add it by IP, not by name: `10.0.4.21`. Driver: *Generic PCL 6*. Auto-discovery picks the wrong one.",
  },
  {
    file: "vscode.md", date: ago(27, 17, 45), tags: "vscode",
    title: "Speed up VS Code in the big monorepo",
    body: '```json\n"files.watcherExclude": {\n  "**/node_modules/**": true,\n  "**/dist/**": true\n}\n```',
  },
  {
    file: "keys.md", date: ago(34, 10, 15), tags: "security, checklist",
    title: "Rotating the API keys",
    body: "- [x] Create the new key in the dashboard\n- [x] Update the secret in CI\n- [ ] Revoke the old key after 24 h",
  },
];

/** A Windows-style error dialog, for the "text in screenshots is searchable" note. */
function errorShot(file) {
  ps(
    `Add-Type -AssemblyName System.Drawing; $b=New-Object System.Drawing.Bitmap 760,230; $g=[System.Drawing.Graphics]::FromImage($b); ` +
      `$g.SmoothingMode='AntiAlias'; $g.TextRenderingHint='ClearTypeGridFit'; $g.Clear([System.Drawing.Color]::White); ` +
      `$g.FillRectangle((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(243,243,243))),0,0,760,40); ` +
      `$f=New-Object System.Drawing.Font('Segoe UI',15,[System.Drawing.GraphicsUnit]::Pixel); ` +
      `$fb=New-Object System.Drawing.Font('Segoe UI Semibold',20,[System.Drawing.GraphicsUnit]::Pixel); ` +
      `$g.DrawString('Windows Update',$f,[System.Drawing.Brushes]::Black,16,10); ` +
      `$g.FillEllipse((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(196,43,28))),28,72,44,44); ` +
      `$g.DrawString('X',(New-Object System.Drawing.Font('Segoe UI Semibold',22,[System.Drawing.GraphicsUnit]::Pixel)),[System.Drawing.Brushes]::White,40,78); ` +
      `$g.DrawString('There were problems installing some updates',$fb,[System.Drawing.Brushes]::Black,92,70); ` +
      `$g.DrawString('2026-11 Cumulative Update for Windows 11 - Error 0x80070005',$f,[System.Drawing.Brushes]::Black,92,108); ` +
      `$g.DrawString('We will try again later.',$f,(New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(90,90,90))),92,134); ` +
      `$g.FillRectangle((New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(0,95,184))),580,176,160,34); ` +
      `$g.DrawString('Retry',$f,[System.Drawing.Brushes]::White,640,183); ` +
      `$b.Save('${file}'); $g.Dispose(); $b.Dispose()`,
  );
}

function writeNotes(dir) {
  fs.mkdirSync(dir, { recursive: true });
  errorShot(path.join(dir, "winupdate-error.png"));
  for (const n of NOTES)
    fs.writeFileSync(
      path.join(dir, n.file),
      `---\ntitle: ${n.title}\ntags: [${n.tags}]\ndate: ${n.date}\n---\n\n${n.body}\n`,
    );
}

// ---------- screen capture ----------

/** Saves part of the screen as a PNG. */
function grab(file, r) {
  ps(
    `Add-Type -AssemblyName System.Drawing; $i=New-Object System.Drawing.Bitmap ${r.w},${r.h}; $g=[System.Drawing.Graphics]::FromImage($i); ` +
      `$g.CopyFromScreen(${r.x},${r.y},0,0,(New-Object System.Drawing.Size ${r.w},${r.h})); $i.Save('${file}'); $g.Dispose(); $i.Dispose()`,
  );
}

/** Outer rectangle of the test copy's main window. */
function mainWindowRect() {
  const out = ps(
    `Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class BtRect {
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Ri, B; }
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out R r, int s);
}
'@; $p = Get-Process braintracker | ? { $_.Path -like '${cfg.work}*' -and $_.MainWindowHandle -ne 0 } | Select-Object -First 1; ` +
      `$r=New-Object BtRect+R; [void][BtRect]::DwmGetWindowAttribute($p.MainWindowHandle,9,[ref]$r,16); "$($p.MainWindowTitle)|$($r.L) $($r.T) $($r.Ri) $($r.B)"`,
  );
  const [title, rect] = out.split("|");
  const [l, t, r, b] = rect.split(" ").map(Number);
  if (!(r > l)) throw new Error(`main window not found (${out})`);
  return { title, x: l, y: t, w: r - l, h: b - t };
}

function startRecording(file) {
  const p = spawn(
    "ffmpeg",
    ["-y", "-loglevel", "error", "-f", "gdigrab", "-framerate", "30", "-draw_mouse", "0",
      "-offset_x", String(REGION.x), "-offset_y", String(REGION.y), "-video_size", `${REGION.w}x${REGION.h}`, "-i", "desktop",
      "-c:v", "libx264", "-preset", "ultrafast", "-crf", "10", "-pix_fmt", "yuv444p", file],
    { stdio: ["pipe", "ignore", "inherit"] },
  );
  return () =>
    new Promise((res) => {
      p.on("exit", res);
      p.stdin.write("q");
    });
}

/** The finished files for the README: a small MP4 and the looping GIF that plays on the page. */
function encode(raw) {
  const ff = (...a) => execFileSync("ffmpeg", ["-y", "-loglevel", "error", ...a], { stdio: "inherit" });
  ff("-i", raw, "-c:v", "libx264", "-crf", "20", "-preset", "slow", "-pix_fmt", "yuv420p", "-movflags", "+faststart", path.join(MEDIA, "demo.mp4"));
  const pal = path.join(cfg.work, "palette.png");
  const vf = "fps=15,scale=960:-1:flags=lanczos";
  ff("-i", raw, "-vf", `${vf},palettegen=stats_mode=diff:max_colors=200`, pal);
  ff("-i", raw, "-i", pal, "-lavfi", `${vf}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle`, "-loop", "0", path.join(MEDIA, "demo.gif"));
}

// ---------- the demo ----------

export async function run() {
  fs.mkdirSync(MEDIA, { recursive: true });
  const backdrop = spawn("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File", path.join(HERE, "backdrop.ps1")], { stdio: "ignore" });
  await sleep(1500);
  const d = await start();
  const keys = (...k) => d.actions().sendKeys(...k).perform();
  const typeSlow = async (s, ms = 60) => {
    for (const ch of s) {
      await keys(ch);
      await sleep(ms);
    }
  };
  const click = async (css) => {
    for (let i = 0; ; i++) {
      try {
        return await (await $(d, css)).click();
      } catch (e) {
        if (i > 4 || !/stale/i.test(e.name + e.message)) throw e;
        await sleep(200);
      }
    }
  };
  /** Types like a person, into one element (sendKeys per character keeps it in that field). */
  const typeInto = async (css, s, ms = 60) => {
    const el = await $(d, css);
    await el.click();
    for (const ch of s) {
      await el.sendKeys(ch);
      await sleep(ms);
    }
  };
  /** Screen rectangle of the current (popup) window. */
  const popupRect = async (drv) => {
    const r = await drv.manage().window().getRect();
    return { x: r.x, y: r.y, w: r.width, h: r.height };
  };
  const around = (r, m = 36) => ({ x: r.x - m, y: r.y - m, w: r.w + 2 * m, h: r.h + 2 * m });
  const closeSpotlight = async () => {
    for (let i = 0; i < 4 && windowVisible("BrainTracker — Quick search"); i++) {
      try {
        await (await $(d, ".spot-bar input")).sendKeys(Key.ESCAPE);
      } catch {
        break; // window hidden
      }
      await sleep(250);
    }
  };
  const search = async (q) => {
    const s = await $(d, ".search input");
    await s.click();
    await s.sendKeys(Key.CONTROL, "a", Key.NULL, Key.BACK_SPACE);
    await sleep(300);
    await typeSlow(q, 90);
  };
  const openFirst = async () => {
    const row = await $(d, ".list .row");
    const want = (await (await row.findElement({ css: ".row-title" })).getText()).replace(/^●\s*/, "").replace(/…$/, "");
    await row.click();
    await waitFor(async () => (await (await $(d, "input.title")).getAttribute("value")).startsWith(want.slice(0, 15)), 3000, "note to open");
    await waitFor(() => exists(d, "article.markdown"), 3000, "preview");
    await sleep(250);
  };
  let stopRec;
  try {
    await waitFor(() => exists(d, ".app"), 15000, "app to load");
    const main = await d.getWindowHandle();
    const info = await invoke(d, "app_info");
    if (!info.portable || !info.dataDir.toLowerCase().startsWith(cfg.appDir.toLowerCase())) throw new Error(`not isolated: ${info.dataDir}`);

    const notes = path.join(cfg.work, "notes");
    writeNotes(notes);
    await invoke(d, "import_markdown", { paths: [notes] });
    const list = await invoke(d, "list_entries", { query: null, tag: null });
    for (const n of NOTES.filter((n) => n.pin)) await invoke(d, "set_pinned", { id: list.find((e) => e.title === n.title).id, pinned: true });
    console.log(`Imported ${list.length} example notes`);
    // Text in the screenshot is read by Windows OCR in the background: wait until search finds it.
    const ocr = await waitFor(async () => (await invoke(d, "list_entries", { query: "0x80070005", tag: null })).length > 0, 40000, "OCR")
      .then(() => true, () => false);
    if (!ocr) console.log("  (OCR didn't read the screenshot: no Windows OCR language installed? Skipping that part.)");

    await d.navigate().refresh(); // show the imported notes
    await waitFor(() => exists(d, ".list .row"), 10000, "notes listed");
    // A local build calls itself 0.1.0, so the app offers an update; hide that banner for the
    // recording. (Not offline mode: that currently hides pasted screenshots too.)
    await d.executeScript(`const s = document.createElement("style"); s.textContent = ".update-banner{display:none!important}"; document.head.append(s);`);
    await openFirst(); // the pinned WSL note
    await sleep(800);
    const mainRect = mainWindowRect();
    console.log(`Main window at ${JSON.stringify(mainRect)}; recording ${JSON.stringify(REGION)}`);

    // ---------------- screenshots ----------------
    grab(path.join(MEDIA, "main.png"), mainRect);

    await search("dock");
    await sleep(700);
    grab(path.join(MEDIA, "search.png"), mainRect);

    await search("tag:ssh");
    await openFirst();
    await click("article.markdown .copy-btn");
    await waitFor(() => exists(d, ".ph-form"), 2000, "fill-in form");
    await (await $(d, '.ph-form input[name="host"]')).sendKeys("staging-02");
    await sleep(400);
    grab(path.join(MEDIA, "blanks.png"), mainRect);
    await keys(Key.ESCAPE);

    if (ocr) {
      await search("0x80070005");
      await openFirst();
      await sleep(800);
      grab(path.join(MEDIA, "ocr.png"), mainRect);
    }

    press("ctrl+shift+e");
    await toWindow(d, "capture.html");
    await waitFor(() => exists(d, ".capture-title"), 3000, "capture");
    await (await $(d, ".capture-title")).sendKeys("Bumped Node to 22 in CI");
    await (await $(d, ".capture-body-input")).sendKeys("Changed `node-version` in .github/workflows/ci.yml — the old 18 image was failing on the new lockfile.");
    await (await $(d, ".capture-tags input")).sendKeys("ci, node");
    await sleep(500);
    grab(path.join(MEDIA, "capture.png"), around(await popupRect(d)));
    await (await $(d, ".capture-title")).sendKeys(Key.CONTROL, Key.SHIFT, Key.ENTER, Key.NULL); // save & close
    await sleep(800);

    await d.switchTo().window(main);
    press("ctrl+alt+space");
    await toWindow(d, "spotlight.html");
    await waitFor(() => exists(d, ".spot-bar input"), 3000, "spotlight");
    await (await $(d, ".spot-bar input")).sendKeys("cert");
    await sleep(900);
    grab(path.join(MEDIA, "spotlight.png"), around(await popupRect(d)));
    await closeSpotlight();
    await d.switchTo().window(main);

    // ---------------- the video ----------------
    await search("");
    await (await $(d, ".search input")).sendKeys(Key.ESCAPE);
    await openFirst();
    await sleep(600);
    const raw = path.join(cfg.work, "raw.mkv");
    stopRec = startRecording(raw);
    await sleep(1800);

    // 1. search as you type
    await search("dock");
    await sleep(1400);
    await openFirst();
    await sleep(1800);

    // 2. commands with blanks
    await search("tag:ssh");
    await sleep(700);
    await openFirst();
    await sleep(900);
    await click("article.markdown .copy-btn");
    await waitFor(() => exists(d, ".ph-form"), 2000, "fill-in form");
    await sleep(500);
    await (await $(d, '.ph-form input[name="host"]')).click();
    await typeSlow("staging-02", 80);
    await sleep(900);
    await keys(Key.ENTER);
    await sleep(1400);

    // 3. text inside screenshots
    if (ocr) {
      await search("0x80070005");
      await sleep(600);
      await openFirst();
      await sleep(2200);
    }

    // 4. quick capture from anywhere
    press("ctrl+shift+e");
    await toWindow(d, "capture.html");
    await waitFor(() => exists(d, ".capture-title"), 3000, "capture");
    await sleep(500);
    await typeInto(".capture-title", "Clear the Gradle cache", 55);
    await typeInto(".capture-body-input", "rm -rf ~/.gradle/caches  (fixed the stuck build)", 35);
    await typeInto(".capture-tags input", "android", 60);
    await sleep(600);
    await (await $(d, ".capture-title")).sendKeys(Key.CONTROL, Key.SHIFT, Key.ENTER, Key.NULL);
    await sleep(900);

    // 5. quick search from anywhere
    await d.switchTo().window(main);
    press("ctrl+alt+space");
    await toWindow(d, "spotlight.html");
    await waitFor(() => exists(d, ".spot-bar input"), 3000, "spotlight");
    await sleep(400);
    await typeInto(".spot-bar input", "cert", 110);
    await sleep(1100);
    await (await $(d, ".spot-bar input")).sendKeys(Key.ENTER);
    await waitFor(() => exists(d, ".spot-note"), 3000, "note in quick search");
    await sleep(2600);
    await closeSpotlight();
    await sleep(400);

    // 6. back in the app: the new note is at the top
    await d.switchTo().window(main);
    await search("");
    await (await $(d, ".search input")).sendKeys(Key.ESCAPE);
    await sleep(500);
    await openFirst();
    await sleep(2200);

    await stopRec();
    stopRec = undefined;
    console.log("Encoding…");
    encode(raw);
    for (const f of fs.readdirSync(MEDIA)) console.log(`  docs/media/${f}  ${(fs.statSync(path.join(MEDIA, f)).size / 1e6).toFixed(2)} MB`);
    return 0;
  } catch (e) {
    console.error("demo:", e);
    return 1;
  } finally {
    if (stopRec) await stopRec();
    await stop(d);
    backdrop.kill();
    try { ps("Get-CimInstance Win32_Process -Filter \"Name='powershell.exe'\" | ? { $_.CommandLine -like '*backdrop.ps1*' } | % { Stop-Process -Id $_.ProcessId -Force }"); } catch {}
  }
}
