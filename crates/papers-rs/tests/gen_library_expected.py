#!/usr/bin/env python3
"""Generate tests/expected/library/ from the reference, papers-cli (Python).

The Rust tests (tests/names.rs, tests/identity.rs, tests/library.rs) compare
what papers-rs does with what is recorded here:

  names.json      slugify, path_safe, the title words, the key suffix, the name and key
                  templates, the names of the files of an entry, tags
  identity.json   normalization, identifiers, the same-paper verdicts (with and without
                  distinct.json), the search for duplicates, the groups
  merge.json      the merge of entries that are the same paper
  library.json    sequences of operations on a library (insert, remove, rename a key, tags,
                  files, check duplicates), the text saved, distinct.json
  files.json      the files of an entry placed by the name template, on a tree of files

The transliteration is the published table of papers (AnyAscii), tested in tests/tables.rs.

Run with the python that has papers installed (the reference), from anywhere:

    /path/to/python3 tests/gen_library_expected.py

It never touches the configuration or the libraries of the user: HOME and the cache are a
temporary folder.
"""
import contextlib
import io
import json
import logging
import os
import random
import shutil
import sys
import tempfile
import warnings

HERE = os.path.dirname(os.path.abspath(__file__))
TMP = tempfile.mkdtemp(prefix='papers-rs-expected-')
os.environ['HOME'] = TMP  # before papers is imported
os.environ['XDG_CACHE_HOME'] = os.path.join(TMP, 'cache')
os.environ['XDG_CONFIG_HOME'] = os.path.join(TMP, 'config')
os.chdir(TMP)
warnings.simplefilter('ignore')

import fnmatch  # noqa: E402
import unicodedata  # noqa: E402

import papers  # noqa: E402
from papers import duplicate as dup  # noqa: E402
from papers import identifiers as ident  # noqa: E402
from papers.bib import Biblio, _KEY_FORBIDDEN, append_abc, isvalidkey  # noqa: E402
from papers.cli import entry_cmds  # noqa: E402
from papers.entries import entry_from_dict, format_library, library_from_entries, parse_string  # noqa: E402
from papers.files import attachment_names  # noqa: E402
from papers.filename import (Format, TemplateError, fallback_key, fit_name, listtag, path_safe, safe_file_name,  # noqa: E402
                             slugify)
from papers.merge import Merge, Resolver  # noqa: E402

logging.disable(logging.CRITICAL)

EXPECTED = os.path.join(HERE, 'expected', 'library')


def write_json(name, data):
    os.makedirs(EXPECTED, exist_ok=True)
    with open(os.path.join(EXPECTED, name), 'w', encoding='utf-8') as f:
        if isinstance(data, dict):  # compact, one line per key
            f.write('{\n' + ',\n'.join(f'{json.dumps(k)}: {json.dumps(v, ensure_ascii=False, separators=(",", ":"))}'
                                       for k, v in data.items()) + '\n}\n')
        else:  # a list: one line per case
            f.write('[\n' + ',\n'.join(json.dumps(v, ensure_ascii=False, separators=(',', ':')) for v in data) + '\n]\n')


def entry_text(key, type_, fields):
    "the bibtex of an entry (values without unbalanced braces)"
    body = ''.join(f' {k} = {{{v}}},\n' for k, v in fields)
    return f'@{type_}{{{key},\n{body}}}\n'


def dump_entry(e):
    return {'type': e.entry_type, 'key': e.key, 'fields': [[f.key, f.value] for f in e.fields]}


def formatted(entries):
    "the text of entries as papers writes them (fields sorted)"
    return format_library(library_from_entries(entries))


# names
# =====

FRAGMENTS = ['Müller', "l'eau", '10.1000/xyz(1)', '1,000', '&amp;', '&eacute;', '&#233;', '&#x41;', '&#999999999;',
             '&#65;', '  ', '--', 'Ångström', 'straße', 'Œuvre', 'naïve', 'café', 'ﬁ', '½', '²', '①', "'", "''",
             'a,b', '1,2,3', 'x_y', 'A-B', 'É', '—', '…', ' ', '\t', '\n', '&', '&;', '&#;', '&#x;', '&amp',
             '&lt;', '&gt;', '&quot;', '&nbsp;', '&Aacute;', '&#x1F600;', '&#1633;', 'Ñandú', 'Łódź', 'Dvořák',
             'Sea level rise', 'The {CO2} problem', 'hello', 'World', '2020', 'a/b', 'a\\b', '..', '.', '( )',
             '[x]', 'ǅ', 'ĳ', 'ǆ', '℃', 'x²', 'ı', 'İ', 'ü', 'ö', 'ç', 'ñ', 'ß', 'æ', 'ø', 'þ', 'ð', 'đ', 'ħ',
             'Σ', 'α', 'Ж', 'я']


def fuzz_text(rng, n=(1, 5)):
    return ''.join(rng.choice(FRAGMENTS) for _ in range(rng.randint(*n)))


NAMES = ['Smith', 'Müller', 'van der Berg', '{van der Berg}', 'Ångström', "O'Brien", 'de la Cruz', 'Li', 'Nguyễn',
         'Perrette', 'Dvořák', 'Łukasz', 'Ünal', 'Ño', 'St-Pierre', 'Doe', 'M{\\"u}ller', 'Sch{\\"o}n', '{\\AA}ngstr{\\"o}m',
         '{Barnes and Noble}', '{Smith, Jones}', '{World Health Organization}', 'Fontaine, de la']
GIVEN = ['John', 'J.', 'Jane', 'J. D.', 'Mahé', 'Jörg', 'M.', 'Anne-Marie', 'A.-M.', 'Wei', 'Zoë', 'J{\\"o}rg', '', 'John {Q.} Adams', '{de la}']
TITLE_WORDS = ['climate', 'Sea', 'level', 'rise', 'of', 'the', 'ice', 'Sheets', 'in', 'a', 'warming', 'world', 'and',
               'AMOC', '{AMOC}', 'CO2', 'Über', 'München', 'naïve', 'résumé', "Hawai'i", '3.5', '1,000', 'x–y',
               'D{\\"u}sseldorf', '\\emph{Glacier}', 'tipping', 'points:', 'review']


def make_author(rng):
    n = rng.choice([1, 1, 2, 2, 3, 4])
    out = []
    for _ in range(n):
        family, given = rng.choice(NAMES), rng.choice(GIVEN)
        style = rng.randrange(3)
        if not given:
            out.append(family)
        elif style == 0:
            out.append(f'{family}, {given}')
        elif style == 1:
            out.append(f'{given} {family}')
        else:
            out.append(f'{family},{given}')
    return rng.choice([' and ', ' and ', ' and  ', ' AND ', '\nand ']).join(out) if len(out) > 1 else out[0]


def make_title(rng):
    return ' '.join(rng.choice(TITLE_WORDS) for _ in range(rng.randint(1, 9)))


def make_entry_fields(rng):
    "the fields of a random entry, some missing, some odd"
    fields = []
    if rng.random() < 0.85:
        fields.append(('author', make_author(rng)))
    if rng.random() < 0.85:
        fields.append(('title', make_title(rng)))
    if rng.random() < 0.8:
        fields.append(('year', rng.choice(['2020', '2019', '1999', '0000', '2020a', '/../x', '..', '.', 'n.d.'])))
    if rng.random() < 0.4:
        fields.append(('journal', rng.choice(['Nature', 'J. Clim.', 'The Cryosphere', 'a/b', 'Ocean & Sea', '..', 'Nature: "A|B" <x>?*', 'con.x', 'Nul',
                                                'é' * 130, 'x ', 'x.', '', 'a\x01b'])))
    if rng.random() < 0.5:
        fields.append(('doi', rng.choice(['10.1000/abc', '10.1000/ABC', 'https://doi.org/10.1000/abc', 'doi: 10.1000/abc',
                                          '10.5194/tc-14-1-2020', '10.1000/x(1)', 'dx', '', '10.48550/arXiv.2101.01234',
                                          'http://dx.doi.org/10.1000/def', '10.1/a/b', '  ', ' ', '\t'])))
    return fields


