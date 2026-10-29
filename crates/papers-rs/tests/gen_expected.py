#!/usr/bin/env python3
"""Generate tests/expected/ from the reference, papers-cli (Python).

The Rust tests (tests/bib.rs) compare what papers-rs reads and writes with
what is recorded here, for the bibtex files of tests/fixtures/:

  expected/<name>.json       the blocks as papers reads them (before the sort),
                             and the text of `Biblio.format()` and of a save
  expected/<name>.saved.bib  the text of a save (Biblio.sort, then format)
  expected/latexenc.json     papers.latexenc on strings
  expected/encoding.json     names, keywords and the file field
  expected/fuzz.json         texts made from the fixtures by random edits

Run with the python that has papers installed (the reference), from anywhere:

    /path/to/python3 tests/gen_expected.py

It never touches the configuration or the libraries of the user: HOME and
the cache are a temporary folder.
"""
import json
import logging
import os
import random
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ARGS = list(sys.argv)
TMP = tempfile.mkdtemp(prefix='papers-rs-expected-')
os.environ['HOME'] = TMP  # before papers is imported
os.environ['XDG_CACHE_HOME'] = os.path.join(TMP, 'cache')
os.environ['XDG_CONFIG_HOME'] = os.path.join(TMP, 'config')
os.chdir(TMP)

import bibtexparser  # noqa: E402
from bibtexparser.model import (Entry, ExplicitComment, ImplicitComment, ParsingFailedBlock,  # noqa: E402
                                Preamble, String)

import papers  # noqa: E402
from papers.bib import Biblio, unreadable_blocks  # noqa: E402
from papers.encoding import family_names, parse_keywords, standard_name  # noqa: E402
from papers.entries import format_file, parse_file, parse_string  # noqa: E402
from papers.latex import latex_to_unicode  # noqa: E402
from papers.latexenc import protect_uppercase, string_to_latex  # noqa: E402

logging.disable(logging.CRITICAL)

FIXTURES = os.path.join(HERE, 'fixtures')
EXPECTED = os.path.join(HERE, 'expected')


def read(path):
    "the text of a file as papers reads it"
    with open(path, encoding='utf-8-sig') as f:
        return f.read()


def dump_block(b):
    if isinstance(b, Entry):
        return {'kind': 'entry', 'type': b.entry_type, 'key': b.key,
                'fields': [[f.key, f.value, f.enclosing == 'no-enclosing'] for f in b.fields]}
    if isinstance(b, String):
        return {'kind': 'string', 'key': b.key, 'value': b.value, 'verbatim': b.enclosing == 'no-enclosing'}
    if isinstance(b, Preamble):
        return {'kind': 'preamble', 'value': b.value}
    if isinstance(b, ExplicitComment):
        return {'kind': 'comment', 'value': b.comment}
    if isinstance(b, ImplicitComment):
        return {'kind': 'text', 'value': b.comment}
    if isinstance(b, ParsingFailedBlock):
        name = type(b).__name__
        d = {'kind': 'unreadable', 'raw': b.raw, 'line': b.start_line, 'class': name}
        if name == 'DuplicateBlockKeyBlock':
            d['key'] = b.key
        return d
    raise TypeError(type(b))


def dump(lib):
    return {'blocks': [dump_block(b) for b in lib.blocks],
            'unreadable': [[line, reason] for line, reason in unreadable_blocks(lib)]}


def edit(lib):
    "the same edits as tests/bib.rs: a field set, a field removed"
    for e in lib.entries:
        e['x-edited'] = 'yes {braced} & "quoted"'
        if 'abstract' in e:
            del e['abstract']
        if 'title' in e:
            e['title'] = e['title'] + ' (edited)'
    return lib


def saved_text(lib):
    "Biblio.save: sort, then format"
    b = Biblio(lib)
    b.sort()
    return b.format()


