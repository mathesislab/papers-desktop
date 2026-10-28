# latexenc-rs

A Rust port of [pylatexenc](https://github.com/phfaist/pylatexenc) **2.11** by Philippe Faist
(MIT): parse LaTeX into a tree of nodes, convert LaTeX to plain unicode text, encode unicode text
as LaTeX. The crate is `latexenc-rs`, the library `latexenc`; the modules mirror those of the
Python package, with the same names and options.

```rust
use latexenc::latex2text::{LatexNodes2Text, MathMode};
use latexenc::latexencode::unicode_to_latex;

let l2t = LatexNodes2Text::builder().math_mode(MathMode::Verbatim).build()?;
assert_eq!(l2t.latex_to_text(r#"{M\"u}ller and $\alpha$"#)?, "Müller and $\\alpha$");
assert_eq!(unicode_to_latex("Müller – α"), r#"M\"uller {\textendash} \ensuremath{\alpha}"#);
```

| Python | Rust |
| --- | --- |
| `latexwalker`: `LatexWalker`, `LatexToken`, the node classes, `ParsingState`, `nodelist_to_latex`, `disp_node`, the JSON encoder | `latexwalker`: `LatexWalker` (`LatexWalkerOptions`, `TokenOptions`, `GetNodesOptions`), `LatexToken`, `LatexNode` + `NodeKind`, `ParsingState`, `nodelist_to_latex`, `disp_node`, `nodelist_to_json` |
| `macrospec`: `MacroSpec`, `EnvironmentSpec`, `SpecialsSpec`, `std_macro`, `LatexContextDb`, `MacroStandardArgsParser`, `VerbatimArgsParser` | `macrospec`: the same; custom args parsers implement the `ArgsParser` trait; `ContextDb<M, E, S>` (`LatexContextDb` for parsing specs, `latex2text::TextContextDb` for text specs) |
| `latex2text`: `LatexNodes2Text(latex_context, math_mode, keep_comments, strict_latex_spaces, keep_braced_groups, keep_braced_groups_minlen, fill_text)`, `MacroTextSpec`, ... | `latex2text`: `LatexNodes2Text::builder()...build()`, `MacroTextSpec`, `EnvironmentTextSpec`, `SpecialsTextSpec`, `Repl` (a string or a function) |
| `latexencode`: `UnicodeToLatexEncoder`, `unicode_to_latex`, `PartialLatexToLatexEncoder`, `UnicodeToLatexConversionRule`, `utf8tolatex` | `latexencode`: `UnicodeToLatexEncoder::builder()...build()`, `unicode_to_latex[_with]`, `PartialLatexToLatexEncoder`, `UnicodeToLatexConversionRule` (`Rule::Dict`, `Rule::Regex`, `Rule::Callable`), `utf8tolatex` |

Positions and lengths (of tokens and nodes) count chars (code points), as Python indexes a `str`;
`SourceText::byte_offset` converts.

## Origin

The Rust code started from
[tex2word-latexwalker](https://github.com/yfyang86/rstex2word) 2.1.0 by Yifan Yang (MIT), a
partial port of pylatexenc's latexwalker, macrospec and latex2text for tex2word, hard-wired to
that application's configuration. This crate takes its structure (the text as shared source, the
tolerant parsing as pylatexenc's two exceptions with recovery at the same points) but is written
against the Python source throughout, as a general library: all the options, the custom context
databases and parsers, latexencode, the default tables regenerated from pylatexenc 2.11
(`tools/gen_tables.py`). See `LICENSE` for the copyrights (the character maps of latexencode are
from latexcodec by Peter Troeger and Matthias C. M. Troffaes, through pylatexenc).

## How the port is checked

- `tests/latexwalker.rs`, `macrospec.rs`, `latex2text.rs`, `latexencode.rs`: the test suite of
  pylatexenc 2.11 (`test/*.py`) in Rust, test for test.
- `tests/differential.rs`: latexenc-rs against what pylatexenc 2.11 gave
  (`tests/gen_expected.py` records it in `tests/expected/`, run with the python that has
  pylatexenc, bibtexparser and papers): a corpus of 635 texts (bibtex values, LaTeX with every
  construct, malformed LaTeX) through 49 named configurations of latex2text (every option,
  custom contexts, the configurations of bibtexparser's `LatexDecodingMiddleware` and of papers),
  the same for latexencode (208 strings, 89 configurations: every option, custom dict/regex/callable rules, bibtexparser's
  `LatexEncodingMiddleware`, regex rules with each class), the node trees, errors and tokens of the walker,
  every code point through the encoders, and random texts. The outputs (or the exception raised, and its message
  for parse errors) are equal.
- Regex classes: 94 patterns (`\w \W \d \D \s \S \b \B`, alone, in sets and negated sets, with `(?a)`,
  `(?i)`, `(?ai)`) over every code point (1.1 million) against Python's `re`; the 3561 code points
  where the engine's own classes differ (`tests/regex_class_diff.json`) through regex rules; 3000
  random patterns, each matched at every position of 8 random texts (the spans of all groups).
- `tools/gen_tables.py` generates the data (`src/*/defaultspecs.rs`, `uni2latexmap*.rs`,
  `pystr_tables.rs`, checked against `re`'s `\w \d \s` for every code point) from the installed
  pylatexenc and CPython.

## Differences from pylatexenc

- Python callables become Rust closures. A replacement function (`simplify_repl`) gets the node
  and the converter (Python's gets the arguments it names: `l2tobj`, `macroname`, ...); a custom
  conversion rule gets the encoder and an `EncodeInput` (text, chars, position).
- Regex rules of latexencode take Python `re` patterns (`str`, with the inline flags) and run them
  on the `fancy-regex` crate (lookaround and backreferences as in `re`) after `pyregex::translate`:
  `\w \W \d \D \s \S \b \B`, inside and outside sets, become explicit classes from CPython's own
  tables (`'₂'` is `\w`, `'\x1c'` is `\s`), the ASCII ones with `(?a)`; `(?i)` does not fold them;
  `$`, `\Z`, the set syntax (`[[]`, `[a&&b]`) and the placement and errors of the flags (global
  flags at the start, `a`, `u`, `L`) are those of `re`. What still differs: case-insensitive
  matching of literals and ranges follows the engine's Unicode folding, not Python's (`(?i)[a-z]`
  matches `'ı'` in Python only; `(?ai)k` still matches the Kelvin sign); a quantifier on a group
  that holds only a lookaround is an error here (not in Python); the
  captures of a quantified group that matched empty can differ; an empty match is not a match
  (Python would loop forever); no backtracking limit (as in Python).
- `\verb` at the very end of the text raises an `IndexError` in Python 2.11; so does the Rust
  (`LatexWalkerError::Other`). A macro read as the argument of another macro has `nodeargs` `None`
  (as in Python, where `nodelist_to_text` of two of them raises a `TypeError`; here an error).
- Nesting: texts that nest more than 250 levels (`MAX_NESTING_DEPTH`) give an error
  (`LatexWalkerError::Other("RecursionError: ...")`), as Python raises a `RecursionError` (at 165 to
  330 levels, with its default limit); the recursion grows the stack with the `stacker` crate.
- `\today` and the date of `\maketitle` use the date in UTC (Python: the local time).
  `str.upper()` and NFC normalization follow the Unicode version of Rust and `unicode-normalization`,
  the character classes (`isspace`, `isalpha`, `\w`) that of Python 3.14 (Unicode 16).
- `get_latex_nodes(s, ...)` of the module level ignores its `pos`, as in pylatexenc 2.11.
- `set_tex_input_directory`/`\input`: files are read as UTF-8 text with universal newlines; the
  paths are resolved with the file system (`canonicalize`), the `strict_input` prefix check is
  pylatexenc's string prefix.
- Warnings that Python logs go to the `log` crate.
- Not ported: `latex_string_class`; the deprecated 1.x names (`MacrosDef`, `default_macro_dict`,
  `macro_dict=`, `keep_inline_math=`, `text_replacements=`; `latex2text()`, `latexnodes2text()`
  and `utf8tolatex()` are there); the command line programs
  (`python -m pylatexenc.latex2text`, ...); the Python-2 compatibility code.
- A `MacroStandardArgsParser` with an invalid `argspec` panics in `new` (Python: `TypeError`);
  `try_new` returns the error.
