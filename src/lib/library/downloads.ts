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
  /** the papers of the library that link it, or a file with the same content; none: not in the library */
  matches: Match[];
};

/** a paper of the library that has the PDF: mirrors downloads::Match */
export type Match = {
  id: string;
  key: string;
  /** the file the paper links, relative to the folder of its .bib (the full path when elsewhere) */
  file: string;
  /** not the same file, but one with the same contents */
  same_contents: boolean;
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

/** What the tooltip of a PDF in the library says of it, a line per paper:
 *  "in the library: smith2020 — files/smith_2020.pdf", "in the library: jones2021 — same contents as jones.pdf". */
export function libraryNote(matches: Match[]): string {
  return matches
    .map((m) => `in the library: ${m.key} — ${m.same_contents ? "same contents as " : ""}${m.file}`)
    .join("\n");
}
