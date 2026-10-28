#!/usr/bin/env python3
"""Generate tests/expected/extract/ from the reference, papers-cli (Python).

The Rust tests (tests/extract.rs, tests/fetch.rs) compare what papers-rs does with what is
recorded here:

  doi_text.json   parse_dois on texts (the unit tests of papers, and generated texts that mix what
                  PDFs hold: labels, URLs, line breaks inside DOIs, invisible characters, DOIs of
                  data repositories, supplements, citations...), with the tail of the document
  doi_misc.json   the pieces: the normalization of the text, isvaliddoi, the DOI of the XMP and of
                  the document information, title_in_text, query_text
  fetch.json      crossref_to_bibtex on records (real ones from the cache of papers, and invented
                  ones with what a record can lack or hold that is odd), the score of the
                  results of a search, token_set_ratio, the files of the cache, the links of
                  an HTML page
  flows.json      the queries (by DOI, arXiv, doi.org, full text) and the extraction of metadata
                  from a text, with the answers of the network replayed from fixtures: the
                  result or the error, and the requests made

Run with the python that has papers installed (the reference), from anywhere:

    /path/to/python3 tests/gen_extract_expected.py [--cache DIR] [--count N] [--seed S] [--out DIR]

`--cache` is the cache of papers to take real crossref records from (default: the one of the
user, read only; if there is none, only the invented records are used). `--count`, `--seed` and
`--out` make a larger fuzz run in another folder (see tests/extract.rs, `PAPERS_RS_EXTRACT_FUZZ`).
It never writes the configuration or the cache of the user: HOME and the cache of papers are a
temporary folder.
"""
import argparse
import glob
import hashlib
import json
import logging
import os
import random
import re
import shutil
import sys
import tempfile
import warnings
from urllib.parse import quote

HERE = os.path.dirname(os.path.abspath(__file__))
TMP = tempfile.mkdtemp(prefix='papers-rs-expected-')
os.environ['HOME'] = TMP
os.environ['XDG_CACHE_HOME'] = os.path.join(TMP, 'cache')
os.environ['XDG_CONFIG_HOME'] = os.path.join(TMP, 'config')
os.chdir(TMP)
warnings.simplefilter('ignore')

import requests  # noqa: E402
from requests.utils import get_encoding_from_headers  # noqa: E402
from bs4 import BeautifulSoup  # noqa: E402
from rapidfuzz.fuzz import token_set_ratio  # noqa: E402

import papers.config  # noqa: E402
import papers.fetch as pf  # noqa: E402
from papers import extract as ex  # noqa: E402
from papers.cache import _cache_path  # noqa: E402

logging.disable(logging.CRITICAL)

parser = argparse.ArgumentParser()
parser.add_argument('--cache', default=os.path.expanduser('~/.cache/papers'))
parser.add_argument('--count', type=int, default=2000)
parser.add_argument('--seed', type=int, default=20260101)
parser.add_argument('--out', default=os.path.join(HERE, 'expected', 'extract'))
OPTIONS = parser.parse_args()
OUT = OPTIONS.out


def write_json(name, data):
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, name), 'w', encoding='utf-8') as f:
        if isinstance(data, dict):  # compact, one line per key
            f.write('{\n' + ',\n'.join(f'{json.dumps(k)}: {json.dumps(v, ensure_ascii=False, separators=(",", ":"))}'
                                       for k, v in data.items()) + '\n}\n')
        else:  # a list: one line per case
            f.write('[\n' + ',\n'.join(json.dumps(v, ensure_ascii=False, separators=(',', ':')) for v in data) + '\n]\n')


def outcome(fun, *args):
    "the value, or the kind and the message of the exception"
    try:
        return {'ok': fun(*args)}
    except pf.DOINotFoundError as error:
        return {'err': ['not_found', str(error)]}
    except pf.DOIRequestError as error:
        return {'err': ['request', str(error)]}
    except ValueError as error:
        return {'err': ['value', str(error)]}
    except Exception as error:
        return {'err': ['other', str(error)]}


rng = random.Random(OPTIONS.seed)

# DOIs of a text
# ==============

# the unit tests of papers (tests/test_extract_unit.py) and what PDFs print
HANDPICKED = [
    'doi:10.5194/bg-8-515-2011', 'DOI:10.5194/test-2020', 'doi:10.5194/ESD-4-11-2013', 'not a doi', 'arxiv:1234.5678',
    '(doi:10.1007/s00382-010-0904-1)', 'http://dx.doi.org/10.1007/s00382-010-0904-1)', 'doi:10.1234/foo(bar)baz',
    'doi:10.1234/foo(bar)', 'Published: https://doi.org/10.1126/science.1166510\nOther text',
    'Science\n10.1126/science.aaa1234\nAbstract', 'cited 10.1000/a.b and 10.1000/c.d, then 10.1000/e',
    'see (10.1175/1520-0493(1990)118<2228:WMGD>2.0.CO;2).', 'doi 10.1073/pnas.\n 1234567118 and more',
    'doi:10.1175/jcli-d-21-\n0636.1 x', '10.\n1073/pnas.2000000117', 'www.pnas.org/cgi/doi/10.1073/pnas.2000000117\n10.1000/cited',
    'x 10.1000/cited.paper y https://doi.org/10.1000/real.one z', '10.1175/jcli-d-16-0271.s1 and 10.1175/jcli-d-16-0271.1',
    '10.5281/zenodo.123456 data, 10.1000/article.1 paper', '10.5281/zenodo.123456 only data',
    'doi:10.1000/abcpreprint', 'doi:10.1000/abc.pdf', 'doi:10.1000/abc/-/DCSupplemental', 'doi:10.1000/abc-supplement',
    'doi:10.1000/abc.received', 'doi:10.1000/abc published', 'doi:10.1000/Abstract', '10.1000/x',
    'doi:10.1007/s11214-019-0620-2(0123456789().,-volV)', 'ID\x0210.1000\x02abc', 'a​10.1000/ab­c',
    'ab10.1000/x.y 10.2000/z.w', 'a-10.1000/x.y .10.2000/z.w _10.3000/q.r', '10.123/short 10.12345678901/long 10.1234567890/ok',
    'arXiv: 2101.01234v2 [astro-ph]', 'ARXIV:\n2101.01234', 'arXiv:1234.56 arXiv:12345.6789', 'DOI ١٢٣٤/x',
    '10.١٢٣٤/abc.def', 'doi:10.1000/abc.', 'doi:10.1000/abc:', 'doi:10.1000/(abc))', 'doi:10.1000/a–b—c',
]

