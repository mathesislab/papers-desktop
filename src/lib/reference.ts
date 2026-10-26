// A pasted reference, found by the entries closest to it rather than as a query.

/** Whether a pasted text is a reference: with a DOI, or six words or more. */
export function isReference(text: string): boolean {
  return /10\.\d{4,9}\/\S/.test(text) || text.trim().split(/\s+/).length >= 6;
}