def names_cases():
    rng = random.Random(11)
    out = {}

    strings = ['', 'Müller', "l'eau", '10.1000/xyz(1)', '1,000', 'Hello World', '  a  b  ', '&amp; &#233; &#x41;',
               '--a--b--', "a''b", "'a'", 'ÅÄÖ åäö', 'ΑΒΓ αβγ', 'Привет', '日本語', '한국어', 'ﬃ ﬀ', 'x²+y³', '①②',
               '1,2,3', '1, 2', ',1,', '&#233;', '&eacute;', '&#999999999999;', '&#x110000;', '&#0;', '&#xD800;',
               '&&amp;', '&amp;amp;', '&#38;amp;', 'a&b', '&#65', '&#;', "O'Brien", 'D’Arcy', 'İstanbul', 'Straße',
               # html references (library.md, Slugify)
               '&eacute;t&eacute;', 'l&#39;eau', "l&#39;&#39;eau", '&#X41;&#x41;&#65;', '&#1114112;', '&#x10FFFF;', '&#x110000;',
               'a&#65;b &#1114112; &#66;c', '&amp;#38;', '&#38;#38;', '&apos;', '&Eacute;&eacute;&EACUTE;', '&#00000000065;',
               '&#4294967361;', '&#xD7FF;&#xD800;&#xDFFF;&#xE000;', '&#1633;', '&#x٣;', '&#٣;', '&#;&#x;&#X;', '&nbsp;x&nbsp;',
               '&#9;', '&#x20;a', '&lt;b&gt;', '&hearts;&euro;&frac12;', '&sup2;', '&AMP;', '&amp', '& amp;', '&#x41', '&#X',
               'x˯x', 'Ь', 'ЬЬ', 'D’e', '日本語&amp;中文', '１,２', '１２,３４', '٣,٤', '1،2']
    for _ in range(1500):
        strings.append(fuzz_text(rng))
    seen, items = set(), []
    for s in strings:
        if s not in seen:
            seen.add(s)
            items.append([s, slugify(s)])
    out['slugify'] = items

    # the key of a new entry that the key template gives an empty key (bibtex.md 8)
    fb = []
    for title, year in [('Only a title', '2000'), ('Only a title', ''), ('A b', ''), ('A b', '2000'), ('100 years', '2000'),
                        ('100', '2000'), ('', '2000'), ('', ''), ('The {CO2} problem', '1999'), ('3 ab 4 cde', '2000'),
                        ('a bb ccc dddd', '2000'), ('ab cd', 'n.d.'), ('ab cd', '2000a'), ('Über München', '2020'),
                        ("O'Brien's", ''), ('&eacute;t&eacute; &amp; x', '2000'), ('x', '/../x'), ('٣ ab', '2000'), ('²b ab', ''),
                        ('日本語', '2000'), ('--', '2000'), ('12ab abc', ''), ('a1 b2c', '٢٠٢٠')]:
        fb.append([title, year, fallback_key(parse_string('@misc{x, title = {%s}, year = {%s}}' % (title, year)).entries[0])])
    out['fallback_key'] = fb

    sfn = ['', 'a', 'a:b', 'a/b', '/a', '//a', 'a//b', './a', '../a', 'a/./b', 'a?b*c|d<e>f"g', 'a\x01\x1fb', 'a.', 'a ', 'a. .', '...', '. ', '.hidden',
           'CON', 'con', 'Con.txt', 'a/NUL/b', 'nul.tar.gz', 'COM1', 'com9.x', 'COM0', 'LPT3', 'lpt3.', 'AUX ', 'CONX', 'x.con',
           'é' * 200, 'é' * 124 + 'a', 'a' * 300, 'a' * 250, 'a' * 251, ('a' * 300) + '/' + ('é' * 200), 'CON' + 'a' * 300, ('a' * 249) + '.',
           'a' * 248 + '. ', 'x/' + 'y' * 260 + '.pdf']
    out['safe_file_name'] = [[x, safe_file_name(x)] for x in sfn]
    fits = ['a.pdf', '/lib/files/a.pdf', 'a' * 300 + '.pdf', '/lib/' + 'é' * 200 + '.pdf', 'x/' + 'a' * 250 + '.tar.gz', 'a' * 251,
            '/lib/' + 'a' * 247 + '.pdf', '/lib/' + 'a' * 246 + '.pdf', '/lib/' + 'a' * 300 + '.' + 'b' * 300, '.' + 'a' * 300,
            '/lib/' + 'a' * 240 + ' .pdf', '/lib/' + 'a' * 300, '/lib/' + 'a' * 245 + '.. .pdf', 'é' * 130 + '.pdf', '/' + 'a' * 260]
    out['fit_name'] = [[x, fit_name(x)] for x in fits]

    out['path_safe'] = [[s, path_safe(s)] for s in ['', 'a', 'a/b', 'a\\b', 'a\0b', '.', '..', '...', '2013', '.a', 'a.',
                                                    '/../../x', '..\\..', ' .. ', ' ']]

    cases = []
    for _ in range(300):
        words = [rng.choice(['a', 'bb', 'ccc', 'climate', 'sensitivity', 'revisited', '', 'x', 'ocean']) for _ in range(rng.randint(0, 6))]
        args = (rng.choice([0, 1, 5, 10, 20, 30, 100, -1]), rng.choice([0, 1, 2, 3]), rng.choice([1, 2, 3, 100, 0, -1, -2]),
                rng.choice(['-', '--', '_', '']))
        cases.append([words, *args, listtag(words, maxlength=args[0], minwordlen=args[1], n=args[2], sep=args[3])])
    out['listtag'] = cases

    abc = []
    for key, keys in [('Author2000', []), ('Author2000b', []), ('Author2000', ['Author2000', 'Author2000b']),
                      ('Author2000', ['author2000', 'author2000b']), ('perrette', ['perrette']), ('10-1000-abc', ['10-1000-abc']),
                      ('Author2000z', ['Author2000z']), ('Ab', ['ab']), ('Author2000B', []), ('x1', []), ('a', []), ('', []),
                      ('É2000', []), ('Author2000a', []), ('Author2000y', ['author2000z']), ('Ü9z', []),
                      ('Author2000Z', []), ('Author2000z', []), ('Author2000B', ['author2000c']), ('Author2000Y', ['author2000z']),
                      ('Ab', ['ab'] + ['ab' + c for c in 'bcdefghijklmnopqrstuvwxyz']),
                      ('Author2000z', ['author2000za']), ('Author2000Z', ['author2000za', 'author2000zb']),
                      ('Author2000', ['author2000'] + ['author2000' + c for c in 'bcdefghijklmnopqrstuvwxyz']),
                      ('Author2000z', ['author2000z'] + ['author2000z' + c for c in 'abcdefghijklmnopqrstuvwxyz']),
                      ('1Z', []), ('x.B', []), ('K2000K', []), ('smith2000e', ['smith2000e', 'smith2000f']),
                      # only the ASCII digits make a letter a suffix
                      ('٣b', []), ('x٣z', []), ('x²b', []), ('x３B', []), ('x9b', []), ('x٣z', ['x٣zb'])]:
        try:
            r = append_abc(key, keys)
        except ValueError as e:
            r = {'error': str(e)}
        abc.append([key, keys, r])
    for _ in range(100):
        base = rng.choice(['smith2020', 'Smith2020', 'smith2020b', 'smith', 'a', 'x1', 'doe_smith2020c', 'k9', 'Smith2020B', 'smith2020z', 'Smith2020Y'])
        letters = 'abcdefghijklmnopqrstuvwxyz'
        keys = [base + rng.choice(letters) for _ in range(rng.randint(0, 30))] + [base]
        try:
            r = append_abc(base, keys)
        except ValueError as e:
            r = {'error': str(e)}
        abc.append([base, keys, r])
    out['append_abc'] = abc

    keys = ['', 'a', 'Author2020', '2020Author', '10.5194/bg', 'a b', 'a,b', 'a{b', 'a}b', 'a"b', 'a#b', 'a%b', 'a~b',
            'a=b', 'a(b', 'a)b', 'a\\b', 'a\tb', 'a b', '²a', '٣a', 'é', 'a-b_c:d', 'a/b', '0a', '9a', '３a', '٣', '²', '१a']
    out['valid_key'] = [[k, bool(isvalidkey(k)) and not _KEY_FORBIDDEN.search(k)] for k in keys]

    # the name and key templates
    fmt = []
    templates = ['{authorX}_{year}_{title}', '{author}{year}', '{Author}{year}-{Title}', '{AUTHOR}', '{year}/{Author}{year}',
                 '{key}', '{ID}', '{doi}', '{doi_}', '{doi_or_id}', '{journal}', '{AuthorX}', '{{x}}{year}', '{title:.8}',
                 '{author!r}', '{year:>8}', '{year:08}', '{author:*^11}', '{author[0]}', '{xxx}', '{}', '{0}', '{year', 'a}',
                 '{year!x}', '{year:d}', '{author.real}', '{title:{year}}', '../{author}', '/tmp/{author}', '{author}/../../x',
                 '.', '', '{journal:>5}', '{journal!s}', '{journal!r}', '{year:#}', '{year:+}', '{year:,}', '{year:10.3s}',
                 '{ year }', '{year!}', '{year!r:}', '{year:}', '{year[1:2]}', '{year[9]}', '{author}{', '}{author}', '{{}}',
                 '{year:{}}', '{year:{{}}}', '{journal}/{author}_{year}', '{journal}/{journal}', '/{journal}/{author}', 'x/{journal}',
                 '{author}{journal}', '{journal}/{Title}', '//{author}', '{journal}//{author}', '{journal}.{journal}', '{author}.',
                 '{title}/{journal}.pdf', '{doi}', 'a:b', 'a?b', '{author}:{year}']
    for i in range(400):
        template = rng.choice(templates)
        kind = rng.choice(['name', 'key'])
        options = dict(
            author_num=rng.choice([0, 1, 2, 3, 10, -1]), author_sep=rng.choice(['_', '-', '', '+', 'é']),
            title_word_num=rng.choice([0, 1, 2, 5, 100, -1]), title_word_size=rng.choice([0, 1, 2, 3]),
            title_length=rng.choice([0, 5, 10, 30, 100]), title_sep=rng.choice(['-', '_', '', '--']),
            unknown_strict=rng.random() < 0.3,
            unknown_template=rng.choice([None, '', 'unknown_{doi_or_id}', '{doi_}', '{xx}', '{key}']))
        if i < 120:
            options = {}
        type_ = rng.choice(['article', 'book', 'misc'])
        fields = make_entry_fields(rng)
        key = rng.choice(['smith2020', 'a/b', '..', 'Müller2000', 'k'])
        if rng.random() < 0.15:
            fields = [f for f in fields if f[0] != 'author'] + [('author', 'unknown')]
        entry = entry_from_dict(dict(fields, ID=key, ENTRYTYPE=type_))
        f = Format(template, kind=kind, **options)
        try:
            r = f(entry)
            err = None
        except TemplateError as e:
            r, err = None, str(e)
        except (AttributeError, TypeError) as e:  # what papers does not catch
            r, err = None, 'PYTHON ' + type(e).__name__
        fmt.append({'kind': kind, 'template': template, 'options': options, 'entry': dump_entry(entry), 'out': r, 'error': err})
    # the defaults of papers
    for kind in ('name', 'key'):
        from papers.filename import KEYFORMAT, NAMEFORMAT
        for _ in range(150):
            fields = make_entry_fields(rng)
            entry = entry_from_dict(dict(fields, ID='k', ENTRYTYPE='article'))
            f = NAMEFORMAT if kind == 'name' else KEYFORMAT
            fmt.append({'kind': kind, 'template': f.template, 'options': {}, 'entry': dump_entry(entry), 'out': f(entry),
                        'error': None, 'defaults': True})
    out['format'] = fmt

    att = []
    for files, target in [(['/x/a.pdf'], '/l/f/doe'), (['/x/a.pdf', '/x/notes.txt'], '/l/f/doe'),
                          (['/x/doe.pdf', '/x/doe--notes.txt'], '/l/f/doe'), (['/x/old.pdf', '/x/old--supp.zip'], '/l/f/new'),
                          (['/x/a.pdf', '/x/s.pdf', '/x/s.txt', '/x/s.pdf'], '/l/f/doe'), (['/x/.hidden', '/x/b'], '/l/f/doe'),
                          (['/x/a.tar.gz', '/x/b.TXT'], '/l/f/doe'), (['/x/doe--.pdf', '/x/doe--.txt'], '/l/f/doe'),
                          (['/x/a.PDF', '/x/doe--a.pdf'], '/l/f/doe'), (['a.pdf', 'b'], 'doe'), (['/x/a.pdf', '/y/a.pdf', '/z/a.pdf'], '/l/f/a'),
                          (['/x/a.pdf', '/a/supplement.pdf', '/b/supplement.pdf'], '/l/f/' + 'a' * 240),
                          (['/x/a.pdf', '/a/' + 'é' * 100 + '.pdf', '/b/' + 'é' * 100 + '.pdf'], '/l/f/' + 'b' * 200),
                          (['/x/a.pdf'], '/l/f/' + 'c' * 300), (['/x/a.pdf', '/x/b.pdf'], '/l/f/' + 'é' * 160)]:
        att.append([files, target, attachment_names(files, target)])
    out['attachment_names'] = att

    inside = []
    for fd, name in [('/lib/files', '2020/doe'), ('/lib/files', '../doe'), ('/lib/files', '.'), ('/lib/files', '/tmp/doe'),
                     ('/lib/files', 'a/../../x'), ('/lib/files', 'a/..'), ('/lib/files/', 'x'), ('/lib/files', ''),
                     ('/lib/files', 'files/x'), ('/lib/files', '../files/x'), ('/lib/files', 'a//b/./c'), ('/lib/files', '..\\x')]:
        target = os.path.normpath(os.path.join(os.path.abspath(fd), name))
        fda = os.path.abspath(fd)
        ok = os.path.commonpath([fda, target]) == fda and target != fda
        inside.append([fd, name, target if ok else None])
    out['target_in'] = inside

    tags = []
    for tag, pattern in [('climate/amoc', 'climate'), ('climate', 'Climate/'), ('ice-review', 'ice*'), ('ice-review', 'ice'),
                         ('a/b/c', 'a/b'), ('a/b', 'a/*'), ('AB', 'a[!c]'), ('ac', 'a[!c]'), ('a-b', 'a-[a-c]'), ('a]', 'a[]]'),
                         ('a', '[a'), ('a', '[!]'), ('a!', 'a[!]'), ('ab', '[a-b][a-b]'), ('x', '[z-a]'), ('?', '[?]'),
                         ('a*', 'a[*]'), ('', ''), ('', '*'), ('a', '**'), ('abc', 'a*c'), ('abcabc', 'a*c*c'), ('abc', '*b*'),
                         ('a/b', 'a//'), ('é', 'É'), ('x', '[]x]'), (']', '[]x]'), ('^', '[^a]'), ('b', '[^a]'), ('-', '[-a]'),
                         ('-', '[a-]'), ('c', '[a-c-e]'), ('d', '[a-c-e]'), ('e', '[a-c-e]'), ('-', '[a-c-e]'), ('\\', '[\\]')]:
        tags.append([tag, pattern, entry_cmds.tag_matches(tag, pattern)])
    for _ in range(300):
        alphabet = 'ab/*?[]!-^'
        pattern = ''.join(rng.choice(alphabet) for _ in range(rng.randint(0, 6)))
        tag = ''.join(rng.choice('ab/-]^!') for _ in range(rng.randint(0, 6)))
        tags.append([tag, pattern, entry_cmds.tag_matches(tag, pattern)])
    out['tag_matches'] = tags
    out['tag_parents'] = [[t, entry_cmds.tag_parents(t)] for t in ['a', 'a/b', 'a/b/c', '/a', 'a//b', 'a/', '', '/']]

    ops = []
    for _ in range(300):
        old = rng.choice(['a', 'a/b', 'b', 'c', 'A', 'A/B', 'B'])
        new = rng.choice(['x', 'x/y', 'a', 'b', 'a/b', 'X', 'A', 'B/c'])
        e = entry_from_dict({'ID': 'k', 'ENTRYTYPE': 'article',
                             'keywords': ', '.join(rng.choice(['a', 'a/b', 'a/b/c', 'b', 'c', 'x', 'x/y', 'ab', 'A', 'A/b', 'B', 'X/Y']) for _ in range(rng.randint(0, 5)))})
        before = e['keywords'] if 'keywords' in e else None
        e2 = entry_from_dict(dict(e.items()))
        added = [rng.choice(['a', 'z', 'z', 'a/b', 'q', 'A', 'Z', 'A/B']) for _ in range(rng.randint(0, 3))]
        removed = [rng.choice(['a', 'b', 'x', 'z', 'A', 'B', 'X']) for _ in range(rng.randint(0, 2))]
        entry_cmds.rename_tag([e], old, new)
        renamed = e['keywords'] if 'keywords' in e else None
        entry_cmds.add_tags([e2], added)
        added_res = e2['keywords'] if 'keywords' in e2 else None
        e3 = entry_from_dict(dict(e2.items()))
        entry_cmds.remove_tags([e3], removed)
        removed_res = e3['keywords'] if 'keywords' in e3 else None
        ops.append({'keywords': before, 'rename': [old, new, renamed], 'add': [added, added_res], 'remove': [removed, removed_res]})
    out['tag_ops'] = ops
    counts = []
    es = [entry_from_dict({'ID': f'k{i}', 'ENTRYTYPE': 'article', 'keywords': ', '.join(rng.choice(['a', 'a/b', 'a/b/c', 'b', 'c/d', '/x', 'a//b']) for _ in range(rng.randint(0, 4)))}) for i in range(30)]
    counts = sorted(entry_cmds.tag_counts(es).items())
    out['tag_counts'] = {'keywords': [e['keywords'] if 'keywords' in e else None for e in es], 'counts': counts}
    return out


