// What the main window (src/routes/main/+page.svelte) gives its Feeds and Search tabs alike, through the context:
// the switch of the tabs, the details beside the list or not, the panes' widths and whether they show, the
// library's Add sheet, a paper shown in the Library tab. The tabs and their ItemList read it as they start.

import { createContext, type Snippet } from "svelte";

export interface Tab {
  /** the switch of the tabs of the window, at the start of the toolbar */
  readonly tabs: Snippet;
  /** the details beside the list (the switch `feeds-beside`), else in place of it, to read */
  readonly beside: boolean;
  /** the widths of the side panes and whether they show: the window's layout, as in the library */
  sidebar: number;
  details: number;
  readonly showSidebar: boolean;
  readonly showDetails: boolean;
  /** the library's Add sheet, over the tab: a DOI (or arXiv id) to fetch, its free PDF looked for or not. The
   *  answer is what was added, said; none when the sheet was closed */
  onadd(initial: { id: string; pdf: boolean }): Promise<string | null>;
  /** a paper of the library shown, in the Library tab */
  onentry(id: string): void;
}

export const [tabContext, setTabContext] = createContext<Tab>();
