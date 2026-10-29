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
