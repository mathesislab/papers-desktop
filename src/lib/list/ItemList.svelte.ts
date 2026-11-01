// The list of the Feeds tab or the Search tab, and its item in focus (ItemPane draws them): the rows, the selection,
// the item read in place of the list or its details beside it (the window's switch, `Tab.beside`), its abstract
// looked up and its free PDF asked, the triage shown at once, what opens and adds items, the keys that move through
// the list, what was just done said for a moment. A tab makes one as it starts: its effects are the tab's, and the
// window's context (`tabContext`) its own.

import { ask } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import * as api from "$lib/feeds/api";
import type { FeedDetail, FeedRow, Store, Triage } from "$lib/feeds/api";
import { addTarget, pageOf } from "$lib/feeds/items";
import { isPost } from "$lib/feeds/kinds";
import { mac, matches, type Shortcut } from "$lib/keys";
import { OPEN_ASK } from "$lib/library/selection";
import { Selection } from "./selection.svelte";
import { tabContext, type Tab } from "./tab";

const ALL: Shortcut = { key: "a", mod: true };
/** how long an item is shown before its abstract is looked up: j held down does not ask for each */
const LOOKUP_DELAY = 300;

/** A key typed in a field. */
export const typing = (e: KeyboardEvent) =>
  e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement;

export interface ListOptions {
  /** the folder of feeds the items are of */
  store: Store;
  /** the query whose words the details mark */
  query: () => string;
  /** the list again, after a change refused: the backend tells of those made (`feeds-changed`), which ask for it */
  reload: () => unknown;
  /** the pages of items opened in the browser */
  opened?: (ids: string[]) => void;
}

export class ItemList {
  rows = $state<FeedRow[]>([]);
  readonly selection = new Selection();
  /** the item in focus read in place of the list */
  reading = $state(false);
  detail = $state<FeedDetail | null>(null);
  /** the details are asked again: the items changed */
  version = $state(0);
  /** what was just done, said for a moment */
  notice = $state<string | null>(null);
  /** the tab's search field */
  input = $state<HTMLInputElement>();
  /** the items triaged here since the list was last let go: it keeps them in place (`mergeRows`) */
  readonly sticky = new Set<string>();

  byId = $derived(new Map(this.rows.map((r) => [r.id, r])));
  /** the row of the item in focus, pinned over the item read */
  focusRow = $derived(this.selection.focus === null ? undefined : this.byId.get(this.selection.focus));

  readonly #options: ListOptions;
  readonly #tab: Tab;
  #noticeTimer: ReturnType<typeof setTimeout> | undefined;
  /** the items whose abstract was looked up here, once each */
  readonly #lookedUp = new Set<string>();

