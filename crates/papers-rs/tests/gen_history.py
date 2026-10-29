#!/usr/bin/env python3
"""Generate tests/history/*.json from the reference, papers-cli (Python).

Each scenario is a sequence of steps run with the real `papers` in a sandbox (a
temporary HOME and working folder: the configuration, the history and the libraries
of the user are never touched): commands, edits made outside papers, files put in
the library, the time passing for the retention. After each step the file records
what changed in the working folder and in the history store (the deltas, the
store as papers wrote it: its objects are zlib streams), with the standard output
of the command. The Rust tests (tests/history.rs) replay them:

  - papers-rs undoes, redoes and restores what papers recorded, and says what
    papers said (the report on the files);
  - papers-rs records the step of each command, given what the command did, and the
    store is the one papers wrote (decompressed objects, the states and heads
    without their times);
  - `papers history` and `papers history --all` print what papers printed.

Run with the reference papers:

    python3 tests/gen_history.py [--papers PATH] [SCENARIO ...]

The paths of the sandbox are written `$TMP` in the files. The identifiers of the
libraries are random: they are what papers made. A step of the history is named by the
hash of its state, which depends on the time: the arguments name it by its place in the
history (`$STEP:3`, see below), and the Rust tests compare the stores and the outputs with
the identifiers replaced by that place.
"""
import argparse
import base64
import hashlib
import json
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, 'history')

BIB = """@article{doe2000,
 title = {Ice sheets and sea level},
 author = {Doe, Jane and Roe, Richard},
 year = {2000},
 journal = {Nature},
 doi = {10.1000/doe.2000},
}

@article{roe2001,
 title = {Sea level rise in the next century},
 author = {Roe, Richard},
 year = {2001},
 journal = {Science},
 doi = {10.1000/roe.2001},
}

@book{smith2002,
 title = {Glaciers},
 author = {Smith, John},
 year = {2002},
 publisher = {Ice Press},
}
"""

MORE = """@article{new2003,
 title = {A new paper},
 author = {New, Nick},
 year = {2003},
 journal = {Nature},
 doi = {10.1000/new.2003},
}
"""

OTHER = """@article{other2005,
 title = {Another paper},
 author = {Other, Olga},
 year = {2005},
 journal = {Science},
 doi = {10.1000/other.2005},
}
"""

PDF_A = b'%PDF-1.4\nfake pdf a\n%%EOF\n'
PDF_B = b'%PDF-1.4\nfake pdf b, longer than a\n%%EOF\n'


def big_bibtex(seed, entries=2500):
    "a bibtex of random entries (about 1 MB that compresses badly)"
    rnd = random.Random(seed)
    out = []
    for i in range(entries):
        words = ' '.join(''.join(rnd.choice('abcdefghijklmnopqrstuvwxyz0123456789') for _ in range(9)) for _ in range(40))
        out.append('@article{k%s_%d,\n title = {%s},\n author = {Doe, Jane},\n year = {%d},\n}\n' % (seed, i, words, 1900 + i % 100))
    return '\n'.join(out)


def many_bibtex(n):
    "n entries (the history cuts a bibtex into pieces of about 32 entries)"
    out = []
    for i in range(n):
        out.append('@article{k%03d,\n title = {Paper number %d on the topic %s},\n author = {Author%d, Anna},\n year = {%d},\n'
                   ' journal = {Journal %d},\n doi = {10.1000/many.%d},\n}\n' % (i, i, 'x' * (i % 7), i, 1950 + i % 70, i % 9, i))
    return '\n'.join(out)


# the scenarios
# =============
# a step is a tuple:
#   ('papers', argv, {cwd, lib, stdin, bibtex})   run papers in work/<cwd> (default: the library `lib`)
#   ('write', path, text) / ('writeb', path, bytes) / ('rm', path) / ('cp', src, dst) / ('mv', src, dst)
#   ('age', seconds)       the objects of the store are older (the retention keeps the young ones)
#   ('numbered',)          the store as papers 3.0.0b5 and earlier wrote it: states named by numbers (papers upgrades it
#                          at its first command: the identifiers do not depend on the numbers)
# in the arguments of a command:
#   $ID:folder             the identifier of the library in that folder
#   $STEP:k                the first 8 characters of the identifier of the k-th state (from 1, by time) of the library
#                          of the command (what papers 3.0.0b5 called step k)
#   $STEP4:k               the shortest start of it of 4 characters or more that no other state has
#   $STEPU:k               $STEP:k in upper case

