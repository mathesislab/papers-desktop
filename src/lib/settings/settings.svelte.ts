// The settings of the settings window (src/routes/settings): as saved, with what the window shows beside them (the
// bundled citation styles, whether the app starts at login, the home folder); a change saved at once (`save`); the
// line at the foot of the window that says it was saved, or why not (`notice`). The page makes one, as it starts.

import { invoke } from "@tauri-apps/api/core";

// Mirrors src-tauri/src/settings.rs and the SettingsView of lib.rs
export type Settings = {
  bibtex: string[];
  hotkey: string;
  /** on Linux only */
  selection_hotkey?: string;
  matching: "word-start" | "inside-words";
  citation_style: string;
  check_updates: boolean;
  after_close: "library" | "next";
  /** the zoom of the windows: the text size */
  text_size: "smallest" | "smaller" | "default" | "larger" | "largest";
  /** what writes the papers libraries: papers-rs, built in, or a papers command */
  writer: "built-in" | "command";
  /** the papers command, for the writer "command"; empty: the one the login shell finds */
  papers: string;
  /** sent to doi.org and OpenAlex, which answer faster to programs that give one; empty: nothing is sent */
  email: string;
  /** a free key of OpenAlex, for more searches a day; empty: none. Sent to OpenAlex only */
  openalex_key: string;
  /** the tags pinned to the keys 1–9, set from the sidebar of the main window: kept as they are */
  pinned_tags: (string | null)[];
  /** the Downloads panel under the sidebar of the main window: the PDFs of a folder outside the library */
  downloads_panel: boolean;
  /** its folder (~ for the home folder) */
  downloads_folder: string;
  /** the user's services of the Search tab's *Search online*, after Google Scholar and Google: a name and an address
   *  with {query} or {scholar} in it (checked when saved) */
  search_services: { name: string; url: string }[];
  /** the background polling of the feeds: set in their section (feeds_set_settings), kept as they are here */
  feeds_poll: boolean;
  feeds_poll_minutes: number;
};
export type View = { settings: Settings; styles: string[]; start_at_login: boolean; home: string | null };

export class Preferences {
  /** none until loaded */
  settings = $state<Settings | null>(null);
  /** the citation styles bundled with the app */
  styles = $state<string[]>([]);
  startAtLogin = $state(false);
  home = $state<string | null>(null);
  /** what was just done, or why not: a success said for a moment, a failure until the next */
  notice = $state<{ text: string; error: boolean } | null>(null);
  #timer: ReturnType<typeof setTimeout> | undefined;

  /** The settings as saved, and what the window shows beside them. */
  async load() {
    const view = await invoke<View>("get_settings");
    this.settings = view.settings;
    this.styles = view.styles;
    this.startAtLogin = view.start_at_login;
    this.home = view.home;
  }

  /** Save a change, which applies at once; on failure, show the settings as they are. Whether it was saved. */
  save = async (change: Partial<Settings>): Promise<boolean> => {
    if (!this.settings) return false;
    const changed = { ...this.settings, ...change };
    try {
      await invoke("save_settings", { settings: changed });
      this.settings = changed;
      this.show("Saved", false);
      return true;
    } catch (err) {
      this.show(String(err), true);
      await this.load();
      return false;
    }
  };

  /** Say something at the foot of the window. */
  show = (text: string, error: boolean) => {
    this.notice = { text, error };
    clearTimeout(this.#timer);
    if (!error) this.#timer = setTimeout(() => (this.notice = null), 1500);
  };
}
