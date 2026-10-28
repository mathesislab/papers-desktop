#!/usr/bin/env python3
"""Generate tests/expected/pools.json from the reference, papers-cli (Python).

The origin records of a library and how the exchanges match the entries of a library and of its
pool (`papers.pools`, spec `exchange.md` sections 2.3, 3 and 4):

  names     the file of the origin records of a pool id
  origin    sequences of operations on the records (`Origin.set`, `set_files`, `remove`, `rekey`,
            `set_pulled`) and the files that `Origin.save` writes (the times of the records aside)
  matching  for libraries made of a bibtex, the bibtex of the pool, origin records and pairs that are
            not the same paper: the pool entry of each library entry and its record (`pool_entry`),
            the entries shared with the pool and those removed from it (`sharing`), the library entry
            of each pool entry (`library_entry`) and the pool entries missing from the library

tests/pools.rs replays them. Run with the python that has papers installed (the reference), from
anywhere:

    /path/to/python3 tests/gen_pools.py

It never touches the configuration or the libraries of the user: HOME is a temporary folder.
"""
import json
import logging
import os
import re
import shutil
import tempfile
import warnings

HERE = os.path.dirname(os.path.abspath(__file__))
TMP = tempfile.mkdtemp(prefix='papers-rs-pools-')
os.environ['HOME'] = TMP  # before papers is imported
os.environ['XDG_CACHE_HOME'] = os.path.join(TMP, 'cache')
os.environ['XDG_CONFIG_HOME'] = os.path.join(TMP, 'config')
os.chdir(TMP)
warnings.simplefilter('ignore')

from papers.config import Config, MachineConfig  # noqa: E402
from papers.entries import entry_from_dict  # noqa: E402
from papers.library import get_biblio  # noqa: E402
from papers.pools import Exchange, Origin, origin_file_name  # noqa: E402

logging.disable(logging.CRITICAL)

IDS = ['../pool', '../pool/', '../x/../pool', '~/Documents/papers', '../shared-with-bob', '../My Papers', '.',
       './sub', '/abs/path/to/library', '../../home/me/Documents/bob', '', '../ü-pool', '../a b/c.d', 'pool',
       '../' + 'long' * 30]


def names():
    return {i: origin_file_name(i) for i in IDS}


def entry(key, fields, type='article'):
    return entry_from_dict({'ID': key, 'ENTRYTYPE': type, **fields})


TIME = re.compile(r'papers-time = \{[^}]*\}')


def origin_texts(dir):
    out = {}
    for name in sorted(os.listdir(dir)) if os.path.isdir(dir) else []:
        out[name] = TIME.sub('papers-time = {$TIME}', open(os.path.join(dir, name)).read())
    return out


OPS = [
    # a pull of two entries, a link, a time, a rekey, a removal
    [['set', 'Smith2020', '../pool', {'ID': 'smith_2020', 'title': 'T', 'year': '2020'}, ''],
     ['set', 'brown2021', '../pool', {'ID': 'brown2021', 'title': 'B', 'file': ':f.pdf:pdf'}, None],
     ['pulled', '../pool', 1793189510108569698],
     ['set_files', 'Smith2020', '../pool', ':../pool/files/a.pdf:pdf'],
     ['rekey', 'Smith2020', 'Smith2021'],
     ['remove', 'brown2021', '../pool']],
    # two pools, records of the same key, the same record set again, with and without a link
    [['set', 'a', '../one', {'ID': 'a1', 'title': 'A'}, None],
     ['set', 'a', '../two', {'ID': 'a2', 'title': 'A', 'doi': '10.1/x'}, ':x:pdf'],
     ['set', 'a', '../one', {'ID': 'a1', 'title': 'A'}, ':y:pdf'],
     ['set', 'a', '../one', {'ID': 'a1', 'title': 'A2'}, ':y:pdf'],
     ['pulled', '../two', 5], ['pulled', '../one', 7]],
    # unicode, braces, long values, the key case
    [['set', 'Müller2019', '../p', {'ID': 'mueller', 'title': 'Über {Ice}', 'author': 'Müller, Hans and Doe, Jane',
                                    'abstract': 'x ' * 200}, ''],
     ['set', 'ZED', '../p', {'ID': 'zed', 'title': 'Z'}, None],
     ['set', 'abc', '../p', {'ID': 'abc', 'title': 'A'}, None]],
]


