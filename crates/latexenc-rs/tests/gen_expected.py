#!/usr/bin/env python3
"""Generate tests/expected/ from the reference, pylatexenc 2.11 (Python).

The Rust tests (tests/differential.rs) compare latexenc-rs with what is recorded here:

  expected/latex2text.json   corpus x named configurations of LatexNodes2Text
  expected/latexwalker.json  node trees (tolerant or not) and tokens of the corpus
  expected/latexencode.json  corpus x named configurations of UnicodeToLatexEncoder
  expected/codepoints.json   every code point through the 'defaults' and 'unicode-xml' encoders
  expected/fuzz.json         random texts through the main configurations

The configurations have the same names in tests/differential.rs. `bibtexparser` is the
configuration of bibtexparser's LatexDecodingMiddleware and LatexEncodingMiddleware (made by the
middlewares themselves), `papers` that of papers (papers.formats._latex_to_unicode).

Run with the python that has pylatexenc 2.11, bibtexparser and papers installed:

    /path/to/python3 tests/gen_expected.py

(`\\today` and `\\maketitle` without a date are left out of the corpus: they depend on the day.)
"""
import json
import logging
import os
import random
import sys
import tempfile
import unicodedata

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
os.environ['HOME'] = tempfile.mkdtemp(prefix='latexenc-rs-expected-')  # papers must not touch the real one

import pylatexenc  # noqa: E402
from pylatexenc import latex2text, latexencode, latexwalker, macrospec  # noqa: E402
from pylatexenc.latex2text import (EnvironmentTextSpec, LatexNodes2Text, MacroTextSpec,  # noqa: E402
                                   SpecialsTextSpec)
from pylatexenc.latexencode import (RULE_CALLABLE, RULE_DICT, RULE_REGEX, PartialLatexToLatexEncoder,  # noqa: E402
                                    UnicodeToLatexConversionRule, UnicodeToLatexEncoder, utf8tolatex)
from pylatexenc.latexwalker import LatexWalker  # noqa: E402

import corpus  # noqa: E402

assert pylatexenc.__version__ == '2.11', pylatexenc.__version__
logging.disable(logging.CRITICAL)
import warnings  # noqa: E402

warnings.simplefilter('ignore')

EXPECTED = os.path.join(HERE, 'expected')


def run(f):
    "the result of a call, or the exception"
    try:
        return {'out': f()}
    except Exception as e:  # noqa: BLE001
        return {'err': type(e).__name__, 'msg': str(e)}


# --------------------------------------------------------------------------- latex2text


def custom_text_db():
    "a context with custom macros, environments, specials and callables"
    db = latex2text.get_default_latex_context_db()
    db.add_context_category('custom', prepend=True, macros=[
        MacroTextSpec('url', simplify_repl='%s'),
        MacroTextSpec('mymacro', simplify_repl='<%s>'),
        MacroTextSpec('textbf', simplify_repl='BOLD(%s)'),
        MacroTextSpec('emph', simplify_repl='_%(1)s_'),
        MacroTextSpec('two', simplify_repl='%(1)s+%(2)s'),
        MacroTextSpec('three', simplify_repl='%(3)s'),
        MacroTextSpec('percent', simplify_repl='100%%'),
        MacroTextSpec('badfmt', simplify_repl='%d'),
        MacroTextSpec('dropit'),
        MacroTextSpec('keepargs', discard=False),
        MacroTextSpec('hspace', simplify_repl='[SPACE]'),
        MacroTextSpec('call', simplify_repl=lambda n, l2tobj: 'CALL(' + l2tobj.node_arg_to_text(n, 0) + ')'),
        MacroTextSpec('callname', simplify_repl=lambda n, macroname: 'NAME=' + macroname),
        MacroTextSpec('callnone', simplify_repl=lambda n: None),
        MacroTextSpec('ethz', simplify_repl='ETH Z\u00fcrich'),
    ], environments=[
        EnvironmentTextSpec('quote', simplify_repl='"%s"'),
        EnvironmentTextSpec('center', simplify_repl='[[%(body)s]]'),
        EnvironmentTextSpec('dropenv', discard=True),
        EnvironmentTextSpec('callenv', simplify_repl=lambda n, l2tobj: 'ENV(' + l2tobj.nodelist_to_text(n.nodelist) + ')'),
        EnvironmentTextSpec('badenv', simplify_repl='%(nothere)s'),
    ], specials=[
        SpecialsTextSpec('--', 'DASH'),
        SpecialsTextSpec('~', ' '),
        SpecialsTextSpec('<<', '\u00ab'),
        SpecialsTextSpec('>>', '\u00bb'),
        SpecialsTextSpec('&', 'AND'),
    ])
    return db