# identity
# ========

def variant_of(rng, fields):
    "a variant of the fields of an entry that is, or may be, the same paper"
    out = []
    for k, v in fields:
        r = rng.random()
        if k == 'author' and r < 0.5:
            v = rng.choice([v.upper(), v.replace(',', ''), v.replace('Müller', 'M{\\"u}ller'), v.replace('ü', 'u'),
                            ' and '.join(n.split(',')[0] + ', ' + n.split(',')[-1].strip()[:1] + '.' for n in v.split(' and ')),
                            v + ' and Extra, A.'])
        elif k == 'title' and r < 0.5:
            v = rng.choice([v.lower(), v.upper(), '{' + v + '}', v.replace(' ', '  '), 'Data for: ' + v, v + '.',
                            v.replace('Über', '{\\"U}ber')])
        elif k == 'doi' and r < 0.5:
            v = rng.choice([v.upper(), 'https://doi.org/' + v, 'doi:' + v, ' ' + v + ' ', v + 'x'])
        elif r < 0.1:
            continue
        out.append((k, v))
    return out


ARXIVS = ['2101.01234', '2101.01234v2', 'arXiv:2101.01234', 'hep-th/9901001', 'math.AG/0601001', '1999.9999', '21011.1234',
          'https://arxiv.org/abs/2101.01234v3', 'http://arxiv.org/pdf/2101.01234.pdf', 'ARXIV: 2101.01234', 'x', '']
