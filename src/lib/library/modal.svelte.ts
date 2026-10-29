// The modals drawn in the page (the Add sheet, the question before a delete, the notice of papers missing): a
// native view over the page (the PDFKit view of a paper, native.svelte.ts) lies above them, as above all of the
// web view, so the page tells the reader when one is open and it steps aside until the last has closed.
// A modal calls `onMount(holdModal)`.

const held = $state({ count: 0 });

/** Whether a modal of the page is open. */
export const modalOpen = () => held.count > 0;

/** A modal is open; returns what says it is closed. */
export function holdModal(): () => void {
  held.count++;
  return () => held.count--;
}
