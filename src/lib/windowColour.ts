// The colour of the bar at the top of the main window, to tell it apart among others (Mission Control, small
// windows): kept in the settings (src-tauri/src/toggle.rs), set in Settings → General and by a right-click on the
// bar; every window told (`EVENT`).

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type WindowColour = "blue" | "teal" | "green" | "yellow" | "orange" | "red" | "pink" | "purple" | "graphite" | "none";

/** The colours, in the order of the menus: their names and how they are drawn, pastel (a bar to tell the window
 *  apart, not to catch the eye); none: no bar. */
export const WINDOW_COLOURS: { colour: WindowColour; name: string; css: string | null }[] = [
  { colour: "blue", name: "Blue", css: "#a8c8f0" },
  { colour: "teal", name: "Teal", css: "#9fdcd3" },
  { colour: "green", name: "Green", css: "#b5e0b0" },
  { colour: "yellow", name: "Yellow", css: "#f3e19a" },
  { colour: "orange", name: "Orange", css: "#f7c79a" },
  { colour: "red", name: "Red", css: "#f2aaa6" },
  { colour: "pink", name: "Pink", css: "#f2b5d4" },
  { colour: "purple", name: "Purple", css: "#cdb9f0" },
  { colour: "graphite", name: "Graphite", css: "#c3c6cc" },
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