FRAGMENTS = [
    'doi:', 'DOI: ', 'doi ', 'https://doi.org/', 'http://dx.doi.org/', 'www.pnas.org/cgi/doi/', 'www.agu.org/journals/', 'doi.org/',
    '10.1234/abc-def', '10.1073/pnas.2000000117', '10.1175/jcli-d-21-0636.1', '10.1126/science.aaa1234', '10.1016/j.epsl.2011.07.008',
    '10.5194/tc-11-1015-2017', '10.1038/s41561-026-01959-6', '10.5281/zenodo.1234567', '10.1594/pangaea.925552',
    '10.1175/1520-0493(1990)118<2228:WMGD>2.0.CO;2', '10.1029/2011GL048531', '10.1007/s00382-010-0904-1', '10.1175/jcli-d-16-0271.s1',
    '10.1000/abc.pdf', '10.1000/abcpreprint', '10.1000/abc/-/DCSupplemental', '10.1000/abc(0123456789().,-volV)', '10.1000/xabstract',
    '10.1000/x.received', '10.1000/x-supplement', '10.1000/xsupplement', '10.1234/foo(bar)', '10.1234/foo)', '10.12345678/x', '10.123/x',
    '10.1234567890/x', '10.\n1234/x', '10.\n 1073/pnas.5', '10.1073/pnas.\n123', '10.1175/jcli-\nd-17-0112.1', '10.1175/jcli-d-21-\n 0636.1',
    '10.1000/a.\nb', 'arXiv:1234.5678', 'arxiv: 2101.0123', 'arXiv:\n1234.5678', '\n', '\n', '\n\n', ' ', ' ', '  ', '\t', ' ',
    '​', '­', '–', '—', '\x02', '\x00', '(', ')', '.', ',', ';', ':', '-', '_', '/', '<', '>', 'x', 'Received',
    'Published', 'Abstract', 'Keywords', 'References', 'the', 'paper', 'Smith et al.', '2021', 'e', 'S1', '١٢٣٤',
    'İ', 'K', 'ſ', 'é', 'Σ', 'Nature', 'ISBN 978-3-16-148410-0',
]


def random_text():
    n = rng.randint(1, 14)
    return ''.join(rng.choice(FRAGMENTS) for _ in range(n))


def doi_cases(count):
    cases = []
    texts = [(t, '') for t in HANDPICKED]
    for _ in range(count):
        texts.append((random_text(), random_text() if rng.random() < 0.3 else ''))
    for text, tail in texts:
        try:
            r = {'dois': ex.parse_dois(text, tail)}
        except ex.DOIParsingError as error:
            r = {'error': str(error)}
        cases.append({'text': text, 'tail': tail, **r})
    return cases


# the other pieces of extract.py
def misc_cases():
    out = {}
    out['normalize'] = [[t, ex._normalize_doi_text(t)] for t in HANDPICKED + [random_text() for _ in range(200)]]
    out['isvaliddoi'] = [[d, ex.isvaliddoi(d)] for d in [
        '10.5194/bg-8-515-2011', 'invalid', '10.1000/ABC', '10.1000/abc.pdf', '10.1000/a b', '', '10.1234/foo(bar)', '10.1234/foo)',
        '10.5281/zenodo.1', '10.1000/x']]
    xmp = []
    for tag in ['prism:doi', 'pdfx:doi', 'crossmark:DOI']:
        xmp.append(f'<x><{tag}>10.1038/ngeo1797</{tag}></x>')
        xmp.append(f'<x><{tag.upper()}>10.1038/NGEO1797</{tag.upper()}></x>')
        xmp.append(f'<x><{tag}>10.1038/ngeo1797</{tag.split(":")[0]}:other></x>')
    xmp += ['<dc:identifier>doi:10.1000/Abc.1</dc:identifier>', '<dc:identifier>10.1000/abc</dc:identifier>',
            '<prism:doi>10.1000/a b</prism:doi>', '<prism:doi>10.1000/x</prism:doi><pdfx:doi>10.2000/y</pdfx:doi>',
            '<pdfx:doi>10.2000/y</pdfx:doi><prism:doi>10.1000/x</prism:doi>', '<prism:doi>10.\n1000/x</prism:doi>',
            '<prism:doi>10.1000/xKſİı</prism:doi>', '<PRIſM:DOI>10.1000/x</prism:doi>', '', 'no tags 10.1000/x']
    out['xmp'] = [[x, ex._parse_doi_from_metadata_string(x)] for x in xmp]
    info = [('', '', ''), ('doi 10.1000/Abc.Def', '', ''), ('', 'k 10.1000/x, y', 't 10.2000/z'), ('', '', 'Title 10.1038/NGEO1797 end'),
            ('no', 'doi:10.1234/foo(bar)).', ''), ('10.12/x', '10.123/x', '10.12345/x'), ('a10.1000/xyz', '', ''),
            ('10.1000/xKſİıé', '', ''), ('10.١٢٣٤/x', '', '')]
    out['info'] = [[s, k, t, ex._parse_doi_from_info_fields({'subject': s, 'keywords': k, 'title': t})] for s, k, t in info]
    titles = [('Climate sensitivity revisited', 'Some text on Climate\nSensitivity revisited here'),
              ('Short', 'x'), ('CO<sub>2</sub> and ice', 'co2 and ice sheets'), ('Modèle Atmosphérique Régional', 'Modele Atmospherique Regional'),
              ('Bølling warming and its contribution', 'bolling warming and its contribution to'),
              ('A very long title that is not there', 'nothing similar in this text at all'),
              ('<b>Bold</b> and <i>italic</i> title text', 'bold and italic title text'), ('<> odd < and > title text here', 'odd and title text here'),
              ('Title with αβγ greek letters', 'title with abg greek letters'), ('Plio‐Pleistocene temperature records', 'plio-pleistocene temperature records'),
              ('中文标题 title of the paper', 'title of the paper'), ('ab', ''), ('', 'x'), ('a<', 'a<')]
    out['title_in_text'] = [[a, b, ex.title_in_text(a, b)] for a, b in titles]
    q = []
    paragraphs = ['One two three four five.', 'References\n\nSmith 2020 something', 'reference list should go\n\nbut this stays', 'Abstract. é words in here\n\n\n\nlast one']
    for txt in ['a b c', 'a b', '', '  a  b\tc  d \n', 'x y z w v u t'] + ['\n\n'.join(rng.sample(paragraphs, rng.randint(1, 4))) for _ in range(20)]:
        for n in [200, 3, 1, 0, -1, 4, 2]:
            try:
                q.append([txt, n, {'ok': ex.query_text(txt, n)}])
            except ValueError as error:
                q.append([txt, n, {'err': str(error)}])
    out['query_text'] = q
    out['split'] = [[t, t.split()] for t in ['a b', ' a b c​d\x1ce\x85f ', '　x　', '']]
    return out