def p(*argv, **kw):
    return ('papers', list(argv), kw)


SCENARIOS = {
    'basic': dict(steps=[
        p('init', 'lib', cwd='.', lib=None),
        p('add', '/dev/stdin', stdin=BIB),
        p('history'),
        p('tag', 'doe2000', '--add', 'x'),
        p('edit', 'doe2000', '--set', 'year=2001'),
        p('rm', 'roe2001'),
        p('history'),
        p('undo'),
        p('history'),
        p('undo', '-n', '2'),
        p('history'),
        p('redo'),
        p('redo', '-n', '2'),
        p('undo'),
        p('tag', 'smith2002', '--add', 'branch'),
        p('history'),
        p('restore', '$STEP:3'),
        p('history', '-n', '2'),
        p('restore', '$STEP4:5'),
        p('redo'),
        p('undo', '-n', '9'),
        p('undo', '-n', '4'),
        p('undo'),
        p('undo'),
        p('redo', '-n', '9'),
        p('restore', '99'),
        p('history', '--all'),
    ]),
    'many': dict(steps=[
        p('init', 'lib', cwd='.', lib=None),
        p('add', '/dev/stdin', stdin=many_bibtex(110)),
        p('tag', 'k007', '--add', 'x'),
        p('tag', 'k090', '--add', '\u00fcn\u00ef'),
        p('edit', 'k100', '--set', 'year=2001'),
        p('rm', 'k001', 'k109'),
        p('history'),
        p('undo', '-n', '2'),
        p('redo'),
        p('undo', '-n', '5'),
        p('restore', '$STEP:4'),
        p('redo', '-n', '9'),
        p('history', '-n', '0'),
    ]),
    'files': dict(steps=[
        p('init', 'lib', cwd='.', lib=None),
        p('add', '/dev/stdin', stdin=BIB),
        ('writeb', 'dl/a.pdf', PDF_A),
        ('writeb', 'dl/b.pdf', PDF_B),
        p('file', 'attach', 'doe2000', '../dl/a.pdf', '--move'),
        p('file', 'attach', 'roe2001', '../dl/b.pdf'),
        p('tag', 'doe2000', '--add', 'x'),
        p('rm', 'doe2000', '--delete-files'),
        p('history'),
        p('undo'),
        p('redo'),
        p('undo', '-n', '3'),
        p('redo', '-n', '2'),
        p('file', 'mv', 'files/roe_2001_sea-level-rise-in-the-next-century.pdf', 'files/moved.pdf'),
        p('undo'),
        p('redo'),
        ('writeb', 'lib/files/stray.pdf', b'%PDF-1.4\nstray\n'),
        p('file', 'clean', '-y'),
        p('history'),
        p('undo'),
        p('redo'),
        p('file', 'detach', '-k', 'roe2001', '-y', 'files/moved.pdf'),
        p('history'),
        p('undo'),
        p('redo'),
        p('undo', '-n', '3'),
        ('writeb', 'lib/files/moved.pdf', b'changed since'),
        p('undo', '-n', '2'),
        p('redo', '-n', '2'),
        ('rm', 'lib/files/moved.pdf'),
        p('undo', '-n', '2'),
        p('restore', '$STEP:4'),
        p('restore', '$STEPU:1'),
        p('restore', '$STEP:9'),
        p('history', '--drop-files', '$STEP:8', '-y'),
        p('history', '--drop-files', '$STEP4:7', '-y'),
        p('history', '--drop-files', '$STEP:9', '-y'),
        p('restore', '$STEP:6'),
        p('history', '--drop-files', 'files/doe_and_roe_2000_ice-sheets-and-sea-level.pdf', '-y'),
        p('restore', '$STEP:3'),
        p('restore', '$STEP:6'),
        p('history', '--drop-files', '99', '-y'),
    ]),
    'outside': dict(steps=[
        p('init', 'lib', cwd='.', lib=None),
        p('add', '/dev/stdin', stdin=BIB),
        ('write', 'lib/papers.bib', BIB.replace('Glaciers', 'Glaciers and ice')),
        p('tag', 'doe2000', '--add', 'x'),
        p('history'),
        ('write', 'lib/papers.bib', BIB.replace('Nature', 'Cryosphere')),
        p('undo'),
        p('history'),
        p('redo'),
        p('undo'),
        ('write', 'lib/papers.bib', BIB.replace('2002', '2003')),
        p('redo'),
        p('history'),
        p('restore'),
        p('restore'),
        ('write', 'lib/papers.bib', ''),
        p('restore', '$STEP:2'),
        ('write', 'lib/papers.bib', BIB + '\n% a comment\n'),
        p('history'),
        p('tag', 'smith2002', '--add', 'y'),
        ('write', 'lib/.papers/distinct.json', '{"distinct": []}'),
        p('undo', '-n', '2'),
        p('history'),
    ]),
    'copies': dict(steps=[
        p('init', 'lib', cwd='.', lib=None),
        p('add', '/dev/stdin', stdin=BIB),
        p('tag', 'doe2000', '--add', 'x'),
        ('cp', 'lib', 'lib2'),
        p('tag', 'doe2000', '--add', 'y', lib='lib2'),
        p('history', lib='lib2'),
        p('history'),
        ('mv', 'lib', 'lib3'),
        p('tag', 'roe2001', '--add', 'z', lib='lib3'),
        p('history', lib='lib3'),
        p('undo', lib='lib2'),
        p('history', '--all', lib='lib3'),
        ('cp', 'lib3', 'lib4'),
        ('write', 'lib4/papers.bib', BIB),
        p('undo', lib='lib4'),
        p('history', lib='lib4'),
    ]),
    'separate': dict(machine={'history-separate': True}, steps=[
        p('init', 'lib', cwd='.', lib=None),
        p('add', '/dev/stdin', stdin=BIB),
        p('tag', 'doe2000', '--add', 'x'),
        p('history'),
        p('undo'),
        p('redo'),
        p('history', '--all'),
    ]),
    # a shallow library and its pool
    'pools': dict(steps=[
        p('init', 'pool', cwd='.', lib=None),
        p('add', '/dev/stdin', stdin=BIB, lib='pool'),
        p('init', 'lib', '--pool', 'pool', cwd='.', lib=None),
        ('write', 'more.bib', MORE),
        ('writeb', 'a.pdf', PDF_A),
        p('add', '../more.bib'),
        p('history'),
        p('history', lib='pool'),
        p('file', 'attach', 'new2003', '../a.pdf', '--move'),
        p('tag', 'new2003', '--add', 'x'),
        p('history', lib='pool'),
        p('undo'),
        p('history'),
        p('history', lib='pool'),
        p('redo'),
        p('undo', '-n', '3'),
        p('history', lib='pool'),
        p('redo', '-n', '3'),
        p('undo', '--library-only'),
        p('history', lib='pool'),
        p('redo', '--library-only'),
        p('undo', '-n', '2', '--library-only'),
        p('redo', '-n', '2', '--library-only'),
        p('undo', '-n', '2'),
        p('restore', '$STEP:2'),
        p('history', '--all'),
    ]),
    # the pool moves on after a command of the library: undo reverts the pool's step there, redo reverts the
    # revert; undo takes the pool's step back and the pool moves on: redo does the step again there
    'pools-moved': dict(steps=[
        p('init', 'pool', cwd='.', lib=None),
        p('add', '/dev/stdin', stdin=BIB, lib='pool'),
        p('init', 'lib', '--pool', 'pool', cwd='.', lib=None),
        ('write', 'more.bib', MORE),
        ('write', 'other.bib', OTHER),
        ('writeb', 'a.pdf', PDF_A),
        p('add', '../other.bib'),
        p('add', '../more.bib'),
        p('file', 'attach', 'new2003', '../a.pdf', '--move'),
        p('tag', 'doe2000', '--add', 'b', lib='pool'),
        p('undo'),
        p('history'),
        p('history', lib='pool'),
        p('redo'),
        p('history', lib='pool'),
        p('undo', '-n', '2'),
        p('history', lib='pool'),
        p('tag', 'roe2001', '--add', 'c', lib='pool'),
        p('redo', '-n', '2'),
        p('history', lib='pool'),
        p('history'),
        p('undo', '-n', '3'),
        p('history', lib='pool'),
        p('redo', '-n', '3'),
        p('history', '--all'),
    ]),
    # the numbered states of papers 3.0.0b5 and earlier are named by their hash at the first command
    'numbered': dict(steps=[
        p('init', 'lib', cwd='.', lib=None),
        p('add', '/dev/stdin', stdin=BIB),
        p('tag', 'doe2000', '--add', 'x'),
        p('edit', 'doe2000', '--set', 'year=2001'),
        p('undo'),
        p('tag', 'smith2002', '--add', 'branch'),
        ('numbered',),
        p('history'),
        p('undo'),
        p('history'),
        p('redo'),
        p('restore', '$STEP:3'),
        p('history', '-n', '0'),
    ]),
    # the same with a library and its pool in two stores: the link of a step to the pool's step keeps a number
    'numbered-pools': dict(machine={'history-separate': True}, steps=[
        p('init', 'pool', cwd='.', lib=None),
        p('add', '/dev/stdin', stdin=BIB, lib='pool'),
        p('init', 'lib', '--pool', 'pool', cwd='.', lib=None),
        ('write', 'more.bib', MORE),
        p('add', '../more.bib'),
        ('numbered',),
        p('undo'),
        p('history', lib='pool'),
        p('redo'),
        p('history', '--all'),
        p('tag', 'new2003', '--add', 'x'),
        p('history', '--revert', '$STEP:2'),
    ]),
    'remove': dict(steps=[
        p('init', 'lib', cwd='.', lib=None),
        p('add', '/dev/stdin', stdin=BIB),
        p('tag', 'doe2000', '--add', 'x'),
        ('cp', 'lib', 'lib2'),
        p('tag', 'doe2000', '--add', 'y', lib='lib2'),
        p('history', '--remove', '-y', lib='lib2'),
        p('history', lib='lib2'),
        p('history', '--all', lib='lib2'),
        p('history', '--remove', '-y'),
        p('history', '--all'),
        p('history', '--remove', '-y'),
        p('tag', 'doe2000', '--add', 'z'),
        p('history', '--remove', '$ID:lib', '-y'),
        p('history', '--remove', '0123456789abcdef0123456789abcdef', '-y'),
        p('history', '--all'),
        p('history'),
    ]),
    # papers undo, redo, restore on a library that has no history: undo gives it an identifier and a first state
    'fresh': dict(steps=[
        p('init', 'lib', cwd='.', lib=None),
        p('undo'),
        p('redo'),
        p('restore'),
        p('restore', '1'),
        p('history'),
        p('add', '/dev/stdin', stdin=BIB),
        p('undo', '-n', '2'),
        p('redo'),
    ]),
    # a bibtex without library, with history-separate: the code puts it in a store of its own (the specification says the shared store)
    'bibtex-separate': dict(machine={'history-separate': True}, steps=[
        ('write', 'x.bib', BIB),
        p('tag', 'doe2000', '--add', 'x', '--bibtex', '$TMP/work/x.bib', cwd='.', lib=None, bibtex='x.bib'),
        p('undo', '--bibtex', '$TMP/work/x.bib', cwd='.', lib=None, bibtex='x.bib'),
        p('history', '--all', cwd='.', lib=None),
    ]),
    'bibtex': dict(steps=[
        ('write', 'x.bib', BIB),
        p('tag', 'doe2000', '--add', 'x', '--bibtex', '$TMP/work/x.bib', cwd='.', lib=None, bibtex='x.bib'),
        p('edit', 'doe2000', '--set', 'year=2005', '--bibtex', '$TMP/work/x.bib', cwd='.', lib=None, bibtex='x.bib'),
        p('undo', '--bibtex', '$TMP/work/x.bib', cwd='.', lib=None, bibtex='x.bib'),
        p('redo', '--bibtex', '$TMP/work/x.bib', cwd='.', lib=None, bibtex='x.bib'),
        p('undo', '-n', '2', '--bibtex', '$TMP/work/x.bib', cwd='.', lib=None, bibtex='x.bib'),
        p('history', '--all', cwd='.', lib=None),
    ]),
}


