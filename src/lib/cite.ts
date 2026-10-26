// Citations of the entries in the CSL style of the settings, with citeproc-js.

import { invoke } from "@tauri-apps/api/core";
import CSL from "citeproc";

type Style = { name: string; style: string; locale: string };

const MAX_CACHED = 200;
const cache = new Map<string, string>();
/** the engine of the style, and the CSL-JSON items it reads */
let engine: Promise<{ name: string; engine: InstanceType<typeof CSL.Engine> }> | null = null;
const items = new Map<string, object>();

/** Read the style again, after the settings changed. */
export function resetStyle() {
  engine = null;
  cache.clear();
}

export function cached(key: string): string | undefined {
  return cache.get(key);
}

/** The name of the style, once a citation was made. */
export async function styleName(): Promise<string> {
  return (await load()).name;
}

function load() {
  engine ??= invoke<Style>("citation_style").then((s) => ({
    name: s.name,
    engine: new CSL.Engine(
      { retrieveLocale: () => s.locale, retrieveItem: (id) => items.get(id)! },
      s.style,
    ),
  }));
  // a failed style is tried again next time
  engine.catch(() => (engine = null));
  return engine;
}

/** The bibliography entry of `key`, as HTML (citeproc-js escapes the fields). */
export async function citation(key: string): Promise<string> {
  const hit = cache.get(key);
  if (hit !== undefined) return hit;
  const [{ engine: e }, item] = await Promise.all([load(), invoke<object>("csl", { key })]);
  items.set(key, item);
  e.updateItems([key]);
  const bibliography = e.makeBibliography();
  const html = bibliography ? bibliography[1].join("").trim() : "";
  cache.set(key, html);
  if (cache.size > MAX_CACHED) cache.delete(cache.keys().next().value!);
  return html;
}