def case(text):
    "what papers makes of a text: the blocks, the unsorted format, the saved text, the edited saved text"
    lib = parse_string(text)
    out = dump(lib)
    lib_cached = Biblio.load(write_tmp(text), '')  # with the parse cache, as papers loads a bibtex
    cached = dump(lib_cached.db)
    out['cache_agrees'] = cached == out
    # format sorts the fields of the entries in place: keep it for the end
    out['formatted'] = Biblio(parse_string(text)).format()
    out['saved'] = saved_text(parse_string(text))
    out['saved_twice'] = saved_text(parse_string(out['saved']))  # not always the same: see broken.bib
    out['edited_saved'] = saved_text(edit(parse_string(text)))
    return out


_n = [0]


def write_tmp(text):
    _n[0] += 1
    path = os.path.join(TMP, f'lib{_n[0]}.bib')
    with open(path, 'w', newline='') as f:
        f.write(text)
    return path


def fixtures():
    for name in sorted(os.listdir(FIXTURES)):
        if name.endswith('.bib'):
            yield name[:-4], read(os.path.join(FIXTURES, name))


def mutate(rng, text, pool):
    tokens = ['{', '}', '"', ',', '=', '@', '\n', '@article{', '@string{', '@comment{', '@preamble{', '(', ')',
              '\\', '%', '#', ' ', '\t', 'é', 'title = {x},', '@x(', '\n@', ' # ', '{"}', '\\"', '\\{',
              '\\\n', ' ', ' ', '\r']
    for _ in range(rng.randint(1, 5)):
        op = rng.randrange(6)
        if not text:
            text = rng.choice(pool)
        i = rng.randrange(len(text) + 1)
        if op == 0:  # delete a span
            text = text[:i] + text[i + rng.randint(1, 20):]
        elif op == 1:  # insert a token
            text = text[:i] + rng.choice(tokens) + text[i:]
        elif op == 2:  # duplicate a span
            j = min(len(text), i + rng.randint(1, 60))
            text = text[:j] + text[i:j] + text[j:]
        elif op == 3:  # replace a character
            text = text[:i] + rng.choice(tokens) + text[i + 1:]
        elif op == 4:  # cut the text
            text = text[:i]
        else:  # append another text
            text = text + rng.choice(pool)
    return text


def fuzz(n, seed=20260101):
    rng = random.Random(seed)
    pool = [t for _, t in fixtures() if t]
    cases = []
    for _ in range(n):
        base = rng.choice(pool)
        # a piece of a fixture, or the whole of it
        if rng.random() < 0.5:
            i = rng.randrange(len(base))
            base = base[i:i + rng.randint(40, 600)]
        text = mutate(rng, base, pool).replace('\r\n', '\n').replace('\r', '\n')  # as open().read() reads it
        try:
            lib = parse_string(text)
            d = dump(lib)
            entries = [b for b in d['blocks'] if b['kind'] == 'entry']
            cases.append({'text': text, 'entries': entries, 'unreadable': d['unreadable'],
                          'saved': saved_text(parse_string(text)),
                          'cache_agrees': dump(Biblio.load(write_tmp(text), '').db) == d})
        except Exception as error:  # a text papers cannot read at all: not a case
            print('skipped', type(error).__name__, error, file=sys.stderr)
    return cases