# the sandbox
# ===========

def platform_dirs(home):
    if sys.platform == 'darwin':
        support = os.path.join(home, 'Library', 'Application Support', 'papers')
        return support, support
    return os.path.join(home, '.config', 'papers'), os.path.join(home, '.local', 'share', 'papers')


class Sandbox:
    def __init__(self, papers, machine):
        self.papers = papers
        self.root = os.path.realpath(tempfile.mkdtemp(prefix='papers-rs-history-'))
        self.home = os.path.join(self.root, 'home')
        self.work = os.path.join(self.root, 'work')
        os.makedirs(self.home)
        os.makedirs(self.work)
        config, data = platform_dirs(self.home)
        self.store = os.path.join(data, 'history')
        self.machine = dict(machine)
        if machine:
            os.makedirs(config)
            with open(os.path.join(config, 'config.json'), 'w') as f:
                json.dump(dict(machine, version=3), f)

    def env(self):
        return {'PATH': os.environ['PATH'], 'HOME': self.home, 'LANG': 'C.UTF-8', 'LC_ALL': 'C.UTF-8',
                'TZ': 'UTC', 'COLUMNS': '80', 'LINES': '24', 'NO_COLOR': '1', 'TERM': 'dumb'}

    def norm(self, text):
        return text.replace(self.root, '$TMP')

    def denorm(self, text):
        return text.replace('$TMP', self.root)

    def library_id(self, directory):
        with open(os.path.join(self.work, directory, '.papers', 'config.json')) as f:
            return json.load(f)['id']

    def stores(self):
        "the shared store, and the stores of their own"
        roots = [self.store]
        separate = os.path.join(self.store, 'separate')
        if os.path.isdir(separate):
            roots += [os.path.join(separate, name) for name in sorted(os.listdir(separate))]
        return roots

    def states(self, directory):
        "[(time, identifier)] of the states of the library in that folder, by time (its project, in any store)"
        id = self.library_id(directory)
        found = []
        for root in self.stores():
            folder = os.path.join(root, 'projects', id, 'states')
            if os.path.isdir(folder):
                for name in os.listdir(folder):
                    if re.fullmatch(r'[0-9a-f]{64}\.json', name):
                        with open(os.path.join(folder, name)) as f:
                            found.append((json.load(f)['time'], name[:-5]))
        return sorted(found)

    def step_argument(self, arg, directory):
        "$STEP:k and its variants: the identifier of the k-th state (see the top); k beyond the history stays k"
        m = re.fullmatch(r'\$(STEP|STEP4|STEPU):(\d+)', arg)
        if not m:
            return arg
        states = self.states(directory) if directory else []
        k = int(m.group(2))
        if not 1 <= k <= len(states):
            return str(k)
        ident = states[k - 1][1]
        if m.group(1) == 'STEP4':
            others = [i for _, i in states if i != ident]
            n = 4
            while any(o.startswith(ident[:n]) for o in others):
                n += 1
            return ident[:n]
        return ident[:8].upper() if m.group(1) == 'STEPU' else ident[:8]

    def run(self, argv, cwd, stdin, lib=None):
        argv = [self.denorm(a) for a in argv]
        argv = [self.library_id(a[4:]) if a.startswith('$ID:') else self.step_argument(a, lib) for a in argv]
        proc = subprocess.run(self.papers + argv, cwd=os.path.join(self.work, cwd), env=self.env(),
                              input=stdin, capture_output=True, text=True)
        return proc

    def numbered(self):
        """the states of every store as papers 3.0.0b5 and earlier wrote them: 000001.json, ... by time, with
        numbers in parent, reverts, the steps of the pools (the link has no base) and the heads"""
        projects = []
        for root in self.stores():
            folder = os.path.join(root, 'projects')
            if os.path.isdir(folder):
                projects += [(root, name) for name in sorted(os.listdir(folder))]
        numbers = {}
        for root, id in projects:
            states = os.path.join(root, 'projects', id, 'states')
            loaded = {}
            for name in os.listdir(states):
                with open(os.path.join(states, name)) as f:
                    loaded[name[:-5]] = json.load(f)
            order = sorted(loaded, key=lambda n: (loaded[n]['time'], n))
            numbers[id] = {n: i + 1 for i, n in enumerate(order)}
        for root, id in projects:
            states = os.path.join(root, 'projects', id, 'states')
            ids = numbers[id]
            for name in os.listdir(states):
                with open(os.path.join(states, name)) as f:
                    state = json.load(f)
                state['parent'] = ids.get(state.get('parent'))
                if state.get('reverts'):
                    state['reverts'] = [ids[r] for r in state['reverts']]
                if state.get('pools'):
                    state['pools'] = {p: {'state': numbers[p][link['state']], 'config': link['config']}
                                      for p, link in state['pools'].items()}
                os.remove(os.path.join(states, name))
                with open(os.path.join(states, '%06d.json' % ids[name[:-5]]), 'w') as f:
                    json.dump(state, f, indent=1)
            heads_file = os.path.join(root, 'projects', id, 'heads.json')
            with open(heads_file) as f:
                heads = json.load(f)
            for head in heads.values():
                head['state'] = ids[head['state']]
                head['redo'] = [ids[r] for r in head.get('redo') or []]
            with open(heads_file, 'w') as f:
                json.dump(heads, f, indent=1)

    def tree(self, root, skip=lambda rel: False):
        "{relative path: bytes} of the files under root"
        found = {}
        for base, dirs, files in os.walk(root):
            for name in files:
                path = os.path.join(base, name)
                rel = os.path.relpath(path, root)
                if not skip(rel) and not os.path.islink(path):
                    with open(path, 'rb') as f:
                        found[rel] = f.read()
        return found

    def work_tree(self):
        return self.tree(self.work, lambda rel: rel.endswith(os.path.join('.papers', 'lock')))

    def store_tree(self):
        return self.tree(self.store, lambda rel: rel.endswith('.tmp')) if os.path.isdir(self.store) else {}

    def age(self, seconds):
        old = time.time() - seconds
        for sub in ('objects', 'file-objects'):
            for base, _, files in os.walk(os.path.join(self.store, sub)):
                for name in files:
                    os.utime(os.path.join(base, name), (old, old))
        separate = os.path.join(self.store, 'separate')
        if os.path.isdir(separate):
            raise SystemExit('age: not for separate stores')


