// The DOI of an arXiv paper, from its id or the link of its page, for the Add sheet and the Feeds tab. Pure,
// tested by scripts/arxiv.test.ts (npm test).

/** an arXiv id, new (2410.01234) or old (hep-th/9901001, math.AG/0309136), without its version */
const ID = String.raw`(\d{4}\.\d{4,5}|[a-z-]+(?:\.[A-Z]{2})?\/\d{7})(?:v\d+)?`;
const BARE = new RegExp(`^(?:arxiv:)?${ID}$`, "i");
/** the pages of a paper on arXiv and its mirrors: arxiv.org, ar5iv (arXiv's HTML), alphaXiv */
const PAGE = String.raw`(?:(?:www\.|export\.)?arxiv\.org\/(?:abs|pdf|html)|ar5iv(?:\.labs\.arxiv)?\.org\/(?:abs|html)|(?:www\.)?alphaxiv\.org\/(?:abs|pdf|overview))`;
const LINK = new RegExp(String.raw`^https?:\/\/${PAGE}\/${ID}(?:\.pdf)?\/?(?:[?#].*)?$`, "i");

/**
 * An arXiv id (2410.01234, 2410.01234v2, arXiv:hep-th/9901001) or the link of its abstract, PDF or HTML page on
 * arXiv or a mirror (https://arxiv.org/abs/2410.01234v2, https://www.alphaxiv.org/abs/2410.01234): its DOI, which
 * DataCite gives (10.48550/arXiv.2410.01234).
 */
export function arxivDoi(text: string): string | null {
  const t = text.trim();
  const m = BARE.exec(t) ?? LINK.exec(t);
  return m ? `10.48550/arXiv.${m[1]}` : null;
}
