#!/usr/bin/env python3
"""Generate tests/expected/formats.json from the reference, papers-cli (Python):
the export formats and what they are made of, and the fuzzy search.

  names      papers.formats.parse_bibtex_names, format_bibtex_names, _ris_name, _bibtex_name_of_ris
  helpers    dates, months, pages, DOIs, keys
  entries    entry_to_csl / dumps_csl, dumps_ris (rispy), dumps_bibjson of entries with the file
             links given: the texts written
  scores     rapidfuzz token_set_ratio with and without a cutoff
  doi        papers.extract.isvaliddoi

Run with the python that has papers and rispy installed (the reference):

    /path/to/python3 tests/gen_formats.py

Like gen_expected.py, it never touches the configuration or the libraries of the user.
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_expected as G  # noqa: E402  (sets HOME to a temporary folder, silences the log)

import papers.formats as F  # noqa: E402
from papers.encoding import format_entries  # noqa: E402
from papers.extract import isvaliddoi  # noqa: E402
from rapidfuzz.fuzz import token_set_ratio  # noqa: E402

# values of the fields: what gen_unicode.py uses, without the commands that change the state of the converter
ACCENTS = [r"{\'e}", r"\'e", r'{\"u}', r'\"{o}', r'{\c{c}}', r'{\~n}', r'\v{s}', r'{\ss}', r'\ss{}', r'\o', r'{\O}', r'\l',
           r'{\AA}', r'\AE{}', r'{\i}', r'\"{\i}', r'\oe', r'\textdegree', r'\textendash']
MATH = [r'$\alpha$', r'$x_{i}^2$', r'\(a+b\)', r'{CO}$_2$', '$a$', r'$10^{-3}$ m']
MARKUP = [r'\url{http://a.b/c_d%20e}', r'\emph{em}', r'\textbf{b}', r'{\bf bold}', r'\mbox{a b}', r'\cite{k}', '\\\\',
          r'\unknown{x}', r'\unknown', r'\begin{itemize}\item a\item b\end{itemize}', r'\hspace{1em}', r'\LaTeX', r'\ldots']
SPECIAL = ['--', '---', '~', r'\&', r'\%', r'\_', r'\#', r'\$', r'\{', r'\}', "``q''", '&', '%', '\n', '  ', ' ']
UNI = ['Müller', 'Ångström', 'Δ', '北京', '–', '—', '“q”', '…', 'ß', '​', ' ', '­']
BAD = ['{', '}', '\\', r'\verb', r'\begin{x}', '$x', r'\(', r'\textbf{', r'\url{']
WORDS = ['The', 'Ice', 'sheet', 'sea level', 'Smith', 'of', 'in', 'Journal', 'Geophys', 'Res', 'Nature', '2020',
         'Vol', 'Cryosphere', 'The Cryosphere', 'amoc', 'ice; amoc']

NAMES = [
    'Perrette, M.', 'van der Berg, Jan', 'Ludwig van Beethoven', 'Smith, Jr, John', '{World Health Organization}',
    'jean de la fontaine', 'Doe, Jr, Jane, X', 'A AND B', 'Jane Doe', 'Doe', 'Doe, Jane', 'de la Cruz, Maria', 'Maria de la Cruz',
    '{van der} Berg, Jan', 'Berg, {Jan van der}', 'John {van der Berg}', r'M{\"u}ller, J{\"o}rg', r'{\AA}ngstr{\"o}m, Anders',
    '{}', 'Doe,', ', Jane', 'Doe, , Jane', 'Doe, Jr,', 'van Beethoven', 'Beethoven, van', 'ludwig', 'von Neumann, John',
    'John von Neumann', 'John von', 'Jean-Paul Sartre', 'J. R. R. Tolkien', 'Tolkien, J. R. R.', '{\\"O}zil, Mesut',
    'Mesut {\\"O}zil', 'Q. Public, Jr, John', '{A} and {B}', 'x and y', 'Nobody and', 'and Nobody', 'a  and  b', 'a AND b',
    'A, B, C, D, E', '  Padded , Name  ', '{Unbalanced', 'Unbalanced}', 'a}b, c', 'émile Zola', 'zola, émile',
    'ΑΒΓ Delta', 'a\tb', '{a} {b}', '{a}{b}', 'Van Dyke', 'van Dyke, Dick', 'Dick van Dyke', 'x y z w', 'X de Y de Z',
]
NAME_FIELDS = NAMES + [' and '.join(NAMES[i:i + 3]) for i in range(0, len(NAMES), 3)] + [
    '', ' ', 'A and', 'A and  and B', ' and B', 'A and B and', '{A and B} and C', 'A {and} B', 'A\nand\nB', 'A\tand\tB']

RIS_NAMES = ['Smith, John', 'Smith, John, Jr', 'Smith,', 'Smith', 'John Smith', 'World Health', 'Smith, , Jr', 'Smith, John, ',
             ' Smith , John ', 'a,b,c,d', '', ' ']

DATES = ['2013', '2013-05', '2013/05/02/', '2013-05-02T00:00', '13-05', '2013-00-05', ' 2013', '20130', '2013-', '2013-5-2',
         '2013-123', '0000-01', '2013/5', '', '٢٠١٣', 'x2013', '2013 may', '2013-05-00']
MONTHS = ['jan', 'January', '5', '05', '13', '0', '{jan}', ' {Dec} ', 'sept', 'ma', '', 'xyz', '12', '{12}', 'Mai', 'MAY']
PAGES = ['11-29', '11--29', '11 - 29', '11–29', '11—29', '1-2-3', '5', '', ' ', 'e1--e9', '1---2', '--1', '1--', '-', 'a - - b',
         '1 -- 2 -- 3', '12 – 34', 'S1-S2', '11-29 ', '  7  ', '1–2—3', '1 - 2']
DOIS = ['10.1/x', 'doi:10.1/x', 'DOI: 10.1/x', 'https://doi.org/10.1/x', 'HTTP://DX.DOI.ORG/10.1/x', ' http://doi.org/10.1/x ',
        'doi:', '', 'https://example.org/10.1/x', 'doi.org/10.1/x', 'doi: 10.1/x']
KEYS = ['perrette2013', 'ITEM-12', 'item-12', '2013', 'a b', 'a:b', 'a.b+c-d', '', ' x ', 'é1', 'A_1', 'http://x', 'ITEM-',
        'ITEM-1a', 'x\n']
VALID_DOIS = ['10.5194/esd-4-11-2013', '10.1/x', '10.12345/abc', '10.5194/ESD-4-11', '10.1175/1520-0493(1990)118<2228:WMGD>2.0.CO;2',
              '10.1002/(SICI)1097-0258', '10.1234/a b', '10.1234/a.pdf', '10.1234/abstract', 'abc', '', ' ', '10.12/x', '10.1234/',
              '10.1234/x.', '10.1234/x)', '10.1234/(x)', '10.1234/x\n10.4321/y', '10.5281/zenodo.123 10.1234/paper',
              '10.1234/paper 10.5281/zenodo.123', '10.1234/x-supplement', '10.48550/arXiv.2101.00001', 'arxiv:2101.00001',
              '10.1234/a.s1 10.1234/a', 'x10.1234/abc', '10.12345678901/x', '10.1234/é', 'https://doi.org/10.1234/abc',
              'doi:10.1234/abc', '10.1234/a–b', '10.1234/abc.pdf', '10.1234/a/-/dcsupplemental', '10.1234/X(0123456789',
              '10.1234/a:', '10.1234/aedited', '10.1234/a_edited']

FILES = [[], [], ['files/x.pdf'], ['/abs/y.pdf'], ['files/x.pdf', 'files/x--b.pdf'], ['a;b.pdf'], ['é/ü.pdf'], ['sp ace/x.pdf']]

ENTRY_TYPES = ['article', 'book', 'booklet', 'inbook', 'incollection', 'inproceedings', 'conference', 'proceedings', 'phdthesis',
               'mastersthesis', 'thesis', 'techreport', 'report', 'manual', 'misc', 'online', 'electronic', 'www', 'dataset',
               'software', 'unpublished', 'patent', 'other', 'unknowntype']
FIELDS = ['title', 'author', 'editor', 'translator', 'journal', 'booktitle', 'series', 'volume', 'number', 'issue', 'pages',
          'year', 'month', 'date', 'doi', 'url', 'isbn', 'issn', 'publisher', 'school', 'institution', 'organization', 'address',
          'abstract', 'keywords', 'note', 'edition', 'language', 'type', 'eprint', 'urldate', 'pmid', 'pmcid', 'file',
          'crossref', 'x-other']


def value_atoms():
    return ACCENTS + MATH + MARKUP + SPECIAL + UNI + BAD + WORDS * 6


def random_value(rng, field, atoms):
    if field in ('author', 'editor', 'translator'):
        return rng.choice(NAME_FIELDS)
    if field == 'pages':
        return rng.choice(PAGES)
    if field in ('year',):
        return rng.choice(['2013', '2020', '{2021}', ' 1999 ', 'in press', '', '99999', '٢٠١٣', '2013a', '0'])
    if field == 'month':
        return rng.choice(MONTHS)
    if field == 'date':
        return rng.choice(DATES)
    if field == 'urldate':
        return rng.choice(DATES + ['2020-03-04', '2021/1/2'])
    if field == 'doi':
        return rng.choice(DOIS)
    if field == 'keywords':
        return rng.choice(['sea level, ice; amoc', 'a,b,,c', ' x , y ', ',', '', 'one', 'climate/amoc, ice'])
    if field == 'type':
        return rng.choice(['Working paper', 'PhD thesis', '{Tech. Rep.}', 'x'])
    if field == 'eprint':
        return rng.choice(['2101.00001', ''])
    if rng.random() < 0.3:
        return rng.choice(atoms)
    return ''.join(rng.choice(atoms) + (' ' if rng.random() < 0.5 else '') for _ in range(rng.randint(1, 4)))


def entry_text(rng, atoms, n):
    fields = []
    for f in rng.sample(FIELDS, rng.randint(0, 14)):
        v = random_value(rng, f, atoms)
        if rng.random() < 0.85:
            fields.append(f' {f} = {{{v}}}')
        else:
            fields.append(f' {f} = "{v.replace(chr(34), "")}"')
    return '@%s{k%d,\n%s\n}\n' % (rng.choice(ENTRY_TYPES), n, ',\n'.join(fields))


HAND_ENTRIES = [
    # the examples of formats.md
    '@article{perrette2013,\n author = {Perrette, M. and van der Berg, Jan and {World Health Organization} and Smith, Jr, John},\n'
    ' title = {A scaling approach to {S}ea level in G{\\"o}ttingen \\& $\\alpha$ 50\\% ~x},\n journal = {Earth System Dynamics},\n'
    ' year = {2013},\n month = may,\n volume = {4},\n number = {1},\n pages = {11--29},\n doi = {10.5194/esd-4-11-2013},\n'
    ' url = {https://esd.copernicus.org/x},\n issn = {2190-4979},\n abstract = {Sea level\n rise.},\n'
    ' keywords = {sea level, ice; amoc},\n note = {A note},\n file = {:files/perrette_et_al_2013_a-scaling-approach.pdf:pdf}\n}\n',
    '@techreport{rep2020,\n author = {Doe, Jane},\n title = {Report},\n institution = {PIK},\n address = {Potsdam},\n'
    ' type = {Working paper},\n year = {2020}\n}\n',
    '@misc{arx2021, title = {Preprint}, eprint = {2101.00001}, archiveprefix = {arXiv}, year = {2021}}',
    '@misc{arx2022, date = {2022-03-04}, urldate = {2023-01-02}, url = {http://x.org}}',
    '@phdthesis{th2019, author = {Doe, J.}, title = {Thesis}, school = {Univ}, year = {2019}, type = {PhD}}',
    '@mastersthesis{th2018, author = {Doe, J.}, title = {Thesis}, school = {Univ}, year = {2018}}',
    '@book{b, editor = {Ed, A. and Ed, B.}, translator = {Tr, X}, title = {Book}, publisher = {P}, organization = {O}, isbn = {123}}',
    '@inproceedings{c, booktitle = {Proc. of {X}}, organization = {Org}, series = {LNCS}, address = {Berlin}, edition = {2}, language = {en}}',
    '@article{d, issue = {3}, number = {9}, journal = {J}, booktitle = {B}}',
    '@article{e, number = {9}}',
    '@misc{f, number = {9}}',
    '@article{g, year = {2013}, date = {2014-05-06}, month = {13}}',
    '@article{h, year = {n.d.}, date = {2014-05-06}}',
    '@article{i, year = {n.d.}}',
    '@article{j, TITLE = {Upper}, Author = {Last, First}}',
    '@article{k, doi = {doi:}, url = {}, title = {}}',
    '@article{l, keywords = {,}, note = {}}',
    '@article{m, title = {a \\title{T} b}}',
    '@article{n,\n author = {A, B and C, D},\n journal = {{J}ournal of {\\"U}ber},\n x-field = {raw {braces}},\n id = {nope}, type = {Report}\n}\n',
]


def entry_record(text, files):
    "what papers writes of the first entry of the text, in each format"
    lib = G.parse_string(text)
    entries = lib.entries
    out = {'in': text, 'files': files, 'count': len(entries)}
    if not entries:
        return out
    e = entries[0]
    out['csl'] = F.dumps_csl([e], lambda x: files)
    out['bibjson'] = F.dumps_bibjson([e], lambda x: files)
    out['ris'] = F.dumps_ris([e], lambda x: files)
    return out


HAND_IMPORTS = {
    'ris': [
        'TY  - JOUR\nTI  - x\nER  - \n',
        'TY  - JOUR\nTI  - a\n b\n c\nKW  - one\n two\nER  - \n',
        'TY  - JOUR\nTI  - first\nTI  - second\nT1  - t1\nAU  - Doe, Jane\nA1  - Roe, Rick\nAU  - World Org\nER  - \n',
        'TY  - JOUR\nTI  - a\nZZ  - unknown\n more\nC1  - custom\nY2  - 2020\nER  - \n',
        'TY  - JOUR\nTI  - dropped, no ER\n',
        'TY  - JOUR\r\nTI  - crlf\r\nPY  - 2020/05/\r\nER  - \r\n',
        '\ufeffTY  - JOUR\nTI  - bom\nER  - \n',
        'hello', '', '  \n', 'TY\n', 'TYPO text\nTI  - x\nER  - \n', 'TY  - \nER  - \n',
        'TY  - JOUR\nTI  - a\nER  - \nTY  - BOOK\n  \nDO  - 10.1/x\nER  - \n',
        'TY  - JOUR\nTI  - a\nER  - \nTY  - BOOK\nmore\nER  - \n',
        'TY  - JOUR\nKW  - a\nER  - \nTY  - BOOK\nmore\nER  - \n',
        'junk\n more\nTY  - JOUR\n continuation\nTI  - x\nER  - \n',
        'TY  - CHAP\nA2  - Ed, Anne\nA2  - Ed, Bob\nT2  - Book\nTI  - Chapter\nER  - \n',
        'TY  - CHAP\nED  - Ed, Anne\nA2  - Other\nER  - \n',
        'TY  - JOUR\nUR  - https://doi.org/10.1/x;https://example.org/a\nER  - \n',
        'TY  - JOUR\nUR  - https://doi.org/10.1/x\nER  - \n',
        'TY  - JOUR\nDO  - 10.1/y\nUR  - doi:10.1/x\nER  - \n',
        'TY  - JOUR\nSN  - 2190-4979\nSN  - 978-3-16-148410-0\nSN  - 1234-5678\nER  - \n',
        'TY  - JOUR\nPY  - 2013\nDA  - 2013/05/02/\nER  - \n', 'TY  - JOUR\nDA  - 2013/05/\nER  - \n',
        'TY  - JOUR\nY1  - 2013-05\nER  - \n', 'TY  - JOUR\nPY  - 2013\nDA  - 2014/05/\nER  - \n',
        'TY  - JOUR\nSP  - 11\nEP  - 29\nER  - \n', 'TY  - JOUR\nSP  - 11-29\nER  - \n', 'TY  - JOUR\nSP  - 11\nEP  - 11\nER  - \n',
        'TY  - JOUR\nEP  - 29\nER  - \n', 'TY  - JOUR\nN1  - a\nN1  - \nN1  - b\nER  - \n',
        'TY  - JOUR\nKW  - a; b, c\nKW  - d\nER  - \n', 'TY  - RPRT\nM3  - Working paper\nPB  - PIK\nER  - \n',
        'TY  - THES\nPB  - Univ\nM3  - PhD\nER  - \n', 'TY  - BOOK\nPB  - P\nM3  - ignored\nER  - \n',
        'TY  - JOUR\nL1  - file:///a/b%20c.pdf\nL1  - http://x.org/a.pdf\nL1  - desc:/p/q.pdf:pdf\nL1  - C:\\x\\y.pdf\nL1  - rel/z.pdf\nER  - \n',
        'TY  - JOUR\nID  - perrette2013\nER  - \n', 'TY  - JOUR\nID  - 12345\nER  - \n', 'TY  - jour\nTI  - lower\nER  - \n',
        'TY  - UNKNOWN\nT2  - j\nER  - \n', 'TY  - ELEC\nT2  - j\nER  - \n', 'TY  - BOOK\nJF  - j\nJO  - k\nER  - \n',
        'TY  - JOUR\nAU  - Smith, John, Jr\nAU  - Smith,\nAU  - Smith, , Jr\nAU  - Org Name\nAU  - Mono\nER  - \n',
        'TY  - JOUR\nAB  - abs\nN2  - n2\nER  - \n', 'TY  - JOUR\nN2  - n2 only\nER  - \n',
        'TY  - JOUR\nTI  - a\nER  - \n\nTY  - BOOK\nTI  - b\nER  - \n\nTY  - JOUR\nTI  - c\nER  - \n',
        'TY  - JOUR\nTI  -no space\nER  -\n', 'TY  - JOUR\nTI  -  padded value  \nER  - \n',
    ],
    'csl-json': [
        '[]', '{}', '[{}]', '{"items": []}', '{"items": [{"id": "a", "type": "book", "title": "T"}]}',
        '{"id": "a", "title": "one item"}', '{"items": {"id": "a"}}', '"text"', '12', 'null', '', '[1]', '[{"id": 1}, 2]',
        '[{"id": "a", "citation-key": "b", "type": "article-journal", "container-title": "J", "volume": 4, "issue": 1}]',
        '[{"type": "thesis", "genre": "Master\'s thesis", "publisher": "Univ"}, {"type": "thesis", "publisher": "U"}]',
        '[{"type": "report", "publisher": "PIK", "genre": "Working paper"}, {"type": "book", "publisher": "P", "genre": "x"}]',
        '[{"type": "paper-conference", "container-title": ["Proc. A", "B"], "page": "11-29"}]',
        '[{"author": "Doe, Jane"}]', '[{"author": [{"family": "Doe", "given": "Jane"}, "Org", {"literal": "Lit"}, {}, null]}]',
        '[{"author": [{"family": "Berg", "non-dropping-particle": "van der", "given": "Jan", "suffix": "Jr"}]}]',
        '[{"author": [{"family": "X", "suffix": "Jr"}, {"literal": "L", "family": "F"}, {"family": "F", "dropping-particle": "d", "given": "G"}]}]',
        '[{"issued": {"date-parts": [[2013, 5, 2]]}}]', '[{"issued": {"date-parts": [["2013", "05"]]}}]', '[{"issued": {"date-parts": [[2013, null]]}}]',
        '[{"issued": {"date-parts": [[]]}, "accessed": {"raw": "2020-03-04"}}]', '[{"issued": {"literal": "2013-05"}}]', '[{"issued": "2013/05/02"}]',
        '[{"issued": 2013}]', '[{"issued": {"raw": 2013}}]', '[{"issued": {"date-parts": [[0, 5]]}}]', '[{"accessed": {"date-parts": [[2020, 3, 4]]}}]',
        '[{"keyword": "a; b, c"}, {"keyword": ["x", "y"]}, {"keyword": ""}]', '[{"DOI": "https://doi.org/10.1/x", "URL": ["u1", "u2"]}]',
        '[{"title": ["List title"], "note": null, "abstract": "", "language": 0, "edition": false}]',
        '[{"file": "a.pdf;file:///b%20c.pdf;http://x/y.pdf", "custom": {"file": ["rel/z.pdf", "d:/p/q.pdf:pdf"]}}]',
        '[{"id": "ITEM-1"}, {"id": "http://zotero.org/x"}, {"id": "ok2013"}, {"citation-key": "ck", "id": "id"}, {"id": ""}]',
        '[{"number": 5}, {"number": "6", "issue": "7"}, {"issue": 0, "number": 3}]', '[{"page": 12}, {"page": "1-2-3"}, {"page": ""}]',
        '[{"type": "webpage"}, {"type": "post"}, {"type": "xyz"}, {"type": 5}]', '[{"container-title": "J", "type": "book"}, {"container-title": "J", "type": "webpage"}]',
        '[{"publisher-place": "Potsdam", "collection-title": "LNCS", "PMID": 123, "PMCID": "PMC1"}]',
        '{"items": [{"id": "a"}], "other": 1}', '[{"genre": ["Master"], "type": "thesis"}]',
        '[{"author": {"family": "Doe"}}, {"editor": 5}]', '[{"translator": [{"family": "T"}]}]',
        '[{"title": {"a": [1, 2.5, true, null]}}]',
    ],
    'bibjson': [
        '{"metadata": {"records": 0}, "records": []}', '[]', '{}', '[{}]', '{"records": {"id": "a"}}', '{"id": "a", "title": "one"}', '"x"', '',
        '{"records": [{"type": "Article", "id": "k", "title": "T", "author": [{"name": "Doe, Jane"}, {"name": "Roe, R."}], "journal": {"name": "J"}}]}',
        '[{"type": "", "id": "k"}, {"id": "ITEM-2", "cid": "c3"}, {"cid": "ok"}]',
        '[{"_hidden": 1, "type": "book", "id": "b", "collection": "c", "link": [], "license": [], "identifier": [{"type": "DOI", "id": "10.1/x"}]}]',
        '[{"identifier": [{"type": "doi"}, {"type": "doi", "id": "10.2/y"}], "id": "a"}, {"doi": "10.1/z", "identifier": [{"type": "doi", "id": "10.9/q"}]}]',
        '[{"identifier": {"type": "doi", "id": "x"}}, {"identifier": ["a", {"type": "issn", "id": "1234-5678"}]}]',
        '[{"year": 2013, "volume": 4.0, "pages": 1e22, "month": null, "note": true, "x": false, "big": 9007199254740993}]',
        '[{"author": ["Doe, J.", {"name": "Roe"}, 5, null], "editor": {"name": "X"}, "keywords": ["a", "b", ["c", "d"]]}]',
        '[{"file": ["a.pdf", "file:///b.pdf", "http://x"], "id": "f"}, {"file": "p.pdf;q.pdf"}]',
        '[{"title": {"name": "T"}, "journal": "plain", "booktitle": {"x": 1}}]', '[{"title": {"x": 1}}]', '[{"title": []}, {"title": [[]]}]',
        '[{"Title": "Upper", "title": "lower"}]', '[{"type": "BOOK", "id": "a b"}]',
    ],
}


def import_case(fmt, text):
    "what papers reads in a text of a format: its entries, or the error"
    def dump(e):
        return {'type': e.entry_type, 'key': e.key, 'fields': [[f.key, f.value] for f in e.fields]}
    try:
        return {'fmt': fmt, 'in': text, 'entries': [dump(e) for e in F.loads(text, fmt)]}
    except Exception as error:
        return {'fmt': fmt, 'in': text, 'error': str(error), 'class': type(error).__name__}


def mutate_text(rng, text):
    "a text with a few random edits (a character removed, repeated, or a piece of text swapped)"
    pieces = ['\n', ' ', '"', ',', ':', '{', '}', '[', ']', 'null', 'true', '  - ', 'ER  - ', 'TY  - JOUR', 'TI  - x', '10.1/x', '2020']
    for _ in range(rng.randint(1, 3)):
        if not text:
            break
        i = rng.randrange(len(text))
        op = rng.randrange(4)
        if op == 0:
            text = text[:i] + text[i + 1:]
        elif op == 1:
            text = text[:i] + text[i] * 2 + text[i + 1:]
        elif op == 2:
            text = text[:i] + rng.choice(pieces) + text[i:]
        else:
            j = rng.randrange(len(text))
            text = text[:min(i, j)] + text[max(i, j):]
    return text


def main():
    rng = random.Random(23)
    atoms = value_atoms()

    names = []
    for s in NAME_FIELDS:
        parsed = F.parse_bibtex_names(s)
        names.append({'in': s, 'names': parsed, 'bibtex': F.format_bibtex_names(parsed),
                      'ris': [F._ris_name(n) for n in parsed]})
    ris_names = [{'in': s, 'out': F._bibtex_name_of_ris(s)} for s in RIS_NAMES]

    helpers = {
        'dates': [{'in': s, 'out': F._date_parts(s)} for s in DATES],
        'months': [{'in': s, 'out': F.parse_month(s)} for s in MONTHS],
        'pages': [{'in': s, 'bibtex': F._pages_to_bibtex(s), 'split': list(F._split_pages(s))} for s in PAGES],
        'dois': [{'in': s, 'out': F._clean_doi(s)} for s in DOIS],
        'keys': [{'in': s, 'out': F.usable_key(s)} for s in KEYS],
    }

    texts = [t for _, text in G.fixtures() for t in [text]] + HAND_ENTRIES
    entries = []
    for text in texts:
        lib = G.parse_string(text)
        for i, e in enumerate(lib.entries):
            # one record per entry: the entry alone, written as papers writes it
            entries.append(entry_record(format_entries([e]), rng.choice(FILES)))
    for n in range(500):
        entries.append(entry_record(entry_text(rng, atoms, n), rng.choice(FILES)))
    # all of them at once, for the separators between records and the empty list
    everything = [G.parse_string(r['in']).entries[0] for r in entries if r.get('count')]
    several = {'in': [r['in'] for r in entries if r.get('count')],
               'ris': F.dumps_ris(everything, lambda x: ['a/b.pdf']),
               'csl': F.dumps_csl(everything, lambda x: ['a/b.pdf']),
               'bibjson': F.dumps_bibjson(everything, lambda x: ['a/b.pdf'])}
    empty = {'ris': F.dumps_ris([], lambda x: []), 'csl': F.dumps_csl([], lambda x: []),
             'bibjson': F.dumps_bibjson([], lambda x: [])}

    scores = []
    words = ['ice', 'sheet', 'sea', 'level', 'amoc', 'climate', 'smith', 'doe', 'perrette', 'a', 'of', 'the', 'rise', 'future',
             'dynamics', 'model', 'models', 'tipping', 'point', 'x', 'é', 'δ']
    for _ in range(3000):
        a = ' '.join(rng.choice(words) for _ in range(rng.randint(0, 6)))
        b = ' '.join(rng.choice(words) for _ in range(rng.randint(0, 6))) if rng.random() < 0.8 else a + ' ' + rng.choice(words)
        if rng.random() < 0.3:
            b = b.upper() if rng.random() < 0.5 else b + b[:3]
        cutoff = rng.choice([None, 0, 25, 50, 66, 70, 75, 80, 85, 90, 100])
        score = token_set_ratio(a, b) if cutoff is None else token_set_ratio(a, b, score_cutoff=cutoff)
        scores.append({'a': a, 'b': b, 'cutoff': cutoff, 'score': score})
    # long texts, the title of a paper with small changes
    base = 'a scaling approach to sea level in gottingen and the dynamics of the atlantic meridional overturning circulation'
    for i in range(300):
        ws = base.split()
        b = ' '.join(w for w in ws if rng.random() < 0.85)
        if rng.random() < 0.5:
            b = b.replace('sea', 'seas')
        scores.append({'a': base, 'b': b, 'cutoff': 80, 'score': token_set_ratio(base, b, score_cutoff=80)})

    dois = [{'in': s, 'out': isvaliddoi(s)} for s in VALID_DOIS + DOIS + [d for d in rng_dois(rng)]]

    # the import: the texts that papers writes, and hand-written ones with the quirks of the readers
    imports = []
    for r in entries:
        if r.get('count'):
            for fmt, key in (('csl-json', 'csl'), ('ris', 'ris'), ('bibjson', 'bibjson')):
                imports.append(import_case(fmt, r[key]))
    for fmt, texts in HAND_IMPORTS.items():
        for text in texts:
            imports.append(import_case(fmt, text))
    imports.append(import_case('csl-json', several['csl']))
    imports.append(import_case('ris', several['ris']))
    imports.append(import_case('bibjson', several['bibjson']))
    for _ in range(300):
        fmt = rng.choice(['csl-json', 'ris', 'bibjson'])
        imports.append(import_case(fmt, mutate_text(rng, rng.choice(HAND_IMPORTS[fmt]))))

    out = {'names': names, 'ris_names': ris_names, 'helpers': helpers, 'entries': entries, 'several': several,
           'empty': empty, 'scores': scores, 'dois': dois, 'imports': imports}
    with open(os.path.join(G.EXPECTED, 'formats.json'), 'w') as f:
        json.dump(out, f, ensure_ascii=False, indent=0)
        f.write('\n')
    print(f'{len(names)} names, {len(entries)} entries, {len(scores)} scores, {len(dois)} DOIs', file=sys.stderr)


def rng_dois(rng):
    pieces = ['10.1234/', '10.5194/', 'doi:', 'DOI ', ' ', '\n', 'abc', 'x-1', '.', ')', '(', '-', '_', ';', ':', '/', '10.5281/zenodo.1',
              '.pdf', 'supplement', 'received', 'é', '–', '​', '10.', '1234/', '12345678901/', 'arxiv:', '2101.00001']
    for _ in range(400):
        yield ''.join(rng.choice(pieces) for _ in range(rng.randint(1, 5)))


if __name__ == '__main__':
    main()