def encode(rel, data, sandbox, is_store):
    "a file as the fixture holds it: text (with $TMP) when it is text that papers wrote, else base64"
    text_like = not is_store or rel.endswith('.json')
    if text_like:
        try:
            return {'text': sandbox.norm(data.decode('utf-8'))}
        except UnicodeDecodeError:
            pass
    return {'b64': base64.b64encode(data).decode()}


def delta(before, after, sandbox, is_store):
    out = {'set': {}, 'del': sorted(set(before) - set(after))}
    for rel, data in sorted(after.items()):
        if before.get(rel) != data:
            out['set'][rel] = encode(rel, data, sandbox, is_store)
    return out


def run_scenario(name, scenario, papers):
    sb = Sandbox(papers, scenario.get('machine', {}))
    steps = []
    work, store = {}, {}
    for step in scenario['steps']:
        kind = step[0]
        record = {}
        if kind == 'papers':
            _, argv, kw = step
            lib = kw.get('lib', 'lib')
            cwd = kw.get('cwd', lib or '.')
            proc = sb.run(argv, cwd, kw.get('stdin'), lib)
            record = {'kind': 'papers', 'argv': argv, 'cwd': cwd, 'lib': lib, 'exit': proc.returncode,
                      'stdout': sb.norm(proc.stdout), 'stderr': sb.norm(proc.stderr)}
            if kw.get('stdin') is not None:
                record['stdin'] = kw['stdin']
            if kw.get('bibtex'):
                record['bibtex'] = kw['bibtex']
        elif kind == 'write':
            os.makedirs(os.path.dirname(os.path.join(sb.work, step[1])) or sb.work, exist_ok=True)
            with open(os.path.join(sb.work, step[1]), 'w') as f:
                f.write(step[2])
            record = {'kind': 'write', 'path': step[1]}
        elif kind == 'writeb':
            os.makedirs(os.path.dirname(os.path.join(sb.work, step[1])), exist_ok=True)
            with open(os.path.join(sb.work, step[1]), 'wb') as f:
                f.write(step[2])
            record = {'kind': 'write', 'path': step[1]}
        elif kind == 'rm':
            if os.path.exists(os.path.join(sb.work, step[1])):
                os.remove(os.path.join(sb.work, step[1]))
            record = {'kind': 'rm', 'path': step[1]}
        elif kind == 'cp':
            shutil.copytree(os.path.join(sb.work, step[1]), os.path.join(sb.work, step[2]), symlinks=True)
            record = {'kind': 'cp', 'src': step[1], 'dst': step[2]}
        elif kind == 'mv':
            shutil.move(os.path.join(sb.work, step[1]), os.path.join(sb.work, step[2]))
            record = {'kind': 'mv', 'src': step[1], 'dst': step[2]}
        elif kind == 'age':
            sb.age(step[1])
            record = {'kind': 'age', 'seconds': step[1]}
        elif kind == 'numbered':
            sb.numbered()
            record = {'kind': 'numbered'}
        else:
            raise SystemExit(f'unknown step {step}')
        new_work, new_store = sb.work_tree(), sb.store_tree()
        record['work'] = delta(work, new_work, sb, False)
        record['store'] = delta(store, new_store, sb, True)
        work, store = new_work, new_store
        steps.append(record)
        if kind == 'papers' and record['exit'] not in (0, 1):
            raise SystemExit(f'{name}: papers {step[1]} exited {record["exit"]}:\n{record["stderr"]}')
    # the identifier of a bibtex without library is the sha256 of its place (the sandbox is random)
    place_ids = {}
    for step in scenario['steps']:
        bib = step[0] == 'papers' and step[2].get('bibtex')
        if bib:
            place = os.path.realpath(os.path.join(sb.work, bib))
            place_ids[bib] = hashlib.sha256(place.encode()).hexdigest()[:32]
            assert any('/projects/%s/' % place_ids[bib] in rel or rel.startswith('projects/%s/' % place_ids[bib])
                       for rel in sb.store_tree()), 'the identifier of a bibtex without library'
    shutil.rmtree(sb.root, ignore_errors=True)
    return {'name': name, 'machine': scenario.get('machine', {}), 'place_ids': place_ids, 'sandbox': sb.root,
            'steps': steps}


