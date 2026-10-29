// Writes to the library, by the commands of src-tauri/src/writer.rs: a papers
// library is written by papers, any other .bib by the app. The library is
// read again after a write (a "status" event). A write is rejected with a
// WriteError.

import { invoke } from "@tauri-apps/api/core";
import type { AddOptions, Added, FilesMode, Transferred, WriteError, Writer } from "$lib/types";

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

/** Add a PDF to a paper of the library, after the changes of `text` to its entry (the key, tags
 *  and files stay); a plain .bib links the file where it is. */
export function attachPdf(id: string, text: string | null, pdf: string, files: FilesMode | null): Promise<void> {
  return write("attach_pdf", { id, text, pdf, files });
}

/** What is done by default with a PDF added to a paper of a .bib file: the `files` setting of a
 *  papers library; none for a plain .bib. */
export function defaultFiles(bib: string): Promise<FilesMode | null> {
  return invoke("default_files_mode", { bib });
}

/** Remove entries. Their files stay, except the `files` to delete (those of the entries that no other entry
 *  links to): in a papers library papers keeps them in its history (undo brings them back), from a plain .bib
 *  they go to the trash of the system. */
export function removeEntries(ids: string[], files: string[] = []): Promise<void> {
  return write("remove_entries", { ids, files });
}

/** Copy entries to another .bib file of the library, or with `move` move them: added by the writer of that
 *  file with their files (copied into a papers library, linked where they are in a plain .bib). What was
 *  added, and what it already had. */
export function transferEntries(ids: string[], bib: string, move: boolean): Promise<Transferred> {
  return write("transfer_entries", { ids, bib, remove: move });
}

/** Undo the last change of a .bib file. */
export function undo(bib: string): Promise<void> {
  return write("undo", { bib });
}

/** Redo the last change undone of a .bib file. */
export function redo(bib: string): Promise<void> {
  return write("redo", { bib });
}
