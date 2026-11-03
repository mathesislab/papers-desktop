// The settings of the settings window (src/routes/settings): as saved, with what the window shows beside them (the
// bundled citation styles, whether the app starts at login, the home folder); a change saved at once (`save`), read
// again when another window changes them (`follow`); the line at the foot of the window that says it was saved, or why
// not (`notice`). The page makes one, as it starts; its tabs take it. The welcome window (src/routes/welcome) has one
// too.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Mirrors src-tauri/src/settings.rs and the SettingsView of lib.rs
export type Settings = {
  bibtex: string[];
  /** the folder of the main papers library (~ for the home folder): where a new .bib goes by default; kept once
   *  set. None: papers has no default library, and none was made or chosen */
  papers_folder: string | null;
  /** the .bib files whose papers the library part of the score leaves out: those unticked under *Similar to* (the
   *  Score tab), as `bibtex` names them; a .bib added later counts */
  similar_skip: string[];
  hotkey: string;
  /** on Linux only */
  selection_hotkey?: string;
  matching: "word-start" | "inside-words";
  citation_style: string;
  check_updates: boolean;
  after_close: "library" | "next";
  /** the minutes after which a tab of the main window not used has its search and filters cleared; 0: never */
  clear_after_minutes: number;
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
  /** the user's services of the Search tab's *Search online*, after those built in: a name and an address with
   *  {query} or {scholar} in it (checked when saved) */
  search_services: { name: string; url: string }[];
  /** the services built in that *Search online* leaves out, by name (never Google Scholar) */
  search_services_hidden: string[];
  /** the background polling of the feeds: set in the Feeds tab (feeds_set_settings), kept as they are here */
  feeds_poll: boolean;
  feeds_poll_minutes: number;
};
export type View = { settings: Settings; styles: string[]; services: string[]; start_at_login: boolean; home: string | null };

/** mirrors library::ImportReport: what `library_import` did */
export type ImportReport = { added: number; duplicates: number; missingPdfs: number };

/** The name of a file, without its folder. */
export const fileName = (path: string) => path.split(/[\\/]/).pop()!;

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** What an import did, in a line: "Imported 12 entries into papers.bib, 3 already there, 1 PDF not found". */
export function imported(report: ImportReport, bib: string): string {
  const said = [`Imported ${count(report.added, "entry", "entries")} into ${fileName(bib)}`];
  if (report.duplicates) said.push(`${report.duplicates} already there`);
  if (report.missingPdfs) said.push(`${count(report.missingPdfs, "PDF", "PDFs")} not found`);
  return said.join(", ");
}

export class Preferences {
  /** none until loaded */
  settings = $state<Settings | null>(null);
  /** the citation styles bundled with the app */
  styles = $state<string[]>([]);
  /** the services of *Search online* built in, Google Scholar first */
  services = $state<string[]>([]);
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
    this.services = view.services;
    this.startAtLogin = view.start_at_login;
    this.home = view.home;
  }

  /** Read the settings again whenever a window, or the app, changes them (`settings`): a save of this window
   *  starts from them, not to undo the other change. To stop, the function the promise gives. */
  follow() {
    return listen("settings", () => this.load());
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

  /** A path with ~ for the home folder. */
  tilde(path: string): string {
    return this.home && path.startsWith(this.home + "/") ? "~" + path.slice(this.home.length) : path;
  }

  /** A path with the home folder for ~, as the commands take it. */
  expand(path: string): string {
    return this.home && path.startsWith("~/") ? this.home + path.slice(1) : path;
  }

  /** Say something at the foot of the window: a failure, or a report to read (`kept`), until the next. */
  show = (text: string, error: boolean, kept = false) => {
    this.notice = { text, error };
    clearTimeout(this.#timer);
    if (!error && !kept) this.#timer = setTimeout(() => (this.notice = null), 1500);
  };
}
