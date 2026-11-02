// The colour of the bar at the top of the main window, to tell it apart among others (Mission Control, small
// windows): kept in the settings (src-tauri/src/toggle.rs), set in Settings → General and by a right-click on the
// bar; every window told (`EVENT`).

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type WindowColour = "blue" | "teal" | "green" | "yellow" | "orange" | "red" | "pink" | "purple" | "graphite" | "none";

/** The colours, in the order of the menus: their names and how they are drawn; none: no bar. */
export const WINDOW_COLOURS: { colour: WindowColour; name: string; css: string | null }[] = [
  { colour: "blue", name: "Blue", css: "#3b82f6" },
  { colour: "teal", name: "Teal", css: "#14b8a6" },
  { colour: "green", name: "Green", css: "#22c55e" },
  { colour: "yellow", name: "Yellow", css: "#eab308" },
  { colour: "orange", name: "Orange", css: "#f97316" },
  { colour: "red", name: "Red", css: "#ef4444" },
  { colour: "pink", name: "Pink", css: "#ec4899" },
  { colour: "purple", name: "Purple", css: "#8b5cf6" },
  { colour: "graphite", name: "Graphite", css: "#6b7280" },
  { colour: "none", name: "None", css: null },
];

/** How a colour is drawn; none: no bar. */
export const cssOf = (colour: WindowColour) => WINDOW_COLOURS.find((c) => c.colour === colour)?.css ?? null;

export const windowColour = () => invoke<WindowColour>("window_colour");
export const setWindowColour = (colour: WindowColour) => invoke<void>("set_window_colour", { colour });

/** The colour set, from any window; the end of the listening. */
export function onWindowColour(set: (colour: WindowColour) => void): () => void {
  const unlisten = listen<WindowColour>("window-colour", (e) => set(e.payload));
  return () => {
    unlisten.then((f) => f());
  };
}