def walker_db():
    "a walker context to go with custom_text_db, for the walker tests"
    db = latexwalker.get_default_latex_context_db()
    db.add_context_category('custom', prepend=True, macros=[
        macrospec.MacroSpec('mymacro', '{'),
        macrospec.MacroSpec('two', '{{'),
        macrospec.MacroSpec('three', '{{{'),
        macrospec.MacroSpec('call', '[{'),
        macrospec.MacroSpec('keepargs', '*[{'),
        macrospec.MacroSpec('percent', ''),
        macrospec.MacroSpec('badfmt', '{'),
        macrospec.MacroSpec('dropit', '{'),
    ], environments=[
        macrospec.EnvironmentSpec('quote', ''),
        macrospec.EnvironmentSpec('callenv', '{'),
        macrospec.EnvironmentSpec('mymath', '', is_math_mode=True),
    ], specials=[
        macrospec.SpecialsSpec('--'),
        macrospec.SpecialsSpec('<<'),
        macrospec.SpecialsSpec('>>'),
    ])
    return db


def bibtexparser_decoder(keep_braced_groups=None, keep_math_mode=None):
    from bibtexparser.middlewares import LatexDecodingMiddleware
    return LatexDecodingMiddleware(keep_braced_groups=keep_braced_groups, keep_math_mode=keep_math_mode)._decoder


def papers_to_unicode():
    from papers.formats import _latex_to_unicode
    return _latex_to_unicode


NONE = object()