  constructor(options: ListOptions) {
    this.#options = options;
    this.#tab = tabContext();

    // the details of the item in focus, marked by the query; again when the items change
    $effect(() => {
      const id = this.selection.focus;
      const query = options.query();
      this.version;
      if (!id) {
        this.detail = null;
        return;
      }
      let live = true;
      api
        .detail(id, query, options.store)
        .then((d) => live && (this.detail = d))
        .catch(() => live && (this.detail = null));
      return () => {
        live = false;
      };
    });

    // the abstract of the item shown, looked up if its lookups may still find it (the items then change)
    $effect(() => {
      const id = this.detail?.abstractPending ? this.detail.id : null;
      if (!id || this.#lookedUp.has(id)) return;
      const timer = setTimeout(() => {
        this.#lookedUp.add(id);
        api.lookUp(id, options.store).catch((e) => this.say(String(e)));
      }, LOOKUP_DELAY);
      return () => clearTimeout(timer);
    });
  }

  /** the details beside the list, else in place of it */
  get beside(): boolean {
    return this.#tab.beside;
  }

  /** the folder of feeds the items are of */
  get store(): Store {
    return this.#options.store;
  }

  /** the ids of the list, in order */
  get ids(): string[] {
    return this.rows.map((r) => r.id);
  }

  /** The items the keys and the menus act on (`Selection.targets`). */
  targets(): string[] {
    return this.selection.targets(this.ids);
  }

  /** The items the buttons of the details act on: the keys', else the item shown. */
  targetsOr(id: string): string[] {
    const ids = this.targets();
    return ids.length ? ids : [id];
  }

  /** The selection bar: the items selected, in the order of the list. */
  chosen(): string[] {
    return this.ids.filter((id) => this.selection.selected.has(id));
  }

  say = (text: string) => {
    this.notice = text;
    clearTimeout(this.#noticeTimer);
    this.#noticeTimer = setTimeout(() => (this.notice = null), 4000);
  };

  /** A click on a row: selected (`Selection.click`); alone, read in place of the list. */
  select = (id: string, e: MouseEvent) => {
    if (e.button !== 0) return;
    if (this.selection.click(id, e, this.ids) && !this.beside) this.reading = true;
  };

  /** An item to show, from the details (a post that covers it, a linked paper, another version): selected, in the
   *  list or not. */
  showItem = (id: string) => this.selection.only(id);

  /** ⌘F, /: the search field. */
  focusSearch() {
    this.input?.select();
  }

  /** The details of an item: those shown, else asked. */
  async detailOf(id: string): Promise<FeedDetail | null> {
    if (this.detail?.id === id) return this.detail;
    try {
      return await api.detail(id, "", this.#options.store);
    } catch {
      return null;
    }
  }

  /** A triage given to items, shown at once: the list keeps them in place until it is let go (`sticky`), but for
   *  those dismissed, which go, the next selected. The backend tells the tab of the change, which asks for the
   *  list again; refused, the list again here, as it is. */
  async setTo(ids: string[], triage: Triage) {
    if (!ids.length) return;
    const set = new Set(ids);
    if (triage === "dismissed") {
      this.selection.nextAfter(this.ids, set);
      for (const id of ids) this.sticky.delete(id);
      this.rows = this.rows.filter((r) => !set.has(r.id));
    } else {
      for (const id of ids) this.sticky.add(id);
      this.rows = this.rows.map((r) => (set.has(r.id) ? { ...r, triage } : r));
    }
    if (this.detail && set.has(this.detail.id)) this.detail = { ...this.detail, triage };
    try {
      await api.setTriage(ids, triage, this.#options.store);
    } catch (e) {
      this.say(String(e));
      this.#options.reload();
    }
  }

  /** o, a double-click: the page of each item in the browser (above 5, after asking). */
  async openItems(ids: string[]) {
    if (!ids.length) return;
    const question = `Open ${ids.length} pages in the browser?`;
    if (ids.length > OPEN_ASK && !(await ask(question, { title: "Open", okLabel: "Open", cancelLabel: "Cancel" }))) return;
    let none = 0;
    for (const id of ids) {
      const d = await this.detailOf(id);
      const url = d && pageOf(d);
      if (url) await openUrl(url).catch((e) => this.say(String(e)));
      else none++;
    }
    this.#options.opened?.(ids);
    if (none) this.say(`${none} without a link`);
  }

  /** a, Add: each item added in turn, by the Add sheet over the tab; a sheet closed stops the rest. */
  async addItems(ids: string[], pdf: boolean) {
    for (const id of ids) {
      const d = await this.detailOf(id);
      const target = d && addTarget(d);
      if (!target) {
        this.say(d && isPost(d.kind) ? "No paper linked to the post yet: F looks for it" : "No DOI or arXiv id to add it by");
        if (ids.length === 1) return;
        continue;
      }
      if (!(await this.add(target, pdf))) return;
    }
  }

  /** A paper added by its DOI (or arXiv id), the tab staying in front; its item, if it is one, becomes added.
   *  Whether it was added. */
  add = async (target: { id: string; item: string | null }, pdf: boolean): Promise<boolean> => {
    const said = await this.#tab.onadd({ id: target.id, pdf });
    if (said === null) return false;
    this.say(said);
    if (target.item) await this.setTo([target.item], "added");
    return true;
  };

  /** The keys that move through the list and read it, whether `e` was one: the arrows and the page keys move (⇧
   *  extends the selection with the arrows), in a field too; out of one, Home and End, j and k move, ⌘A selects
   *  all, Enter reads the item in place of the list, Esc goes back; / (⇧/ on some keyboards) goes to the search
   *  field, Esc leaves a field. */
  navKey(e: KeyboardEvent): boolean {
    const mod = mac ? e.metaKey : e.ctrlKey;
    const act = (action: () => unknown) => {
      e.preventDefault();
      action();
      return true;
    };
    const go = (to: number | "first" | "last", extend = e.shiftKey) => act(() => this.selection.move(this.ids, to, extend));
    if (e.key === "ArrowDown") return go(mod ? "last" : 1);
    if (e.key === "ArrowUp") return go(mod ? "first" : -1);
    if (e.key === "PageDown") return go(pageRows());
    if (e.key === "PageUp") return go(-pageRows());
    if (typing(e)) return e.key === "Escape" && act(() => (e.target as HTMLElement).blur());
    if (matches(e, ALL)) return act(() => (this.selection.selected = new Set(this.ids)));
    if (e.metaKey || e.ctrlKey || e.altKey) return false;
    if (e.key === "/") return act(() => this.focusSearch());
    if (e.shiftKey) return false;
    switch (e.key) {
      case "Enter":
        return !this.beside && this.selection.focus !== null && act(() => (this.reading = !this.reading));
      case "Escape":
        return this.reading && act(() => (this.reading = false));
      case "Home":
        return go("first");
      case "End":
        return go("last");
      case "j":
        return go(1, false);
      case "k":
        return go(-1, false);
    }
    return false;
  }
}

/** the rows a page key moves: about those in view */
const pageRows = () => Math.max(1, Math.floor((window.innerHeight - 160) / 26));
