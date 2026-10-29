// The context menu of a row of the table: it acts on the papers selected when the row is one of them, else on
// that row. The page gives what each item does; which items apply is `available` (selection.ts).

import { CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";
import { mac } from "$lib/keys";
import { sameTag } from "$lib/tags";
import type { Row } from "$lib/types";
import { fileName } from "./filters";
import { available, targetsOf } from "./selection";

/** what the items do, on the ids of the papers (in the order of the table) */
export type RowActions = {
  open: (ids: string[]) => void;
  /** outside the app: in Preview on macOS, else the default app */
  external: (ids: string[]) => void;
  reveal: (ids: string[]) => void;
  doi: (ids: string[]) => void;
  copyKeys: (ids: string[]) => void;
  copyBibtex: (ids: string[]) => void;
  copyCitation: (ids: string[]) => void;
  edit: (id: string) => void;
  /** the tag on the papers, or off them */
  tag: (tag: string, ids: string[], on: boolean) => void;
  /** the tags of the papers, in the details */
  editTags: (ids: string[]) => void;
  addPdf: (id: string) => void;
  transfer: (ids: string[], bib: string, move: boolean) => void;
  exportBib: (ids: string[]) => void;
  remove: (ids: string[]) => void;
};

/** Show the menu at the pointer. `bibs` are the .bib files of the library, for Copy to and Move to. */
export async function showRowMenu(
  rows: Row[],
  bibs: string[],
  tags: string[],
  actions: RowActions,
): Promise<void> {
  const ids = rows.map((r) => r.id);
  const can = available(rows);
  const item = (text: string, action: () => void, enabled = true) => MenuItem.new({ text, action, enabled });
  const separator = () => PredefinedMenuItem.new({ item: "Separator" });

  const tagItems = await Promise.all(
    tags.map((tag) => {
      const on = rows.every((r) => r.tags.some((t) => sameTag(t, tag)));
      return CheckMenuItem.new({ text: tag, checked: on, action: () => actions.tag(tag, ids, !on) });
    }),
  );
  const tagMenu = await Submenu.new({
    text: "Tags",
    items: [...tagItems, ...(tagItems.length ? [await separator()] : []), await item("Edit tags…", () => actions.editTags(ids))],
  });

  const items = [
    await item("Open", () => actions.open(ids)),
    await item(mac ? "Open in Preview" : "Open in default app", () => actions.external(ids), can.pdf),
    await item(mac ? "Reveal in Finder" : "Show in folder", () => actions.reveal(ids), can.pdf),
    await item("Open DOI page", () => actions.doi(ids), can.doi),
    await separator(),
    await item(rows.length > 1 ? "Copy [@keys]" : "Copy [@key]", () => actions.copyKeys(ids)),
    await item("Copy BibTeX", () => actions.copyBibtex(ids)),
    await item("Copy citation", () => actions.copyCitation(ids)),
    await item("Edit BibTeX", () => actions.edit(ids[0]), can.one),
    await separator(),
    tagMenu,
    await item("Add PDF…", () => actions.addPdf(ids[0]), can.addPdf),
  ];
  const targets = targetsOf(rows, bibs);
  if (targets.length) {
    const to = (move: boolean) =>
      Promise.all(targets.map((bib) => item(fileName(bib), () => actions.transfer(ids, bib, move))));
    items.push(
      await separator(),
      await Submenu.new({ text: "Copy to", items: await to(false) }),
      await Submenu.new({ text: "Move to", items: await to(true) }),
    );
  }
  items.push(
    await separator(),
    await item("Export selection…", () => actions.exportBib(ids)),
    await separator(),
    await item("Delete…", () => actions.remove(ids)),
  );
  const menu = await Menu.new({ items });
  await menu.popup();
}