def l2t_configs():
    "name -> a function that makes (a function from the text to the output)"
    c = {}

    def conv(l2t, **flags):
        return lambda s: l2t.latex_to_text(s, **flags)

    c['default'] = lambda: conv(LatexNodes2Text())
    c['default_strict_parse'] = lambda: conv(LatexNodes2Text(), tolerant_parsing=False)
    c['default_strict_braces'] = lambda: conv(LatexNodes2Text(), tolerant_parsing=False, strict_braces=True)
    c['math_text'] = lambda: conv(LatexNodes2Text(math_mode='text'))
    c['math_with_delimiters'] = lambda: conv(LatexNodes2Text(math_mode='with-delimiters'))
    c['math_verbatim'] = lambda: conv(LatexNodes2Text(math_mode='verbatim'))
    c['math_remove'] = lambda: conv(LatexNodes2Text(math_mode='remove'))
    c['keep_comments'] = lambda: conv(LatexNodes2Text(keep_comments=True))
    c['keep_comments_strict'] = lambda: conv(LatexNodes2Text(keep_comments=True, strict_latex_spaces=True))
    c['strict_true'] = lambda: conv(LatexNodes2Text(strict_latex_spaces=True))
    c['strict_false'] = lambda: conv(LatexNodes2Text(strict_latex_spaces=False))
    c['strict_none'] = lambda: conv(LatexNodes2Text(strict_latex_spaces=None))
    c['strict_on'] = lambda: conv(LatexNodes2Text(strict_latex_spaces='on'))
    c['strict_off'] = lambda: conv(LatexNodes2Text(strict_latex_spaces='off'))
    c['strict_based_on_source'] = lambda: conv(LatexNodes2Text(strict_latex_spaces='based-on-source'))
    c['strict_macros'] = lambda: conv(LatexNodes2Text(strict_latex_spaces='macros'))
    c['strict_except_in_equations'] = lambda: conv(LatexNodes2Text(strict_latex_spaces='except-in-equations'))
    c['strict_default_preset'] = lambda: conv(LatexNodes2Text(strict_latex_spaces='default'))
    c['strict_invalid_preset'] = lambda: conv(LatexNodes2Text(strict_latex_spaces='nope'))
    c['strict_dict_a'] = lambda: conv(LatexNodes2Text(strict_latex_spaces={'between-macro-and-chars': True}))
    c['strict_dict_b'] = lambda: conv(LatexNodes2Text(strict_latex_spaces={'between-latex-constructs': True,
                                                                   'after-comment': True}))
    c['strict_dict_c'] = lambda: conv(LatexNodes2Text(strict_latex_spaces={'between-latex-constructs': True,
                                                                   'in-equations': True},
                                              math_mode='with-delimiters'))
    c['strict_dict_d'] = lambda: conv(LatexNodes2Text(strict_latex_spaces={'between-macro-and-chars': True,
                                                                   'in-equations': {'between-latex-constructs': True,
                                                                                    'after-comment': True}}))
    c['strict_dict_e'] = lambda: conv(LatexNodes2Text(strict_latex_spaces={'in-equations': 'macros'}))
    c['strict_true_math_with_delimiters'] = lambda: conv(LatexNodes2Text(strict_latex_spaces=True,
                                                                 math_mode='with-delimiters'))
    c['strict_macros_math_with_delimiters'] = lambda: conv(LatexNodes2Text(strict_latex_spaces='macros',
                                                                   math_mode='with-delimiters'))
    c['strict_based_math_verbatim'] = lambda: conv(LatexNodes2Text(strict_latex_spaces='based-on-source',
                                                           math_mode='verbatim'))
    c['braced_groups'] = lambda: conv(LatexNodes2Text(keep_braced_groups=True))
    c['braced_groups_min1'] = lambda: conv(LatexNodes2Text(keep_braced_groups=True, keep_braced_groups_minlen=1))
    c['braced_groups_min0'] = lambda: conv(LatexNodes2Text(keep_braced_groups=True, keep_braced_groups_minlen=0))
    c['braced_groups_min5'] = lambda: conv(LatexNodes2Text(keep_braced_groups=True, keep_braced_groups_minlen=5))
    c['fill_80'] = lambda: conv(LatexNodes2Text(fill_text=True))
    c['fill_40'] = lambda: conv(LatexNodes2Text(fill_text=40))
    c['fill_12'] = lambda: conv(LatexNodes2Text(fill_text=12))
    c['fill_5'] = lambda: conv(LatexNodes2Text(fill_text=5))
    c['fill_40_math_verbatim'] = lambda: conv(LatexNodes2Text(fill_text=40, math_mode='verbatim'))
    c['fill_40_math_with_delimiters'] = lambda: conv(LatexNodes2Text(fill_text=40, math_mode='with-delimiters'))
    c['fill_40_strict'] = lambda: conv(LatexNodes2Text(fill_text=40, strict_latex_spaces=True, keep_comments=True))
    c['fill_0'] = lambda: conv(LatexNodes2Text(fill_text=0))
    c['all_options'] = lambda: conv(LatexNodes2Text(keep_comments=True, keep_braced_groups=True, math_mode='with-delimiters',
                                            strict_latex_spaces='except-in-equations', fill_text=30))
    c['custom_context'] = lambda: conv(LatexNodes2Text(latex_context=custom_text_db()))
    c['custom_context_verbatim'] = lambda: conv(LatexNodes2Text(latex_context=custom_text_db(), math_mode='verbatim'))
    c['custom_context_walker'] = lambda: conv(LatexNodes2Text(latex_context=custom_text_db()), latex_context=walker_db())
    c['empty_context'] = lambda: conv(LatexNodes2Text(latex_context=macrospec.LatexContextDb()))
    c['bibtexparser'] = lambda: conv(bibtexparser_decoder())
    c['bibtexparser_text_math'] = lambda: conv(bibtexparser_decoder(keep_math_mode=False))
    c['bibtexparser_keep_braces'] = lambda: conv(bibtexparser_decoder(keep_braced_groups=True))
    c['bibtexparser_keep_braces_text_math'] = lambda: conv(bibtexparser_decoder(keep_braced_groups=True, keep_math_mode=False))
    c['papers'] = lambda: papers_to_unicode()
    return c