ISBNS = ['978-3-16-148410-0', '3-16-148410-X', '0-306-40615-2', '978-3-16-148410-0, 0-306-40615-2', 'bad', '9780306406157',
         'ISBN 0-306-40615-2', 'ISBN: 978-0-306-40615-7', '0-306-40615-3', '', '978 0 306 40615 7; 3-16-148410-X', 'isbn:0306406152']


def make_library_entries(rng, n=160):
    "random entries with planted duplicates, as (key, type, fields)"
    entries = []
    for i in range(n):
        if entries and rng.random() < 0.35:
            _, t, f = rng.choice(entries)
            fields = variant_of(rng, f)
            t = rng.choice([t, t, t, 'misc', 'book', 'dataset'])
        else:
            t = rng.choice(['article', 'article', 'book', 'incollection', 'misc', 'dataset'])
            fields = make_entry_fields(rng)
            if rng.random() < 0.2:
                fields.append(('eprint', rng.choice(ARXIVS)))
                if rng.random() < 0.5:
                    fields.append(('archiveprefix', rng.choice(['arXiv', 'other', 'ARXIV', ' arxiv '])))
            if rng.random() < 0.25:
                fields.append(('isbn', rng.choice(ISBNS)))
        if rng.random() < 0.1:
            fields.append(('file', rng.choice([':files/a.pdf:pdf', ':files/a.pdf:pdf;:files/b.pdf:pdf', 'files/c.pdf', ':../x/y.pdf:pdf',
                                               'Desc:files/d: e.pdf:pdf'])))
        if rng.random() < 0.1:
            fields.append(('keywords', rng.choice(['a, b', 'a/b, c', 'x', ' a ,, b ,'])))
        if rng.random() < 0.05:
            fields.append(('ids', rng.choice(['old', 'old1, old2', ' old '])))
        # no repeated field names, and nothing the bibtex syntax cannot hold
        seen, fields2 = set(), []
        for k, v in fields:
            if k not in seen:
                seen.add(k)
                fields2.append((k, v))
        entries.append((f'k{i}', t, fields2))
    return entries


def pyentries(entries):
    text = '\n'.join(entry_text(k, t, f) for k, t, f in entries)
    return text, parse_string(text).entries


def v(verdict):
    return [verdict.kind, verdict.reason]


def identity_cases():
    rng = random.Random(5)
    out = {}
    entries = make_library_entries(rng)
    text, pe = pyentries(entries)
    out['bibtex'] = text
    out['identifiers'] = []
    for e in pe:
        i = dup.identifiers_of(e)
        out['identifiers'].append({'key': e.key, 'type': i.type, 'doi': i.doi, 'arxiv': i.arxiv, 'isbns': sorted(i.isbns),
                                   'author': i.author, 'title': i.title, 'authortitle': i.authortitle,
                                   'identity': dup.identity(e), 'entry_arxiv': ident.entry_arxiv(e)})
    # pairs, without and with distinct
    pairs = []
    for _ in range(2000):
        a, b = rng.sample(range(len(pe)), 2)
        pairs.append([a, b, v(dup.same_paper(pe[a], pe[b]))])
    # pairs of the planted kind: close entries
    out['pairs'] = pairs
    dpairs = []
    for _ in range(25):
        a, b = rng.sample(range(len(pe)), 2)
        ia, ib = dup.identity(pe[a]), dup.identity(pe[b])
        if ia and ib:
            dpairs.append([ia, ib])
    distinct = dup.Distinct([list(p) for p in dpairs])
    out['distinct_pairs'] = dpairs
    out['pairs_distinct'] = [[a, b, v(dup.same_paper(pe[a], pe[b], distinct))] for a, b, _ in pairs]
    out['distinct_text'] = distinct_text(distinct)
    index = dup.IdentityIndex(pe)
    out['find'] = []
    out['find_distinct'] = []
    for i, e in enumerate(pe):
        out['find'].append([[pe.index(x), *v(verdict)] for x, verdict in dup.find_duplicates(e, pe, None)])
        out['find_distinct'].append([[pe.index(x), *v(verdict)] for x, verdict in dup.find_duplicates(e, pe, distinct)])
    for name, d in (('groups', None), ('groups_distinct', distinct)):
        out[name] = [[[pe.index(e) for e in g.entries], g.kind, g.reason] for g in dup.duplicate_groups(pe, d)]

    # strings
    texts = ['', 'Müller', 'M{\\"u}ller', 'The {CO2}   Problem', '{\\AA}ngstr{\\"o}m', 'Über alles', '{\\"U}ber alles', 'ﬁnite', 'a b',
             'x\ty\nz', '\\url{x}', 'T\\^ete', "\\'e", "{\\'e}", 'ß', 'Ñ', 'ǅ', '日本', 'a{b}c', '}{', '\\textbf{x}', '{\\ss}',
             '\\v{S}koda', 'A{\\dh}algeirsd{\\\'o}ttir', '$\\alpha$-decay', 'CO$_2$', 'a~b', 'a--b', 'a---b', '``q\'\'', '\\emph{x} \\textit{y}',
             '\\o', '{\\o}', '\\"{\\i}', "\\'\\i", '\\AA ngstr\\"om', 'T{\\"u}rkiye', 'm\\"uller', '\\foo{bar}', 'a\\&b', '\\ss  x']
    texts += [fuzz_text(rng) for _ in range(300)]
    out['norm'] = [[t, dup.normalize_text(t)] for t in texts]
    names = ['Smith, John and Doe, Jane', 'John Smith and Jane Doe', 'A and B', 'A and and B', 'A  and  B', 'A\tand\nB',
             'and', ' and ', 'A and', 'and B', 'A AND B', 'A, B and C, D and E, F', '', '   ', 'Smith, J. and', 'x andy', 'x and y',
             'a and b', 'Jane {van der Berg}', 'Smith, John', 'John Quincy Adams', '{van der Berg}, Jane', 'Cher', 'A\nB C',
             'Doe,  Jane', 'Doe,Jane', ',', 'a,b,c', 'John {van} {der Berg}', 'John {a{b}c}', 'John{x}', '{x}', '{', '}', 'John {}',
             '{Barnes and Noble} and Smith, J.', '{Smith, Jones and Co}', '{Smith, Jones}, Ltd', '{Barnes and Noble}',
             'Smith, John and {Barnes and Noble}', '{a and b', 'a} and b', '{a} and {b}', 'x {and} y', 'x {a and} y and z',
             '{World Health Organization}', 'John {van der Berg} and {Intergovernmental Panel, on Climate}', 'A {\\"U}ber and B']
    names += [make_author(rng) for _ in range(150)]
    out['split_names'] = [[n, dup.split_names(n)] for n in names]
    parsed = []
    for n in names:
        for part in dup.split_names(n) + [n]:
            parsed.append([part, list(dup.parse_name(part))])
    out['parse_name'] = parsed
    out['family_name_ids'] = [[n, dup.family_name_ids(n)] for n in names]

    dois = ['10.1000/abc', '10.1000/ABC', 'https://doi.org/10.1000/abc', 'HTTP://DX.DOI.ORG/10.1/X', 'doi:10.1/x', 'DOI:   10.1/x',
            'doi.org/10.1/x', '10.1', '10./', '10.1/', ' 10.1/x ', 'https://dx.doi.org/10.1/x', 'https://doi.org/ 10.1/x', 'ftp://doi.org/10.1/x',
            '10.48550/arXiv.2101.01234', '', 'doi:', 'x/10.1/y', 'https://doi.org/doi:10.1/x', 'doi: doi:10.1/x', '10.1000/É']
    out['doi'] = [[d, ident.strip_doi(d), ident.normalize_doi(d), ident.arxiv_of_doi(d)] for d in dois]
    arx = ARXIVS + ['2101.0123', '2101.012345', '2101.01234v', '2101.01234v1v2', '0000.00000', 'hep-th/9901001v1', 'hep-th/99010011',
                    'hep-th/990100', 'math.AGx/0601001', 'math.AG/0601001', 'a/1234567', 'A/1234567', '-a/1234567', 'a-b/1234567v12',
                    'arxiv:  hep-th/9901001', 'https://arxiv.org/abs/hep-th/9901001v2', 'https://arxiv.org/pdf/2101.01234v2.PDF',
                    '2101.01234.pdf', ' 2101.01234 ', '2101.01234\n', 'ARXIV:2101.01234', 'http://arxiv.org/x/2101.01234', '٢١٠١.٠١٢٣٤',
                    '２１０１.０１２３４', '2101.0123４', '2101.01234v٢', 'hep-th/٩٩٠١٠٠١', 'hep-th/9901001v１', 'hep-th/９９０１００１']
    out['arxiv'] = [[a, ident.normalize_arxiv(a)] for a in arx]
    isbns = ISBNS + ['0-8044-2957-X', '080442957X', '080442957x', '0-8044-2957-x', 'ISBN0306406152', 'ISBN::0306406152', '123456789X',
                     '1234567890123', '978030640615', '97803064061577', 'abc', ' 978-0-306-40615-7 ', '٠٣٠٦٤٠٦١٥٢', '9780306406157\n',
                     '９７８０３０６４０６１５７', '٩٧٨٠٣٠٦٤٠٦١٥٧', '030640615２', '978030640615７', '0-8044-2957-Ｘ', 'ISBN 978-0-306-40615-7']
    out['isbn'] = [[i, ident.normalize_isbn(i)] for i in isbns]
    args = ['smith2020', 'Smith2020', '10.1000/abc', 'doi:10.1000/ABC', '2101.01234', 'arXiv:2101.01234', '9780306406157',
            '0-306-40615-2', '2020smith', '  Key  ', '', '10.5', 'hep-th/9901001', '٣abc', '²abc', '３abc', '९ab', '0abc', '9abc', ' 5abc',
            '٢١٠١.٠١٢٣٤', '９７８０３０６４０６１٥٧']
    parsed = []
    for a in args:
        try:
            parsed.append([a, list(ident.parse_identifier(a))])
        except ValueError as e:
            parsed.append([a, {'error': str(e)}])
    out['parse_identifier'] = parsed
    out['entry_identifiers'] = [[e.key, sorted(map(list, ident.entry_identifiers(e)))] for e in pe[:80]]
    # selections on the library
    select = []
    keys = [e.key for e in pe]
    queries = [['k1'], ['K1'], ['k1', 'k2', 'k1'], ['nothere'], ['10.1000/abc'], ['2101.01234'], ['old'], ['old1'], ['9780306406157'],
               ['3-16-148410-X'], ['1abc'], ['k1', '1abc', 'nothere'], ['10.1000/ABC'], ['hep-th/9901001'], ['10.48550/arXiv.2101.01234']]
    biblio = Biblio(library_from_entries(pe))
    for q in queries:
        try:
            found, missing = ident.select_some(biblio, q)
            select.append([q, [e.key for e in found], missing])
        except Exception as e:  # PapersExit
            select.append([q, {'error': str(e)}])
    out['select'] = select
    return out


