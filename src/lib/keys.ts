// Keyboard shortcuts: ⌘ on macOS, Ctrl elsewhere.

export const mac = navigator.userAgent.includes("Mac");

/** `code`: the physical key, for one whose character changes with ⇧ (⇧] is }) */
export type Shortcut = { key: string; code?: string; mod?: boolean; shift?: boolean; alt?: boolean };

export function matches(e: KeyboardEvent, s: Shortcut): boolean {
  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  // ⌥ changes the character of a key on macOS (⌥C is ç): its letter is the code
  const key = s.code
    ? e.code === s.code
    : s.alt && s.key.length === 1
      ? e.code === `Key${s.key.toUpperCase()}`
      : e.key.toLowerCase() === s.key.toLowerCase();
  return key && mod === !!s.mod && e.shiftKey === !!s.shift && e.altKey === !!s.alt;
}

/** The step of the text size a key asks, ⌘+ or ⌘= (1), ⌘- (-1), ⌘0 (0: the default); none for another key. */
export function zoomStep(e: KeyboardEvent): number | null {
  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  if (!mod || e.altKey) return null;
  if (e.key === "=" || e.key === "+") return 1;
  if (e.key === "-" || e.key === "_") return -1;
  return e.key === "0" ? 0 : null;
}

const names: Record<string, string> = mac ? { Enter: "↵", Backspace: "⌫" } : {};

export function label(s: Shortcut): string {
  const key = names[s.key] ?? (s.key.length === 1 ? s.key.toUpperCase() : s.key);
  if (mac) return (s.mod ? "⌘" : "") + (s.alt ? "⌥" : "") + (s.shift ? "⇧" : "") + key;
  return [s.mod && "Ctrl", s.alt && "Alt", s.shift && "Shift", key].filter(Boolean).join("+");
}
