// Tags compared as the library counts them (same_tag of the core): "AMOC" is
// "amoc".

/** A tag folded: without accents, in lower case. */
export const foldTag = (tag: string) => tag.normalize("NFKD").replace(/\p{M}/gu, "").toLowerCase();

/** Whether two tags are the same. */
export const sameTag = (a: string, b: string) => foldTag(a) === foldTag(b);