def distinct_text(distinct):
    with tempfile.TemporaryDirectory() as d:
        path = os.path.join(d, 'distinct.json')
        distinct.changed = True
        distinct.save(path)
        with open(path) as f:
            return f.read()


# merge
# =====

RAW_MERGES = [
    '@article{a, month = jan, title = {T}}\n@article{b, title = {T}, year = {2000}}\n',
    '@article{a, title = {T}, year = {2000}}\n@article{b, month = jan, title = {T}}\n',
    '@article{a, month = jan, title = {T}}\n@article{b, month = {jan}, title = {T}}\n',
    '@article{a, month = {jan}, title = {T}}\n@article{b, month = jan, title = {T}}\n',
    '@article{a, month = jan, title = {T}}\n@article{b, month = feb, title = {T}}\n',
    '@article{a, month = {jan}, title = {T}}\n@article{b, month = jan, title = {T}}\n@article{c, month = jan, title = {T}}\n',
    '@article{a, year = 2000, title = {T}}\n@article{b, year = {2000}, title = {T}}\n',
    '@string{s = {Nature}}\n@article{a, journal = s, title = {T}}\n@article{b, journal = {Science}, title = {T}}\n',
    '@string{s = {Nature}}\n@article{a, journal = {Science}, title = {T}}\n@article{b, journal = s, title = {T}}\n',
    '@article{a, note = {}, title = {T}}\n@article{b, pages = {}, title = {T}}\n',
    '@article{a, title = {T}}\n@article{b, pages = {}, note = {x}, title = {T}}\n',
    '@article{a, pages = {}, title = {T}}\n@article{b, pages = {1--2}, title = {T}}\n',
    '@article{a, pages = {1--2}, title = {T}}\n@article{b, pages = {}, title = {T}}\n',
    '@article{a, author = {Smith, J.}, title = {T}}\n@article{b, author = {Smith, John Quincy}, title = {T}}\n',
    '@article{a, author = {Smith, John Quincy}, title = {T}}\n@article{b, author = {Smith, J.}, title = {T}}\n',
    '@article{a, author = {Smith, J. and {Barnes and Noble}}, title = {T}}\n@article{b, author = {Smith, John and {Barnes and Noble}}, title = {T}}\n',
    '@article{a, author = {John {van der Berg}}, title = {T}}\n@article{b, author = {{van der Berg}, J.}, title = {T}}\n',
    '@article{a, author = {Smith, John}, title = {T}}\n@article{b, author = {Smith, John D.}, title = {T}}\n',
    '@article{a, author = {Smith, John D.}, title = {T}}\n@article{b, author = {Smith, Jo}, title = {T}}\n',
]


def describe_merge(text, es, keys_to_ids, rel):
    m = Merge(es, relative_to=rel, keys_to_ids=keys_to_ids)
    case = {'bibtex': text, 'keys_to_ids': keys_to_ids, 'relative_to': rel}
    case['fields'] = [[x.name, x.status, x.values, x.value, x.alternatives()] for x in m.fields]
    case['files'] = m.files
    case['new_files'] = m.new_files
    case['ids'] = m.ids
    case['ids_added'] = m.ids_added
    case['exact'] = m.is_exact()
    case['result'] = formatted([m.result()])
    for choice in (0, 1):
        m2 = Merge(es, relative_to=rel, keys_to_ids=keys_to_ids)
        m2.choose_all(choice)
        case[f'choose{choice}'] = formatted([m2.result()])
    if len(es) > 1:
        m3 = Merge(es, relative_to=rel, keys_to_ids=keys_to_ids).with_target(1)
        case['with_target1'] = formatted([m3.result()])
    return case


def merge_cases():
    rng = random.Random(9)
    cases = []
    base = make_library_entries(rng, 120)
    for i in range(220):
        _, t, f = rng.choice(base)
        group = [(f'a{i}', t, f)]
        for j in range(rng.choice([1, 1, 1, 2, 3])):
            group.append((f'b{i}_{j}', rng.choice([t, t, 'misc']), variant_of(rng, f)))
        for n, (k, tt, ff) in enumerate(group):
            ff = list(ff)
            if rng.random() < 0.3:
                ff.append(('ids', rng.choice(['old', 'a0, b', k, 'Old, old2'])))
            if rng.random() < 0.3:
                ff.append(('file', rng.choice([':/lib/files/a.pdf:pdf', ':/lib/files/a.pdf:pdf;:/lib/other/b.pdf:pdf', ':/lib/./files/../files/a.pdf:pdf',
                                               'x/c.pdf', ':/lib/files/a.pdf:'])))
            if rng.random() < 0.15:
                ff.append(('pages', rng.choice(['', '  ', '1--2', '1-2'])))
            seen, f2 = set(), []
            for kk, vv in ff:
                if kk not in seen:
                    seen.add(kk)
                    f2.append((kk, vv))
            group[n] = (k, tt, f2)
        text, es = pyentries(group)
        keys_to_ids = rng.random() < 0.5
        rel = rng.choice(['/lib', '/lib/sub', None])
        cases.append(describe_merge(text, es, keys_to_ids, rel))
    # texts written by hand: bare values (month = jan, a @string), empty fields, given names
    for text in RAW_MERGES:
        es = parse_string(text).entries
        for keys_to_ids, rel in ((False, None), (True, '/lib')):
            cases.append(describe_merge(text, es, keys_to_ids, rel))
    out = {'cases': cases}
    names = [(a, b) for a in ['Smith, John', 'J. Smith', 'Smith, J.', 'Smith, John D.', 'John Smith', 'Smith, Jo', 'Smith, John and Doe, J.',
                              'M{\\"u}ller, H.', 'Müller, Hans', 'Perrette, M.', 'M. Perrette', 'Smith, J. D.', 'Smith, J.-D.', 'Smith, J~D', '',
                              'Smith', 'Smith, John Quincy', 'Smith, J. Q.', 'Doe, Jane and Smith, John', 'Smith, John and Doe, Jane']
             for b in ['Smith, John', 'J. Smith', 'Smith, J.', 'Smith, John D.', 'Smith, Jane', 'Müller, H.', 'Smith, John Quincy', 'Smith, J.',
                       'Smith, John and Doe, Jane', '']]
    from papers.merge import names_compatible, _fullness, equivalent
    out['names'] = [[a, b, bool(names_compatible(a, b)), _fullness(a), equivalent('author', a, b)] for a, b in names]
    eq = []
    for _ in range(300):
        name = rng.choice(['title', 'doi', 'journal', 'ENTRYTYPE'])
        a = rng.choice(['10.1/x', 'https://doi.org/10.1/X', 'doi:10.1/x', 'Nature', 'nature', '{Nature}', 'Über', '{\\"U}ber', 'x', 'article', 'misc', 'Sea  level', 'sea level', ''])
        b = rng.choice(['10.1/x', 'https://doi.org/10.1/X', 'doi:10.1/x', 'Nature', 'nature', '{Nature}', 'Über', '{\\"U}ber', 'y', 'article', 'misc', 'Sea level', ''])
        eq.append([name, a, b, bool(equivalent(name, a, b))])
    out['equivalent'] = eq
    return out


# library
# =======

LIB_TEXT = """@article{smith2020climate,
 author = {Smith, John and Doe, Jane},
 doi = {10.1000/abc123},
 file = {:files/smith.pdf:pdf},
 journal = {Nature},
 keywords = {climate, mine},
 title = {Climate sensitivity revisited},
 year = {2020}
}

@book{handbook,
 doi = {10.1000/book},
 isbn = {0-306-40615-2},
 title = {Handbook of climate}
}

@article{child,
 author = {Doe, Jane},
 crossref = {handbook},
 title = {A chapter},
 year = {2019}
}

@comment{a comment}

@string{jgr = "J. Geophys. Res."}
"""

