#!/usr/bin/env python3
"""Generate tests/expected/unicode.json from the reference, papers-cli (Python):
the LaTeX of the values as unicode, by pylatexenc, as papers does it.

  libraries   whole bibtex texts (the fixtures, hand-written ones, random entries, the texts of
              expected/fuzz.json) through papers.encoding.latex_to_unicode_library: the blocks
              after, the unreadable ones, the text of a save
  entries     papers.encoding.convert_entry_to_unicode on the first entry of a text: its fields
              after, or the error
  values      papers.formats._latex_to_unicode on single values: ONE sequence, in this order (papers
              has one converter for the process, whose state \\title, \\author, \\date stays)
  fields      papers.formats._unicode_fields of entries (the same converter, after `values`)

Run after gen_expected.py (it reads expected/fuzz.json), with the python that has papers installed:

    /path/to/python3 tests/gen_unicode.py

Like gen_expected.py, it never touches the configuration or the libraries of the user.
"""
import datetime
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_expected as G  # noqa: E402  (sets HOME to a temporary folder, silences the log)

from papers.encoding import convert_entry_to_unicode, latex_to_unicode_library  # noqa: E402
from papers.formats import _latex_to_unicode, _unicode_fields  # noqa: E402

ACCENTS = [r"{\'e}", r"\'e", r"\'{e}", r'{\"u}', r'\"{o}', r'\"a', r'{\c{c}}', r'\c c', r'{\~n}', r'\^{o}', r'\`a', r'\v{s}',
           r'\H{o}', r'\k{a}', r'\r{a}', r'\={a}', r'\.{z}', r'\u{g}', r'{\ss}', r'\ss{}', r'\o', r'{\O}', r'\l', r'{\L}',
           r'{\AA}', r'\AE{}', r'{\i}', r'\"{\i}', r'\^\i', r'\j', r'\oe', r'\aa', r'\textdegree', r'\textendash']
MATH = [r'$\alpha$', r'$x_{i}^2$', r'$$E=mc^2$$', r'\(a+b\)', r'\[x\]', r'$\mathrm{CO}_2$', r'{CO}$_2$', '$a$', '$',
        r'$\frac{a}{b}$', r'\begin{equation}x\end{equation}', r'$10^{-3}$ m', r'\ensuremath{\mu}', r'\textmu m']
MARKUP = [r'\url{http://a.b/c_d%20e}', r'\url{https://doi.org/10.1/x~y}', r'\href{http://x.org}{text}', r'\emph{em}',
          r'\textbf{b}', r'\textit{i}', r'\texttt{t}', r'\textsc{Sc}', r'{\bf bold}', r'{\it it}', r'{\em em}', r'\mbox{a b}',
          r'\footnote{n}', r'\cite{k}', r'\ref{x}', '\\\\', r'\newline', r'\par', r'\,', '\\ ', r'\-', r'\/', r'\@',
          r'\begin{itemize}\item a\item b\end{itemize}', r'\begin{verbatim}x_y\end{verbatim}', r'\verb|x|', r'\verb+a b+',
          r'\hspace{1em}', r'\vspace*{2pt}', r'\textbackslash', r'\textasciitilde', r'\LaTeX', r'\TeX', r'\ldots',
          r'\dots', r'\copyright', r'\S', r'\P', r'\pounds', r'\euro', r'\texteuro', r'\unknown{x}', r'\unknown', r'\x[a]{b}']
SPECIAL = ['--', '---', '~', r'\&', r'\%', r'\_', r'\#', r'\$', r'\{', r'\}', "``q''", "`s'", '"', '<', '>', '|', '&', '%',
           '% comment', '#', '_', '^', '@', '*', '\n', '\n\n', '  ', '\t', '\u00a0']
UNI = ['Müller', 'Ångström', 'Çelik', 'Dvořák', 'Łukasiewicz', 'Δ', '北京', 'Москва', '–', '—', '“q”', '…', 'é', 'é', 'ß', 'ø',
       '\U0001d7d8', '\u200b', '\u2009', '\u00ad']
