// Keyboard shortcuts: ⌘ on macOS, Ctrl elsewhere.

export const mac = navigator.userAgent.includes("Mac");

export type Shortcut = { key: string; mod?: boolean; shift?: boolean; alt?: boolean };

export function matches(e: KeyboardEvent, s: Shortcut): boolean {
  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  // ⌥ changes the character of a key on macOS (⌥C is ç): its letter is the code
  const key =
    s.alt && s.key.length === 1
      ? e.code === `Key${s.key.toUpperCase()}`
      : e.key.toLowerCase() === s.key.toLowerCase();
  return key && mod === !!s.mod && e.shiftKey === !!s.shift && e.altKey === !!s.alt;
}

const names: Record<string, string> = mac ? { Enter: "↵" } : {};

export function label(s: Shortcut): string {
  const key = names[s.key] ?? (s.key.length === 1 ? s.key.toUpperCase() : s.key);
  if (mac) return (s.mod ? "⌘" : "") + (s.alt ? "⌥" : "") + (s.shift ? "⇧" : "") + key;
  return [s.mod && "Ctrl", s.alt && "Alt", s.shift && "Shift", key].filter(Boolean).join("+");
}
