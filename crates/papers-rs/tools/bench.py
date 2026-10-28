"""Time papers (Python) and papers-rs on the same library: the median of 5 runs
of each command, each program with its own temporary HOME (the user's
configuration, history and caches are never touched).

    python3 tools/bench.py LIBRARY.bib --papers papers --papers-rs target/release/papers-rs

The library is copied into a temporary folder and initialized with papers
(`papers init`); the original is only read. The writing commands add and
remove a made-up entry, and add and remove a tag on the first entry.
"""

import argparse
import os
import re
import shutil
import statistics
import subprocess
import tempfile
import time

NEW = """@article{benchnew2026,
 author = {Doe, Jane},
 title = {A new paper for the benchmark},
 journal = {J},
 year = {2026},
 doi = {10.9999/bench.2026}
}
"""
RUNS = 5


def first_key(bib):
    "the key of the first entry (not a @comment, @string or @preamble)"
    for line in open(bib, encoding='utf-8'):
        m = re.match(r'@(?!comment|string|preamble)\w+\s*\{([^,\s]+),', line, re.I)
        if m:
            return m.group(1)
    raise SystemExit(f'{bib}: no entry')


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument('bib')
    p.add_argument('--papers', default='papers')
    p.add_argument('--papers-rs', default='target/release/papers-rs')
    o = p.parse_args()
    progs = {'papers': o.papers, 'papers-rs': os.path.abspath(o.papers_rs)}

    work = tempfile.mkdtemp(prefix='papers-bench-')
    lib, new = os.path.join(work, 'lib'), os.path.join(work, 'new.bib')
    os.makedirs(lib)
    shutil.copy(o.bib, os.path.join(lib, 'papers.bib'))
    open(new, 'w').write(NEW)
    home = os.path.join(work, 'home-init')
    os.makedirs(home)
    subprocess.run([o.papers, 'init'], cwd=lib, env=dict(os.environ, HOME=home),
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    original = open(os.path.join(lib, 'papers.bib'), encoding='utf-8').read()
    key = first_key(os.path.join(lib, 'papers.bib'))

    reads = [
        ('list', ['list']),
        ('list --format bibtex', ['list', '--format', 'bibtex']),
        ('list --format csl-json', ['list', '--format', 'csl-json']),
        ('list WORDS', ['list', 'ice', 'sheet']),
        ('list --duplicates', ['list', '--duplicates']),
    ]
    # pairs that undo each other, run alternately
    writes = [
        [('tag --add (write)', ['tag', key, '--add', 'benchtag']),
         ('tag --rm (write)', ['tag', key, '--rm', 'benchtag'])],
        [('add bibtex (write)', ['add', new]),
         ('rm (write)', ['rm', 'benchnew2026', '-y'])],
    ]

    def run(prog, home, argv):
        env = dict(os.environ, HOME=home)
        env.pop('XDG_CONFIG_HOME', None)
        t = time.perf_counter()
        r = subprocess.run([prog, *argv], cwd=lib, env=env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        dt = time.perf_counter() - t
        if r.returncode:
            raise SystemExit(f'{prog} {" ".join(argv)}: exit {r.returncode}\n{r.stderr.decode(errors="replace")}')
        return dt

    times = {}
    cold = {}
    for name, prog in progs.items():
        open(os.path.join(lib, 'papers.bib'), 'w', encoding='utf-8').write(original)
        home = os.path.join(work, f'home-{name}')
        os.makedirs(home)
        cold[name] = run(prog, home, ['list'])  # an empty HOME: no parse cache
        for label, argv in reads:
            times.setdefault(label, {})[name] = statistics.median(run(prog, home, argv) for _ in range(RUNS))
        for pair in writes:
            ts = {label: [] for label, _ in pair}
            for _ in range(RUNS):
                for label, argv in pair:
                    ts[label].append(run(prog, home, argv))
            for label, _ in pair:
                times.setdefault(label, {})[name] = statistics.median(ts[label])

    n = original.count('\n@') + original.startswith('@')
    print(f'{o.bib}: {len(original.encode()) / 1e6:.1f} MB, about {n} blocks; median of {RUNS} runs, seconds\n')
    print(f"| command | papers | papers-rs | × |\n|---|---|---|---|")
    print(f"| `list`, first run (empty HOME) | {cold['papers']:.3f} | {cold['papers-rs']:.3f} | {cold['papers'] / cold['papers-rs']:.1f} |")
    for label in times:
        a, b = times[label]['papers'], times[label]['papers-rs']
        print(f'| `{label}` | {a:.3f} | {b:.3f} | {a / b:.1f} |')
    shutil.rmtree(work)


if __name__ == '__main__':
    main()
