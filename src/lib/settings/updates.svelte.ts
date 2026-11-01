// The updates of the app, as the settings window shows them (the General tab): the version running, the one found
// and the download of its install, the one installed just before this start. The backend tells of an update found
// and of the progress of its download (`update`, `update-progress`). The page makes one, as it starts: the events
// are listened to while it is open.

import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** mirrors update::Available */
export type Update = { version: string };
/** mirrors update::Installed */
export type Installed = { version: string; from: string };

export class Updates {
  version = $state("");
  /** the update found; none: none, or not looked for */
  update = $state<Update | null>(null);
  installed = $state<Installed | null>(null);
  updating = $state<"idle" | "checking" | "current" | "installing" | "failed">("idle");
  error = $state("");
  progress = $state<{ done: number; total: number | null }>({ done: 0, total: null });

  /** the line under the version */
  status = $derived.by(() => {
    switch (this.updating) {
      case "checking":
        return "Looking for updates…";
      case "current":
        return "Papers Desktop is up to date.";
      case "failed":
        return this.error;
      case "installing":
        return this.progress.total
          ? `Downloading… ${Math.round((100 * this.progress.done) / this.progress.total)}%`
          : "Downloading…";
      default:
        return this.update ? `Version ${this.update.version} is available.` : "";
    }
  });

  constructor() {
    $effect(() => {
      const unlisten = [
        listen<Update | null>("update", (e) => (this.update = e.payload)),
        listen<[number, number | null]>("update-progress", (e) => {
          this.progress = { done: e.payload[0], total: e.payload[1] };
        }),
      ];
      return () => unlisten.forEach((u) => u.then((f) => f()));
    });
  }

  /** The version running, the update found already, the one installed before this start. */
  async load() {
    await Promise.allSettled([
      getVersion().then((v) => (this.version = v)),
      invoke<Update | null>("found_update").then((found) => (this.update = found)),
      invoke<Installed | null>("installed_update").then((done) => (this.installed = done)),
    ]);
  }

  check = async () => {
    this.updating = "checking";
    try {
      this.update = await invoke<Update | null>("check_update");
      this.updating = this.update ? "idle" : "current";
    } catch (err) {
      this.error = `Cannot look for updates: ${err}`;
      this.updating = "failed";
    }
  };

  /** Look for one, unless found, or being looked for or installed. */
  lookFor() {
    if (!this.update && this.updating !== "checking" && this.updating !== "installing") this.check();
  }

  /** Download and install the update; the app then restarts. */
  install = async () => {
    this.updating = "installing";
    try {
      await invoke("install_update");
    } catch (err) {
      this.error = `Cannot install the update: ${err}`;
      this.updating = "failed";
    }
  };
}