def l2t_cases(inputs, names=None):
    confs = l2t_configs()
    res = {}
    for name, make in confs.items():
        if names is not None and name not in names:
            continue
        res[name] = apply_config(make, inputs)
    return res


def apply_config(make, inputs):
    try:
        f = make()
    except Exception as e:  # noqa: BLE001
        return [{'err': type(e).__name__, 'msg': str(e)} for _ in inputs]
    return [run(lambda s=s: f(s)) for s in inputs]


# --------------------------------------------------------------------------- latexwalker


def dump_args(argd):
    if argd is None:
        return None
    d = {'argspec': argd.argspec, 'argnlist': [dump(x) for x in argd.argnlist]}
    if hasattr(argd, 'verbatim_text'):
        d['verbatim_text'] = argd.verbatim_text
        d['verbatim_delimiters'] = list(argd.verbatim_delimiters) if argd.verbatim_delimiters else None
    return d


def dump(n):
    if n is None:
        return None
    t = type(n).__name__
    d = {'t': t, 'pos': n.pos, 'len': n.len}
    if t == 'LatexCharsNode':
        d['chars'] = n.chars
    elif t == 'LatexGroupNode':
        d['nodelist'] = [dump(x) for x in n.nodelist]
        d['delimiters'] = list(n.delimiters)
    elif t == 'LatexCommentNode':
        d['comment'] = n.comment
        d['post'] = n.comment_post_space
    elif t == 'LatexMacroNode':
        d['macroname'] = n.macroname
        d['post'] = n.macro_post_space
        d['nodeargd'] = dump_args(n.nodeargd)
        d['nodeoptarg'] = dump(n.nodeoptarg)
        d['nodeargs'] = None if n.nodeargs is None else [dump(x) for x in n.nodeargs]
    elif t == 'LatexEnvironmentNode':
        d['environmentname'] = n.environmentname
        d['nodelist'] = [dump(x) for x in n.nodelist]
        d['nodeargd'] = dump_args(n.nodeargd)
        d['optargs'] = [dump(x) for x in n.optargs]
        d['args'] = [dump(x) for x in n.args]
    elif t == 'LatexSpecialsNode':
        d['specials_chars'] = n.specials_chars
        d['nodeargd'] = dump_args(n.nodeargd)
    elif t == 'LatexMathNode':
        d['displaytype'] = n.displaytype
        d['nodelist'] = [dump(x) for x in n.nodelist]
        d['delimiters'] = list(n.delimiters)
    else:
        raise TypeError(t)
    return d


def dump_error(e):
    d = {'err': type(e).__name__, 'msg': str(e)}
    if isinstance(e, latexwalker.LatexWalkerParseError):
        d.update(pos=e.pos, lineno=e.lineno, colno=e.colno, open_contexts=[list(c) for c in e.open_contexts])
    return d


def walk(s, **flags):
    try:
        nodes, pos, length = LatexWalker(s, **flags).get_latex_nodes()
        return {'nodes': [dump(n) for n in nodes], 'pos': pos, 'len': length,
                'latex': latexwalker.nodelist_to_latex(nodes)}
    except Exception as e:  # noqa: BLE001
        return dump_error(e)


def dump_token(t):
    return {'tok': t.tok, 'arg': t.arg if t.tok != 'specials' else t.arg.specials_chars, 'pos': t.pos,
            'len': t.len, 'pre_space': t.pre_space, 'post_space': t.post_space if t.tok in ('macro', 'comment') else ''}


def tokens(s, **kw):
    res = []
    for pos in range(len(s) + 1):
        try:
            res.append(dump_token(LatexWalker(s).get_token(pos, **kw)))
        except Exception as e:  # noqa: BLE001
            res.append(dump_error(e))
    return res