NEW_TEXT = """@article{Smith_2020,
 author = {Smith, J. and Doe, J.},
 doi = {10.1000/ABC123},
 file = {:other/smith-preprint.pdf:pdf},
 journal = {Nature Climate Change},
 title = {Climate Sensitivity Revisited},
 volume = {10},
 year = {2020}
}
"""


class Scenario:
    "a library on disk, loaded as papers loads it, and the operations of the sequences"

    def __init__(self, text, nameformat=None, keyformat=None, files=True):
        self.dir = tempfile.mkdtemp(dir=TMP)
        self.bibtex = os.path.join(self.dir, 'lib', 'papers.bib')
        os.makedirs(os.path.dirname(self.bibtex))
        with open(self.bibtex, 'w') as f:
            f.write(text)
        kw = {}
        if nameformat:
            kw['nameformat'] = nameformat
        if keyformat:
            kw['keyformat'] = keyformat
        self.biblio = Biblio.load(self.bibtex, os.path.join(self.dir, 'lib', 'files') if files else None, **kw)
        self.biblio.mark_loaded()

    def run(self, op):
        out = io.StringIO()
        b = self.biblio
        try:
            with contextlib.redirect_stdout(out):
                result = self.apply(b, op)
            res = {'result': result}
        except Exception as e:  # PapersExit, DuplicateKeyError, ValueError...
            res = {'error': str(e)}
        res['stdout'] = out.getvalue().splitlines()
        return res

    def apply(self, b, op):
        kind = op['op']
        if kind == 'add':
            o = op.get('options', {})
            kw = dict(update_key=o.get('update_key', False), check_duplicate=o.get('check_duplicate', False),
                      on_duplicate=o.get('on_duplicate', 'error'))
            if o.get('metadata'):
                kw['metadata'] = dict(o['metadata'])
            rel = os.path.join(self.dir, op['relative_to']) if op.get('relative_to') else None
            result = b.add_bibtex(op['bibtex'], relative_to=rel, **kw)
            return [e.key for e in result]
        if kind == 'remove':
            e = next(e for e in b.entries if e.key == op['key'])
            b.db.remove(e)
            return None
        if kind == 'set_key':
            e = next(e for e in b.entries if e.key == op['key'])
            b.set_key(e, op['new'], keep_old=op.get('keep_old', False))
            return None
        if kind in ('tags_add', 'tags_remove'):
            es = [e for e in b.entries if e.key in op['keys']]
            (entry_cmds.add_tags if kind == 'tags_add' else entry_cmds.remove_tags)(es, op['tags'])
            return None
        if kind == 'tag_rename':
            return entry_cmds.rename_tag(b.entries, op['old'], op['new'])
        if kind == 'set_files':
            e = next(e for e in b.entries if e.key == op['key'])
            b.set_files(e, [os.path.join(self.dir, f) for f in op['files']])
            return None
        if kind == 'files':
            e = next(e for e in b.entries if e.key == op['key'])
            return [os.path.relpath(f, self.dir) for f in b.get_files(e)]
        if kind == 'check_duplicates':
            b.check_duplicates(on_duplicate=op['mode'])
            return None
        if kind == 'generate_key':
            es = parse_string(op['bibtex']).entries
            return [b.generate_key(e, new=True) for e in es]
        if kind == 'regenerate_key':
            # of an entry of the library (`check --fix-key`): its own key is not taken; empty when the template gives none
            e = next(e for e in b.entries if e.key == op['key'])
            return b.generate_key(e)
        if kind == 'relative_to':
            new = '/' if op['to'] == 'absolute' else os.path.dirname(self.bibtex)
            return [list(map(lambda t: [t[0].replace(self.dir, '$'), t[1].replace(self.dir, '$')], x)) for x in [b.update_file_path(new)]][0]
        raise ValueError(kind)

    def save(self):
        "what papers.library.savebib does around Biblio.save"
        b = self.biblio
        for old, new in b.renamed_keys():
            b.rekey_references(old, new)
        unchanged = b.unchanged()
        if not unchanged:
            b.save(self.bibtex)
        with open(self.bibtex) as f:
            text = f.read()
        dpath = os.path.join(self.dir, 'lib', '.papers', 'distinct.json')
        distinct = open(dpath).read() if os.path.exists(dpath) else None
        new, modified = b.changed_entries()
        return {'text': text, 'distinct': distinct, 'unchanged': unchanged, 'new': [e.key for e in new],
                'modified': [e.key for e in modified]}


def add(bibtex, **options):
    return {'op': 'add', 'bibtex': bibtex, 'options': options}


