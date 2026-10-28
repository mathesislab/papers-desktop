"""Record what papers' Biblio.fix_entry (`papers check`) does, and what entry_diff shows, for the tests
of papers-rs (tests/check.rs): `python3 tests/gen_check.py` (python with papers installed) writes
tests/expected/check.json.

A case: a bibtex of entries, the entry fixed (an index), the options of fix_entry, what the fetch of
crossref gives (mocked: a bibtex, an error, or nothing asked), whether the answer to `update?` is yes,
and the entry after, and the diff shown.
"""
import builtins
import io
import json
import os
import random
import sys
from contextlib import redirect_stdout
from unittest import mock

import papers.utils
from papers.bib import Biblio, entry_diff
from papers.entries import parse_string
from papers.filename import KEYFORMAT

papers.utils.use_color(False)
random.seed(20261028)

AUTHORS = ['John Smith', 'Smith, John', 'M. Perrette and A. Yool', 'Jörg Müller and Anne-Marie Dupont', 'Müller, Jörg',
           "Fran{\\c{c}}ois M{\\'e}nard", 'Ludwig van Beethoven', 'de la Cruz, José and Ñandú, A.', 'Søren Kierkegaard',
           'Smith', 'A. B. Cee and D. Ee and F. Gee', 'Łukasz Żółć', 'Zhang Wei and 李明', "O'Brien, P.", 'Tom  Jones',
           'X and Y and Z and W', '  padded name  ', 'Jean-Paul Sartre and Simone de Beauvoir', '', 'Dr. J. R. R. Tolkien',
           'Author, A.', 'Author, Anne']
TITLES = ["Test Article {\\'e}", 'Über die {Natur} von $\\alpha$', 'Near-ubiquity of ice-edge blooms', 'The \\"u{ }ber thing',
          "Caf\\'e au lait \\& more", 'Plain title', '{\\o}ster {\\AA}ngstr{\\"o}m', "M\\\"uller's 100\\% result", 'ünïcödé tïtle',
          'a_b ~ c', 'Ocean heat uptake: \\emph{new} results', 'x \\textbf{y} z', '{\\ss}tra{\\ss}e', 'Ma\\v{r}ák', '\\c{c}a va',
          'One', 'The role of ocean heat uptake in climate change', 'The {Role} of Ocean Heat Uptake in Climate Change',
          'Something Else Entirely']
DOIS = ['10.5194/bg-8-515-2011', 'DOI:10.5194/bg-8-515-2011', 'https://doi.org/10.1000/ABC123', 'doi:10.1000/xyz.v1',
        '10.1000/xyz.received', 'not a doi', 'http://dx.doi.org/10.1038/nature12345', '10.1000/a_b', ' 10.1002/abc ',
        '10.1016/j.foo.2020.01.001.', '10.1000/(abc)', 'arXiv:1234.5678', '10.48550/arXiv.2101.00001', '', '10.1000/a']
KEYS = ['smith2020', '10.1000/a', '2000bad', 'Müller2000', 'ok_key', 'x', 'Key-With-Dash', '123', 'a:b', 'ünï2020',
        'Zhang2019', 'author2000', 'a2000', '²x']
JOURNALS = ['Nature', 'J. Geophys. Res.', "Ann{\\'e}e", '\\emph{Science}', 'Tellus A']
TYPES = ['article', 'book', 'inproceedings', 'misc']


def bibtex_entry(i, key=None, author=None, title=None, doi=None):
    f = {}
    if author is not None or random.random() < .9:
        f['author'] = author if author is not None else random.choice(AUTHORS)
    if random.random() < .2:
        f['editor'] = random.choice(AUTHORS)
    if title is not None or random.random() < .9:
        f['title'] = title if title is not None else random.choice(TITLES)
    if random.random() < .8:
        f['year'] = str(random.randint(1990, 2024))
    if doi is not None or random.random() < .6:
        f['doi'] = doi if doi is not None else random.choice(DOIS)
    if random.random() < .4:
        f['journal'] = random.choice(JOURNALS)
    if random.random() < .2:
        f['abstract'] = "An abstract with \\'e and _under_ and Ä"
    if random.random() < .2:
        f['url'] = 'https://example.org/a_b?x=~y'
    if random.random() < .2:
        f['file'] = ':files/a_b.pdf:pdf'
    if random.random() < .2:
        f['month'] = 'jan'
    if random.random() < .1:
        f['isbn'] = '978-3-16-148410-0'
    key = key or random.choice(KEYS) + str(i)
    body = ',\n'.join(f' {k} = {{{v}}}' for k, v in f.items())
    return f'@{random.choice(TYPES)}{{{key},\n{body}\n}}\n'


def entry_json(e):
    return {'type': e.entry_type, 'key': e.key, 'fields': [[f.key, f.value] for f in e.fields]}


