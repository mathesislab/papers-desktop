// First-page thumbnails of the PDFs, rendered with pdf.js and kept in memory.

import { invoke } from "@tauri-apps/api/core";
import { GlobalWorkerOptions, getDocument } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";

GlobalWorkerOptions.workerSrc = workerUrl;

const MAX_CACHED = 100;
const cache = new Map<string, string>();

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
    if (cache.size > MAX_CACHED) cache.delete(cache.keys().next().value!);
    return url;
  } finally {
    await task.destroy();
  }
}
