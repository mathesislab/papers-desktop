// The part of citeproc-js (https://citeproc-js.readthedocs.io) the panel uses.

declare module "citeproc" {
  type Sys = {
    retrieveLocale: (lang: string) => string;
    retrieveItem: (id: string) => object;
  };

  class Engine {
    constructor(sys: Sys, style: string);
    updateItems(ids: string[]): void;
    /** [formatting parameters, the entries as HTML] */
    makeBibliography(): [object, string[]] | false;
  }

  const CSL: { Engine: typeof Engine };
  export default CSL;
}