def options():
    o = {}
    for name in ['fix_doi', 'fix_key', 'auto_key', 'key_ascii', 'format_name']:
        o[name] = random.random() < .4
    o['fetch'] = random.random() < .25
    o['fetch_all'] = random.random() < .25
    o['encoding'] = random.choice([None, None, 'latex', 'unicode'])
    o['interactive'] = random.random() < .5
    return o


def fetched_for(entry_text, o):
    """what the mocked crossref gives: a bibtex like the entry's, another paper, an error, nothing"""
    r = random.random()
    if r < .15:
        return {'error': 'offline'}
    if r < .25:
        return {'text': ''}
    db = parse_string(entry_text)
    e = db.entries[0]
    title = e['title'] if 'title' in e and random.random() < .6 else random.choice(TITLES)
    author = random.choice(AUTHORS) if random.random() < .25 else (e['author'] if 'author' in e else 'Author, Anne')
    return {'text': bibtex_entry(0, key='x', author=author or 'Author, Anne', title=title,
                                 doi=e['doi'] if 'doi' in e and random.random() < .5 else random.choice(DOIS))}


def case():
    n = random.randint(1, 3)
    seen, text = set(), ''
    for i in range(n):
        t = bibtex_entry(i)
        k = t.split('{', 1)[1].split(',', 1)[0].lower()
        if k in seen:
            continue
        seen.add(k)
        text += t + '\n'
    index = random.randrange(len(seen))
    o = options()
    fetched = fetched_for(text, o) if (o['fetch'] or o['fetch_all']) else None
    return {'bibtex': text, 'index': index, 'options': o, 'fetched': fetched, 'answer': random.random() < .6}


def run(c):
    db = parse_string(c['bibtex'])
    biblio = Biblio(db=db)
    e = biblio.entries[c['index']]
    before = entry_json(e)
    o = dict(c['options'])
    fetched = c['fetched']

    def doi(_):
        if fetched is None or 'error' in fetched:
            raise ValueError((fetched or {}).get('error', 'none'))
        return fetched['text']

    def full(txt, **kw):
        return doi(txt)

    out = io.StringIO()
    try:
        with mock.patch('papers.bib.fetch_bibtex_by_doi', side_effect=doi), \
                mock.patch('papers.bib.fetch_bibtex_by_fulltext_crossref', side_effect=full), \
                mock.patch('builtins.input', return_value='y' if c['answer'] else 'n'), redirect_stdout(out):
            biblio.fix_entry(e, **o)
    except Exception as error:  # papers crashes: the case says so
        return {'before': before, 'crash': type(error).__name__}
    after = entry_json(e)
    printed = out.getvalue()
    return {'before': before, 'after': after, 'printed': printed}


def pair_diff(a, b, color):
    """the diff as papers shows it (entry_diff(a, b) as `check` calls it): with the colors on or off"""
    papers.utils.use_color(color)
    try:
        return entry_diff(a, b)
    finally:
        papers.utils.use_color(False)


def diffs():
    out = []
    for _ in range(150):
        a = parse_string(bibtex_entry(0, key='k')).entries[0]
        b = parse_string(bibtex_entry(0, key='k')).entries[0]
        if random.random() < .5:  # a near copy: one field changed
            b = parse_string(bibtex_entry(0, key='k')).entries[0]
            for f in a.fields:
                if random.random() < .7:
                    b[f.key] = f.value
            if random.random() < .5 and a.fields:
                b[random.choice(a.fields).key] = random.choice(TITLES) + ' ' * random.randint(0, 3)
        out.append({'old': entry_json(a), 'new': entry_json(b), 'diff': pair_diff(a, b, False),
                    'diff_color': pair_diff(a, b, True)})
    # long values (the "popular" characters of the matcher), tabs and unusual line ends
    long_a = parse_string('@article{k,\n abstract = {' + 'a b c d e f. ' * 40 + '},\n title = {T}\n}').entries[0]
    long_b = parse_string('@article{k,\n abstract = {' + 'a b c d e g. ' * 40 + '},\n title = {T}\n}').entries[0]
    out.append({'old': entry_json(long_a), 'new': entry_json(long_b), 'diff': pair_diff(long_a, long_b, False),
                'diff_color': pair_diff(long_a, long_b, True)})
    odd_a = parse_string('@article{k,\n note = {a b\tc  d},\n title = {T}\n}').entries[0]
    odd_b = parse_string('@article{k,\n note = {a b\tc  e},\n title = {T2}\n}').entries[0]
    out.append({'old': entry_json(odd_a), 'new': entry_json(odd_b), 'diff': pair_diff(odd_a, odd_b, False),
                'diff_color': pair_diff(odd_a, odd_b, True)})
    return out


if __name__ == '__main__':
    cases = []
    for _ in range(400):
        c = case()
        c['expected'] = run(c)
        cases.append(c)
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, 'expected', 'check.json'), 'w') as f:
        json.dump({'fix': cases, 'diffs': diffs()}, f, ensure_ascii=False, indent=0)
    print(len(cases), 'cases;', sum('crash' in c['expected'] for c in cases), 'crashes')
