#!/usr/bin/env python3
"""Generate tests/expected/merge3.json from the reference, papers-cli (Python).

The three-way merge (`papers.merge.Merge` with a base: pull, push, history --revert), the
two-way merge with the rules of an exchange, and the dialogue that decides it: random sets of
base, target and incoming entries (a field unchanged, changed to an equivalent spelling or to
another value, removed, added; the same change on both sides; blank values; the type, the keys,
the former keys and the links), and for each what papers makes of it:

  model    the fields (status, values, value, alternatives, where they changed, removal), the
           links and former keys, the result, the result with each side chosen, with the
           additions declined, the text of the editor
  context  one dialogue context per case (pull, push, revert; add, check without a base): the
           screen (with and without colours), the menu, the help, the questions, the parsing
           of answers, the descriptions and decisions of the resolver for each mode, and a
           scripted dialogue (the answers, what is printed, what it decides)

tests/merge3.rs replays the model, the dialogue of papers-rs-cli (src/dialogue.rs) the rest.

Run with the python that has papers installed (the reference), from anywhere:

    /path/to/python3 tests/gen_merge3.py [--count N] [--seed S] [--out FILE]

The default writes tests/expected/merge3.json (what the tests read); a larger run, for a
fuzz of the same tests: `--count 20000 --seed 5 --out /tmp/merge3_big.json`, then
`PAPERS_RS_MERGE3_FUZZ=/tmp/merge3_big.json cargo test -p papers-rs --test merge3 fuzz_file` (and
the same variable for `cargo test -p papers-rs-cli dialogue`). It never touches the
configuration or the libraries of the user: HOME is a temporary folder.
"""
import argparse
import json
import logging
import os
import random
import tempfile
import warnings

HERE = os.path.dirname(os.path.abspath(__file__))
TMP = tempfile.mkdtemp(prefix='papers-rs-merge3-')
os.environ['HOME'] = TMP  # before papers is imported
os.environ['XDG_CACHE_HOME'] = os.path.join(TMP, 'cache')
os.environ['XDG_CONFIG_HOME'] = os.path.join(TMP, 'config')
os.chdir('/')  # the paths of the screens are relative to the current folder
warnings.simplefilter('ignore')

from papers.duplicate import Verdict, SAME  # noqa: E402
from papers.entries import entry_from_dict, format_library, library_from_entries  # noqa: E402
from papers.merge import Dialogue, DuplicateError, Merge, Resolver, ask  # noqa: E402
from papers.utils import use_color  # noqa: E402

logging.disable(logging.CRITICAL)

TITLES = ['Climate sensitivity revisited', 'climate sensitivity revisited', '{Climate} sensitivity revisited',
          'Climate  sensitivity revisited', 'Sea level rise', 'A very long title ' + 'of many words ' * 8,
          'Ice sheets and {\\"u}ber-models']
AUTHORS = ['Smith, John and Doe, Jane', 'Smith, J. and Doe, J.', 'Doe, Jane and Smith, John', 'Smith, John',
           'Smith, John D.', 'J. Smith', 'Müller, Hans', 'M{\\"u}ller, H.', 'Smith, Jane']
JOURNALS = ['Nature', 'nature', 'Nat.', 'Science', 'Nature Climate Change', '{Nature}']
YEARS = ['2020', '2021', '2020 ', '1999']
VOLUMES = ['1', '10', '10 ', '7']
PAGES = ['1--2', '1-2', '100--120', '5']
DOIS = ['10.1000/abc123', '10.1000/ABC123', 'https://doi.org/10.1000/abc123', 'doi:10.1000/abc123', '10.1000/other']
KEYWORDS = ['climate', 'climate, mine', 'a, b', 'b, a', 'x/y']
NOTES = ['n', 'a note', 'a  note', 'A NOTE']
ABSTRACTS = ['An abstract that is rather long ' * 4, 'short']
EXTRA = ['x', 'y', '']
VALUES = {'title': TITLES, 'author': AUTHORS, 'journal': JOURNALS, 'year': YEARS, 'volume': VOLUMES, 'pages': PAGES,
          'doi': DOIS, 'keywords': KEYWORDS, 'note': NOTES, 'abstract': ABSTRACTS, 'editor': AUTHORS,
          'publisher': EXTRA, 'url': EXTRA}
TYPES = ['article', 'book', 'misc', 'inproceedings']
FILES = [':/lib/files/a.pdf:pdf', ':/lib/files/a.pdf:pdf;:/lib/other/b.pdf:pdf', ':/lib/other/b.pdf:pdf', 'files/a.pdf',
         ':/lib/./files/../files/a.pdf:pdf', ':/lib/files/c.pdf:']