def run_ops(ops):
    d = tempfile.mkdtemp(dir=TMP)
    origin = Origin(os.path.join(d, 'origin'))
    for op in ops:
        kind = op[0]
        if kind == 'set':
            _, key, pool, fields, files = op
            origin.set(key, pool, entry(fields.pop('ID'), fields), files=files)
        elif kind == 'set_files':
            origin.set_files(op[1], op[2], op[3])
        elif kind == 'remove':
            origin.remove(op[1], op[2])
        elif kind == 'rekey':
            origin.rekey(op[1], op[2])
        elif kind == 'pulled':
            origin.set_pulled(op[1], op[2])
    origin.save()
    return origin_texts(origin.dir)


def sets(ops):
    "the operations with fresh dicts (the runs consume them)"
    return json.loads(json.dumps(ops))


def bibtex(entries):
    out = ''
    for e in entries:
        fields = ''.join(f' {k} = {{{v}}},\n' for k, v in e['fields'])
        out += f'@{e.get("type", "article")}{{{e["key"]},\n{fields}}}\n\n'
    return out


def e(key, *fields, type='article'):
    return {'key': key, 'type': type, 'fields': [list(f) for f in fields]}


A = ('author', 'Smith, John and Doe, Jane')
SCENARIOS = [
    {'name': 'same key and doi',
     'library': [e('smith2020', A, ('title', 'Ice'), ('doi', '10.1000/abc'))],
     'pool': [e('smith2020', A, ('title', 'Ice'), ('doi', '10.1000/abc'))], 'records': []},
    {'name': 'the pool renamed the key',
     'library': [e('smith2020', A, ('title', 'Ice'), ('doi', '10.1000/abc'))],
     'pool': [e('smith_doe2020', A, ('title', 'Ice'), ('doi', '10.1000/abc'))],
     'records': [e('smith2020', A, ('title', 'Ice'), ('doi', '10.1000/abc'), ('papers-pool-key', 'smith2020'))]},
    {'name': 'no record, the same doi under another key',
     'library': [e('mine', ('title', 'Mine'), ('doi', '10.1000/xyz'))],
     'pool': [e('theirs', ('title', 'Mine'), ('doi', '10.1000/XYZ'))], 'records': []},
    {'name': 'no record, same authors and title without doi',
     'library': [e('a1', A, ('title', 'Same title'))],
     'pool': [e('b1', ('author', 'Smith, J. and Doe, J.'), ('title', 'Same  Title'))], 'records': []},
    {'name': 'the title changed on both sides: found by the pool key',
     'library': [e('lib1', A, ('title', 'Library title'))],
     'pool': [e('pool1', A, ('title', 'Pool title'))],
     'records': [e('lib1', A, ('title', 'Old title'), ('papers-pool-key', 'pool1'))]},
    {'name': 'the pool key was reused by another paper',
     'library': [e('lib1', ('title', 'A'), ('doi', '10.1/a'))],
     'pool': [e('pool1', ('title', 'B'), ('doi', '10.1/b'))],
     'records': [e('lib1', ('title', 'A'), ('doi', '10.1/a'), ('papers-pool-key', 'pool1'))]},
    {'name': 'the pool has the paper twice: the record picks one',
     'library': [e('lib1', ('title', 'Twice'), ('doi', '10.1/t'))],
     'pool': [e('first', ('title', 'Twice'), ('doi', '10.1/t')), e('second', ('title', 'Twice'), ('doi', '10.1/t'))],
     'records': [e('lib1', ('title', 'Twice'), ('doi', '10.1/t'), ('papers-pool-key', 'second'))]},
    {'name': 'the pool has the paper twice, no record: the first',
     'library': [e('lib1', ('title', 'Twice'), ('doi', '10.1/t'))],
     'pool': [e('first', ('title', 'Twice'), ('doi', '10.1/t')), e('second', ('title', 'Twice'), ('doi', '10.1/t'))],
     'records': []},
    {'name': 'distinct pairs',
     'library': [e('lib1', ('title', 'Distinct'), ('doi', '10.1/d'))],
     'pool': [e('pool1', ('title', 'Distinct'), ('doi', '10.1/d'))], 'records': [],
     'distinct': [[{'doi': '10.1/d', 'title': 'distinct'}, {'doi': '10.1/d', 'title': 'distinct'}]]},
    {'name': 'removed from the pool',
     'library': [e('lib1', ('title', 'Gone'), ('doi', '10.1/g')), e('lib2', ('title', 'Here'), ('doi', '10.1/h'))],
     'pool': [e('here', ('title', 'Here'), ('doi', '10.1/h'))],
     'records': [e('lib1', ('title', 'Gone'), ('doi', '10.1/g'), ('papers-pool-key', 'gone'))]},
    {'name': 'a former key of the entry finds its record',
     'library': [e('newkey', ('title', 'Renamed'), ('doi', '10.1/r'), ('ids', 'oldkey'))],
     'pool': [e('pool1', ('title', 'Renamed'), ('doi', '10.1/r'))],
     'records': [e('oldkey', ('title', 'Renamed'), ('doi', '10.1/r'), ('papers-pool-key', 'pool1'))]},
    {'name': 'the record key without case',
     'library': [e('Lib1', ('title', 'Case'), ('doi', '10.1/c'))],
     'pool': [e('Pool1', ('title', 'Case'), ('doi', '10.1/c'))],
     'records': [e('lib1', ('title', 'Case'), ('doi', '10.1/c'), ('papers-pool-key', 'pool1'))]},
    {'name': 'arxiv and isbn',
     'library': [e('x1', ('title', 'Preprint'), ('eprint', 'arXiv:2101.00001')),
                 e('b1', ('title', 'A book'), ('isbn', '978-3-16-148410-0'), type='book')],
     'pool': [e('y1', ('title', 'Preprint'), ('eprint', '2101.00001v2')),
              e('b2', ('title', 'A book'), ('isbn', '9783161484100'), type='book')], 'records': []},
    {'name': 'different dois are not the same paper',
     'library': [e('lib1', ('title', 'Same title'), ('doi', '10.1/one'), A)],
     'pool': [e('pool1', ('title', 'Same title'), ('doi', '10.1/two'), A)], 'records': []},
    {'name': 'missing entries and entries of two records',
     'library': [e('l1', ('title', 'One'), ('doi', '10.1/1'))],
     'pool': [e('p1', ('title', 'One'), ('doi', '10.1/1')), e('p2', ('title', 'Two'), ('doi', '10.1/2')),
              e('p3', ('title', 'Three'), ('doi', '10.1/3'))],
     'records': [e('l1', ('title', 'One'), ('doi', '10.1/1'), ('papers-pool-key', 'p1')),
                 e('l9', ('title', 'Two'), ('doi', '10.1/2'), ('papers-pool-key', 'p2'))]},
]