# crossref records
# ================

def real_records(cache):
    files = sorted(glob.glob(os.path.join(cache, 'doi', '*', '*.json')))
    chosen = []
    seen_types = {}
    records = []
    for f in files:
        try:
            v = json.load(open(f)).get('value')
        except (OSError, ValueError):
            continue
        if isinstance(v, dict) and v.get('type') not in (None, 'journal-article'):
            records.append(v)
    journal = []
    for f in files[::max(1, len(files) // 40)]:
        try:
            v = json.load(open(f)).get('value')
        except (OSError, ValueError):
            continue
        if isinstance(v, dict) and v.get('type') == 'journal-article':
            journal.append(v)
    return records + journal[:40]


WORDS = ['ice', 'sheet', 'Greenland', 'climate', 'Müller', 'Bølling', 'CO<sub>2</sub>', '{brace', 'a}b', '50% of $x$', 'tilde~', 'back\\slash',
         'quote"s', "it's", 'line\nbreak', 'two  spaces', ' lead', 'trail ', '#hash', '&amp;', '中文', 'ligaturﬁ', '', 'x', 'LaTeX \\alpha']


def random_text_value():
    return ' '.join(rng.choice(WORDS) for _ in range(rng.randint(0, 4)))


def random_person():
    p = {}
    if rng.random() < 0.9:
        p['family'] = random_text_value()
    if rng.random() < 0.8:
        p['given'] = random_text_value()
    if rng.random() < 0.1:
        p['name'] = 'Consortium'
    return p


def random_record():
    m = {}
    if rng.random() < 0.9:
        m['type'] = rng.choice(['journal-article', 'book', 'book-chapter', 'proceedings-article', 'conference-paper', 'report', 'thesis',
                                'posted-content', 'dataset', 'article-journal', 'chapter', None, 5, ''])
    if rng.random() < 0.9:
        m['title'] = [random_text_value() or 'T'] if rng.random() < 0.9 else rng.choice([[], 'string title', None, [''], ['A', 'B']])
    if rng.random() < 0.85:
        m['author'] = [random_person() for _ in range(rng.randint(0, 4))]
    if rng.random() < 0.3:
        m['editor'] = [random_person() for _ in range(rng.randint(0, 2))]
    if rng.random() < 0.9:
        m['DOI'] = rng.choice(['10.1234/abc', '10.1000/X.Y(z)', '10.1234/a b', '', '10.5555/é'])
    if rng.random() < 0.7:
        m['URL'] = 'https://doi.org/10.1234/abc?x=1&y=2'
    for field in ['published-print', 'published-online', 'issued']:
        if rng.random() < 0.4:
            m[field] = rng.choice([{'date-parts': [[rng.randint(1990, 2026), 3]]}, {'date-parts': [[None]]}, {'date-parts': [[]]},
                                   {'date-parts': []}, {'other': 1}, {'date-parts': [[2020]]}])
    if rng.random() < 0.6:
        m['container-title'] = rng.choice([['Journal of Things'], [], ['A', 'B'], 'J', ['']])
    for field, values in [('volume', ['12', 3, '', None]), ('issue', ['4', 4, '']), ('page', ['1-10', 'e123', 7, 3.5, '']),
                          ('publisher', ['Springer', '']), ('publisher-location', ['Berlin', '']), ('ISBN', [['978-0'], [], '978', ['']])]:
        if rng.random() < 0.5:
            m[field] = rng.choice(values)
    if rng.random() < 0.3:
        m['institution'] = rng.choice([{'name': 'University of X'}, {'name': ''}, {}, [{'name': 'Y'}], []])
    if rng.random() < 0.2:
        m['abstract'] = '<jats:p>An abstract about ice.</jats:p>'
    return m


INVENTED = [
    {'type': 'journal-article', 'DOI': '10.1/x', 'volume': 12, 'issue': 'a', 'title': ['T'], 'page': 3.5},
    {'type': 'journal-article', 'DOI': '10.1/x', 'title': ['T'], 'author': [{'family': 'A', 'given': 'B'}, {'given': 'Only'}, {'family': 'F'}],
     'issued': {'date-parts': [[None]]}},
    {'type': 'journal-article', 'DOI': '10.1/x', 'title': ['T {x'], 'container-title': [], 'issued': {'date-parts': [[2020, 1]]}},
    {'type': 'report', 'DOI': '10.1/x', 'title': ['T'], 'institution': [{'name': 'A'}]},
    {'type': 'report', 'DOI': '10.1/x', 'title': ['T'], 'institution': {'name': 'A'}},
    {'type': 'thesis', 'DOI': '10.1/x', 'title': ['T'], 'institution': {'name': 'A'}},
    {'type': 'book', 'DOI': '10.1/x', 'title': ['T'], 'ISBN': ['123'], 'editor': [{'family': 'E', 'given': 'F'}], 'publisher': 'P', 'publisher-location': 'L'},
    {'type': 'proceedings-article', 'DOI': '10.1/x', 'title': ['T'], 'container-title': ['Proc'], 'publisher': 'P', 'editor': [{'family': 'E'}]},
    {'type': 'book-chapter', 'DOI': '10.1/x', 'title': ['T'], 'container-title': ['B']},
    {'type': None, 'DOI': '10.1/x', 'title': ['T']}, {'DOI': '10.1/x', 'title': 'Hello'}, {}, {'title': ['a'], 'abstract': 'x'},
    {'DOI': '10.1/x', 'title': ['T\nU  V'], 'URL': 'https://a/b?c=d&e=f%20'},
    {'DOI': '10.1/x', 'title': ['T'], 'published-print': {'date-parts': []}},
    {'DOI': '10.1/x', 'title': ['T'], 'published-print': {'date-parts': [[]]}, 'issued': {'date-parts': [[1999]]}},
    {'DOI': '10.1/x', 'title': ['T'], 'published-print': {'other': 1}, 'issued': {'date-parts': [[1999]]}},
    {'DOI': 'a b', 'title': ['é ü']},
]


def crossref_cases(cache):
    records = INVENTED + [random_record() for _ in range(max(300, OPTIONS.count // 5))]
    if os.path.isdir(cache):
        records = real_records(cache) + records
    cases = []
    for r in records:
        cases.append({'record': r, **outcome(pf.crossref_to_bibtex, r)})
    return cases


def score_cases(cache):
    cases = []
    texts = ['ice sheet mass balance Greenland', 'Climate sensitivity revisited Smith Jones', 'Bølling warming of the Laurentide',
             'x y z', 'ice ' * 30, 'Smith Jones Wang Li ice sheet', 'éè stuff']
    pool = real_records(cache) if os.path.isdir(cache) else []
    for r in pool[:20] + [random_record() for _ in range(60)]:
        for t in rng.sample(texts, 3):
            try:
                score = pf._crossref_score(t, r)
                cases.append({'text': t, 'record': r, 'score': score, 'float': isinstance(score, float)})
            except Exception:
                cases.append({'text': t, 'record': r, 'err': True})
    return cases


def token_cases():
    words = ['ice', 'sheet', 'mass', 'balance', 'Greenland', 'climate', 'Müller', 'a', 'b', 'Smith,', 'x1', 'quite-long-token', '中文', 'ice']
    cases = []
    for _ in range(400):
        a = ' '.join(rng.choice(words) for _ in range(rng.randint(0, 8)))
        b = ' '.join(rng.choice(words) for _ in range(rng.randint(0, 8)))
        cases.append([a, b, token_set_ratio(a, b)])
    return cases


def cache_cases():
    keys = ['10.1038/ngeo1797', '10.1038/s41561-021-00754-9', '10.1000/a b', '10.1000/é', '10.1002/(sici)1099-1085(199803)12:3<425::aid-hyp>3.0.co;2-v',
            '10.1000/' + 'x' * 300, '2101.01234', 'math.ag/0601001', 'a', 'ab', '10.1000/', '10.1000/a/b/c', '10.1000/%41', '10.1000/a+b~c_d.e-f*g']
    keys += [hashlib.sha256(k.encode()).hexdigest()[:16] for k in ['x', 'y']]
    paths = [[k, _cache_path('/c/doi', k)] for k in keys]
    hashes = [[t, hashlib.sha256(t.encode('utf-8')).hexdigest()[:16]] for t in ['{"rows": 5}', 'q é']]
    return {'paths': paths, 'hashes': hashes}


# the links of an HTML page
HTML = [
    '<a href="x.bib">bib</a>', "<A HREF='/a/b.bib'>x</A>", '<a href=y.bib>y</a><a href="z.html">z</a>', '<!-- <a href="no.bib"> --><a href="yes.bib">',
    '<script>var s = "<a href=\'no.bib\'>";</script><a href="yes.bib">', '<style>a { }</style><a href="s.bib">', '<a href="a&amp;b.bib">x</a>',
    '<a class="c" href = "sp.bib" >x</a>', '<a href="d1.bib" href="d2.bib">dup</a>', '<a>none</a><a href>empty</a><a href="">e</a>',
    '<p><a href="https://example.org/x/y.bib?download=1">q</a><a href="https://example.org/x/y.bib">r</a>', '<a href="&#x61;.bib">hex</a><a href="&#98;.bib">dec</a>',
    '<a\nhref="nl.bib"\n>x</a>', '<a href="un.bib', '<a href=">x.bib">', '<<a href="lt.bib">', 'text <a href="t.bib"/>', '<a/href="slash.bib">',
    '<link href="l.bib"><a href="k.bib">', '<AREA href="area.bib"><a href="A.BIB">', '<a href="../up.bib">u</a>', '<a href="//host/p.bib">h</a>',
]


def html_cases():
    out = []
    for h in HTML:
        soup = BeautifulSoup(h, 'html.parser')
        out.append([h, [link['href'] for link in soup.find_all('a', href=True)]])
    return out


# the queries, with the network replayed
# ======================================

class Replay:
    "papers.fetch.http_get with answers from a table"

    def __init__(self, fixtures):
        self.fixtures = fixtures
        self.requests = []

    def __call__(self, url, **kw):
        full = requests.Request('GET', url, params=kw.get('params')).prepare().url
        accept = (kw.get('headers') or {}).get('Accept')
        self.requests.append([full, accept])
        f = self.fixtures.get(full)
        if f is None:
            raise requests.exceptions.ConnectionError(f'no fixture: {full}')
        response = requests.Response()
        response.status_code = f['status']
        response.reason = f.get('reason', '')
        response._content = f['body'].encode('utf-8')
        response.headers['content-type'] = f.get('content_type', '')
        response.url = full
        response.encoding = get_encoding_from_headers(response.headers)
        return response


def run_scenario(scenario):
    cache = tempfile.mkdtemp(prefix='cache-', dir=TMP)
    papers.config.CACHE_DIR = cache
    replay = Replay(scenario['fixtures'])
    pf.http_get = replay
    results = []
    for call in scenario['calls']:
        name, args = call['fn'], call['args']
        before = len(replay.requests)
        if name == 'fetch_bibtex_by_doi':
            r = outcome(pf.fetch_bibtex_by_doi, *args)
        elif name == 'fetch_doi_metadata':
            r = outcome(pf.fetch_doi_metadata, *args)
        elif name == 'fetch_bibtex_by_fulltext_crossref':
            r = outcome(pf.fetch_bibtex_by_fulltext_crossref, *args)
        elif name == 'fetch_bibtex_on_journal_website':
            r = outcome(lambda doi: pf.fetch_bibtex_on_journal_website(doi, as_string=True), *args)
        elif name == 'extract_txt_metadata':
            text, tail, options = args
            r = outcome(lambda: ex.extract_txt_metadata(text, tail=tail, **options))
        elif name == 'confirm_doi':
            r = outcome(lambda dois, text: ex.confirm_doi(dois, text), *args)
            if 'ok' in r:
                r['ok'] = list(r['ok']) if r['ok'] else None
        elif name == 'extract_pdf_doi':
            r = outcome(lambda pdf: ex.extract_pdf_doi(pdf), *args)
        elif name == 'extract_pdf_metadata':
            pdf, options = args
            r = outcome(lambda: ex.extract_pdf_metadata(pdf, **options))
        else:
            raise ValueError(name)
        if 'ok' in r and isinstance(r['ok'], dict):
            r['ok'] = r['ok']
        results.append({**call, **r, 'requests': replay.requests[before:]})
    return {'name': scenario['name'], 'fixtures': scenario['fixtures'], 'calls': results}


def fixture(body, status=200, content_type='application/json', reason='OK'):
    return {'status': status, 'reason': reason, 'content_type': content_type, 'body': body if isinstance(body, str) else json.dumps(body)}


CROSSREF = 'https://api.crossref.org/works/'
DOIORG = 'https://doi.org/'
SELECT = pf.CROSSREF_SELECT


def record(doi, title, **kw):
    m = {'DOI': doi, 'title': [title], 'type': 'journal-article', 'container-title': ['Journal of Things'],
         'author': [{'given': 'Ada', 'family': 'Lovelace'}, {'given': 'Charles', 'family': 'Babbage'}],
         'issued': {'date-parts': [[2020, 5, 1]]}, 'volume': '7', 'issue': '2', 'page': '10-20', 'URL': 'https://doi.org/' + doi}
    m.update(kw)
    return m


def query_url(params):
    return requests.Request('GET', 'https://api.crossref.org/works', params=json.loads(json.dumps(params, sort_keys=True))).prepare().url


def scenarios(extra):
    out = []
    ice = 'Ice sheet response to warming in the early Holocene'
    d1, d2, d3 = '10.1234/ice.2020.1', '10.1234/cited.2019.7', '10.5555/other.data'
    text1 = f'{ice}\nAda Lovelace, Charles Babbage\nJournal of Things 7, 10-20 (2020) doi:{d1}\nAbstract. We study the ice sheet.\n'
    ok = fixture({'status': 'ok', 'message': record(d1, ice)})
    cited = fixture({'status': 'ok', 'message': record(d2, 'A different paper about ocean heat')})
    out.append({'name': 'by_doi', 'fixtures': {CROSSREF + d1: ok}, 'calls': [
        {'fn': 'fetch_bibtex_by_doi', 'args': [d1]}, {'fn': 'fetch_bibtex_by_doi', 'args': [d1.upper()]}, {'fn': 'fetch_doi_metadata', 'args': [d1]}]})
    out.append({'name': 'doi_with_odd_characters', 'fixtures': {CROSSREF + '10.1002/%28sici%291099-1085%28199803%2912%3A3%3C425%3A%3Aaid-hyp%3E3.0.co%3B2-v': ok},
                'calls': [{'fn': 'fetch_bibtex_by_doi', 'args': ['10.1002/(sici)1099-1085(199803)12:3<425::aid-hyp>3.0.co;2-v']}]})
    out.append({'name': 'crossref_404_then_doi_org', 'fixtures': {
        CROSSREF + '10.5281/zenodo.1': fixture('Resource not found.', 404, 'text/plain', 'Not Found'),
        DOIORG + '10.5281/zenodo.1': fixture({'DOI': '10.5281/zenodo.1', 'title': 'A dataset', 'type': 'dataset', 'author': [{'given': 'D', 'family': 'Ata'}],
                                              'issued': {'date-parts': [[2019]]}, 'container-title': 'Zenodo', 'publisher': 'Zenodo'},
                                             content_type='application/vnd.citationstyles.csl+json; charset=utf-8')},
                'calls': [{'fn': 'fetch_bibtex_by_doi', 'args': ['10.5281/zenodo.1']}]})
    csl_types = fixture({'DOI': '10.1/csl', 'title': 'Chapter title', 'type': 'chapter', 'container-title': 'Book title', 'issued': {'date-parts': [[2001, 2]]}},
                        content_type='application/vnd.citationstyles.csl+json')
    out.append({'name': 'csl_types', 'fixtures': {CROSSREF + '10.1/csl': fixture('x', 404, 'text/html', 'Not Found'), DOIORG + '10.1/csl': csl_types},
                'calls': [{'fn': 'fetch_bibtex_by_doi', 'args': ['10.1/csl']}]})
    out.append({'name': 'csl_without_type', 'fixtures': {CROSSREF + '10.1/nt': fixture('x', 404, 'text/html', 'Not Found'),
                                                          DOIORG + '10.1/nt': fixture({'DOI': '10.1/nt', 'title': ['T']}, content_type='application/json')},
                'calls': [{'fn': 'fetch_bibtex_by_doi', 'args': ['10.1/nt']}, {'fn': 'fetch_doi_metadata', 'args': ['10.1/nt']}]})
    out.append({'name': 'not_found', 'fixtures': {CROSSREF + '10.9999/nope': fixture('Resource not found.', 404, 'text/plain', 'Not Found'),
                                                  DOIORG + '10.9999/nope': fixture('', 404, 'text/html', 'Not Found')},
                'calls': [{'fn': 'fetch_bibtex_by_doi', 'args': ['10.9999/nope']}, {'fn': 'fetch_bibtex_by_doi', 'args': ['10.9999/NOPE']}]})
    out.append({'name': 'server_errors', 'fixtures': {
        CROSSREF + '10.1/e500': fixture('oops', 500, 'text/plain', 'Internal Server Error'), DOIORG + '10.1/e500': fixture('', 503, 'text/html', 'Service Unavailable'),
        CROSSREF + '10.1/badjson': fixture('<html>', 200, 'application/json'), DOIORG + '10.1/badjson': fixture('', 200, 'text/html'),
        CROSSREF + '10.1/nomsg': fixture({'status': 'ok'}), DOIORG + '10.1/nomsg': fixture('{}', 200, 'application/json'),
        CROSSREF + '10.1/list': fixture([1]), DOIORG + '10.1/list': fixture('[1]', 200, 'application/json'),
        CROSSREF + '10.1/notitle': fixture({'message': {'DOI': '10.1/notitle', 'title': []}}),
        CROSSREF + '10.1/badcsl': fixture('x', 404, 'text/plain', 'Not Found'), DOIORG + '10.1/badcsl': fixture('not json', 200, 'application/json')},
        'calls': [{'fn': 'fetch_bibtex_by_doi', 'args': [d]} for d in ['10.1/e500', '10.1/badjson', '10.1/nomsg', '10.1/list', '10.1/notitle', '10.1/badcsl', '10.1/offline']]})
    arxiv_bib = '@misc{arxiv2101.01234,\n  title={An arXiv paper},\n  author={Ar Xiv},\n  year={2021},\n  eprint={2101.01234},\n}\n'
    out.append({'name': 'arxiv', 'fixtures': {'https://arxiv.org/bibtex/2101.01234': fixture(arxiv_bib, content_type='text/plain; charset=utf-8'),
                                              'https://arxiv.org/bibtex/math.AG/0601001': fixture('Not found', 404, 'text/plain', 'Not Found'),
                                              'https://arxiv.org/bibtex/2102.00001': fixture('@misc{x,\n title = {é}\n}\n', content_type='text/plain')},
                'calls': [{'fn': 'fetch_bibtex_by_doi', 'args': ['10.48550/arXiv.2101.01234']}, {'fn': 'fetch_bibtex_by_doi', 'args': ['10.48550/arxiv.2101.01234']},
                          {'fn': 'fetch_bibtex_by_doi', 'args': ['10.48550/arXiv.math.AG/0601001']}, {'fn': 'fetch_bibtex_by_doi', 'args': ['10.48550/arXiv.2102.00001']}]})
    # the full-text query
    q = {'rows': 5, 'select': SELECT, 'query.bibliographic': ice}
    items = [{'DOI': '10.1/other', 'title': ['Something about oceans'], 'type': 'journal-article', 'author': [{'family': 'Wang'}], 'issued': {'date-parts': [[2010]]}},
             record(d1, ice, abstract='We study the ice sheet.'), {'DOI': '10.1/third', 'title': ['Ice'], 'type': 'book', 'issued': {'date-parts': [[2000]]}}]
    out.append({'name': 'fulltext', 'fixtures': {query_url(q): fixture({'status': 'ok', 'message': {'items': items}})},
                'calls': [{'fn': 'fetch_bibtex_by_fulltext_crossref', 'args': [ice]}, {'fn': 'fetch_bibtex_by_fulltext_crossref', 'args': [ice, None, None, 5]}]})
    q2 = {'rows': 5, 'select': SELECT, 'query.bibliographic': 'T ' + ice, 'query.author': 'Lovelace'}
    out.append({'name': 'fulltext_title_author', 'fixtures': {query_url(q2): fixture({'status': 'ok', 'message': {'items': items}})},
                'calls': [{'fn': 'fetch_bibtex_by_fulltext_crossref', 'args': [ice, 'T', 'Lovelace']}]})
    q3 = {'rows': 5, 'select': SELECT, 'query.bibliographic': 'nothing'}
    q4 = {'rows': 5, 'select': SELECT, 'query.bibliographic': 'broken'}
    q5 = {'rows': 5, 'select': SELECT, 'query.bibliographic': 'odd é "quotes" & more'}
    out.append({'name': 'fulltext_errors', 'fixtures': {query_url(q3): fixture({'status': 'ok', 'message': {'items': []}}),
                                                        query_url(q4): fixture('x', 500, 'text/plain', 'Internal Server Error'),
                                                        query_url(q5): fixture({'message': {'items': [record(d1, 'T')]}})},
                'calls': [{'fn': 'fetch_bibtex_by_fulltext_crossref', 'args': [t]} for t in ['nothing', 'broken', 'odd é "quotes" & more', 'offline']]})
    # the text of a paper: the DOIs it holds and the title that tells which is its own
    text2 = f'See also doi:{d2} for the ocean.\n' + text1
    options = {'search_doi': True, 'search_fulltext': False}
    out.append({'name': 'extract_text', 'fixtures': {CROSSREF + d1: ok, CROSSREF + d2: cited}, 'calls': [
        {'fn': 'extract_txt_metadata', 'args': [text1, '', options]},
        {'fn': 'extract_txt_metadata', 'args': [text2, '', options]},
        {'fn': 'extract_txt_metadata', 'args': [f'only the cited {d2}\n', '', options]},
        {'fn': 'extract_txt_metadata', 'args': ['no doi here\n', '', options]},
        {'fn': 'extract_txt_metadata', 'args': ['no doi here\n', f'https://doi.org/{d1}', options]},
        {'fn': 'confirm_doi', 'args': [[d2, d1], text1]}, {'fn': 'confirm_doi', 'args': [[d2], text1]}, {'fn': 'confirm_doi', 'args': [['10.1/offline'], text1]},
    ]})
    text3 = f'{ice} by Lovelace and Babbage, with no identifier printed'
    q6 = {'rows': 5, 'select': SELECT, 'query.bibliographic': 'no doi here but ice sheet words'}
    out.append({'name': 'extract_fulltext', 'fixtures': {query_url(q6): fixture({'message': {'items': items}}), CROSSREF + d3: fixture('x', 404, 'text/plain', 'Not Found'),
                                                         DOIORG + d3: fixture('', 404, 'text/html', 'Not Found'),
                                                         query_url({'rows': 5, 'select': SELECT, 'query.bibliographic': f'{ice} but doi:{d3}'}): fixture({'message': {'items': items}})},
                'calls': [{'fn': 'extract_txt_metadata', 'args': ['no doi here but ice sheet words', '', {'search_doi': True, 'search_fulltext': True}]},
                          {'fn': 'extract_txt_metadata', 'args': ['no doi here but ice sheet words', '', {'search_doi': False, 'search_fulltext': True, 'max_query_words': 200}]},
                          {'fn': 'extract_txt_metadata', 'args': ['two words', '', {'search_doi': False, 'search_fulltext': True}]},
                          {'fn': 'extract_txt_metadata', 'args': [f'{ice} but doi:{d3}', '', {'search_doi': True, 'search_fulltext': True}]},
                          {'fn': 'extract_txt_metadata', 'args': ['no doi here but ice sheet words', '', {'search_doi': True, 'search_fulltext': False}]},
                          {'fn': 'extract_txt_metadata', 'args': [text3, '', {'search_doi': True, 'search_fulltext': True}]},
                          {'fn': 'extract_txt_metadata', 'args': [text3, '', {'search_doi': False, 'search_fulltext': True, 'max_query_words': 6}]}]})
    out[-1]['fixtures'][query_url({'rows': 5, 'select': SELECT, 'query.bibliographic': text3})] = fixture({'message': {'items': items}})
    out[-1]['fixtures'][query_url({'rows': 5, 'select': SELECT, 'query.bibliographic': ' '.join(text3.split()[:6])})] = fixture({'message': {'items': items}})
    # the journal website: a page that links a .bib file, resolved against doi.org
    page = '<html><body><a href="https://pub.example.org/cite/other.bib">o</a><a href="https://pub.example.org/cite/this.bib">t</a><a href="x.html">h</a></body></html>'
    bib_other = '@article{o,\n doi = {10.1/another},\n title = {O}\n}\n'
    bib_this = '@article{t,\n DOI = {10.7777/Journal.1},\n title = {This one}\n}\n'
    out.append({'name': 'journal_website', 'fixtures': {
        CROSSREF + '10.7777/journal.1': fixture('x', 404, 'text/plain', 'Not Found'), DOIORG + '10.7777/journal.1': fixture('', 200, 'text/html'),
        'https://doi.org/10.7777/journal.1': fixture(page, content_type='text/html; charset=utf-8'),
        'https://pub.example.org/cite/other.bib': fixture(bib_other, content_type='text/plain'),
        'https://pub.example.org/cite/this.bib': fixture(bib_this, content_type='application/x-bibtex')},
        'calls': [{'fn': 'fetch_bibtex_by_doi', 'args': ['10.7777/journal.1']}, {'fn': 'fetch_bibtex_on_journal_website', 'args': ['10.7777/journal.1']},
                  {'fn': 'fetch_bibtex_on_journal_website', 'args': ['10.7777/missing']}]})
    out[-1]['fixtures']['https://doi.org/10.7777/missing'] = fixture('<a href="https://pub.example.org/cite/other.bib">o</a>', content_type='text/html')
    out[-1]['calls'].append({'fn': 'fetch_bibtex_on_journal_website', 'args': ['10.7777/gone']})
    out[-1]['fixtures']['https://doi.org/10.7777/gone'] = fixture('', 410, 'text/html', 'Gone')
    out.extend(extra)
    return out


def recorded_scenarios():
    "the answers recorded from the network (tests/record_network.py)"
    path = os.path.join(HERE, 'fixtures', 'network.json')
    if not os.path.exists(path):
        return []
    recorded = json.load(open(path, encoding='utf-8'))
    fixtures = {url: {k: v for k, v in f.items() if k != 'accept'} for url, f in recorded.items()}
    title = 'Reconstructions of the 1900–2015 Greenland ice sheet surface mass balance using the regional climate MAR model'
    text = f'The Cryosphere, 11, 1015–1033, 2017\nwww.the-cryosphere.net/11/1015/2017/\ndoi:10.5194/tc-11-1015-2017\n{title}\nXavier Fettweis\n'
    query = 'Reconstructions of the 1900-2015 Greenland ice sheet surface mass balance using the regional climate MAR model'
    return [{'name': 'recorded', 'fixtures': fixtures, 'calls': [
        {'fn': 'fetch_bibtex_by_doi', 'args': ['10.5194/tc-11-1015-2017']},
        {'fn': 'fetch_bibtex_by_doi', 'args': ['10.5281/zenodo.2580252']},
        {'fn': 'fetch_bibtex_by_doi', 'args': ['10.9999/not.a.doi']},
        {'fn': 'fetch_bibtex_by_doi', 'args': ['10.48550/arXiv.1706.03762']},
        {'fn': 'fetch_bibtex_by_fulltext_crossref', 'args': [query]},
        {'fn': 'extract_txt_metadata', 'args': [text, '', {'search_doi': True, 'search_fulltext': False}]},
        {'fn': 'confirm_doi', 'args': [['10.5281/zenodo.2580252', '10.5194/tc-11-1015-2017'], text]},
    ]}]


def write_pdfs(directory):
    "small PDFs: the DOI in the XMP, in the information, in the text; no DOI; two pages"
    os.makedirs(directory, exist_ok=True)

    def pdf(pages, info=None, xmp=None):
        objects = []

        def add(body):
            objects.append(body)
            return len(objects)

        catalog_id = add(None)
        pages_id = add(None)
        font_id = add(b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>')
        page_ids = []
        for lines in pages:
            stream = b'BT /F1 12 Tf 14 TL 72 720 Td ' + b' '.join(b'(' + l.replace('\\', '\\\\').replace('(', '\\(').replace(')', '\\)').encode('latin-1') + b') Tj T*' for l in lines) + b' ET'
            content_id = add(b'<< /Length %d >>\nstream\n' % len(stream) + stream + b'\nendstream')
            page_ids.append(add(b'<< /Type /Page /Parent %d 0 R /MediaBox [0 0 612 792] /Contents %d 0 R /Resources << /Font << /F1 %d 0 R >> >> >>' % (pages_id, content_id, font_id)))
        objects[pages_id - 1] = b'<< /Type /Pages /Kids [' + b' '.join(b'%d 0 R' % i for i in page_ids) + b'] /Count %d >>' % len(page_ids)
        root = b'<< /Type /Catalog /Pages %d 0 R' % pages_id
        if xmp:
            xmp_id = add(b'<< /Type /Metadata /Subtype /XML /Length %d >>\nstream\n' % len(xmp) + xmp + b'\nendstream')
            root += b' /Metadata %d 0 R' % xmp_id
        objects[catalog_id - 1] = root + b' >>'
        trailer = b'/Root 1 0 R'
        if info:
            info_id = add(b'<< ' + b' '.join(b'/%s (%s)' % (k.encode(), v.encode('latin-1')) for k, v in info.items()) + b' >>')
            trailer += b' /Info %d 0 R' % info_id
        out = b'%PDF-1.4\n'
        offsets = []
        for i, body in enumerate(objects, 1):
            offsets.append(len(out))
            out += b'%d 0 obj\n' % i + body + b'\nendobj\n'
        start = len(out)
        out += b'xref\n0 %d\n0000000000 65535 f \n' % (len(objects) + 1) + b''.join(b'%010d 00000 n \n' % o for o in offsets)
        out += b'trailer\n<< /Size %d %s >>\nstartxref\n%d\n%%%%EOF\n' % (len(objects) + 1, trailer, start)
        return out

    ice = 'Ice sheet response to warming in the early Holocene'
    body = ['Ada Lovelace and Charles Babbage', 'Journal of Things 7, 10-20 (2020)', 'doi:10.1234/ice.2020.1', 'Abstract. We study the ice sheet and its response.']
    files = {
        'text-doi.pdf': pdf([[ice] + body]),
        'xmp-doi.pdf': pdf([[ice, 'Nothing else here.']], xmp=b'<?xpacket begin="" id="x"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">'
                                                              b'<rdf:Description xmlns:prism="http://prismstandard.org/namespaces/basic/2.0/"><prism:doi>10.1234/ICE.2020.1</prism:doi></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>'),
        'info-doi.pdf': pdf([[ice, 'Nothing else here.']], info={'Title': 'Ice sheet', 'Subject': 'Journal of Things, doi:10.1234/ice.2020.1', 'Keywords': 'ice'}),
        'no-doi.pdf': pdf([['No identifier in this document at all, only some words to read.']], info={'Title': 'Untitled'}),
        'two-pages.pdf': pdf([[ice] + body[:2], ['Second page text', 'references: 10.1234/cited.2019.7'], ['Last page: https://doi.org/10.1234/ice.2020.1']]),
        # a real paper's title and DOI, for the cases that use the network (spec/cases)
        'real-text-doi.pdf': pdf([['The Cryosphere, 11, 1015-1033, 2017', 'doi:10.5194/tc-11-1015-2017',
                                   'Reconstructions of the 1900-2015 Greenland ice sheet surface mass', 'balance using the regional climate MAR model',
                                   'Xavier Fettweis, Jason E. Box, Cecile Agosta and others']]),
        'real-xmp-doi.pdf': pdf([['Reconstructions of the 1900-2015 Greenland ice sheet surface mass balance']],
                                xmp=b'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:prism="http://prismstandard.org/namespaces/basic/2.0/">'
                                    b'<prism:doi>10.5194/tc-11-1015-2017</prism:doi></rdf:Description></rdf:RDF></x:xmpmeta>'),
    }
    for name, data in files.items():
        with open(os.path.join(directory, name), 'wb') as f:
            f.write(data)
    return files


def pdf_scenarios(directory):
    ice = 'Ice sheet response to warming in the early Holocene'
    d1, d2 = '10.1234/ice.2020.1', '10.1234/cited.2019.7'
    ok = fixture({'status': 'ok', 'message': record(d1, ice)})
    cited = fixture({'status': 'ok', 'message': record(d2, 'A different paper about ocean heat')})
    path = lambda name: os.path.join(directory, name)  # noqa: E731
    fx = {CROSSREF + d1: ok, CROSSREF + d2: cited, CROSSREF + '10.1234/ICE.2020.1': ok}
    calls = []
    for name in ['text-doi.pdf', 'xmp-doi.pdf', 'info-doi.pdf', 'no-doi.pdf', 'two-pages.pdf']:
        calls.append({'fn': 'extract_pdf_doi', 'args': [name]})
        calls.append({'fn': 'extract_pdf_metadata', 'args': [name, {'search_fulltext': False}]})
    return {'name': 'pdfs', 'fixtures': fx, 'calls': calls, 'pdfs': directory}


def main():
    if os.path.isdir(OUT) and OUT == os.path.join(HERE, 'expected', 'extract'):
        pass
    write_json('doi_text.json', doi_cases(OPTIONS.count))
    write_json('doi_misc.json', misc_cases())
    write_json('fetch.json', {'crossref': crossref_cases(OPTIONS.cache), 'score': score_cases(OPTIONS.cache), 'token_set_ratio': token_cases(),
                              'cache': cache_cases(), 'html': html_cases()})
    # the PDFs of the tests, in tests/fixtures/pdf, and the flows on them
    pdfs = os.path.join(HERE, 'fixtures', 'pdf')
    write_pdfs(pdfs)
    result = []
    flows = scenarios(recorded_scenarios())
    # the PDF scenario reads the files by their path, which the test gives relative to tests/fixtures/pdf
    pdf_flow = pdf_scenarios(pdfs)
    for call in pdf_flow['calls']:
        call['args'][0] = os.path.join(pdfs, call['args'][0])
    for scenario in flows + [pdf_flow]:
        r = run_scenario(scenario)
        if scenario is pdf_flow:
            for call in r['calls']:  # the path of the file, relative
                call['args'][0] = os.path.basename(call['args'][0])
        result.append(r)
    write_json('flows.json', result)
    shutil.rmtree(TMP, ignore_errors=True)


main()
