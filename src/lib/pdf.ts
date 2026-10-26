// First-page thumbnails of the PDFs, rendered with pdf.js and kept in memory.

import { invoke } from "@tauri-apps/api/core";
import { GlobalWorkerOptions, getDocument } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";

GlobalWorkerOptions.workerSrc = workerUrl;

const MAX_CACHED = 100;
const cache = new Map<string, string>();
/** the small first pages that dragged PDFs show */
const icons = new Map<string, string>();
const ICON_WIDTH = 64;

export function cached(id: string): string | undefined {
  return cache.get(id);
}

/** The first page of the PDF of item `id`, `width` CSS pixels wide, as an image URL. */
export async function thumbnail(id: string, width: number): Promise<string> {
  const hit = cache.get(id);
  if (hit) return hit;
  const data = await invoke<ArrayBuffer>("pdf", { id });
  const task = getDocument({ data: new Uint8Array(data) });
  try {
    const page = await (await task.promise).getPage(1);
    const scale = (width * devicePixelRatio) / page.getViewport({ scale: 1 }).width;
    const viewport = page.getViewport({ scale });
    const canvas = document.createElement("canvas");
    canvas.width = Math.ceil(viewport.width);
    canvas.height = Math.ceil(viewport.height);
    await page.render({ canvas, viewport }).promise;
    const url = canvas.toDataURL("image/jpeg", 0.85);
    cache.set(id, url);
    icons.set(id, icon(canvas));
    if (cache.size > MAX_CACHED) {
      const oldest = cache.keys().next().value!;
      cache.delete(oldest);
      icons.delete(oldest);
    }
    return url;
  } finally {
    await task.destroy();
  }
}

/** What a dragged PDF shows: its small first page once seen in the preview,
 *  else a blank page. A PNG data URL, as the drag wants. */
export function dragIcon(id: string): string {
  return icons.get(id) ?? (blank ??= icon(null));
}

let blank: string | undefined;

/** A page `ICON_WIDTH` pixels wide (A4), with a border: `page` or blank. */
function icon(page: HTMLCanvasElement | null): string {
  const canvas = document.createElement("canvas");
  canvas.width = ICON_WIDTH;
  const ratio = page ? page.height / page.width : Math.SQRT2;
  canvas.height = Math.round(ICON_WIDTH * ratio);
  const g = canvas.getContext("2d")!;
  g.fillStyle = "#fff";
  g.fillRect(0, 0, canvas.width, canvas.height);
  if (page) g.drawImage(page, 0, 0, canvas.width, canvas.height);
  g.strokeStyle = "#999";
  g.strokeRect(0.5, 0.5, canvas.width - 1, canvas.height - 1);
  return canvas.toDataURL("image/png");
}
