// The Downloads panel of the sidebar: how a PDF of the folder is described (size and date). Pure functions,
// tested by scripts/downloads.test.ts (npm test).

/** a PDF of the folder: mirrors downloads::Download of src-tauri */
export type Download = {
  path: string;
  name: string;
  /** bytes */
  size: number;
  /** when it came, ms since the epoch */
  added: number;
  /** a paper of the library links it, or has a file with the same content */
  in_library: boolean;
};

/** what `downloads_list` answers: mirrors downloads::DownloadsView */
export type DownloadsView = {
  shown: boolean;
  folder: string;
  error: string | null;
  files: Download[];
};

/** "512 B", "48 KB", "1.2 MB" */
export function formatSize(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1000;
  let unit = 0;
  while (value >= 999.5 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

const sameDay = (a: Date, b: Date) =>
  a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();

/** The time of today's files ("14:05"), the day of the others ("3 Oct"), with the year when it is not this one. */
export function formatDate(ms: number, now = Date.now(), locale?: string): string {
  const [date, today] = [new Date(ms), new Date(now)];
  if (sameDay(date, today)) return date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
  const year = date.getFullYear() === today.getFullYear() ? {} : { year: "numeric" as const };
  return date.toLocaleDateString(locale, { day: "numeric", month: "short", ...year });
}

/** The name of a folder: the last part of its path ("/Users/me/Downloads" → "Downloads"). */
export function folderName(path: string): string {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path;
}
