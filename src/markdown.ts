import type { MouseEvent } from "react";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { openUrl } from "@tauri-apps/plugin-opener";
import { resolveImages } from "./images";

marked.setOptions({ gfm: true, breaks: true });

const COPY_LABEL = "Copy";

/** Markdown → sanitized HTML, with a copy button on every code block. */
export function renderMarkdown(body: string): string {
  const clean = DOMPurify.sanitize(marked.parse(resolveImages(body) || "*Nothing written yet.*", { async: false }) as string);
  // Added after sanitizing: our own markup only.
  return clean
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

/**
 * Click handler for rendered Markdown: copy buttons copy their code block,
 * links open in the default browser instead of navigating the app.
 * Returns true if it handled the click.
 */
export function handleMarkdownClick(e: MouseEvent): boolean {
  const target = e.target as HTMLElement;
  const btn = target.closest<HTMLButtonElement>(".copy-btn");
  if (btn) {
    e.preventDefault();
    const code = btn.parentElement?.querySelector("pre")?.innerText ?? "";
    copyText(code.replace(/\n$/, "")).then(() => {
      btn.textContent = "Copied ✓";
      btn.classList.add("done");
      setTimeout(() => {
        btn.textContent = COPY_LABEL;
        btn.classList.remove("done");
      }, 1500);
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

export function isInsideCopyButton(e: MouseEvent): boolean {
  return !!(e.target as HTMLElement).closest(".copy-btn");
}