def walker_cases(inputs):
    out = {'inputs': inputs, 'walk': {}, 'tokens': {}}
    out['walk']['tolerant'] = [walk(s) for s in inputs]
    out['walk']['strict'] = [walk(s, tolerant_parsing=False) for s in inputs]
    out['walk']['strict_braces'] = [walk(s, tolerant_parsing=False, strict_braces=True) for s in inputs]
    wdb = walker_db()
    out['walk']['custom'] = [walk(s, latex_context=wdb) for s in inputs]
    short = [s for s in inputs if len(s) <= 60]
    out['json'] = []
    for s in short[:150]:
        lw = LatexWalker(s)
        nodes = lw.get_latex_nodes()[0]
        out['json'].append(json.dumps(nodes, cls=latexwalker.make_json_encoder(lw)))
    out['tokens_inputs'] = short
    out['tokens']['default'] = [tokens(s) for s in short]
    out['tokens']['no_environments'] = [tokens(s, environments=False) for s in short]
    out['tokens']['brackets'] = [tokens(s, brackets_are_chars=False) for s in short]
    out['tokens']['braces'] = [tokens(s, include_brace_chars=[('(', ')'), ('<', '>')]) for s in short]
    return out


# --------------------------------------------------------------------------- latexencode


def encode_configs():
    import re
    c = {}

    def enc(**kw):
        e = UnicodeToLatexEncoder(**kw)
        return e.unicode_to_latex

    c['defaults'] = lambda: enc()
    c['function'] = lambda: lambda s: latexencode.unicode_to_latex(s)
    c['function_non_ascii_only'] = lambda: lambda s: latexencode.unicode_to_latex(s, non_ascii_only=True)
    c['function_options'] = lambda: lambda s: latexencode.unicode_to_latex(
        s, replacement_latex_protection='braces-all', unknown_char_policy='replace', unknown_char_warning=False)
    c['unicode_xml'] = lambda: enc(conversion_rules=['unicode-xml'])
    c['unicode_xml_defaults'] = lambda: enc(conversion_rules=['unicode-xml', 'defaults'])
    c['defaults_unicode_xml'] = lambda: enc(conversion_rules=['defaults', 'unicode-xml'])
    c['non_ascii_only'] = lambda: enc(non_ascii_only=True)
    c['non_ascii_only_xml'] = lambda: enc(non_ascii_only=True, conversion_rules=['unicode-xml'])
    for p in ('none', 'braces', 'braces-almost-all', 'braces-all', 'braces-after-macro'):
        c['protection_' + p.replace('-', '_')] = lambda p=p: enc(replacement_latex_protection=p)
        c['protection_xml_' + p.replace('-', '_')] = lambda p=p: enc(replacement_latex_protection=p,
                                                                     conversion_rules=['unicode-xml'])
    c['protection_underscore'] = lambda: enc(replacement_latex_protection='braces_all')
    c['protection_invalid'] = lambda: enc(replacement_latex_protection='nope')
    c['protection_callable'] = lambda: enc(replacement_latex_protection=lambda r: '<' + r + '>')
    for p in ('keep', 'replace', 'ignore', 'fail', 'unihex'):
        c['unknown_' + p] = lambda p=p: enc(unknown_char_policy=p)
    c['unknown_invalid'] = lambda: enc(unknown_char_policy='nope')
    c['unknown_callable'] = lambda: enc(unknown_char_policy=lambda ch: '[U+%04X]' % ord(ch))
    c['unknown_callable_u2l'] = lambda: enc(unknown_char_policy=lambda ch, u2lobj: '<%s>' % type(u2lobj).__name__)
    c['unknown_nowarn'] = lambda: enc(unknown_char_warning=False, unknown_char_policy='unihex')
    c['invalid_builtin'] = lambda: enc(conversion_rules=['nope'])
    # bibtexparser's encoders, made by the middleware
    from bibtexparser.middlewares import LatexEncodingMiddleware
    for km in (True, False):
        for eu in (True, False):
            e = LatexEncodingMiddleware(keep_math=km, enclose_urls=eu)._encoder
            c['bibtexparser_math%d_urls%d' % (km, eu)] = (lambda e=e: e.unicode_to_latex)
    c['bibtexparser'] = lambda: LatexEncodingMiddleware()._encoder.unicode_to_latex
    # custom rules
    c['rule_dict'] = lambda: enc(conversion_rules=[
        UnicodeToLatexConversionRule(RULE_DICT, {ord('\u00e9'): 'E-ACUTE', ord('x'): '\\times', ord('{'): '<'}),
        'defaults'])
    c['rule_dict_alone'] = lambda: enc(conversion_rules=[
        UnicodeToLatexConversionRule(RULE_DICT, {ord('\u00e9'): 'E-ACUTE', ord('x'): '\\times'})])
    c['rule_dict_protection'] = lambda: enc(conversion_rules=[
        UnicodeToLatexConversionRule(RULE_DICT, {ord('\u00e9'): '\\acute e', ord('x'): '\\x'},
                                     replacement_latex_protection='braces-all'),
        'defaults'])
    c['rule_dict_protection_none'] = lambda: enc(conversion_rules=[
        UnicodeToLatexConversionRule(RULE_DICT, {ord('\u00e9'): '\\acute', ord('x'): '\\x'},
                                     replacement_latex_protection='none'),
        'defaults'])
    c['rule_regex_template'] = lambda: enc(conversion_rules=[
        UnicodeToLatexConversionRule(RULE_REGEX, [(re.compile(r'(?P<word>(?<![A-Za-z0-9_])[A-Z]{2,}(?![A-Za-z0-9_]))'), r'\\textsc{\g<word>}'),
                                                  (re.compile(r'(\d+)-(\d+)'), r'\1\\textendash{}\2'),
                                                  (re.compile(r'\\([A-Za-z0-9_]+)'), r'<\1>'),
                                                  (re.compile(r'\u00e9+'), r'e\\n\\\\x')]),
        'defaults'])
    c['rule_regex_callable'] = lambda: enc(conversion_rules=[
        UnicodeToLatexConversionRule(RULE_REGEX, [(re.compile(r'[aeiou]+'), lambda m: m.group(0).upper())]),
        'defaults'])
    c['rule_regex_lookbehind'] = lambda: enc(conversion_rules=[
        UnicodeToLatexConversionRule(RULE_REGEX, [(re.compile(r'(?<![\\a-z])a[a-z]*'), lambda m: '[' + m.group(0) + ']'),
                                                  (re.compile(r'^x'), 'X')]),
        'defaults'])
    c['rule_regex_group_error'] = lambda: enc(conversion_rules=[
        UnicodeToLatexConversionRule(RULE_REGEX, [(re.compile(r'q'), r'\1')]),
        'defaults'])

    def fn_rule(s, pos):
        if s.startswith('ab', pos):
            return (2, 'AB')
        if s[pos] == '\u00e9':
            return (1, '\\acute{e}')
        return None

    def fn_rule_u2l(s, pos, u2lobj):
        if s[pos] == 'z':
            return (1, 'z' + type(u2lobj).__name__[:3])
        return None

    c['rule_callable'] = lambda: enc(conversion_rules=[UnicodeToLatexConversionRule(RULE_CALLABLE, fn_rule), 'defaults'])
    c['rule_callable_u2l'] = lambda: enc(conversion_rules=[UnicodeToLatexConversionRule(RULE_CALLABLE, fn_rule_u2l),
                                                   'defaults'])
    c['rule_callable_many'] = lambda: enc(conversion_rules=[
        UnicodeToLatexConversionRule(RULE_CALLABLE, lambda s, pos: (3, 'X') if s.startswith('abc', pos) else None),
        UnicodeToLatexConversionRule(RULE_DICT, {ord('a'): 'A'}),
        'defaults'])
    c['partial'] = lambda: PartialLatexToLatexEncoder().unicode_to_latex
    c['partial_xml'] = lambda: PartialLatexToLatexEncoder(conversion_rules=['unicode-xml']).unicode_to_latex
    c['partial_keep_dollar'] = lambda: PartialLatexToLatexEncoder(keep_latex_chars='$').unicode_to_latex
    c['partial_keep_braces'] = lambda: PartialLatexToLatexEncoder(keep_latex_chars='{}', non_ascii_only=True).unicode_to_latex
    c['partial_protection'] = lambda: PartialLatexToLatexEncoder(replacement_latex_protection='braces-all').unicode_to_latex
    c['utf8tolatex'] = lambda: lambda s: utf8tolatex(s)
    c['utf8tolatex_non_ascii_only'] = lambda: lambda s: utf8tolatex(s, non_ascii_only=True)
    c['utf8tolatex_nobrackets'] = lambda: lambda s: utf8tolatex(s, brackets=False)
    c['utf8tolatex_substitute'] = lambda: lambda s: utf8tolatex(s, substitute_bad_chars=True)
    c['utf8tolatex_fail'] = lambda: lambda s: utf8tolatex(s, fail_bad_chars=True)
    return c


