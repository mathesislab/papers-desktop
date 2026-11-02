// What the main window (src/routes/main/+page.svelte) gives its Feeds and Search tabs alike, through the context:
// the switch of the tabs, the details beside the list or not, the panes' widths and whether they show, the colours
// of the interests, the library's Add sheet, a paper shown in the Library tab and the .bib it is in. The tabs and their ItemList read it as they start.

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
  showDetails: boolean;
  /** the colours of the interests of the settings (`interestColours`): an interest's the same in both tabs */
  readonly colours: Map<string, string>;
  /** the library's Add sheet, over the tab: a DOI (or arXiv id) to fetch, its free PDF looked for or not. The
   *  answer is what was added, said; none when the sheet was closed */
  onadd(initial: { id: string; pdf: boolean }): Promise<string | null>;
  /** a paper of the library shown, in the Library tab */
  onentry(id: string): void;
  /** the PDF of a paper of the library opened, in the Library tab (its web page, without a PDF) */
  onpdf(id: string): void;
  /** the .bib file a paper of the library is in, by its name, when the library has several; none: it has one */
  libraryOf(id: string): string | null;
}

export const [tabContext, setTabContext] = createContext<Tab>();

/** The paper of the details in the library, to show there: the buttons say so (`ActionButtons.owned`) when they act
 *  on it alone. */
export function ownedOf(tab: Tab, d: { inLibrary: string[] }, ids: string[]): (() => void) | undefined {
  const entry = d.inLibrary[0];
  return entry && ids.length === 1 ? () => tab.onentry(entry) : undefined;
}