def latexenc_cases(extra):
    rng = random.Random(7)
    strings = list(extra)
    strings += [
        '', 'hello', 'Hello World', "{\\'e}", "{\\'E}", '{\\"u}', '{\\c{c}}', '{\\~n}', "\\'{e}", "\\'{a}", '\\`{e}',
        "\\'", '\\', '{', '}', ' { } ', 'é', 'á', 'ñ', 'ç', 'ü', 'ö', 'François', 'Ångström', 'Œuvre', 'ß', 'ø',
        'é́', 'é', 'ABC', 'Hello', 'A{B}C', '{A}', 'Zeta2020', 'a_b~c', '5% of $x$', '"quoted"',
        "\\'e\\`a", "\\^{o}\\v{s}\\H{o}", '\\c c', '\\AA ngstr\\"om', 'M{\\"u}ller', 'x\\\\y', '– — “',
        '\U0001d7d8', 'αβγ', '\\alpha', '\\textbf{a}', 'a\n\nb', '\\ss{}', '\\o{}', '\\"{\\i}', '\\^\\i', "\\'\\i",
        # the decoder of the same-paper rule (bibtex.md 4.1)
        'T\\^ete', "\\'e", '\\v{S}koda', 'A{\\dh}algeirsd{\\\'o}ttir', '\\url{x}', '$\\alpha$-decay', 'CO$_2$',
        "\\'{\\\"u}", '\\"u\\"u', '\\c c', '\\c{c}', '{\\c c}', '\\k{a}', '\\=o', '\\.z', '\\u{g}', '\\r{a}', '\\H{o}',
        '\\d{s}', '\\b{b}', '\\t{oo}', "\\'", '\\^', 'a\\^', '\\i', '\\j', '\\"\\i', '\\'+'{\\j}', '\\AA ngstr\\"om',
        '\\o  x', '\\ss  y', '\\l', '\\th', '\\textendash', '\\textendash x', '\\space', '\\copyright', '\\times',
        '\\ell', '\\dh', '\\foo', '\\foo{bar}', '\\foo bar', '\\&\\%\\$\\#\\_\\{\\}', 'a\\ b\\,c\\;d\\:e\\!f\\\\g',
        'a\\-b', 'a~b', 'a--b', 'a---b', '``q\'\'', '\\emph{x} \\textit{y} \\href{u}{t}', '\\cyrchar\\CYRA', '$x$',
        '\\\'{}e', '\\\'  {e}', '{\\\'{\\i}}', '\\mathrm{CO}', '\\relax', 'x\\', '\\\n', '\\é', '\\textbf', '{{a}}',
        '{', '}}', '\\\'{e', '\\\'{e}}', '\\\'{{e}}', '\\\'\\\'e', '\\v\\c{s}',
    ]
    atoms = ['a', 'e', 'o', 'C', 'ss', ' ', '{', '}', '\\', "\\'", '\\"', '\\`', '\\^', '\\~', '\\c', '\\v', '\\H',
             '\\u', '\\=', '\\.', '\\AA', '\\ss', '\\i', 'é', 'ü', '́', '̈', '$', '%', '_', '~',
             '#', '&', '–', 'A', 'Z', '\\textbf', '\\alpha', '\\\\', '--', '\'\'', '`', '\\url', '\\emph', '\\j', '\\o',
             '\\k', '\\d', '\\cyrchar', '\\-', '\\,', '\n', '\\space', '\\textendash']
    for _ in range(1500):
        strings.append(''.join(rng.choice(atoms) for _ in range(rng.randint(1, 8))))
    seen, out = set(), []
    for s in strings:
        if s in seen:
            continue
        seen.add(s)
        out.append({'in': s, 'to_latex': string_to_latex(s), 'to_unicode': latex_to_unicode(s),
                    'to_unicode_path': latex_to_unicode(s, text=False), 'protect': protect_uppercase(s)})
    return out


