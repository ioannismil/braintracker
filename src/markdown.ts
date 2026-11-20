import type { MouseEvent } from "react";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { prefsApi } from "./api";
import { findBlanks, fillBlanks, PLACEHOLDER, type Blank } from "./blanks";
import { resolveImages } from "./images";

marked.setOptions({ gfm: true, breaks: true });

const COPY_LABEL = "Copy";

// ---------- offline mode ----------

/**
 * In offline mode, web images in notes aren't loaded (nothing connects to the internet).
 * Rendered notes never contain a web image's address as `src`: it's held in `data-remote`
 * and only put in once this window knows offline mode is off, so there's no window where a
 * request could slip out before the setting has loaded.
 */
let offline: boolean | null = null;
const waiting = new Set<HTMLImageElement>();

function settleImage(img: HTMLImageElement) {
  const src = img.dataset.remote;
  if (!src) return;
  if (offline === null) {
    waiting.add(img);
  } else if (offline) {
    const host = src.replace(/^https?:\/\//i, "").split("/")[0];
    const note = el("span", "blocked-img", `🌐 Web image from ${host} not loaded (offline mode)`);
    note.title = "Offline mode is on. Turn it off in Settings to load web images.";
    img.replaceWith(note);
  } else {
    img.removeAttribute("data-remote");
    img.src = src;
  }
}

function setOffline(v: boolean) {
  offline = v;
  waiting.forEach(settleImage);
  waiting.clear();
}

prefsApi.getOffline().then(setOffline).catch(() => setOffline(false));
listen<boolean>("offline-changed", (e) => setOffline(e.payload)).catch(() => {});
new MutationObserver((changes) => {
  for (const c of changes)
    c.addedNodes.forEach((n) => {
      if (!(n instanceof Element)) return;
      if (n instanceof HTMLImageElement) settleImage(n);
      n.querySelectorAll<HTMLImageElement>("img[data-remote]").forEach(settleImage);
    });
}).observe(document.documentElement, { childList: true, subtree: true });

const REMOTE_IMG = /<img\b[^>]*>/gi;

/** Markdown → sanitized HTML, with a copy button on every code block. */
export function renderMarkdown(body: string): string {
  const clean = DOMPurify.sanitize(
    marked.parse(resolveImages(body) || "*Nothing written yet.*", { async: false }) as string,
  ).replace(REMOTE_IMG, (tag) => tag.replace(/\bsrc="(https?:[^"]*)"/i, 'data-remote="$1"'));
  // Added after sanitizing: our own markup only.
  return clean
    .replace(/<pre>([\s\S]*?)<\/pre>/g, (_m, inner: string) => {
      const marked = inner.replace(
        PLACEHOLDER,
        (whole) => `<span class="ph" title="Filled in when you copy">${whole}</span>`,
      );
      return `<pre>${marked}</pre>`;
    })
    .replace(/<pre>/g, `<div class="code-wrap"><button type="button" class="copy-btn" title="Copy to clipboard">${COPY_LABEL}</button><pre>`)
    .replace(/<\/pre>/g, "</pre></div>");
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
  }
}

function flashCopied(btn: HTMLElement) {
  btn.textContent = "Copied ✓";
  btn.classList.add("done");
  setTimeout(() => {
    btn.textContent = COPY_LABEL;
    btn.classList.remove("done");
  }, 1500);
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string) {
  const e = document.createElement(tag);
  if (className) e.className = className;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** Shows the fill-in form under a code block; copying fills the blanks in. */
async function askForBlanks(wrap: HTMLElement, btn: HTMLElement, code: string, blanks: Blank[]) {
  const existing = wrap.nextElementSibling;
  if (existing?.classList.contains("ph-form")) {
    existing.remove(); // clicking Copy again closes it
    return;
  }
  const cfg = await prefsApi.getPlaceholders().catch(() => null);
  const saved = cfg?.values ?? {};

  const form = el("form", "ph-form");
  form.appendChild(el("div", "ph-head", "Fill in before copying"));
  const inputs: HTMLInputElement[] = [];
  for (const b of blanks) {
    const label = el("label", "ph-field");
    label.appendChild(el("span", "ph-name", b.name));
    const input = el("input");
    input.name = b.name;
    input.spellcheck = false;
    input.autocomplete = "off";
    input.value = saved[b.name] ?? "";
    input.placeholder = b.fallback || b.name;
    label.appendChild(input);
    form.appendChild(label);
    inputs.push(input);
  }
  const preview = el("code", "ph-preview");
  form.appendChild(preview);
  const actions = el("div", "ph-actions");
  const raw = el("button", "tool", "Copy as is");
  raw.type = "button";
  const cancel = el("button", "tool", "Cancel");
  cancel.type = "button";
  const submit = el("button", "primary small", "Copy");
  submit.type = "submit";
  actions.append(cancel, raw, submit);
  form.appendChild(actions);

  const values = () => Object.fromEntries(inputs.map((i) => [i.name, i.value]));
  const update = () => (preview.textContent = fillBlanks(code, values()));
  update();
  const close = () => form.remove();

  form.addEventListener("input", update);
  // Typing here mustn't trigger the window's own keys (Enter, Esc, arrows, shortcuts).
  form.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  });
  form.addEventListener("dblclick", (e) => e.stopPropagation());
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const v = values();
    await copyText(fillBlanks(code, v));
    prefsApi.rememberPlaceholders(v).catch(() => {});
    close();
    flashCopied(btn);
  });
  raw.addEventListener("click", async () => {
    await copyText(code);
    close();
    flashCopied(btn);
  });
  cancel.addEventListener("click", close);

  wrap.after(form);
  (inputs.find((i) => !i.value) ?? inputs[0])?.focus();
}

/**
 * Click handler for rendered Markdown: copy buttons copy their code block (asking for any
 * {{blanks}} first), links open in the default browser instead of navigating the app.
 * Returns true if it handled the click.
 */
export function handleMarkdownClick(e: MouseEvent): boolean {
  const target = e.target as HTMLElement;
  if (target.closest(".ph-form")) return true; // the form handles its own clicks
  const btn = target.closest<HTMLButtonElement>(".copy-btn");
  if (btn) {
    e.preventDefault();
    const wrap = btn.parentElement as HTMLElement;
    const code = (wrap.querySelector("pre")?.innerText ?? "").replace(/\n$/, "");
    const blanks = findBlanks(code);
    if (!blanks.length) {
      copyText(code).then(() => flashCopied(btn));
      return true;
    }
    prefsApi
      .getPlaceholders()
      .catch(() => null)
      .then((cfg) => {
        if (cfg && !cfg.ask) copyText(code).then(() => flashCopied(btn));
        else askForBlanks(wrap, btn, code, blanks);
      });
    return true;
  }
  const a = target.closest("a");
  if (a) {
    e.preventDefault();
    const href = a.getAttribute("href") ?? "";
    if (/^(https?:|mailto:)/i.test(href)) openUrl(href);
    return true;
  }
  return false;
}

/** True for clicks on a copy button or inside the fill-in form (not "the note itself"). */
export function isInsideCopyButton(e: MouseEvent): boolean {
  return !!(e.target as HTMLElement).closest(".copy-btn, .ph-form");
}