def matching(scenario):
    d = tempfile.mkdtemp(dir=TMP)
    for sub, cfg in [('lib', {'bibtex': 'papers.bib', 'filesdir': 'files', 'pool': '../pool', 'version': 3}),
                     ('pool', {'bibtex': 'papers.bib', 'filesdir': 'files', 'pool': 'none', 'version': 3})]:
        os.makedirs(os.path.join(d, sub, '.papers'))
        json.dump(cfg, open(os.path.join(d, sub, '.papers', 'config.json'), 'w'))
    open(os.path.join(d, 'lib', 'papers.bib'), 'w').write(bibtex(scenario['library']))
    open(os.path.join(d, 'pool', 'papers.bib'), 'w').write(bibtex(scenario['pool']))
    if scenario['records']:
        os.makedirs(os.path.join(d, 'lib', '.papers', 'origin'))
        text = '@comment{papers-pool ../pool}\n\n' + bibtex(scenario['records'])
        open(os.path.join(d, 'lib', '.papers', 'origin', origin_file_name('../pool')), 'w').write(text)
    if scenario.get('distinct'):
        json.dump({'distinct': scenario['distinct']}, open(os.path.join(d, 'lib', '.papers', 'distinct.json'), 'w'))
    config = Config.load(os.path.join(d, 'lib', '.papers', 'config.json'), machine=MachineConfig())
    biblio = get_biblio(config)
    exchange = Exchange(config, biblio)
    pool = exchange.pools.target()
    entries = []
    for e in biblio.entries:
        pe, record = exchange.pool_entry(e, pool)
        entries.append({'key': e.key, 'pool_key': pe.key if pe is not None else None,
                        'record': record.pool_key if record is not None else None})
    shared, removed = exchange.sharing(biblio.entries, pool)
    library_entries = {}
    for pe in pool.biblio.entries:
        le = exchange.library_entry(pe, pool)
        library_entries[pe.key] = le.key if le is not None else None
    return {'entries': entries, 'shared': [x.key for x in shared], 'removed': [x.key for x in removed],
            'library_entries': library_entries, 'missing': [x.key for x in exchange.missing(pool)]}


def main():
    out = {'names': names(), 'origin': [], 'matching': []}
    for ops in OPS:
        out['origin'].append({'ops': ops, 'files': run_ops(sets(ops))})
    for s in SCENARIOS:
        out['matching'].append({'scenario': s, 'expected': matching(s)})
    path = os.path.join(HERE, 'expected', 'pools.json')
    with open(path, 'w') as f:
        json.dump(out, f, indent=1, sort_keys=True, ensure_ascii=False)
        f.write('\n')
    shutil.rmtree(TMP, ignore_errors=True)
    print('wrote', path)


if __name__ == '__main__':
    main()
