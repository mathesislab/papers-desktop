// The switches of the main window (src-tauri/src/toggle.rs): its Feeds and Search tabs shown, an item's details
// beside the list (in the Feeds and Search tabs; in the Library tab). The settings keep them; the settings window and the Window menu set them, and every window is
// told (`EVENT`).

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Toggle = "feeds-tab" | "search-tab" | "feeds-beside" | "library-beside";
export type Toggles = Record<Toggle, boolean>;

const TOGGLES: Toggle[] = ["feeds-tab", "search-tab", "feeds-beside", "library-beside"];

/** Whether a switch is on. */
export const toggle = (toggle: Toggle) => invoke<boolean>("toggle", { toggle });
/** A switch set: the windows are told, and the menu follows. */
export const setToggle = (toggle: Toggle, on: boolean) => invoke<void>("set_toggle", { toggle, on });

/** Every switch, as it is. */
export async function toggles(): Promise<Toggles> {
  const on = await Promise.all(TOGGLES.map(toggle));
  return Object.fromEntries(TOGGLES.map((t, i) => [t, on[i]])) as Toggles;
}

/** A switch set, from any window: `set` told which and how; the end of the listening. */
export function onToggle(set: (toggle: Toggle, on: boolean) => void): () => void {
  const unlisten = listen<{ toggle: Toggle; on: boolean }>("toggle", (e) => set(e.payload.toggle, e.payload.on));
  return () => {
    unlisten.then((f) => f());
  };
}
