// The commands of the main window, and their keys: in the menu of the app on
// macOS (src-tauri/src/menu.rs), where they also work while the PDF view has
// the keys; caught by the page elsewhere.

import type { Shortcut } from "./keys";

export const commands: Record<string, Shortcut> = {
  close: { key: "w", mod: true },
  add: { key: "n", mod: true },
  find: { key: "f", mod: true },
  "zoom-in": { key: "=", mod: true },
  "zoom-out": { key: "-", mod: true },
  "zoom-fit": { key: "0", mod: true },
  sidebar: { key: "s", mod: true, alt: true },
  details: { key: "i", mod: true, alt: true },
  library: { key: "1", mod: true },
  feeds: { key: "2", mod: true },
  "show-next-tab": { key: "]", code: "BracketRight", mod: true, shift: true },
  "show-previous-tab": { key: "[", code: "BracketLeft", mod: true, shift: true },
  settings: { key: ",", mod: true },
  // the library's, except in a field, where they are the field's (the menu of macOS has them)
  undo: { key: "z", mod: true },
  redo: { key: "z", mod: true, shift: true },
};
