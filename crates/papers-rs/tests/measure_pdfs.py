#!/usr/bin/env python3
"""How well the text that papers-rs reads of PDFs does what the text of papers does.

papers reads PDFs with PyMuPDF; papers-rs with pdf-extract. What papers does with the text
(finding the DOIs, checking the title) is the same code; so the question is whether the
text gives the same DOIs. This compares, for a set of PDFs:

  * the DOI of the PDF metadata,
  * the DOIs that `parse_dois` finds in the first pages and the last page (the first one is
    what `extract` goes by; the list is what it tries in turn),
  * the DOI that `extract` would take, when the metadata of the DOIs are known: it takes the
    first DOI whose crossref title appears in the text (`--cache DIR`: the cache of papers,
    its `doi` folder, has them; no network is used).

Run with the python that has papers installed (the reference), read-only on the PDFs:

    python3 tests/measure_pdfs.py FILE_OR_DIR... [--sample N] [--seed S] [--cache DIR]

`--sample N` takes N of the PDFs found, at random. It runs `cargo run --release --example
pdftext` for the papers-rs side. A temporary HOME is used for papers.
"""
import argparse
import collections
import json
import os
import random
import subprocess
import sys
import tempfile
from urllib.parse import quote

HERE = os.path.dirname(os.path.abspath(__file__))
TMP = tempfile.mkdtemp(prefix='papers-rs-measure-')
# the workspace aborts on a panic in a release build; pdf-extract panics on some PDFs, which papers-rs
# catches: with an unwinding build
REAL_ENV = dict(os.environ, CARGO_PROFILE_RELEASE_PANIC='unwind')
os.environ['HOME'] = TMP
os.environ['XDG_CACHE_HOME'] = os.path.join(TMP, 'cache')
os.environ['XDG_CONFIG_HOME'] = os.path.join(TMP, 'config')

from papers.extract import (DOIParsingError, parse_dois, parse_doi_from_pdf_metadata,  # noqa: E402
                            pdfhead, pdftail, title_in_text)


def find_pdfs(paths):
    found = []
    for path in paths:
        if os.path.isdir(path):
            for d, _, files in os.walk(path):
                found.extend(os.path.join(d, f) for f in files if f.lower().endswith('.pdf'))
        else:
            found.append(path)
    return sorted(found)


def reference(pdf):
    rec = {'pdf': pdf}
    try:
        rec['meta'] = parse_doi_from_pdf_metadata(pdf)
        head, tail = pdfhead(pdf), pdftail(pdf)
        rec['head'], rec['tail'] = head, tail
        try:
            rec['dois'], rec['err'] = parse_dois(head, tail), None
        except DOIParsingError as error:
            rec['dois'], rec['err'] = None, str(error)
    except Exception as error:
        rec['exc'] = repr(error)
    return rec


def cached_title(cache, doi):
    key = doi.lower()
    head, _, tail = key.partition('/')
    path = os.path.join(cache, 'doi', quote(head, safe=''), quote(tail, safe='') + '.json')
    try:
        value = json.load(open(path)).get('value')
    except (OSError, ValueError):
        return None
    if not value:
        return None
    title = value.get('title') or ['']
    return title[0] if isinstance(title, list) else title


def confirm(cache, dois, text):
    for doi in dois or []:
        title = cached_title(cache, doi)
        if title and title_in_text(title, text):
            return doi
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('paths', nargs='+')
    parser.add_argument('--sample', type=int)
    parser.add_argument('--seed', type=int, default=1)
    parser.add_argument('--cache')
    parser.add_argument('--show', type=int, default=10, help='how many differences to show')
    o = parser.parse_args()
    pdfs = find_pdfs(o.paths)
    if o.sample and o.sample < len(pdfs):
        pdfs = random.Random(o.seed).sample(pdfs, o.sample)
    refs = {}
    for pdf in pdfs:
        refs[pdf] = reference(pdf)
    run = subprocess.run(['cargo', 'run', '--release', '-q', '--example', 'pdftext'], cwd=HERE, env=REAL_ENV,
                         input='\n'.join(pdfs) + '\n', capture_output=True, text=True, check=True)
    rs = {}
    for line in run.stdout.splitlines():
        rec = json.loads(line)
        rs[rec['pdf']] = rec
    stats = collections.Counter()
    shown = []
    lines_found = lines_total = 0
    for pdf in pdfs:
        a, b = refs[pdf], rs[pdf]
        if 'exc' in a:
            stats['papers failed'] += 1
            continue
        stats['files'] += 1
        if 'exc' in b:
            stats['papers-rs failed'] += 1
            shown.append((pdf, 'papers-rs failed: ' + b['exc']))
            continue
        mine = collections.Counter(' '.join(x.split()) for x in (b['head'] + b['tail']).splitlines() if x.strip())
        theirs = collections.Counter(' '.join(x.split()) for x in (a['head'] + a['tail']).splitlines() if x.strip())
        lines_found += sum((theirs & mine).values())
        lines_total += sum(theirs.values())
        if a['meta'] == b['meta']:
            stats['same metadata DOI'] += 1
        else:
            shown.append((pdf, f"metadata DOI: {a['meta']!r} / {b['meta']!r}"))
        if a['dois']:
            stats['papers finds DOIs'] += 1
        if (a['dois'] or [None])[0] == (b['dois'] or [None])[0]:
            stats['same first DOI'] += 1
        else:
            shown.append((pdf, f"DOIs: {(a['dois'] or [None])[:2]} / {(b['dois'] or [None])[:2]}"))
        if a['dois'] == b['dois']:
            stats['same list of DOIs'] += 1
        if o.cache and a['dois']:
            ca, cb = confirm(o.cache, a['dois'], a['head']), confirm(o.cache, b['dois'], b['head'])
            if cached_title(o.cache, a['dois'][0]):
                stats['with the metadata of the first DOI'] += 1
                if ca == cb:
                    stats['same DOI confirmed by the title'] += 1
                else:
                    shown.append((pdf, f'confirmed: {ca} / {cb}'))
    for key, n in stats.items():
        print(f'{key}: {n}')
    if lines_total:
        print(f'lines of the text of papers that are lines of the text of papers-rs: '
              f'{lines_found} of {lines_total} ({100 * lines_found / lines_total:.1f}%)')
    for pdf, what in shown[:o.show]:
        print(' ', os.path.basename(pdf), what)


if __name__ == '__main__':
    main()
