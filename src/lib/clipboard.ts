// The clipboard: plain text, with HTML for the apps that paste it formatted.

import { invoke } from "@tauri-apps/api/core";

/** What to copy: the text, and the HTML of a formatted paste. */
export type Copy = { text: string; html?: string };

export function copy({ text, html }: Copy): Promise<void> {
  return invoke("copy_text", { text, html: html ?? null });
}

export function escape(text: string): string {
  return text.replace(
    /[&<>"]/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!,
  );
}

/** The text of some HTML, its white space collapsed. */
export function plain(html: string): string {
  const body = new DOMParser().parseFromString(html, "text/html").body;
  return (body.textContent ?? "").replace(/\s+/g, " ").trim();
}