def encode_cases(inputs, names=None):
    res = {}
    for name, make in encode_configs().items():
        if names is not None and name not in names:
            continue
        res[name] = apply_config(make, inputs)
    return res


def codepoints():
    "every code point on its own through the built-in rule sets: those that are changed"
    res = {}
    for name, enc in (('defaults', UnicodeToLatexEncoder(unknown_char_warning=False)),
                      ('unicode_xml', UnicodeToLatexEncoder(unknown_char_warning=False,
                                                            conversion_rules=['unicode-xml'])),
                      ):
        d = {}
        for cp in range(0x110000):
            if 0xD800 <= cp < 0xE000:
                continue
            ch = chr(cp)
            out = enc.unicode_to_latex(ch)
            if out != ch:
                d[str(cp)] = out
        res[name] = d
    return res


# --------------------------------------------------------------------------- fuzz

LATEX_ATOMS = [
    'a', 'b', 'x', 'Z', ' ', ' ', '  ', '\n', '\n\n', '\t', '{', '}', '{', '}', '[', ']', '(', ')', '$', '$$', '%',
    '&', '~', '_', '^', '#', '\\', '\\\\', '\\[', '\\]', '\\(', '\\)', '\\begin{equation}', '\\end{equation}',
    '\\begin{itemize}', '\\end{itemize}', '\\begin{a}', '\\end{a}', '\\begin', '\\end', '\\item', '\\textbf',
    '\\emph', '\\url', '\\frac', '\\alpha', '\\beta', '\\sqrt', '\\mathrm', '\\mathbf', '\\text', '\\mbox', '\\section',
    '\\footnote', '\\href', '\\verb', '\\verb|', '|', "\\'", '\\"', '\\`', '\\^', '\\~', '\\c', '\\v', '\\H', '\\=',
    '\\.', '\\u', '\\k', '\\r', '\\d', '\\b', '\\i', '\\j', '\\ss', '\\o', '\\AA', '\\ae', '\\,', '\\ ', '\\%',
    '\\&', '\\$', '\\{', '\\}', '\\_', '\\#', '\\input', '\\item[', '--', '---', '``', "''", '!`', '?`', '<', '>', "'",
    '`', '"', '-', '.', ',', ';', ':', '*', '+', '=', '0', '1', '2', '\u00e9', '\u00fc', '\u00df', '\u03b1', '\u2013',
    '\u00a0', '\u0301', '\u4e2d', '\U0001d7d8', '\\newcommand', '\\def', '\\\\[2mm]', '\\\\*', '\\hspace', '\\hspace*',
    '\\not', '\\vec', '\\hat', '\\not=', '\\left(', '\\right)', '\\begin{pmatrix}', '\\end{pmatrix}', '\\begin{align}',
    '\\end{align}', '\\begin{verbatim}', '\\end{verbatim}', '\\title', '\\author', '\\date', '\\textcolor',
    '\\textcolor{red}', '\\includegraphics', '\\ref', '\\cite',
]