def library_cases():
    cases = []

    def scenario(name, text, ops, **kw):
        s = Scenario(text, **{k: v for k, v in kw.items() if k in ('nameformat', 'keyformat')})
        steps = [s.run(op) for op in ops]
        case = {'name': name, 'library': text, 'ops': ops, 'steps': steps, 'saved': s.save()}
        for k in ('nameformat', 'keyformat'):
            if kw.get(k):
                f = kw[k]
                case[k] = {'template': f.template, 'kind': f.kind, 'author_num': f.author_num, 'author_sep': f.author_sep,
                           'title_word_num': f.title_word_num, 'title_word_size': f.title_word_size,
                           'title_length': f.title_length, 'title_sep': f.title_sep, 'unknown_strict': f.unknown_strict,
                           'unknown_template': f.unknown_template}
        cases.append(case)

    for mode in ('error', 'keep', 'update', 'skip'):
        scenario(f'add duplicate {mode}', LIB_TEXT, [add(NEW_TEXT, check_duplicate=True, on_duplicate=mode)])
    scenario('add without check', LIB_TEXT, [add(NEW_TEXT)])
    scenario('add update key', LIB_TEXT, [add(NEW_TEXT, update_key=True)])
    scenario('add update key and check', LIB_TEXT, [add(NEW_TEXT, update_key=True, check_duplicate=True, on_duplicate='keep')])
    exact = entry_text('otherkey', 'article', [('author', 'Smith, J. and Doe, J.'), ('doi', '10.1000/abc123'),
                                              ('file', ':other/smith-preprint.pdf:pdf'), ('title', 'Climate sensitivity revisited')])
    scenario('add exact duplicate', LIB_TEXT, [add(exact, check_duplicate=True)])
    scenario('add exact duplicate twice', LIB_TEXT, [add(exact, check_duplicate=True), add(exact, check_duplicate=True)])
    same_key = NEW_TEXT.replace('Smith_2020', 'smith2020climate')
    scenario('add same key other paper', LIB_TEXT, [add(same_key.replace('10.1000/ABC123', '10.5/other'))])
    scenario('add same key same paper', LIB_TEXT, [add(same_key.replace('Smith, J. and Doe, J.', 'Smith, John and Doe, Jane')
                                                       .replace('Climate Sensitivity Revisited', 'Climate sensitivity revisited'))])
    scenario('add same key same paper, title by case', LIB_TEXT, [add(same_key.replace('Smith, J. and Doe, J.', 'Smith, John and Doe, Jane')
                                                                    .replace(' file = {:other/smith-preprint.pdf:pdf},\n', ''))])
    scenario('add same key same paper, a field to add', LIB_TEXT, [add(same_key.replace('Smith, J. and Doe, J.', 'Smith, John and Doe, Jane')
                                                                     .replace('Nature Climate Change', 'Nature'))])
    scenario('add same key update key', LIB_TEXT, [add(same_key.replace('10.1000/ABC123', '10.5/other'), update_key=True)])
    chapter = entry_text('chap', 'incollection', [('doi', '10.1000/abc123'), ('title', 'Sea level rise in the next century')])
    scenario('title guard keep', LIB_TEXT, [add(chapter, check_duplicate=True, on_duplicate='keep')])
    scenario('title guard error', LIB_TEXT, [add(chapter, check_duplicate=True, on_duplicate='error')])
    scenario('title guard skip', LIB_TEXT, [add(chapter, check_duplicate=True, on_duplicate='skip')])
    scenario('key from template', LIB_TEXT, [add('@article{,\n author = {Smith, John},\n year = {2020},\n title = {Other}\n}\n', update_key=True),
                                             add('@article{,\n author = {Smith, John},\n year = {2020},\n title = {Another}\n}\n', update_key=True)])
    scenario('missing key', LIB_TEXT, [add('@article{,\n author = {Smith, John},\n year = {2020},\n title = {Other}\n}\n')])
    scenario('several entries', LIB_TEXT, [add(NEW_TEXT + '\n' + NEW_TEXT.replace('Smith_2020', 'Smith_2020b') + '\n'
                                                '@article{X,\n author = {Brown, Bob},\n title = {Ocean tides}\n}\n',
                                                check_duplicate=True, on_duplicate='keep')])
    scenario('metadata', LIB_TEXT, [add('@article{x,\n title = {T}\n}\n', metadata={'title': 'Given', 'year': '2001', 'ID': 'given', 'ENTRYTYPE': 'misc'})])
    scenario('metadata several', LIB_TEXT, [add(NEW_TEXT + '\n' + NEW_TEXT.replace('Smith_2020', 'b'), metadata={'title': 'Given'})])
    scenario('relative_to of added', LIB_TEXT, [{'op': 'add', 'bibtex': NEW_TEXT, 'relative_to': 'lib/sub', 'options': {}}])
    scenario('remove', LIB_TEXT, [{'op': 'remove', 'key': 'child'}])
    scenario('set key', LIB_TEXT, [{'op': 'set_key', 'key': 'handbook', 'new': 'handbook2'}])
    scenario('set key keep old', LIB_TEXT, [{'op': 'set_key', 'key': 'handbook', 'new': 'handbook2', 'keep_old': True}])
    scenario('set key keep old twice', LIB_TEXT, [{'op': 'set_key', 'key': 'handbook', 'new': 'h2', 'keep_old': True},
                                                  {'op': 'set_key', 'key': 'h2', 'new': 'h3', 'keep_old': True}])
    scenario('set key keep old, back and forth', LIB_TEXT, [{'op': 'set_key', 'key': 'handbook', 'new': 'h2', 'keep_old': True},
                                                            {'op': 'set_key', 'key': 'h2', 'new': 'handbook', 'keep_old': True},
                                                            {'op': 'set_key', 'key': 'handbook', 'new': 'h2', 'keep_old': True}])
    scenario('set key, ids', LIB_TEXT.replace(' isbn =', ' ids = {Handbook, old1, handbook3},\n isbn ='),
             [{'op': 'set_key', 'key': 'handbook', 'new': 'handbook2'}, {'op': 'set_key', 'key': 'handbook2', 'new': 'HANDBOOK3'},
              {'op': 'set_key', 'key': 'HANDBOOK3', 'new': 'old1', 'keep_old': True}])
    scenario('set key, ids emptied', LIB_TEXT.replace(' isbn =', ' ids = {Handbook2},\n isbn ='), [{'op': 'set_key', 'key': 'handbook', 'new': 'handbook2'}])
    scenario('set key same key other case', LIB_TEXT, [{'op': 'set_key', 'key': 'handbook', 'new': 'Handbook', 'keep_old': True}])
    scenario('set key back', LIB_TEXT, [{'op': 'set_key', 'key': 'handbook', 'new': 'h2'}, {'op': 'set_key', 'key': 'h2', 'new': 'handbook'}])
    scenario('set key invalid', LIB_TEXT, [{'op': 'set_key', 'key': 'handbook', 'new': new} for new in
                                           ['2020', 'a b', 'a,b', '', 'Child', 'a{b', 'a\\b', 'a#b', 'a=b']])
    scenario('set key swap', LIB_TEXT, [{'op': 'set_key', 'key': 'handbook', 'new': 'tmp'}, {'op': 'set_key', 'key': 'child', 'new': 'handbook'}])
    scenario('tags', LIB_TEXT, [{'op': 'tags_add', 'keys': ['handbook', 'child'], 'tags': ['a/b', 'c', 'a/b']},
                                {'op': 'tags_remove', 'keys': ['child'], 'tags': ['c']},
                                {'op': 'tag_rename', 'old': 'a', 'new': 'z'},
                                {'op': 'tags_add', 'keys': ['smith2020climate'], 'tags': ['mine', 'Mine']}])
    scenario('tags without case', LIB_TEXT.replace('climate, mine', 'Climate/Arctic, mine, climate/amoc'),
             [{'op': 'tags_add', 'keys': ['smith2020climate'], 'tags': ['CLIMATE/ARCTIC', 'MINE', 'new', 'New']},
              {'op': 'tags_remove', 'keys': ['smith2020climate'], 'tags': ['MINE']},
              {'op': 'tag_rename', 'old': 'climate', 'new': 'ocean'},
              {'op': 'tag_rename', 'old': 'NEW', 'new': 'New'}])
    scenario('tags unchanged', LIB_TEXT, [{'op': 'tags_add', 'keys': ['smith2020climate'], 'tags': ['mine']}])
    scenario('files', LIB_TEXT, [{'op': 'set_files', 'key': 'child', 'files': ['lib/files/a.pdf', 'lib/files/a.pdf', 'other/b.pdf', 'lib/x.txt']},
                                 {'op': 'files', 'key': 'child'}, {'op': 'set_files', 'key': 'smith2020climate', 'files': []}])
    scenario('files absolute', LIB_TEXT, [{'op': 'relative_to', 'to': 'absolute'}, {'op': 'files', 'key': 'smith2020climate'},
                                          {'op': 'relative_to', 'to': 'relative'}])
    scenario('generate key', LIB_TEXT, [{'op': 'generate_key', 'bibtex': '@article{x,\n author = {Smith, John},\n year = {2020}\n}\n'},
                                        {'op': 'generate_key', 'bibtex': '@article{x,\n doi = {10.5194/bg-8(515)-2011}\n}\n'},
                                        {'op': 'generate_key', 'bibtex': '@article{x,\n author = {Smith, John},\n year = {2020}\n}\n' * 1}])
    scenario('generate key taken', LIB_TEXT.replace('smith2020climate', 'smith2020'), [
        {'op': 'generate_key', 'bibtex': '@article{x,\n author = {Smith, John},\n year = {2020}\n}\n'}])
    scenario('generate key z', '\n'.join(f'@misc{{smith2020{c},\n title = {{x}}\n}}\n' for c in 'bcdefghijklmnopqrstuvwxyz') + '@misc{smith2020,\n title = {x}\n}\n', [
        {'op': 'generate_key', 'bibtex': '@article{x,\n author = {Smith, John},\n year = {2020}\n}\n'}])
    group = LIB_TEXT + '\n' + NEW_TEXT + """
@misc{smith2019preprint,
 author = {Smith, John and Doe, Jane},
 doi = {10.5194/egusphere-2019-1},
 title = {Climate sensitivity revisited},
 year = {2019}
}

@article{brown2021,
 author = {Brown, Bob},
 title = {Other},
 year = {2021}
}
"""
    for mode in ('keep', 'skip', 'error'):
        scenario(f'check duplicates {mode}', group, [{'op': 'check_duplicates', 'mode': mode}])
    scenario('check duplicates twice', group, [{'op': 'check_duplicates', 'mode': 'keep'}, {'op': 'check_duplicates', 'mode': 'keep'}])
    scenario('check duplicates ids', group.replace(' journal = {Nature},', ' ids = {old, Smith_2020},\n journal = {Nature},'),
             [{'op': 'check_duplicates', 'mode': 'keep'}])
    scenario('crossref follows', LIB_TEXT, [{'op': 'set_key', 'key': 'handbook', 'new': 'handbook2'}])
    scenario('crossref follows the merge', group + '\n@inproceedings{talk,\n crossref = {Smith_2020},\n title = {A talk}\n}\n'
             + '\n@inproceedings{talk2,\n crossref = {SMITH2020CLIMATE},\n title = {Another talk}\n}\n', [{'op': 'check_duplicates', 'mode': 'keep'}])
    scenario('crossref merged away', group.replace('@article{child', '@article{child2'), [{'op': 'check_duplicates', 'mode': 'keep'}])
    # templates
    nf = Format('{Author}{year}-{Title}', title_word_num=2, kind='key')
    scenario('key template', LIB_TEXT, [add('@article{,\n author = {Smith, John},\n year = {2020},\n title = {The ice sheets of Greenland}\n}\n', update_key=True)],
             keyformat=nf)
    uf = Format('{author}{year}', unknown_template='x{doi_}', kind='key')
    scenario('unknown key template', LIB_TEXT, [add('@article{,\n doi = {10.5194/tc-14-1-2020}\n}\n', update_key=True),
                                                add('@article{,\n doi = {10.5194/tc-14-1-2020}\n}\n', update_key=True)], keyformat=uf)

    # a key template that gives an empty key: a new entry gets a key of its title (bibtex.md 8), a rekeyed one keeps its own
    scenario('empty key: add', LIB_TEXT, [
        add('@misc{,\n title = {Only a title},\n year = {2000}\n}\n', update_key=True),
        add('@misc{,\n title = {Only a title},\n year = {2000}\n}\n', update_key=True),
        add('@misc{,\n title = {Only another},\n}\n', update_key=True),
        add('@misc{,\n title = {100 years},\n year = {2000}\n}\n'),
        add('@misc{a,\n title = {A b}\n}\n@misc{b,\n year = {1999}\n}\n@misc{c,\n title = {100}\n}\n@misc{d,\n title = {The {CO2} of 3 seas},\n year = {1999}\n}\n', update_key=True),
        add('@misc{given,\n title = {Titled}\n}\n', update_key=True),
        add('@misc{,\n}\n')])
    scenario('empty key: with a duplicate check', LIB_TEXT, [
        add('@misc{,\n title = {Only a title},\n year = {2000}\n}\n', check_duplicate=True, on_duplicate='keep'),
        add('@misc{,\n title = {Only a title},\n year = {2000}\n}\n', check_duplicate=True, on_duplicate='keep')])
    scenario('empty key: generate', LIB_TEXT.replace('smith2020climate', 'only2000') + '\n@misc{blank,\n title = {Blank}\n}\n', [
        {'op': 'generate_key', 'bibtex': '@misc{x,\n title = {Only a title},\n year = {2000}\n}\n@misc{y,\n title = {Only a title}\n}\n'},
        {'op': 'generate_key', 'bibtex': '@misc{x,\n title = {x},\n year = {2000}\n}\n@misc{y,\n}\n'},
        {'op': 'regenerate_key', 'key': 'blank'}, {'op': 'regenerate_key', 'key': 'child'}])

    # a library with odd things: unreadable blocks, bare values, comments, strings
    odd = open(os.path.join(HERE, 'fixtures', 'varied.bib')).read()
    scenario('varied: nothing changed', odd, [])
    scenario('varied: tags', odd, [{'op': 'tag_rename', 'old': 'a', 'new': 'b'}])
    scenario('library: add', open(os.path.join(HERE, 'fixtures', 'library.bib')).read(), [add(NEW_TEXT, check_duplicate=True, on_duplicate='keep')])

    # fuzz: sequences of additions on a random library
    rng = random.Random(21)
    for n in range(40):
        entries = make_library_entries(rng, rng.randint(2, 25))
        text = '\n'.join(entry_text(k, t, f) for k, t, f in entries)
        added = make_library_entries(rng, rng.randint(1, 8))
        # some added entries are variants of the library's, some have its keys
        add_entries = []
        for k, t, f in added:
            if rng.random() < 0.5:
                _, t, f0 = rng.choice(entries)
                f = variant_of(rng, f0)
            key = rng.choice([k, k, rng.choice(entries)[0], rng.choice(entries)[0].upper(), ''])
            add_entries.append((key, t, f))
        ops = []
        for _ in range(rng.randint(1, 4)):
            batch = rng.sample(add_entries, rng.randint(1, min(3, len(add_entries))))
            ops.append(add('\n'.join(entry_text(k, t, f) for k, t, f in batch), update_key=rng.random() < 0.5,
                           check_duplicate=rng.random() < 0.8,
                           on_duplicate=rng.choice(['keep', 'update', 'skip', 'error'])))
        if rng.random() < 0.5:
            ops.append({'op': 'check_duplicates', 'mode': rng.choice(['keep', 'skip', 'error'])})
        if rng.random() < 0.5:
            ops.append({'op': 'tag_rename', 'old': 'a', 'new': 'z'})
        if rng.random() < 0.3:
            ops.append({'op': 'set_key', 'key': entries[0][0], 'new': rng.choice(['renamed', 'k1', 'x y']), 'keep_old': rng.random() < 0.5})
        scenario(f'fuzz {n}', text, ops)
    return cases