def encoding_cases(authors, keywords, files):
    names = ['Smith, John', 'John Smith', 'Doe, Jane and Smith, John', 'John Smith and Jane Doe', 'Cher',
             'van der Berg, Jane and {The Collaboration}', ' A B  and  C D ', 'Perrette, M. and Yool, A.', '',
             'A and', 'A and and B', 'Jean-Paul Sartre and Simone de Beauvoir', 'x, y, z and u v',
             'John {van der Berg}', '{van der Berg}, Jane', '{World Health Organization}', 'Jane {Van Der} Berg and X',
             '{Barnes and Noble} and Doe, J.', '{Intergovernmental Panel on Climate Change}', 'a {b, c} d', '{a, b}',
             'A  and\tB', 'A and\nB', 'A AND B', '{x and y', 'x} and y z', '{a} {b}', 'A  B ', 'Smith ,John', ',',
             'a and  and b c', 'J. {de la} Cruz and {\\"O}zt{\\"u}rk, Ali']
    kws = ['', 'ocean', 'kiwi, ocean, climate', ' sea-level ,  projections ', ',,a,,', 'a;b, c']
    fs = ['', 'file.pdf:/path/to/file.pdf:pdf', ':/path/to/file.pdf:pdf', '/path/to/file.pdf:pdf', '/path/to/file.pdf',
          ':/path/to/file.pdf:', ':/path/to/file1.pdf:pdf;:/path/to/file2.pdf:pdf',
          ':/path/to/American Economic Journal: Applied Economics.pdf:pdf', 'desc:/path/with:colon.pdf:pdf', '::pdf',
          ':/path/to/file.pdf:pdf;', ';;', ' ; ', 'a:b:c:d:e', 'a:b', ':', '::', ':::', 'x;y;z', 'files/a.b.pdf:pdf',
          'a.pdf; b.pdf', ' :  x.pdf : pdf ', ':  :pdf', 'd: p.pdf :pdf; \t', ' a b.pdf ', 'C:\\x.pdf', ':C:\\x.pdf:pdf']
    lists = [['/path/to/file.pdf'], ['/a/b.pdf', '/a/c.tar.gz'], ['noext'], ['.hidden'], ['x.'], ['a/.b/c'],
             ['a.b/c'], ['..x.txt'], ['dir.d/file'], ['a..b'], [], ['/p/q.PDF', 'r.djvu']]
    return {
        'names': [{'in': a, 'standard': standard_name(a), 'family': family_names(a)} for a in names + authors],
        'keywords': [{'in': k, 'parse': parse_keywords({'keywords': k})} for k in kws + keywords],
        'files': [{'in': f, 'parse': parse_file(f)} for f in fs + files],
        'format_file': [{'in': l, 'out': format_file(l)} for l in lists],
    }


def main(fuzz_n=300, fuzz_out=None):
    os.makedirs(EXPECTED, exist_ok=True)
    values = {'author': [], 'keywords': [], 'file': [], 'all': []}
    for name, text in fixtures():
        out = case(text)
        if not out['cache_agrees']:
            print(f'{name}: the parse with the cache is not the parse without it', file=sys.stderr)
        for b in out['blocks']:
            if b['kind'] == 'entry':
                for k, v, _ in b['fields']:
                    values['all'].append(v)
                    if k in values:
                        values[k].append(v)
        saved = out.pop('saved')
        with open(os.path.join(EXPECTED, f'{name}.saved.bib'), 'w', newline='') as f:
            f.write(saved)
        with open(os.path.join(EXPECTED, f'{name}.json'), 'w') as f:
            json.dump(out, f, ensure_ascii=False, indent=1)
            f.write('\n')
    with open(os.path.join(EXPECTED, 'latexenc.json'), 'w') as f:
        json.dump(latexenc_cases(values['all']), f, ensure_ascii=False, indent=0)
        f.write('\n')
    with open(os.path.join(EXPECTED, 'encoding.json'), 'w') as f:
        json.dump(encoding_cases(values['author'], values['keywords'], values['file']), f, ensure_ascii=False, indent=0)
        f.write('\n')
    cases = fuzz(fuzz_n, int(os.environ.get('FUZZ_SEED', 20260101)))
    bad = sum(1 for c in cases if not c['cache_agrees'])
    if bad:
        print(f'fuzz: {bad} of {len(cases)} texts parse differently with the cache', file=sys.stderr)
    with open(fuzz_out or os.path.join(EXPECTED, 'fuzz.json'), 'w') as f:
        json.dump(cases, f, ensure_ascii=False, indent=0)
        f.write('\n')
    print(f'papers {papers.__version__ if hasattr(papers, "__version__") else "?"}, '
          f'bibtexparser {bibtexparser.__version__ if hasattr(bibtexparser, "__version__") else "?"}: done',
          file=sys.stderr)


if __name__ == '__main__':
    n = int(ARGS[1]) if len(ARGS) > 1 else 300
    main(n, os.path.abspath(sys.argv[2]) if len(sys.argv) > 2 else None)