IDS = ['old', 'old, other', 'a', 'B', 'smith2020, old']
KEYS = ['smith2020climate', 'Smith_2020', 'smith2020', 'doe2019']


def entry(key, type_, fields):
    return entry_from_dict(dict(fields, ID=key, ENTRYTYPE=type_))


def dump(e):
    return {'type': e.entry_type, 'key': e.key, 'fields': [[f.key, f.value] for f in e.fields]}


def formatted(e):
    return format_library(library_from_entries([e]))


def pick_value(rng, name, base):
    "a value of the field, near the base value more often than not"
    pool = VALUES[name]
    if base is not None and rng.random() < 0.4:
        # an equivalent spelling, or a small change
        return rng.choice([base.upper(), base.lower(), '{' + base + '}', base + ' ', ' ' + base, base.replace(' ', '  ')])
    return rng.choice(pool)


def make_case(rng):
    names = rng.sample(sorted(VALUES), rng.randint(2, 9))
    base = {n: rng.choice(VALUES[n]) for n in names if rng.random() < 0.85}
    sides = []
    shared = {}  # the same change on both sides, now and then
    for _ in range(2):
        fields = {}
        for n in names:
            b = base.get(n)
            r = rng.random()
            if r < 0.45:
                v = b
            elif r < 0.55 and b is not None:
                v = pick_value(rng, n, b)
            elif r < 0.75:
                v = shared.setdefault(n, pick_value(rng, n, b)) if rng.random() < 0.4 else pick_value(rng, n, b)
            elif r < 0.92:
                v = None
            else:
                v = rng.choice(['', '  ', '{}'])
            if v is not None:
                fields[n] = v
        sides.append(fields)
    kind = rng.random()
    # the type, the keys, the former keys and the links
    types = [rng.choice(TYPES)] * 3
    if rng.random() < 0.25:
        types[rng.randrange(3)] = rng.choice(TYPES)
    keys = [rng.choice(KEYS) for _ in range(3)]
    entries_fields = []
    for i, f in enumerate([sides[0], sides[1], base]):
        f = dict(f)
        if rng.random() < 0.25:
            f['ids'] = rng.choice(IDS)
        if rng.random() < 0.25:
            f['file'] = rng.choice(FILES)
        entries_fields.append(list(f.items()))
    # the order of the fields is the order of the entry, not that of the sampling
    for fs in entries_fields:
        rng.shuffle(fs)
    target = entry(keys[0], types[0], entries_fields[0])
    other = entry(keys[1], types[1], entries_fields[1])
    base_entry = entry(keys[2], types[2], entries_fields[2])
    mode = 'three' if kind < 0.7 else ('exchange' if kind < 0.85 else 'two')
    return target, other, base_entry, mode


def model(m):
    out = {
        'fields': [[f.name, f.status, f.values, f.value, f.alternatives(), f.changed, bool(f.removal), f.base]
                   for f in m.fields],
        'files': m.files, 'new_files': m.new_files, 'ids': m.ids, 'ids_added': m.ids_added,
        'exact': bool(m.is_exact()), 'three_way': m.three_way, 'exchange': bool(m.exchange),
        'conflicts': [f.name for f in m.conflicts], 'additions': [f.name for f in m.additions],
        'result': formatted(m.result()), 'edit_text': m.edit_text(),
    }
    return out


def variants(make):
    "the result of a merge with each side chosen, with the additions declined, with the fields chosen one by one"
    out = {}
    for choice in (0, 1):
        m = make()
        m.choose_all(choice)
        out[f'choose{choice}'] = formatted(m.result())
    m = make()
    for f in m.additions:
        f.decline()
    out['declined'] = formatted(m.result())
    m = make()
    for i, f in enumerate(m.fields):
        if f.status == '!':
            f.choose(i % 2)
        elif f.status == '+' and i % 3 == 0:
            f.decline()
    out['mixed'] = formatted(m.result())
    m = make()
    out['target_key'] = m.key
    if m.three_way is False and len(m.entries) > 1:
        out['with_target1'] = formatted(make().with_target(1).result())
    return out


ANSWERS = ['1', '2', '3', 'f', 'e', 'n', 's', 'q', '?', 't2', 't 1', 't', 'x', '', ' 1 ', '0', 'F', '12']
FIELD_ANSWERS = ['1', '2', '3', 'e', 'x', '0', ' 2', '']