def retention_cases(papers):
    """the retention of papers.history.Store.prune on a store built with its own functions: states of
    three projects with times, heads, redo lists, pieces and file objects, some of them old; pruned
    with several limits (bytes), also dry. The python of the reference: `papers` is importable there."""
    python = os.path.join(os.path.dirname(papers[0]), 'python3')
    code = r"""
import json, os, random, shutil, sys, tempfile, time
from papers import history as H
root = os.path.realpath(tempfile.mkdtemp(prefix='papers-rs-retention-'))
st = H.Store(os.path.join(root, 'store'))
NOW = 2000000000.0
rnd = random.Random(7)
def text(n):
    return ''.join(rnd.choice('abcdefghijklmnopqrstuvwxyz') for _ in range(n)) + chr(10)
def fileobj(name, n):
    path = os.path.join(root, name)
    data = bytes(rnd.randrange(256) for _ in range(n))
    with open(path, 'wb') as f:
        f.write(data)
    digest = H.sha256(data)
    st.put_file(path, digest)
    return digest
files = [fileobj('a.pdf', 300), fileobj('b.pdf', 500)]
t = 1000.0
for pid, count, head, redo in (('aaa', 6, 4, [5, 6]), ('bbb', 4, 4, []), ('ccc', 3, 1, [2, 3])):
    project = st.project(pid)
    parent = None
    made = []
    for n in range(1, count + 1):
        body = '@a{k%d,\n}\n' % n + text(150 + 20 * n)
        state = dict(parent=parent, time=t, kind='command', message='papers x %s %d' % (pid, n), ops=[],
                     deleted=[files[0]] if (pid, n) in (('aaa', 2), ('bbb', 3)) else ([files[1]] if (pid, n) == ('ccc', 1) else []),
                     bibtex={'pieces': st.put_text(body), 'sha': H.sha256(body.encode())},
                     files={'.papers/files.json': st.put_text(text(60))})
        got = project.add(state)
        made.append(got)
        parent = got
        t += 10 + n
    project.set_head('/place/' + pid, made[head - 1], [made[r - 1] for r in redo])
# unreferenced objects: old, and young
for i in range(3):
    st.put_text(text(200))
# the times of the objects: some old (a day), some young
objs = sorted(st._sizes().items(), key=lambda kv: kv[1][0])
for i, (ref, (path, size, mtime)) in enumerate(objs):
    when = NOW - (86400 if i % 3 else 60)
    os.utime(path, (when, when))
def snapshot(base):
    out = {}
    for b, d, fs in os.walk(base):
        for f in fs:
            p = os.path.join(b, f)
            out[os.path.relpath(p, base)] = (open(p, 'rb').read(), os.stat(p).st_mtime)
    return out
before = snapshot(st.root)
cases = []
total = st.size()
for limit, extra, dry in ((10**9, 0, False), (total, 0, False), (int(total * 0.85), 0, True), (int(total * 0.85), 0, False), (int(total * 0.7), 0, False), (int(total * 0.7), 300, True), (int(total * 0.5), 0, False), (int(total * 0.3), 0, False), (1, 0, False)):
    work = os.path.join(root, 'copy')
    shutil.rmtree(work, ignore_errors=True)
    shutil.copytree(st.root, work)
    for rel, (_, mtime) in before.items():
        os.utime(os.path.join(work, rel), (mtime, mtime))
    s2 = H.Store(work)
    gone = []
    dropped = s2.prune(limit, extra=extra, dry=dry, discard=lambda p: (gone.append(os.path.relpath(p, work)), os.remove(p)), now=NOW)
    after = snapshot(work)
    cases.append({'limit': limit, 'extra': extra, 'dry': dry, 'now': NOW,
                  'dropped': [[pid, n] for pid, n, _ in dropped], 'discarded': sorted(gone),
                  'after': sorted(after)})
import base64
print(json.dumps({'store': {k: {'b64': base64.b64encode(v[0]).decode(), 'mtime': v[1]} for k, v in before.items()},
                  'cases': cases}))
shutil.rmtree(root)
"""
    proc = subprocess.run([python, '-c', code], capture_output=True, text=True)
    if proc.returncode:
        raise SystemExit(proc.stderr)
    return json.loads(proc.stdout)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--papers', default=os.path.join(os.path.dirname(sys.executable), 'papers'),
                    help='the reference command (default: the papers of this python)')
    ap.add_argument('scenarios', nargs='*')
    o = ap.parse_args()
    papers = o.papers.split()
    os.makedirs(OUT, exist_ok=True)
    if not o.scenarios or 'retention' in o.scenarios:
        with open(os.path.join(OUT, 'retention.json'), 'w') as f:
            json.dump(retention_cases(papers), f, indent=1, sort_keys=True)
            f.write('\n')
        print('retention')
    for name, scenario in SCENARIOS.items():
        if o.scenarios and name not in o.scenarios:
            continue
        result = run_scenario(name, scenario, papers)
        with open(os.path.join(OUT, name + '.json'), 'w') as f:
            json.dump(result, f, indent=1, sort_keys=True)
            f.write('\n')
        bad = [s for s in result['steps'] if s['kind'] == 'papers' and s['exit'] != 0]
        print(f'{name}: {len(result["steps"])} steps' + (f', {len(bad)} with an error exit' if bad else ''))


if __name__ == '__main__':
    main()
