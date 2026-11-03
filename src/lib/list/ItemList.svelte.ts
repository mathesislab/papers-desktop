// The list of the Feeds tab or the Search tab, and its item in focus (ItemPane draws them): the rows, the selection,
// the item read in place of the list or its details beside it (the window's switch, `Tab.beside`), its abstract
// looked up and its free PDF asked, the triage shown at once, what opens and adds items, the keys that move through
// the list, what was just done said for a moment. A tab makes one as it starts: its effects are the tab's, and the
// window's context (`tabContext`) its own.

import { MenuItem } from "@tauri-apps/api/menu";
import { untrack } from "svelte";
import { ask } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import * as api from "$lib/feeds/api";
import type { FeedDetail, FeedRow, Store, Triage } from "$lib/feeds/api";
import { titled, type Action, type Item } from "$lib/feeds/actions";
import { addTarget, pageOf, toggled, type TriageAction } from "$lib/feeds/items";
import { isPost } from "$lib/feeds/kinds";
import { mac, matches, type Shortcut } from "$lib/keys";
import { OPEN_ASK } from "$lib/library/selection";
import type { Look } from "./columns";
import { Selection } from "./selection.svelte";
import { tabContext, type Tab } from "./tab";
import { rowOf } from "./virtual.svelte";

const ALL: Shortcut = { key: "a", mod: true };
/** how long an item is shown before its abstract is looked up: j held down does not ask for each */
const LOOKUP_DELAY = 300;

/** An action as an item of a menu, on the items of those ids. */
export const menuItem = (action: Action, ids: string[], items: Item[]) =>
  MenuItem.new({ text: titled(action.label(items)), enabled: action.enabled(items), action: () => action.run(ids) });

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
  /** the items opened: their pages in the browser, their details (`showDetails`, the item read in place) */
  opened?: (ids: string[]) => void;
  /** how the list shows (the tab's choice, which the layout keeps); none: a table */
  look?: () => Look;
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

    // the item read in place of the list is opened, as the keys move through the list
    $effect(() => {
      const id = this.reading ? this.selection.focus : null;
      if (id !== null) untrack(() => options.opened?.([id]));
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

  /** how the list shows: simple, or a table */
  get look(): Look {
    return this.#options.look?.() ?? "table";
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

  /** A click on a row: selected (`Selection.click`). */
  select = (id: string, e: MouseEvent) => {
    if (e.button === 0) this.selection.click(id, e, this.ids);
  };

  /** A double-click, Enter: the details of the item in focus, beside the list (the pane shown) or in place of it. */
  showDetails = () => {
    const id = this.selection.focus;
    if (id === null) return;
    if (!this.beside) return (this.reading = true);
    this.#tab.showDetails = true;
    this.#options.opened?.([id]);
  };

  /** Space: the PDF of the item in focus, the library's in the Library tab if it is there, else its free copy in the
   *  browser. */
  async openPdf() {
    const id = this.selection.focus;
    if (id === null) return;
    const entry = this.byId.get(id)?.inLibrary[0];
    if (entry) return this.#tab.onpdf(entry);
    try {
      const pdf = await api.pdfOf(id, this.#options.store);
      if (pdf.url && pdf.access !== "closed") await openUrl(pdf.url);
      else this.say("No free PDF");
    } catch (e) {
      this.say(String(e));
    }
  }

  /** An item to show, from the details (a post that covers it, a linked paper, another version): selected, in the
   *  list or not. */
  showItem = (id: string) => this.selection.only(id);

  /** ⌘F, /: the search field. */
  focusSearch() {
    this.input?.select();
  }

  /** The items of some ids that the actions know: the details shown (they say its link), else the rows. */
  itemsOf(ids: string[]): Item[] {
    return ids.flatMap((id): Item[] => {
      const item = this.detail?.id === id ? this.detail : this.byId.get(id);
      return item ? [item] : [];
    });
  }

  /** The triage an action toggles on items (`toggled`), an item not in the list (a linked paper) asked for; none:
   *  nothing to change. */
  async toggled(action: TriageAction, ids: string[]): Promise<{ ids: string[]; triage: Triage } | null> {
    const found = await Promise.all(ids.map((id) => this.byId.get(id) ?? this.detailOf(id)));
    return toggled(
      found.filter((r): r is FeedRow | FeedDetail => !!r),
      action,
    );
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

  /** A triage given to items, shown at once: the list keeps them in place until it is let go (`sticky`), unless
   *  `keep` is off (a mark of many: they leave where they no longer belong when the list comes again), but for
   *  those removed, which go, the next selected. The backend tells the tab of the change, which asks for the
   *  list again; refused, the list again here, as it is. */
  async setTo(ids: string[], triage: Triage, keep = true) {
    if (!ids.length) return;
    const set = new Set(ids);
    if (triage === "removed") {
      this.selection.nextAfter(this.ids, set);
      for (const id of ids) this.sticky.delete(id);
      this.rows = this.rows.filter((r) => !set.has(r.id));
    } else {
      for (const id of ids) keep ? this.sticky.add(id) : this.sticky.delete(id);
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

  /** o: the page of each item in the browser (above 5, after asking). */
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
   *  all, Enter shows the details of the item (`showDetails`), Esc goes back from the item read (else selects
   *  nothing), Space opens its PDF;
   *  / (⇧/ on some keyboards) goes to the search field, Esc leaves a field. */
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
    const page = pageRows(rowOf(this.look));
    if (e.key === "PageDown") return go(page);
    if (e.key === "PageUp") return go(-page);
    if (typing(e)) return e.key === "Escape" && act(() => (e.target as HTMLElement).blur());
    if (matches(e, ALL)) return act(() => (this.selection.selected = new Set(this.ids)));
    if (e.metaKey || e.ctrlKey || e.altKey) return false;
    if (e.key === "/") return act(() => this.focusSearch());
    if (e.shiftKey) return false;
    switch (e.key) {
      case "Enter":
        return this.selection.focus !== null && act(this.showDetails);
      case " ":
        return this.selection.focus !== null && act(() => this.openPdf());
      case "Escape":
        if (this.reading) return act(() => (this.reading = false));
        return this.selection.selected.size > 0 && act(() => this.selection.only(null));
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

/** the rows a page key moves: about those in view, `row` px high */
const pageRows = (row: number) => Math.max(1, Math.floor((window.innerHeight - 160) / row));