def script(rng):
    "the answers of the menu and of the questions of a field (not `e`: no editor in the tests)"
    answers = [rng.choice(ANSWERS[:12] + ['1', '2', 'f', 'f', 's']) if rng.random() < 0.7 else rng.choice(ANSWERS)
               for _ in range(rng.randint(1, 6))]
    fields = [rng.choice(FIELD_ANSWERS) for _ in range(8)]
    return [a for a in answers if a != 'e'], [a for a in fields if a != 'e']


def run_script(make, context, reason, answers, field_answers):
    "the dialogue with scripted answers (never the editor): what is printed, what is decided"
    m = make()
    answers, fields = list(answers), list(field_answers)
    printed = []

    def input_(prompt):
        # an exhausted script quits the menu, and answers the questions of a field with 1
        main = prompt == '>>> '
        queue = answers if main else fields
        return queue.pop(0) if queue else ('q' if main else '1')

    decision = ask(m, context, reason, input=input_, output=printed.append, color=False)
    return {'printed': printed, 'kind': decision.kind,
            'result': formatted(decision.entry) if decision.entry is not None else None,
            'key': decision.merge.key if decision.merge is not None else None}


def context_case(rng, make, context, reason):
    m = make()
    d = Dialogue(m, context, reason, color=False)
    out = {'context': context, 'reason': reason, 'screen': d.screen(), 'menu': d.menu(), 'summary': d.summary(),
           'help': d.help(), 'header': d.header(),
           'screen_color': None}
    if rng.random() < 0.3:
        use_color(True)  # the colours of the screen are those of bcolors
        try:
            out['screen_color'] = Dialogue(make(), context, reason, color=True).screen()
        finally:
            use_color(False)
    out['questions'] = [[f.name, d.field_question(f)] for f in d.field_questions()]
    out['parse'] = [[a, d.parse(a).kind, d.parse(a).index] for a in ANSWERS]
    out['parse_field'] = [[f.name, [[a, d.parse_field(f, a).kind, d.parse_field(f, a).index] for a in FIELD_ANSWERS]]
                          for f in d.field_questions()]
    verdict = Verdict(SAME, reason)
    resolver = {}
    for mode in ('keep', 'update', 'skip', 'error', 'ask'):
        r = Resolver(mode)
        if mode == 'ask':
            resolver[mode] = None
            continue
        try:
            decision = r.decide(make(), context, verdict)
            resolver[mode] = {'kind': decision.kind,
                              'result': formatted(decision.entry) if decision.entry is not None else None}
        except DuplicateError as error:
            resolver[mode] = {'error': str(error)}
    out['resolver'] = resolver
    out['describe'] = Resolver.describe(make(), context, verdict)
    answers, fields = script(rng)
    out['script'] = {'answers': answers, 'field_answers': fields,
                     **run_script(make, context, reason, answers, fields)}
    return out


def generate(count, seed):
    rng = random.Random(seed)
    cases = []
    for _ in range(count):
        target, other, base, mode = make_case(rng)
        rel = rng.choice(['/lib', '/lib/sub', None])
        keys_to_ids = False
        if mode == 'three':
            def make(target=target, other=other, base=base, rel=rel):
                return Merge([target, other], relative_to=rel, base=base)
            contexts = ['pull', 'push', 'revert']
        elif mode == 'exchange':
            def make(target=target, other=other, rel=rel):
                return Merge([target, other], relative_to=rel, exchange=True)
            contexts = ['pull', 'push']
        else:
            keys_to_ids = rng.random() < 0.5

            def make(target=target, other=other, rel=rel, keys_to_ids=keys_to_ids):
                return Merge([target, other], relative_to=rel, keys_to_ids=keys_to_ids)
            contexts = ['add', 'check']
        case = {'mode': mode, 'target': dump(target), 'other': dump(other),
                'base': dump(base) if mode == 'three' else None, 'relative_to': rel, 'keys_to_ids': keys_to_ids}
        case['model'] = model(make())
        case['variants'] = variants(make)
        context = rng.choice(contexts)
        reason = rng.choice(['pool default', 'step 3', '', 'same DOI 10.1000/abc123', '../pool'])
        case['dialogue'] = context_case(rng, make, context, reason)
        cases.append(case)
    return cases


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--count', type=int, default=300)
    parser.add_argument('--seed', type=int, default=3)
    parser.add_argument('--out')
    o = parser.parse_args()
    out = o.out or os.path.join(HERE, 'expected', 'merge3.json')
    cases = generate(o.count, o.seed)
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    with open(out, 'w', encoding='utf-8') as f:
        f.write('[\n' + ',\n'.join(json.dumps(c, ensure_ascii=False, separators=(',', ':')) for c in cases) + '\n]\n')
    print(f'{len(cases)} cases -> {out}')


if __name__ == '__main__':
    main()
