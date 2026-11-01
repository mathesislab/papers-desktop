// The settings of the feeds in the settings window (`feeds_settings`: those of crates/feeds' config.json, and the
// polling of the app's), as saved, shared by the tabs that set them: the Feeds tab (the rules, the fetching) and the
// Score tab (the interests, the title boost, the score's mode). Each saves its part over the settings as saved here,
// not over a copy of its own that the other tab changed since. The page makes one, as it starts.

import * as feeds from "$lib/feeds/api";
import type { FeedSettings } from "$lib/feeds/api";

export class FeedPrefs {
  /** none until loaded */
  saved = $state<FeedSettings | null>(null);

  /** The settings as saved. */
  async load() {
    this.saved = await feeds.settings();
  }

  /** Save a change over the settings as saved; refused (an interest that does not compile, its message thrown),
   *  nothing changes. */
  async save(change: Partial<FeedSettings>) {
    if (!this.saved) return;
    const next = { ...this.saved, ...change };
    await feeds.setSettings(next);
    this.saved = next;
  }
}
