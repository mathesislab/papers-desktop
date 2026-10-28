// The BibTeX of a paper being edited in the details: one at a time, kept here
// for the page (to ask before it is lost), the details (the editor) and the
// sidebar (a dot on the paper's row).

import { invoke } from "@tauri-apps/api/core";
import { errorText } from "$lib/library/writes";
import type { WriteError } from "$lib/types";

export class Editor {
  /** the paper edited; none: no edit */
  id = $state<string | null>(null);
  /** the text in the editor */
  text = $state("");
  /** the text it started from */
  original = $state("");
  /** why the last save was refused, said under the editor */
  error = $state<string | null>(null);
  saving = $state(false);

  /** whether the text is not the one the edit started from */
  get changed(): boolean {
    return this.id !== null && this.text !== this.original;
  }

  /** Start to edit the BibTeX of a paper, as written in its file. */
  async start(id: string) {
    const text = await invoke<string>("bibtex", { id, raw: true });
    this.text = this.original = text;
    this.error = null;
    this.id = id;
  }

  /** Leave the edit, its text lost. */
  stop() {
    this.id = null;
    this.error = null;
  }

  /** A save refused: the editor stays with its text. */
  fail(error: WriteError) {
    this.error = errorText(error);
  }
}
