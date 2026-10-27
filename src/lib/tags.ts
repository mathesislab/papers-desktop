// Tags compared as the library counts them (same_tag of the core): "AMOC" is
// "amoc".

/** A tag folded: without accents, in lower case. */
export const foldTag = (tag: string) => tag.normalize("NFKD").replace(/\p{M}/gu, "").toLowerCase();

/** Whether two tags are the same. */
export const sameTag = (a: string, b: string) => foldTag(a) === foldTag(b);

// A tag with "/" is under the tags before it: "climate/amoc" is under "climate".

/** The parents of a tag, the top one first: "a/b/c" is under "a" and "a/b". */
export function parentsOf(tag: string): string[] {
  const parts = tag.split("/");
  return parts.slice(0, -1).map((_, i) => parts.slice(0, i + 1).join("/"));
}

/** The last part of a tag: "amoc" for "climate/amoc". */
export const lastSegment = (tag: string) => tag.slice(tag.lastIndexOf("/") + 1);

/** The part of a tag before its last segment, with its "/": "climate/" for "climate/amoc"; empty for a top tag. */
export const parentPart = (tag: string) => tag.slice(0, tag.length - lastSegment(tag).length);

/** Whether `tag` is strictly under `parent`: "climate/amoc" is under "climate", "climate" is not. */
export const isUnder = (tag: string, parent: string) => foldTag(tag).startsWith(`${foldTag(parent)}/`);

/** A tag after `old` is renamed to `name`: `old` itself and the tags under it move, the others stay as they are. */
export function movedTag(tag: string, old: string, name: string): string {
  if (sameTag(tag, old)) return name;
  if (!isUnder(tag, old)) return tag;
  return `${name}/${tag.split("/").slice(old.split("/").length).join("/")}`;
}