def fuzz_strings(n, seed):
    rng = random.Random(seed)
    pool = corpus.BIBTEX + corpus.DOCUMENTS
    pool = [p for p in pool if p and '\\today' not in p]
    out = []
    for _ in range(n):
        r = rng.random()
        if r < 0.5:
            s = ''.join(rng.choice(LATEX_ATOMS) for _ in range(rng.randint(1, 14)))
        else:
            s = rng.choice(pool)
            for _ in range(rng.randint(1, 4)):
                if not s:
                    break
                i = rng.randrange(len(s))
                op = rng.randrange(6)
                if op == 0:
                    s = s[:i] + s[i + 1:]
                elif op == 1:
                    s = s[:i] + rng.choice(LATEX_ATOMS) + s[i:]
                elif op == 2:
                    s = s[:i] + rng.choice(pool)[:rng.randint(1, 30)] + s[i:]
                elif op == 3:
                    s = s[:i]
                elif op == 4:
                    s = s[i:]
                else:
                    s = s[:i] + s[i:i + rng.randint(1, 6)] * 2 + s[i:]
        if '\\maketitle' in s or '\\today' in s:
            continue
        out.append(s)
    seen, res = set(), []
    for s in out:
        if s not in seen:
            seen.add(s)
            res.append(s)
    return res


