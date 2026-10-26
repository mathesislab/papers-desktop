// Keyboard shortcuts: ⌘ on macOS, Ctrl elsewhere.

export const mac = navigator.userAgent.includes("Mac");

export type Shortcut = { key: string; mod?: boolean; shift?: boolean };

export function matches(e: KeyboardEvent, s: Shortcut): boolean {
  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  return (
    e.key.toLowerCase() === s.key.toLowerCase() &&
    mod === !!s.mod &&
    e.shiftKey === !!s.shift &&
    !e.altKey
  );
}

const names: Record<string, string> = mac ? { Enter: "↵" } : {};

export function label(s: Shortcut): string {
  const key = names[s.key] ?? (s.key.length === 1 ? s.key.toUpperCase() : s.key);
  if (mac) return (s.mod ? "⌘" : "") + (s.shift ? "⇧" : "") + key;
  return [s.mod && "Ctrl", s.shift && "Shift", key].filter(Boolean).join("+");
}