BAD = ['{', '}', '\\', r'\verb', r'\verb|', r'\verb|x', r'\begin{x}', r'\end{x}', r'\begin{a}\end{b}', '$x', 'x$', r'\(', r'\)',
       r'\[', r'{\"', r'{\"}', r'\"', r"\'", r'\c', r'\textbf{', r'\textbf', r'\url{', r'\url', r'\href{a}', r'\item', r'\\\\\\',
       '{{{', '}}}', r'\begin{itemize}', r'\end{itemize}', r'\title', r'\author{', r'\input{x}', r'\include', r'\newcommand',
       r'\def', '\\\n', '\\ \n', '$$', '$$x', r'\mbox{']
WORDS = ['The', 'Ice', 'sheet', 'sea level', 'Smith', 'and', 'of', 'in', 'Journal', 'Geophys', 'Res', 'a', 'Nature', '2020',
         '10.1029/2019JF005', 'pp', '12--34', 'Vol', 'Cryosphere', 'The Cryosphere']

HAND = [
    '', ' ', 'plain', ' plain text ', '\n x \n', 'a  b', 'x\n\ny', '{}', '{{}}', '\\', '$', '%', '~', '&',
    '{World Health Organization}', r'M{\"u}ller, J{\"o}rg and {\AA}ngstr{\"o}m, Anders', r'{\"U}ber die Gr{\"o}{\ss}e',
    r"Caf{\'e}, na{\"\i}ve, r{\'e}sum{\'e}", r'{CO$_2$} and {N$_2$O}', r'The {A}tlantic {M}eridional {O}verturning',
    'http://example.org/a_b?c=d&e=f#g', r'\url{http://example.org/a_b?c=d&e=f#g}', '10.1002/(SICI)1097-0258',
    r'\textbf{a} \emph{b} {\bf c} {\it d}', 'a % b', r'a \% b', r'50\% of $x$ \& more', '--- a -- b ---', "`a' ``b''",
    r'\title{T}', r'\author{A}', r'\date{1 January 2000}', r'\maketitle', r'\title{T2}\author{A2}\maketitle',
    r'{\verb}', r'\verb', r'x \verb', r'x \verb|',
    '{' * 100 + 'x' + '}' * 100, '{' * 400 + 'x' + '}' * 400,
    r'\textbf{' * 100 + 'x' + '}' * 100, r'\textbf{' * 400 + 'x' + '}' * 400,
]

TEXTS = [
    # a value that cannot be decoded: the entry and the string become unreadable blocks
    '@string{s = {Foo \\verb}}\n@article{a,\n title = {x \\verb},\n year = 2020,\n month = jan,\n note = "q # r",\n}\n\n'
    '@article{b, title={{\\"u}}}\n',
    # the converter of a library has one state: \title of a value is seen by a later \maketitle
    '@string{s = {x \\title{T} \\author{A} \\date{D}}}\n@article{a, title = {\\title{T}\\author{A}\\date{D}}}\n'
    '@article{b, title = {\\maketitle}}\n',
    # string references, numbers, concatenations, enclosings
    '@string{jgr = {J. Geophys. Res.}}\n@string{n = 5}\n@article{a, journal = jgr, volume = n, month = jan, '
    'title = {{\\"U}ber {\\AA}} # { x}, note = "{\\\'e}"}\n',
    '@article{a, title = {M{\\"u}ller}, title = {x}}\n@article{b, title = {{\\"u}}}\n@article{b, title = {{\\"o}}}\n',
    '@comment{ {\\"u} }\n@preamble{ "{\\"u}" }\n% {\\"u}\n@article{a, title = {{\\"u}}}\n',
    '@article{a, TITLE = {{\\"u}}, Author = {M{\\"u}ller}}',
    '@article(a, title = {{\\"u}})',
    '@article{a,\n title = {\\url{http://x.org/a_b}},\n url = {http://x.org/a_b},\n}\n',
    '@article{a, title = {{\\"u}}, abstract = {Maths $\\alpha_{x}$ \\emph{t}\n\n  and {B}}}',
]