def fuzz_unicode(n, seed):
    rng = random.Random(seed)
    alphabet = ['a', 'b', ' ', '\n', '\t', '$', '\\', '{', '}', '%', '_', '^', '~', '#', '&', '"', "'", '<', '>',
                '\u00e9', '\u00fc', '\u00df', '\u00f8', '\u00e5', '\u03b1', '\u03a9', '\u2013', '\u2014', '\u2018',
                '\u2019', '\u00a0', '\u0301', '\u0308', '\u4e2d', '\U0001d538', '\U0001f600', '\u00b2', '\u00b5',
                '\u2264', '\u221e', '\u210f', 'http://a.b/c', 'https://x.y/z_w', 'www.a.b', '$x$', '$a b$', '\\$',
                '\x01', '\x7f', '\x80', '\u200b', '\u2003']
    res, seen = [], set()
    for _ in range(n):
        s = ''.join(rng.choice(alphabet) for _ in range(rng.randint(0, 12)))
        if s not in seen:
            seen.add(s)
            res.append(s)
    return res


# --------------------------------------------------------------------------- main


def dedupe(items):
    seen, out = set(), []
    for s in items:
        if s not in seen:
            seen.add(s)
            out.append(s)
    return out


def write(name, obj):
    os.makedirs(EXPECTED, exist_ok=True)
    path = os.path.join(EXPECTED, name)
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(obj, f, ensure_ascii=False, separators=(',', ':'))
    print('wrote', name, os.path.getsize(path) // 1024, 'KiB')


def main():
    texts = dedupe(corpus.BIBTEX + corpus.DOCUMENTS)
    with open(os.path.join(HERE, 'test_input_1.tex'), encoding='utf-8') as f:
        texts.append(f.read())
    unis = dedupe(corpus.UNICODE + [unicodedata.normalize('NFD', s) for s in corpus.UNICODE[:60]])

    write('latex2text.json', {'inputs': texts, 'configs': l2t_cases(texts)})
    write('latexwalker.json', walker_cases(texts))
    write('latexencode.json', {'inputs': unis, 'configs': encode_cases(unis)})
    write('codepoints.json', codepoints())

    ftexts = fuzz_strings(int(os.environ.get('FUZZ_N', 1500)), int(os.environ.get('FUZZ_SEED', 20260101)))
    funis = fuzz_unicode(int(os.environ.get('FUZZ_N', 1500)), 20260102)
    l2t_names = {'default', 'papers', 'bibtexparser', 'bibtexparser_text_math', 'math_with_delimiters',
                 'strict_true', 'keep_comments', 'braced_groups', 'fill_40', 'custom_context', 'all_options',
                 'default_strict_parse'}
    enc_names = {'defaults', 'unicode_xml', 'bibtexparser', 'bibtexparser_math0_urls0', 'partial', 'non_ascii_only',
                 'unknown_unihex', 'protection_braces_after_macro', 'rule_regex_lookbehind'}
    if os.environ.get('FUZZ_ALL'):  # every configuration (slow, a bigger file): for a one-off check
        l2t_names = enc_names = None
    walk = {'inputs': ftexts, 'tolerant': [walk_(s) for s in ftexts], 'strict': [walk_(s, tolerant_parsing=False)
                                                                              for s in ftexts]}
    write('fuzz.json', {'latex': ftexts, 'l2t': l2t_cases(ftexts, l2t_names), 'walk': walk,
                        'unicode': funis, 'encode': encode_cases(funis, enc_names)})


walk_ = walk

if __name__ == '__main__':
    main()
