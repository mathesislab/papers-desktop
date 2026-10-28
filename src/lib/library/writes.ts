// Writes to the library, by the commands of src-tauri/src/writer.rs: a papers
// library is written by papers, any other .bib by the app. The library is
// read again after a write (a "status" event). A write is rejected with a
// WriteError.

import { invoke } from "@tauri-apps/api/core";
import type { AddOptions, Added, WriteError, Writer } from "$lib/types";

export function isWriteError(e: unknown): e is WriteError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e;
}

/** A rejection as a WriteError: any other error is "failed". */
export function writeError(e: unknown): WriteError {
  return isWriteError(e) ? e : { kind: "failed", message: String(e) };
}

/** What an error says to the user: a refused edit of an entry names what to use instead. */
export function errorText(e: WriteError): string {
  if (e.kind !== "kept") return e.message;
  switch (e.what) {
    case "key":
      return "The key can't be changed here.";
    case "tags":
      return "The tags can't be changed here: use the tags above.";
    case "file":
      return "The files can't be changed here.";
    default:
      return e.message;
  }
}

async function write<T>(command: string, args: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    throw writeError(e);
  }
}

/** The writer of each .bib file of the library. */
export function writers(): Promise<Writer[]> {
  return invoke("writers");
}

/** Add tags to items, and remove tags from them. */
export function setTags(ids: string[], add: string[], remove: string[] = []): Promise<void> {
  return write("set_tags", { ids, add, remove });
}

/** Rename a tag in the whole library. */
export function renameTag(old: string, name: string): Promise<void> {
  return write("rename_tag", { old, new: name });
}

/** Replace the BibTeX of an entry; its key, tags and files stay. */
export function replaceEntry(id: string, text: string): Promise<void> {
  return write("replace_entry", { id, text });
}

/** Add the entries of pasted BibTeX to a .bib file; resolves to their keys,
 *  added and existing (for papers: merged too). */
export function addBibtex(bib: string, text: string, options: AddOptions): Promise<Added> {
  return write("add_bibtex", { bib, text, options });
}

/** Remove entries; their files stay. */
export function removeEntries(ids: string[]): Promise<void> {
  return write("remove_entries", { ids });
}

/** Undo the last change of a .bib file. */
export function undo(bib: string): Promise<void> {
  return write("undo", { bib });
}

/** Redo the last change undone of a .bib file. */
export function redo(bib: string): Promise<void> {
  return write("redo", { bib });
}