# files
# =====

def tree(root):
    out = {}
    for dirpath, _, files in os.walk(root):
        for f in files:
            p = os.path.join(dirpath, f)
            out[os.path.relpath(p, root)] = open(p, 'rb').read().decode('latin-1')
    return out


def file_cases():
    cases = []

    def scenario(name, files, entries, filesdir='lib/files', formatter=None, policy='move', keys=None, extra=()):
        root = tempfile.mkdtemp(dir=TMP)
        for path, content in files.items():
            os.makedirs(os.path.dirname(os.path.join(root, path)), exist_ok=True)
            with open(os.path.join(root, path), 'w') as f:
                f.write(content)
        bibtex = os.path.join(root, 'lib', 'papers.bib')
        os.makedirs(os.path.dirname(bibtex), exist_ok=True)
        text = '\n'.join(entry_text(k, t, f) for k, t, f in entries)
        open(bibtex, 'w').write(text)
        kw = {'nameformat': formatter} if formatter else {}
        b = Biblio.load(bibtex, os.path.join(root, filesdir), **kw)
        b.mark_loaded()
        copy = {'move': False, 'copy': True,
                'link': lambda f: 'link',
                'mixed': lambda f: 'move' if f.endswith('.pdf') else 'copy'}[policy]
        results = []
        for e in b.entries:
            if keys and e.key not in keys:
                continue
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    b.rename_entry_files(e, copy=copy)
                results.append(None)
            except Exception as error:
                results.append(str(error))
        for op in extra:
            e = next(e for e in b.entries if e.key == op['key'])
            if op['op'] == 'set_files':
                b.set_files(e, [os.path.join(root, f) for f in op['files']])
            elif op['op'] == 'rename':
                b.rename_entry_files(e, copy=copy)
        b.save(bibtex)
        case = {'name': name, 'files': files, 'bibtex': text, 'filesdir': filesdir, 'policy': policy, 'keys': keys, 'extra': list(extra),
                'errors': results, 'saved': open(bibtex).read(), 'tree': tree(root)}
        if formatter:
            case['nameformat'] = {'template': formatter.template, 'author_num': formatter.author_num, 'author_sep': formatter.author_sep,
                                  'title_word_num': formatter.title_word_num, 'title_word_size': formatter.title_word_size,
                                  'title_length': formatter.title_length, 'title_sep': formatter.title_sep,
                                  'unknown_strict': formatter.unknown_strict, 'unknown_template': formatter.unknown_template}
        cases.append(case)

    paper = [('author', 'Smith, John and Doe, Jane'), ('title', 'Climate sensitivity revisited'), ('year', '2020')]
    one = [('smith', 'article', paper + [('file', ':../in/a.pdf:pdf')])]
    for policy in ('move', 'copy', 'link', 'mixed'):
        scenario(f'one file {policy}', {'in/a.pdf': 'A'}, one, policy=policy)
    two = [('smith', 'article', paper + [('file', ':../in/a.pdf:pdf;:../in/notes.txt:txt;:../in/notes.pdf:pdf;:../in/b.pdf:pdf')])]
    files = {'in/a.pdf': 'A', 'in/notes.txt': 'N', 'in/notes.pdf': 'NP', 'in/b.pdf': 'B'}
    for policy in ('move', 'copy', 'mixed'):
        scenario(f'several files {policy}', files, two, policy=policy)
    scenario('rename twice', files, two, extra=[{'op': 'rename', 'key': 'smith'}])
    scenario('second rename keeps label', files, two, extra=[{'op': 'rename', 'key': 'smith'}], formatter=Format('{author}'))
    scenario('subfolder template', {'in/a.pdf': 'A'}, one, formatter=Format('{year}/{Author}{year}-{Title}', author_num=1))
    for template in ('../{author}', '/tmp/{author}', '{author}/../../x', '.', '{author}/..', 'a/../{author}'):
        scenario(f'unsafe template {template}', {'in/a.pdf': 'A'}, one, formatter=Format(template))
    for value in ('../../escaped/x', '/../../../escaped/x', '..', '/etc/x', '..\\..\\escaped\\x'):
        entries = [('x', 'article', [('author', 'Doe, John'), ('title', 'A title'), ('year', value), ('journal', value), ('doi', value),
                                     ('file', ':../in/a.pdf:pdf')])]
        scenario(f'unsafe fields {value}', {'in/a.pdf': 'A'}, entries, formatter=Format('{authorX}_{year}_{journal}_{doi}_{title}'))
    scenario('broken link', {}, one)
    scenario('same name two entries', {'in/a.pdf': 'A', 'in/b.pdf': 'A'},
             [('a', 'article', paper + [('file', ':../in/a.pdf:pdf')]), ('b', 'article', paper + [('file', ':../in/b.pdf:pdf')])])
    scenario('already in place', {'lib/files/smith.pdf': 'A'}, [('smith', 'article', paper + [('file', ':files/smith.pdf:pdf')])], formatter=Format('smith'))
    scenario('case only', {'lib/files/Smith.pdf': 'A'}, [('smith', 'article', paper + [('file', ':files/Smith.pdf:pdf')])], formatter=Format('smith'))
    scenario('in the files directory', {'lib/files/old.pdf': 'A'}, [('smith', 'article', paper + [('file', ':files/old.pdf:pdf')])])
    scenario('legacy folder', {'lib/files/old/a.pdf': 'A', 'lib/files/old/.old.bib': '@article{x,}'},
             [('smith', 'article', paper + [('file', ':files/old/a.pdf:pdf')])])
    scenario('legacy folder with other files', {'lib/files/old/a.pdf': 'A', 'lib/files/old/b.txt': 'B', 'lib/files/old/.old.bib': '@article{x,}'},
             [('smith', 'article', paper + [('file', ':files/old/a.pdf:pdf')])])
    scenario('no title', {'in/a.pdf': 'A'}, [('smith', 'article', [('year', '2020'), ('file', ':../in/a.pdf:pdf')])])
    scenario('unknown', {'in/a.pdf': 'A'}, [('smith', 'article', [('doi', '10.1000/x(y)'), ('file', ':../in/a.pdf:pdf')])])
    scenario('no extension', {'in/a': 'A', 'in/.hidden': 'H'}, [('smith', 'article', paper + [('file', ':../in/a:;:../in/.hidden:')])])
    scenario('identical dest', {'in/a.pdf': 'A', 'lib/files/smith_and_doe_2020_climate-sensitivity-revisited.pdf': 'A'}, [('smith', 'article', paper + [('file', ':../in/a.pdf:pdf')])])
    return cases


def main():
    write_json('names.json', names_cases())
    write_json('identity.json', identity_cases())
    write_json('merge.json', merge_cases())
    write_json('library.json', library_cases())
    write_json('files.json', file_cases())
    shutil.rmtree(TMP, ignore_errors=True)


if __name__ == '__main__':
    main()