def value_corpus(extra, n=1200, seed=5):
    rng = random.Random(seed)
    values = HAND + ACCENTS + MATH + MARKUP + SPECIAL + UNI + BAD + [v for v in extra if v]
    atoms = ACCENTS + MATH + MARKUP + SPECIAL + UNI + BAD + WORDS * 6
    for _ in range(n):
        k = rng.randint(1, 7)
        values.append(''.join(rng.choice(atoms) + (' ' if rng.random() < 0.5 else '') for _ in range(k)))
    seen, out = set(), []
    for v in values:
        if v not in seen:
            seen.add(v)
            out.append(v)
    return out


def library_case(text):
    lib = latex_to_unicode_library(G.parse_string(text))
    out = G.dump(lib)
    out['in'] = text
    out['saved'] = G.saved_text(lib)
    return out


def entry_case(text):
    "convert_entry_to_unicode on the first entry of a text: its fields after, or the error"
    entry = G.parse_string(text).entries[0]
    out = {'in': text}
    try:
        convert_entry_to_unicode(entry)
    except Exception as error:
        out['error'] = type(error).__name__
    out['fields'] = [[f.key, f.value, f.enclosing == 'no-enclosing'] for f in entry.fields]
    return out


def entry_text(rng, values):
    fields = []
    for k in rng.sample(['title', 'author', 'journal', 'note', 'abstract', 'url', 'doi', 'keywords', 'booktitle', 'publisher'],
                        rng.randint(1, 5)):
        v = rng.choice(values)
        form = rng.random()
        if form < 0.8 or k in ('url', 'doi'):
            fields.append(f' {k} = {{{v}}}')
        elif form < 0.9:
            fields.append(f' {k} = "{v}"')
        else:
            fields.append(f' {k} = {rng.choice(["jan", "2020", "x # {y}", "{a} # {b}"])}')
    return '@article{k%d,\n%s\n}\n' % (rng.randrange(10**6), ',\n'.join(fields))


def main():
    rng = random.Random(11)
    extra = []
    for _, text in G.fixtures():
        for b in G.dump(G.parse_string(text))['blocks']:
            if b['kind'] == 'entry':
                extra += [v for _, v, _ in b['fields']]
    values = value_corpus(extra)
    with open(os.path.join(G.EXPECTED, 'fuzz.json')) as f:
        fuzz_texts = [c['text'] for c in json.load(f)]

    libraries = [dict(library_case(text), name=name) for name, text in G.fixtures()]
    libraries += [dict(library_case(t), name=f'text{i}') for i, t in enumerate(TEXTS)]
    for i in range(150):
        text = '\n'.join(entry_text(rng, values) for _ in range(rng.randint(1, 4)))
        libraries.append(dict(library_case(text), name=f'random{i}'))
    libraries += [dict(library_case(t), name=f'fuzz{i}') for i, t in enumerate(fuzz_texts[:150])]

    entries = []
    for text in TEXTS + [entry_text(rng, values) for _ in range(300)]:
        if G.parse_string(text).entries:
            entries.append(entry_case(text))

    # the shared converter of formats: one sequence, in this order
    seq = [{'in': v, 'out': _latex_to_unicode(v)}
           for v in [r'\date{1 January 2000}', r'\title{Kept}\author{Me}', r'\maketitle', r'\maketitle'] + values]
    fields = []
    for text in TEXTS + [entry_text(rng, values) for _ in range(200)]:
        for i, e in enumerate(G.parse_string(text).entries):
            fields.append({'in': text, 'index': i, 'out': [list(kv) for kv in _unicode_fields(e).items()]})

    out = {'libraries': libraries, 'entries': entries, 'values': seq, 'fields': fields}
    # the date of \maketitle without \date is the day of the run: tests/unicode.rs puts today's back
    today = '{dt:%B} {dt.day}, {dt.year}'.format(dt=datetime.datetime.now())
    out = json.loads(json.dumps(out).replace(today, '{TODAY}'))
    with open(os.path.join(G.EXPECTED, 'unicode.json'), 'w') as f:
        json.dump(out, f, ensure_ascii=False, indent=0)
        f.write('\n')
    errors = sum(1 for lib in libraries for b in lib['blocks'] if b.get('class') == 'MiddlewareErrorBlock')
    print(f'{len(libraries)} libraries ({errors} blocks with an error), {len(entries)} entries, '
          f'{len(seq)} values, {len(fields)} entries of fields', file=sys.stderr)


if __name__ == '__main__':
    main()
