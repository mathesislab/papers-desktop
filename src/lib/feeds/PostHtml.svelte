<script lang="ts">
  // A post's HTML as the feed gives it, made safe by the app (`safe_html`: no scripts, styles or frames), with its
  // images: a link opens in the browser, not in the window; an image's title (xkcd's joke) shows under it.

  import { openUrl } from "@tauri-apps/plugin-opener";

  let { html }: { html: string } = $props();

  let box = $state<HTMLDivElement>();

  // the title of each image, under it
  $effect(() => {
    html;
    for (const img of box?.querySelectorAll<HTMLImageElement>("img[title]") ?? []) {
      if (img.nextElementSibling?.classList.contains("caption")) continue;
      const caption = document.createElement("p");
      caption.className = "caption";
      caption.textContent = img.title;
      img.after(caption);
    }
  });

  function click(e: MouseEvent) {
    const link = (e.target as HTMLElement).closest("a");
    if (!link?.href) return;
    e.preventDefault();
    openUrl(link.href).catch(() => {});
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (the links keep their keys) -->
<div class="post" bind:this={box} onclick={click}>{@html html}</div>

<style>
  .post {
    line-height: 1.55;
    overflow-wrap: anywhere;
  }
  .post :global(img) {
    display: block;
    max-width: 100%;
    height: auto;
    margin: 6px 0;
  }
  .post :global(.caption) {
    margin: 0 0 8px;
    color: var(--muted);
    font-style: italic;
  }
  .post :global(p) {
    margin: 0 0 8px;
  }
  .post :global(a) {
    color: var(--accent);
  }
  .post :global(figure) {
    margin: 8px 0;
  }
  .post :global(blockquote) {
    margin: 8px 0;
    padding-left: 10px;
    border-left: 3px solid var(--line);
    color: var(--muted);
  }
</style>
