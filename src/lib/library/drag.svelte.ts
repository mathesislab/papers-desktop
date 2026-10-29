// Rows of the table dragged onto the "Open" section of the sidebar, which opens them as tabs. The drag is made of
// pointer events (the table starts it, the sidebar shows where it would drop), not of the drag and drop of the
// page, which the window's drop of PDFs (Tauri's drag-drop event) shares the page with.

/** the papers being dragged (none: no drag), where the pointer is, and whether it is over the Open section */
export const rowDrag = $state({ ids: [] as string[], x: 0, y: 0, over: false });

/** the attribute of what the rows can be dropped on */
export const DROP_OPEN = "data-drop-open";

/** a drag started when the pointer has moved this far from where it was pressed, px */
export const DRAG_DISTANCE = 5;

/** whether the point is over a place the rows can be dropped on */
export function overDrop(x: number, y: number): boolean {
  return !!document.elementFromPoint(x, y)?.closest(`[${DROP_OPEN}]`);
}

export function endDrag() {
  rowDrag.ids = [];
  rowDrag.over = false;
}

// A PDF of the Downloads panel dragged onto the library table (a new paper) or the empty preview of a paper (its
// PDF): the same pointer-event drag, and the same sheet as a PDF dropped on the window.

/** the PDF being dragged (none: no drag), its name, where the pointer is, and what it is over: the paper that
 *  would get it, or none for a new paper (`target` is null when it is over no place to drop) */
export const pdfDrag = $state({
  path: null as string | null,
  name: "",
  x: 0,
  y: 0,
  target: null as { id: string | null } | null,
});

/** the attribute of what takes a PDF as a new paper (the library table); a paper's empty preview has `data-drop-pdf` */
export const DROP_NEW = "data-drop-new";

/** what a PDF dropped at the point would be for: a paper (its empty preview), a new paper (the table), none */
export function pdfTarget(x: number, y: number): { id: string | null } | null {
  const under = document.elementFromPoint(x, y);
  const paper = under?.closest<HTMLElement>("[data-drop-pdf]");
  if (paper) return { id: paper.dataset.dropPdf ?? null };
  return under?.closest(`[${DROP_NEW}]`) ? { id: null } : null;
}

export function endPdfDrag() {
  pdfDrag.path = null;
  pdfDrag.target = null;
}
