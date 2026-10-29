#!/usr/bin/env python3
"""Record what papers-cli (the reference) does in the cases of this file, as
fixtures of crates/papers-rs-cli/tests/fixtures/cases/NAME.json; the test
`conformance` runs papers-rs on the same cases and compares.

    python3 record_cases.py [PAPERS] [CASE...]

A case is a home directory and a working directory before, and steps: a
command line, and what papers prints (standard output and error, exit status)
and leaves (the files, under the home and the working directory) after it.
Paths are normalised: <TMP> is the directory of the case; <MC> and <V2> are
the global configuration of papers (`papers/config.json`) and that of papers 2
(`papersconfig.json`) in the configuration directory of the platform, and
<MC~> and <V2~> their paths as `~/...`.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile

PAPERS = sys.argv[1] if len(sys.argv) > 1 and os.path.exists(sys.argv[1]) else 'papers'
ONLY = [a for a in sys.argv[1:] if a != PAPERS]
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, '..', 'fixtures', 'cases')

# the configuration directory of papers, on macOS (the fixtures are recorded there)
CONFDIR = 'Library/Application Support'
MC = f'{CONFDIR}/papers/config.json'
V2 = f'{CONFDIR}/papersconfig.json'

# not compared: the caches, the locks
IGNORED = ('home/Library/Caches/', 'home/.cache/')


def jd(**kw):
    return json.dumps(kw, sort_keys=True, indent=2)


CASES = []


def case(name, steps, home=None, cwd=None, platform=None):
    CASES.append(dict(name=name, home=home or {}, cwd=cwd or {}, steps=steps, platform=platform))


def step(*args, cwd='.', write=None, rm=None, env=None):
    return dict(args=list(args), cwd=cwd, write=write or {}, rm=rm or [], env=env or {})


# init
# ====
case('init-default', [step('init')])
case('init-verbose', [step('init', '-v'), step('-v', 'init')])
case('init-debug', [step('init', '--debug', 'lib')])
case('init-dir', [step('init', 'lib'), step('init', 'a/b/c'), step('init', 'lib'), step('init', '.')])
case('init-options', [step('init', 'a', '--bibtex', 'lib.bib', '--filesdir', 'docs', '--editor', 'subl -w',
                           '--paths', 'absolute', '--name-author-num', '3', '--key-template', '{Author}{year}',
                           '--name-unknown-strict', '--key-unknown-strict', '--name-template', '{author}',
                           '--name-author-sep', '+', '--name-title-word-num', '5', '--name-title-word-size', '3',
                           '--name-title-length', '50', '--name-title-sep', '_', '--name-unknown-template', 'x_{id}',
                           '--key-author-num', '1', '--key-author-sep', '-', '--key-title-word-num', '2',
                           '--key-title-word-size', '2', '--key-title-length', '20', '--key-title-sep', '.',
                           '--key-unknown-template', 'k{id}'),
                      step('-C', 'a', 'config')])
case('init-formats-default-values', [step('init', '--name-author-num', '2', '--key-template', '{author}{year}',
                                          '--name-unknown-strict', '--no-name-unknown-strict')])
case('init-none-values', [step('init', 'a', '--bibtex', 'none', '--filesdir', 'none'),
                          step('init', 'b', '--bibtex', 'NULL', '--filesdir', 'Unset'),
                          step('init', 'c', '--bibtex', '', '--filesdir', '')])
case('init-existing-bibs', [step('init', 'b'), step('init', 'c'), step('init', 'd'), step('init', 'e')],
     cwd={'b/z.bib': '@article{x, title={t}}\n', 'b/a.bib': '', 'b/papers/': None, 'b/pdfs/': None,
          'c/papers.bib': '', 'c/other.bib': '', 'c/pdf/': None, 'd/.hidden.bib': '', 'd/bibliography/': None,
          'e/papers/': None, 'e/files/': None, 'e/x.bib/': None})
case('init-paths-outside', [step('init', 'e', '--bibtex', '../x.bib', '--filesdir', '../pdfs'),
                            step('init', 'f', '--bibtex', '../other/dir/x.bib', '--filesdir', '/<TMP>/cwd/abs/pdfs'.replace('//', '/'))])
case('init-uri', [step('init', 'a', '--bibtex', 'file://<TMP>/cwd/my%20lib.bib', '--filesdir', 'file://<TMP>/cwd/my%20files')])
case('init-errors', [step('init'), step('init'), step('init', '--global'), step('init', '--shallow'),
                     step('init', 'f', '--pool', 'nolib'), step('init', 'f', '--pool', '.'),
                     step('init', 'f', '--shallow', '--pool', 'x'), step('init', 'f', '--global', '--pool', 'x'),
                     step('init', 'g', '--pool', 'none', '--shallow')])
case('init-global', [step('init', 'lib1', '--global'), step('init', 'lib2', '--shallow'), step('init', 'lib3', '--pool', '../lib1'),
                     step('init', 'lib1', '--global'), step('init', '.', '--global'), step('config', '--global'),
                     step('-C', 'lib2', 'config'), step('init', 'lib4', '--global', '--shallow')])
case('init-global-home', [step('init', '~/papers/lib', '--global'), step('config', '--global'),
                          step('init', 'libb', '--shallow')])
case('init-inside-library', [step('init'), step('init', 'sub'), step('-C', 'sub', 'config', 'pool')])
case('init-ignored-options', [step('init', 'a', '--git'), step('init', 'b', '--no-git', '--git-lfs'),
                              step('init', 'c', '--git', '--git', '--no-git'),
                              step('init', '--git', '--paths', 'bad'), step('init', '--no-git', 'a', 'b', '--git-lfs'),
                              step('init', '--info', '--git', '-h'), step('config', '--info', '--no-git', '--bogus'),
                              step('rm', '--no-git', '--info')])
case('init-info-option', [step('init', 'a', '--info')])
case('init-porcelain', [step('init', '--porcelain'), step('init', 'x', '--porcelain', '-v')])
case('init-machine-defaults',
     [step('init', 'a'), step('init', 'b', '--paths', 'relative'), step('init', 'c', '--name-author-num', '2'),
      step('-C', 'a', 'config')],
     home={MC: jd(version=3, paths='absolute', editor='vim', nameformat={'author_num': 5, 'template': '{title}'},
                  keyformat={'template': '{year}'}, files='copy', **{'history-size': 10})})
case('init-pool-not-library', [step('init', 'x', '--pool', '../plain'), step('init', 'x', '--pool', '../haspool'),
                               step('init', 'x', '--pool', '../both')],
     cwd={'plain/': None, 'haspool/.papers/config.json': jd(version=3, pool='../other', bibtex='p.bib'),
          'other/.papers/config.json': jd(version=3), 'both/.papers/config.json': '{}', 'both/.papersconfig.json': '{}'})
case('init-nested-pool', [step('init', 'A'), step('init', 'pdfs/deeper/nested', '--pool', '../../../A'),
                          step('-C', 'pdfs/deeper/nested', 'config', 'pool'),
                          step('init', 'B', '--pool', 'A'), step('init', 'C', '--pool', '~/nowhere')],
     cwd={'pdfs/': None})
case('init-existing-v2-library', [step('init'), step('init', '--global')],
     cwd={'.papersconfig.json': '{"bibtex": "x.bib"}'})
case('init-two-configs', [step('init'), step('init', 'sub')],
     cwd={'.papers/config.json': '{}', '.papersconfig.json': '{}'})
case('init-in-default-library-origin', [step('init', 'new')],
     home={MC: jd(version=3, default='~/lib')}, cwd={})
case('init-usage-errors', [step('init', '--name-author-num', 'x'), step('init', '--name-author-num'),
                           step('init', 'a', 'b'), step('init', '--paths', 'x'), step('init', '--color', 'red'),
                           step('init', '--bogus'), step('init', '--bogus', 'x', '--paths', 'y'),
                           step('init', '--pool'), step('init', '--shallow=1'), step('init', '--key-title-word-num', '1.5'),
                           step('init', '--editor'), step('init', '-z'), step('init', '--dry-run'), step('init', '--filesdir')])
case('init-abbreviations', [step('init', '--verb', '--bib', 'x.bib', '--files', 'f', '--sha', '--shall'),
                            step('init', 'd', '--sh', '--glob')], home={MC: jd(version=3, default='~/x')},
     cwd={})

# config
# ======
case('config-list', [step('init'), step('config'), step('config', 'bibtex'), step('config', 'pool'),
                     step('config', 'name.template'), step('config', 'editor'), step('config', 'auto-push'),
                     step('config', 'history-size'), step('config', 'paths')])
case('config-no-library', [step('config'), step('config', 'files'), step('config', 'files', 'copy'),
                           step('config', '--unset', 'files'), step('config', '--prune'), step('config', '--global'),
                           step('config', '--global', 'files'), step('config', '--global', 'default')])
case('config-no-library-default-not-library', [step('config'), step('config', '--global')],
     home={MC: jd(version=3, default='~/nolib')})
case('config-set', [step('init'),
                    step('config', 'bibtex', 'other.bib'), step('config', 'filesdir', '../x/pdfs'),
                    step('config', 'paths', 'absolute'), step('config', 'paths', 'foo'), step('config', 'files', 'copy'),
                    step('config', 'files'), step('config', 'auto-push', 'yes'), step('config', 'auto-push', 'maybe'),
                    step('config', 'history-size', '12x'), step('config', 'history-size', '12'),
                    step('config', 'history-size', '-3'), step('config', 'name.template', '{author}'),
                    step('config', 'name.template', '{author}', 'extra'), step('config', 'name.template'),
                    step('config', 'name.unknown-strict', 'on'), step('config', 'key.author-num', '1'),
                    step('config', 'editor', 'subl -w'), step('config', 'checksums', 'false'),
                    step('config', 'checksums', 'true'), step('config', 'checksums', 'yes'), step('config'),
                    step('config', 'history-separate', 'true'), step('config', 'keep-deleted-files', 'false'),
                    step('config', 'history-dir', '/x')])
case('config-equal-to-default', [step('init'), step('config', 'files', 'move'), step('config', 'paths', 'relative'),
                                 step('config', 'name.author-num', '2'), step('config', 'auto-push', 'false'),
                                 step('config', 'editor', ''), step('config')])
case('config-unset', [step('init', '--editor', 'vim', '--name-author-num', '3'), step('config', '--unset', 'editor'),
                      step('config', '--unset', 'name.author-num'), step('config', '--unset', 'files'),
                      step('config', '--unset', 'paths'), step('config', '--unset', 'bibtex'),
                      step('config', '--unset', 'filesdir'), step('config', '--unset', 'nosuch'),
                      step('config', '--unset', 'files', 'x'), step('config', '--unset'), step('config', '--unset=files'),
                      step('config', '--unset', 'pool'), step('config', '--unset', 'history-dir'), step('config')])
case('config-names', [step('init'), step('config', 'nosuch'), step('config', 'nosuch', '1'), step('config', 'git'),
                      step('config', 'git-lfs', 'x'), step('config', 'trash-days', '1'), step('config', '--unset', 'trash-size'),
                      step('config', 'default'), step('config', 'default', 'x'), step('config', 'history-dir'),
                      step('config', 'history-dir', 'x'), step('config', 'keep-deleted-files', 'no'),
                      step('config', '--global', 'pool'), step('config', '--global', 'pool', 'x'),
                      step('config', '--global', 'bibtex'), step('config', '--global', 'filesdir', 'x'),
                      step('config', '--global', 'nosuch'), step('config', '--global', '--unset', 'pool')])
case('config-global', [step('config', '--global', 'history-dir', '/x'), step('config', '--global', 'history-size', '7'),
                       step('config', '--global', 'name.template', '{author}'), step('config', '--global'),
                       step('config', '--global', 'name.template'), step('config', '--global', 'editor'),
                       step('config', '--global', 'history-size', '3', '4'), step('config', '--global', 'editor', 'vim'),
                       step('config', '--global', '--unset', 'editor'), step('config', '--global', '--unset', 'history-dir'),
                       step('config', '--global', 'paths', 'absolute'), step('config', '--global', 'files', 'link'),
                       step('config', '--global', 'checksums', 'false'), step('config', '--global', '--prune'),
                       step('config', '--global', '--dry-run', 'files', 'copy'), step('config', '--global', 'paths', 'x'),
                       step('config', '--global', 'editor', ''), step('config', '--global')])
case('config-global-default', [step('init', 'A'), step('init', 'B', '--pool', '../A'),
                               step('config', '--global', 'default', 'A'), step('config', '--global', 'default'),
                               step('config', '--global'), step('config', '--global', 'default', 'nolib'),
                               step('config', '--global', 'default', 'B'), step('config', '--global', 'default', 'a', 'b'),
                               step('config', '--global', 'default', '~/x'), step('config'), step('config', 'pool'),
                               step('-C', 'A', 'config', 'default'), step('config', '--global', 'default', 'none'),
                               step('config', '--global', 'default'), step('config', '--global'),
                               step('config', '--global', 'default', 'A'), step('config', '--global', '--unset', 'default'),
                               step('config', '--global', 'default', 'A', '--dry-run'), step('config', '--global')])
case('config-default-library-used', [step('config'), step('config', 'bibtex'), step('config', 'files', 'link'),
                                     step('-C', 'sub', 'config', 'files'), step('init', 'x', '--shallow'),
                                     step('-C', 'x', 'config')],
     home={MC: jd(version=3, default='<TMP>/cwd/lib', editor='vim')},
     cwd={'lib/.papers/config.json': jd(version=3, bibtex='lib.bib', filesdir='pdfs'), 'sub/': None})
case('config-default-library-tilde', [step('config'), step('config', 'bibtex')],
     home={MC: jd(version=3, default='~/lib'), 'lib/.papers/config.json': jd(version=3, bibtex='lib.bib')})
case('config-pool', [step('init', 'A'), step('init', 'B'), step('init', 'C', '--pool', '../A'),
                     step('-C', 'B', 'config', 'pool', '../A'), step('-C', 'B', 'config', 'pool', '../A'),
                     step('-C', 'B', 'config', 'pool', 'none'), step('-C', 'B', 'config', 'pool', 'none'),
                     step('-C', 'B', 'config', '--dry-run', 'pool', '../A'), step('-C', 'B', 'config', 'pool', '../A', '--dry-run'),
                     step('-C', 'B', 'config', 'pool', '../A', '-y'), step('-C', 'B', 'config', 'pool', '../nolib'),
                     step('-C', 'A', 'config', 'pool', '../B'), step('-C', 'B', 'config', '--unset', 'pool'),
                     step('-C', 'B', 'config', '--unset', 'pool'), step('-C', 'B', 'config', 'pool', '~/x'),
                     step('-C', 'B', 'config', 'pool', '.'), step('-C', 'B', 'config', 'pool'),
                     step('-C', 'B', 'config', 'pool', 'x', 'y'), step('-C', 'C', 'config', 'pool', 'none', '--dry-run'),
                     step('-C', 'C', 'config', 'pool', 'none'), step('-C', 'C', 'config', 'filesdir', 'none'),
                     step('-C', 'C', 'config', 'pool', '../A'), step('-C', 'C', 'config', 'pool', 'none'),
                     step('-C', 'B', 'config', 'pool', '../A'), step('-C', 'B', 'config', 'pool', '../D'),
                     step('-C', 'B', 'config'), step('-C', 'B', 'config', 'pool', './sub/..//../A')],
     cwd={'D/.papers/config.json': jd(version=3, bibtex='d.bib', pool='../A')})
case('config-pool-missing-pool', [step('init', 'A'), step('init', 'B', '--pool', '../A'),
                                  step('-C', 'B', 'config', 'pool', 'none'), step('-C', 'B', 'config'),
                                  step('-C', 'B', 'config', 'pool', '../A'), step('init', 'A'),
                                  step('-C', 'B', 'config', 'pool', '../A')],
     cwd={})
case('config-pool-entries-without-files',
     [step('init', 'A'), step('init', 'B'),
      step('-C', 'B', 'config', 'pool', '../A', write={'cwd/B/papers.bib': '@article{x,\n title={T},\n author={Smith, J},\n year={2000}\n}\n'}),
      step('-C', 'B', 'config', 'pool', 'none'), step('-C', 'B', 'config', 'pool', '../A', '-y'),
      step('-C', 'B', 'config', 'paths', 'absolute'), step('-C', 'B', 'config', '-v', 'paths', 'relative')])
case('config-pool-none-without-filesdir', [step('init', 'A'), step('init', 'B', '--pool', '../A', '--filesdir', 'none'),
                                           step('-C', 'B', 'config', 'pool', 'none'),
                                           step('-C', 'B', 'config', 'filesdir', 'files'),
                                           step('-C', 'B', 'config', 'pool', '../A'), step('-C', 'B', 'config', 'pool', 'none'),
                                           step('-C', 'B', 'config', '--unset', 'filesdir'), step('-C', 'B', 'config', 'pool', '../A'),
                                           step('-C', 'B', 'config', '--unset', 'pool'),
                                           step('-C', 'B', 'config', '--unset', 'pool', '--dry-run')])
case('config-prune', [step('init'), step('config', '--prune'), step('config', '--prune', 'bibtex'),
                      step('config', '--prune', '--unset', 'files'),
                      step('config', '--prune', write={'cwd/.papers/config.json': jd(version=3, bibtex='papers.bib', files='move', **{'history-size': 500, 'git': True})}),
                      step('config', '--prune', write={'cwd/.papers/config.json': jd(version=3, bibtex='papers.bib', files='move', **{'history-size': 500})}),
                      step('config', '--dry-run', '--prune', write={'cwd/.papers/config.json': jd(version=3, bibtex='papers.bib', files='move', **{'history-size': 500})}),
                      step('config', '--prune'),
                      step('config', '--prune', write={'cwd/.papers/config.json': jd(version=3, bibtex='papers.bib', gitlfs=True, backup_files=True, gitdir='x', **{'trash-days': 3, 'trash-size': 1, 'git': True})}),
                      step('config', '--prune', write={'cwd/.papers/config.json': jd(version=3, bibtex='papers.bib', nameformat={'template': '{author}', 'author_num': 2}, paths='relative', pool='none')}),
                      ], home={MC: jd(version=3, nameformat={'template': '{author}'})})
case('config-dry-run', [step('init'), step('config', '--dry-run', 'paths', 'absolute'), step('config', '-y', '--dry-run', 'files', 'link'),
                        step('config', '--dry-run', '--unset', 'editor'), step('config', '--dry-run'),
                        step('config', '--dry-run', 'bibtex'), step('config', '--dry-run', 'bibtex', 'x.bib'),
                        step('config', '--dry-run', 'checksums', 'false'), step('config')])
case('config-v2-global', [step('config'), step('config', 'paths'), step('config', 'paths', 'relative'), step('config', '--prune'),
                          step('config', '--global'), step('-C', 'sub', 'config', 'bibtex'), step('config', '--unset', 'files'),
                          step('config', 'pool', 'none'), step('config', 'name.author-num'), step('config', 'name.template'),
                          step('init', 'lib'), step('-C', 'lib', 'config', 'bibtex'), step('init'), step('config', 'bibtex')],
     home={V2: '{"bibtex": "/x/y/papers.bib", "filesdir": "/x/y/files", "absolute_paths": true, "nameformat": {"template": "{author}", "author_num": 3}, "local": false, "git": true}'},
     cwd={'sub/': None})
case('config-v2-global-relative-paths', [step('config'), step('config', 'bibtex')],
     home={V2: '{"bibtex": "rel/papers.bib", "filesdir": "rel/files"}'})
case('config-v2-library', [step('config'), step('config', 'name.template'), step('config', 'paths'), step('config', 'files', 'link'),
                           step('config')],
     cwd={'.papersconfig.json': '{"bibtex": "lib.bib", "filesdir": "pdfs", "local": true, "absolute_paths": true, "backup_files": true, "git": true, "nameformat": {"template": "{author}", "author_num": 3}, "keyformat": {"template": "{year}"}}'})
case('config-v2-library-no-local', [step('config'), step('config', 'bibtex'), step('config', '--prune'), step('config', 'pool', 'none')],
     cwd={'.papersconfig.json': '{"bibtex": "lib.bib"}'})
case('config-v2-library-moved-by-dry-run', [step('config', '--dry-run', 'files', 'link'), step('config', '--prune'), step('config', '--global', '--dry-run', 'files', 'copy')],
     cwd={'.papersconfig.json': '{"bibtex": "lib.bib", "local": true}'})
case('config-v2-library-prune', [step('config', '--prune'), step('config', '--prune')],
     cwd={'.papersconfig.json': '{"bibtex": "lib.bib", "local": true, "files": "move", "git": true}'})
case('config-v2-library-both-v2-global', [step('-C', 'lib', 'config', 'files', 'link'), step('-C', 'lib', 'config'), step('config')],
     home={V2: '{"bibtex": "/x/y/papers.bib"}'}, cwd={'lib/.papersconfig.json': '{"bibtex": "lib.bib", "local": true}'})
case('config-two-configs', [step('config'), step('config', 'bibtex'), step('init'), step('config', '--global')],
     cwd={'.papers/config.json': '{}', '.papersconfig.json': '{}'})
case('config-versions', [step('-C', 'v', 'config'), step('-C', 'v', 'config', 'bibtex')],
     cwd={'v/.papers/config.json': '{"version": 4}'})
case('config-version-values',
     [step('-C', 'v', 'config', write={'cwd/v/.papers/config.json': '{"version": "x"}'}),
      step('-C', 'v', 'config', write={'cwd/v/.papers/config.json': '{"version": true}'}),
      step('-C', 'v', 'config', write={'cwd/v/.papers/config.json': '{"version": 3.5}'}),
      step('-C', 'v', 'config', write={'cwd/v/.papers/config.json': '{"version": null}'}),
      step('-C', 'v', 'config', write={'cwd/v/.papers/config.json': '{"version": 2, "bibtex": "a.bib"}'}),
      step('-C', 'v', 'config', write={'cwd/v/.papers/config.json': '{"version": 3, "absolute_paths": true, "backup_files": true, "local": true}'}),
      step('-C', 'v', 'config', write={'cwd/v/.papers/config.json': '{"absolute_paths": true, "bibtex": "a.bib"}'}),
      step('-C', 'v', 'config', write={'cwd/v/.papers/config.json': '{"bibtex": "a.bib", "pool": "../p"}'}),
      step('-C', 'v', 'config', write={'cwd/v/.papers/config.json': '{"version": 3, "pool": "../p", "bibtex": "a.bib"}'}),
      ], cwd={'v/': None})
case('config-pool-lists', [step('-C', 'v', 'config', write={'cwd/v/.papers/config.json': '{"version": 3, "pool": ["a", "b"]}'}),
                           step('-C', 'v', 'config', 'pool', write={'cwd/v/.papers/config.json': '{"version": 3, "pool": ["a"], "bibtex": "b.bib", "id": "abc"}'}),
                           step('-C', 'v', 'config', 'pool', write={'cwd/v/.papers/config.json': '{"version": 3, "pool": []}'}),
                           step('-C', 'v', 'config', 'pool', write={'cwd/v/.papers/config.json': '{"version": 3, "pool": ["", "z"]}'}),
                           step('-C', 'v', 'config', 'pool', write={'cwd/v/.papers/config.json': '{"version": 3, "pools": ["default"]}'}),
                           step('-C', 'v', 'config', 'pool', write={'cwd/v/.papers/config.json': '{"version": 3, "pools": ["../x", "default"], "light": true, "sync": "push", "local": true}'}),
                           step('-C', 'v', 'config', 'pool', write={'cwd/v/.papers/config.json': '{"version": 3, "pools": ["shared"]}'}),
                           step('-C', 'v', 'config', 'pool', write={'cwd/v/.papers/config.json': '{"version": 3, "pools": ["unknown"]}'}),
                           step('-C', 'v', 'config', 'pool', write={'cwd/v/.papers/config.json': '{"version": 3, "pools": []}'}),
                           step('-C', 'v', 'config', 'pool', write={'cwd/v/.papers/config.json': '{"version": 3, "pools": ["none"]}'}),
                           step('-C', 'v', 'config', 'pool', write={'cwd/v/.papers/config.json': '{"version": 3, "pools": ["~/p"]}'}),
                           ], home={MC: jd(version=3, default='~/def', pools={'shared': '/shared/pool'})}, cwd={'v/': None})
case('config-weird-values', [step('-C', 'v', 'config'),
                             step('-C', 'v', 'config', 'files'), step('-C', 'v', 'config', 'editor'),
                             step('-C', 'v', 'config', 'key.title-sep'), step('-C', 'v', 'config', 'history-size'),
                             step('-C', 'v', 'config', 'auto-push'), step('-C', 'v', 'config', 'checksums'),
                             step('-C', 'v', 'config', '--prune'), step('-C', 'v', 'config', 'files', 'move')],
     cwd={'v/.papers/config.json': '{"version": 3, "history-size": "x", "files": 5, "auto-push": null, "editor": ["a", true, null, 1.5, "it\'s"], "keyformat": {"title_sep": 1.5, "author_num": true}, "checksums": 1, "bibtex": "", "filesdir": null, "unknown": 1}'})
case('config-bibtex-filesdir-values', [step('-C', 'v', 'config'), step('-C', 'v', 'config', 'bibtex'),
                                       step('-C', 'v', 'config', 'filesdir'),
                                       step('-C', 'v', 'config', 'bibtex', 'sub/../x.bib'),
                                       step('-C', 'v', 'config', 'filesdir', '/abs/elsewhere'),
                                       step('-C', 'v', 'config', 'bibtex', '~/home.bib'),
                                       step('-C', 'v', 'config', 'bibtex', '../v2/../../out/y.bib'),
                                       step('config', 'bibtex', 'x'), step('-C', 'v', 'config', 'bibtex', 'file://<TMP>/cwd/v/uri.bib')],
     cwd={'v/.papers/config.json': '{"version": 3, "bibtex": "../outside/./a/../a.bib", "filesdir": "pdfs//x/"}'})
case('config-symlinks', [step('-C', 'link', 'config'), step('-C', 'link', 'config', 'bibtex', 'new.bib'),
                         step('-C', 'real', 'config', 'bibtex'), step('-C', 'link/sub', 'config', 'filesdir', '../data/f')],
     cwd={'real/.papers/config.json': '{"version": 3, "bibtex": "a.bib", "filesdir": "files"}', 'real/sub/': None,
          'link': ('symlink', 'real')})
case('config-preserves-id', [step('-C', 'v', 'config', 'files', 'link'), step('-C', 'v', 'config', '--unset', 'files'),
                             step('-C', 'v', 'config', 'files', 'link', write={'cwd/v/.papers/config.json': '{"version": 3, "id": "0123456789abcdef0123456789abcdef", "bibtex": "a.bib"}'}),
                             step('-C', 'v', 'config', 'files', 'copy', write={'cwd/v/.papers/config.json': '{"version": 3, "id": "NOTANID", "bibtex": "a.bib"}'}),
                             step('-C', 'v', 'config', 'files', 'link', write={'cwd/v/.papers/config.json': '{"version": 3, "id": 12, "bibtex": "a.bib"}'})],
     cwd={'v/.papers/config.json': '{"version": 3, "id": "ffffffffffffffffffffffffffffffff", "bibtex": "a.bib"}'})
case('config-unicode', [step('-C', 'v', 'config', 'editor', 'éditeur 😀'), step('-C', 'v', 'config'),
                        step('-C', 'v', 'config', 'name.template', '{auteur}_é\\n\t"x"')], cwd={'v/.papers/config.json': '{"version": 3}'})
case('config-gitignore', [step('-C', 'a', 'config', 'files', 'link'), step('-C', 'b', 'config', 'files', 'link'),
                          step('-C', 'c', 'config', 'files', 'link'), step('-C', 'd', 'config', 'files', 'link')],
     cwd={'a/.papers/config.json': '{"version": 3}', 'b/.papers/config.json': '{"version": 3}', 'b/.papers/.gitignore': 'backup/\n',
          'c/.papers/config.json': '{"version": 3}', 'c/.papers/.gitignore': 'mine\n', 'd/.papers/config.json': '{"version": 3}',
          'd/.papers/.gitignore': ''})
case('config-no-trailing-newline-input', [step('config', 'files', 'link')],
     cwd={'.papers/config.json': '{\n  "version": 3,\n  "bibtex": "a.bib"\n}\n'})
case('config-unreadable-global', [step('config', '--global'), step('config'), step('init', 'lib'), step('config', '--global', 'editor', 'vim'),
                                  step('config', '--global')],
     home={MC: '{bad'})
case('config-unreadable-global-empty', [step('config', '--global'), step('-C', 'v', 'config', 'editor')],
     home={MC: ''}, cwd={'v/.papers/config.json': '{"version": 3}'})
case('config-unreadable-global-messages', [step('config', '--global', write={'home/<MC>': m}) for m in
                                           ['{"a" 1}', '{"a": 1,}', '[1,]', '{} x', '"abc', '{"a": }', '﻿{}', 'nul', '{"a": 1 "b"}']])
case('config-global-version', [step('config', '--global'), step('config', '--global', 'editor')], home={MC: jd(version=4)})
case('config-global-version-invalid', [step('config', '--global')], home={MC: jd(version='x')})
case('config-global-v2-file', [step('config', '--global'), step('-C', 'v', 'config', 'editor'), step('config', '--global', 'files', 'link')],
     home={MC: '{"editor": "vim", "absolute_paths": true, "pool": "none"}'}, cwd={'v/.papers/config.json': '{"version": 3}'})
case('config-global-pool-ignored', [step('config', '--global'), step('-C', 'v', 'config', 'pool'), step('config', '--global', 'files', 'link')],
     home={MC: jd(version=3, pool='/some/pool', editor='vim')}, cwd={'v/.papers/config.json': '{"version": 3}'})
case('config-global-pools-dropped-on-save', [step('config', '--global', 'files', 'link'), step('config', '--global')],
     home={MC: jd(version=3, pools={'a': '/p'}, default='~/x', other=1)})
case('config-global-inherited', [step('init', 'a'), step('-C', 'a', 'config'), step('-C', 'a', 'config', 'files', 'link'),
                                 step('-C', 'a', 'config', 'files', 'copy'), step('-C', 'a', 'config', 'name.author-num', '2'),
                                 step('-C', 'a', 'config', 'editor', 'vim'), step('-C', 'a', 'config', 'editor', 'emacs'),
                                 step('-C', 'a', 'config', '--unset', 'editor'), step('-C', 'a', 'config', '--unset', 'name.author-num'),
                                 step('-C', 'a', 'config', '--prune')],
     home={MC: jd(version=3, files='copy', editor='vim', nameformat={'author_num': 5}, paths='absolute', **{'history-size': 10, 'auto-push': True})})
case('config-global-absolute-paths-v2-key', [step('init', 'a'), step('-C', 'a', 'config', 'paths')],
     home={MC: '{"absolute_paths": true}'})
case('config-bibtex-dirs', [step('-C', 'a', 'config', 'bibtex', 'sub/x.bib'), step('-C', 'a', 'config', 'paths', 'absolute'),
                            step('-C', 'a', 'config', '-v', 'paths', 'relative')],
     cwd={'a/.papers/config.json': '{"version": 3, "bibtex": "x.bib"}', 'a/x.bib': ''})
case('config-paths-with-bibtex', [step('init'), step('config', '-v', 'paths', 'absolute'), step('config', '-v', 'paths', 'relative'),
                                  step('config', '--debug', 'paths', 'absolute'), step('config', '-v', 'paths', 'absolute'),
                                  step('config', '--porcelain', 'bibtex'), step('config', '--porcelain', 'paths', 'relative'),
                                  step('config', '--porcelain'), step('config', '--color', 'always', 'bibtex'),
                                  step('config', '--error', 'paths', 'relative'), step('config', '--warn', '--error', 'bibtex', 'o.bib'),
                                  step('config', '--error', '--warn', 'bibtex', 'p.bib'), step('config', '--info', 'bibtex'),
                                  step('config', '--verb', 'bibtex'), step('config', '--deb', 'bibtex'), step('config', '--debug', '-v', 'bibtex'),
                                  step('config', '--no-git', 'bibtex'), step('config', '-v', 'bibtex', '--', 'foo'),
                                  step('config', '--', 'bibtex')])

# the global configuration of earlier versions, moved on macOS (the XDG-style locations differ from platformdirs there)
case('legacy-global-config-xdg', [step('config', '--global'), step('config', '--global'), step('init', 'lib')],
     home={'.config/papersconfig.json': '{"bibtex": "/x/p.bib", "local": false}'}, platform='macos')
case('legacy-global-config-data', [step('config', '--global'), step('config')],
     home={'.local/share/config.json': '{"bibtex": "/x/p.bib", "local": false}'}, platform='macos')
case('legacy-global-config-both-present', [step('config', '--global'), step('config')],
     home={'.config/papersconfig.json': '{"bibtex": "/x/p.bib"}', V2: '{"bibtex": "/y/p.bib"}',
           '.local/share/config.json': '{"bibtex": "/z/p.bib"}'}, platform='macos')

# -C and the directory
# ====================
case('directory-option', [step('-C', 'lib', 'config', 'bibtex'), step('-C', 'sub/deep', 'config'), step('-C', 'nosuch', 'config'),
                          step('-C', 'sub/deep', '-C', '../../lib', 'config', 'bibtex'), step('-Clib', 'config', 'bibtex'),
                          step('-C', 'sub', '-Clib', 'config', 'bibtex'), step('-C'), step('-C', 'lib'), step('-C', ''),
                          step('-C', 'lib/', 'config', 'bibtex'), step('-C', 'lib', 'init'), step('-C', '.', 'config', 'bibtex'),
                          step('-C', 'lib/../lib', 'config', 'bibtex'), step('-C', 'file.txt', 'config'),
                          step('config', '-C', 'lib', 'bibtex'), step('--version', '-C', 'lib'), step('-C', 'lib', '--version')],
     cwd={'lib/.papers/config.json': jd(version=3, bibtex='lib.bib'), 'sub/deep/': None, 'file.txt': 'x'})
case('library-search-upwards', [step('config', 'bibtex'), step('-C', 'a/b/c', 'config', 'bibtex'), step('-C', 'a/b/c', 'init'),
                                step('-C', 'x', 'config', 'bibtex')],
     cwd={'.papers/config.json': jd(version=3, bibtex='top.bib'), 'a/.papers/config.json': jd(version=3, bibtex='a.bib'),
          'a/b/c/': None, 'x/': None})

# the command line
# ================
case('usage-top', [step(), step('-h'), step('--help'), step('--he'), step('--version'), step('--vers'), step('--v'), step('--version=1'),
                   step('bogus'), step('--bogus'), step('--bogus', 'init'), step('--bogus', 'init', '--bogus2'), step('-C', 'x'),
                   step('--version', 'init'), step('init', '--version'), step('-v', 'config'), step('--version', '--bogus'),
                   step('-5'), step('--', 'init'), step('--', 'bogus'), step('-h', 'bogus'), step('bogus', '-h'),
                   step('lib'), step('-x'), step('-', 'init')],
     cwd={'x.pdf': '', 'lib/': None})
case('usage-help-commands', [step(c, '-h') for c in ['init', 'status', 'migrate', 'install', 'uninstall', 'add', 'check', 'file',
                                                      'filecheck', 'list', 'edit', 'rm', 'tag', 'pull', 'push', 'sync', 'export', 'open',
                                                      'doi', 'fetch', 'extract', 'undo', 'redo', 'restore', 'restore-backup', 'git',
                                                      'history', 'backup']] +
     [step('file', c, '-h') for c in ['rename', 'mv', 'attach', 'detach', 'rm', 'check', 'fix', 'clean', 'checksum']] +
     [step('init', '--help'), step('init', '--he'), step('init', '-h', '--bogus'), step('init', '--bogus', '-h'),
      step('rm', '-h'), step('edit', '-h', 'x'), step('init', 'x', '-h', 'y')])
case('usage-file', [step('file'), step('file', '--bogus'), step('file', 'bogus'), step('file', '-h'), step('file', 'rename', '--bogus'),
                    step('file', '--bogus', 'rename'), step('file', 'attach'), step('file', 'attach', 'k'), step('file', 'mv'),
                    step('file', 'mv', 'a'), step('file', 'rm', '-k'), step('file', 'detach', '--key'), step('file', 'check', '--pool'),
                    step('file', 'fix', '--move=1'), step('file', '-x', 'clean')])
case('usage-required', [step('rm'), step('edit'), step('export'), step('open'), step('doi'), step('fetch'), step('extract'),
                        step('file', 'attach'), step('file', 'mv'), step('add', '--format'), step('rm', '--yes'),
                        step('add', '--format', 'x'), step('add', '--mode', 'z'), step('check', '--on-duplicate', 'update'),
                        step('check', '--encoding', 'x'), step('list', '--format', 'x'), step('list', '--by', 'x'),
                        step('add', '--on-duplicate', 'x'), step('history', '-n', 'x'), step('extract', '-n', 'x'),
                        step('undo', '--steps', 'x'), step('restore', 'x'), step('restore', '--ref', 'x'), step('list', '--fuzzy-ratio', 'x'),
                        step('add', '--metadata', 'x'), step('add', '--metadata'), step('edit', 'k', '--set', 'novalue'),
                        step('edit', 'k', '--set', '=v'), step('tag', '--rename', 'a'), step('tag', '--rename'), step('tag', '--rename', 'a', 'b', 'c'),
                        step('add', '-a'), step('add', '--attach'), step('list', '--author'), step('list', '--detach'), step('list', '--export-files', 'x'),
                        step('export', 'd', '--files', 'x'), step('export', 'd', '--format'), step('rm', 'k', '--color', 'x'),
                        step('list', '--strict', '--fuzzy'), step('list', '--format', 'bibtex', '--plain'), step('list', '--plain', '-l'),
                        step('list', '-K', '--field', 'a'), step('list', '--field', 'a', '-f', 'b'), step('list', '-1', '-K')])
case('usage-unrecognized', [step('rm', 'a', '--bogus'), step('rm', '--bogus'), step('rm', '--bogus', '--bogus2'),
                            step('config', 'a', '--bogus', 'b'), step('add', '--bogus'), step('config', '-z', 'a'),
                            step('config', 'bibtex', '--global=1'), step('config', '-vy'), step('config', '--info'), step('init', '--git-lfs=1'),
                            step('config', 'a', 'b', '--bogus'), step('tag', 'a', '--bogus=3')])
case('usage-abbreviations', [step('config', '--verb', 'bibtex'), step('config', '--dr', 'x'),
                             step('config', '--p'), step('config', '--po'), step('config', '--un', 'files'),
                             step('config', '--glob'), step('config', '--g'), step('rm', '--d', 'k'), step('rm', '--delete', 'k')])
case('usage-interspersed', [step('config', 'bibtex', '--global', 'x'), step('config', '--global', 'default', 'x', '--dry-run'),
                            step('init', 'a', '--bibtex', 'x.bib', 'b'), step('config', '-y', 'files')])
case('usage-negative-numbers', [step('config', 'history-size', '-5'), step('config', 'history-size', '-x'),
                                step('init', '--name-author-num', '-1'), step('config', '--', 'history-size', '-5'), step('config', 'a', '-1.5')])
case('config-values-with-dashes', [step('init'), step('config', 'editor', '--', '-x'), step('config', 'name.template', '--', '-{author}'),
                                   step('config', 'editor', '-x'), step('config', 'editor')])
case('init-values-with-dashes', [step('init', '--editor', '-x'), step('init', '--editor=-x', 'a'), step('init', '--bibtex=-b.bib', 'b'),
                                 step('init', '--bibtex', '-5', 'c')])
case('usage-quoting', [step("bo'gus"), step('bo"gus'), step('bo\'g"us'), step('bog\nus'), step('config', 'new\nline', 'x'), step('init', '--paths', "it's")])


# running
# =======

# the library and its configuration (papers 3.0.0b6)
# ==================================================
def cfg(**kw):
    "a library configuration file, as json"
    return json.dumps({'version': 3, **kw}, sort_keys=True, indent=2)


# settings given are saved, even with the default value; --unset makes them inherited again
case('config-explicit-settings',
     [step('init', 'a', '--paths', 'relative', '--editor', 'vim', '--name-author-num', '2'),
      step('-C', 'a', 'config', 'files', 'move'), step('-C', 'a', 'config', 'auto-push', 'false'),
      step('-C', 'a', 'config', 'key.template', '{author}{year}'), step('-C', 'a', 'config', 'editor', 'vim'),
      step('-C', 'a', 'config'), step('-C', 'a', 'config', '--unset', 'files'),
      step('-C', 'a', 'config', '--unset', 'name.author-num'), step('-C', 'a', 'config', '--unset', 'paths'),
      step('-C', 'a', 'config', '--unset', 'editor'), step('-C', 'a', 'config', '--unset', 'bibtex'),
      step('-C', 'a', 'config', '--unset', 'filesdir'), step('-C', 'a', 'config', '--unset', 'pool'),
      step('-C', 'a', 'config'), step('-C', 'a', 'config', 'pool', 'none'), step('-C', 'a', 'config', '--prune')],
     home={MC: jd(version=3, editor='vim', files='move')})
case('config-explicit-in-file',
     [step('config'), step('config', 'name.title-sep', '-'), step('config', '--prune'), step('config', '--dry-run', '--prune'),
      step('config'), step('config', 'editor', 'vi')],
     cwd={'.papers/config.json': cfg(bibtex='x.bib', files='move', paths='relative', editor='vi', pool='none',
                                     nameformat={'author_num': 2, 'title_sep': '_'}, keyformat={'template': '{author}{year}'},
                                     **{'history-size': 500, 'auto-push': False})},
     home={MC: jd(version=3, keyformat={'template': '{author}{year}'}, editor='vi')})
# keys that papers does not know are kept when it saves the file
case('config-unknown-keys-kept',
     [step('config', 'files', 'link'), step('config', '--unset', 'files'), step('config', 'nameformat.x', 'y'),
      step('config', 'name.template', '{year}'), step('config', '--unset', 'name.template'), step('config', '--prune'),
      step('config', 'editor', 'vim'), step('config', '--unset', 'editor'), step('config')],
     cwd={'.papers/config.json': cfg(bibtex='x.bib', filesdir=None, future={'a': [1, 2], 'b': None}, other=3,
                                     nameformat={'template': '{title}', 'future-field': 7, 'author_num': 2},
                                     keyformat={'future_key': [1, 2]}, gitdir='x', gitlfs=True, git=True,
                                     **{'git-lfs': True, 'trash-days': 3}, pools=['x'], light=True, sync='push',
                                     local=True, absolute_paths=True, backup_files=True)})
case('config-unknown-keys-v2-library',
     [step('config', 'files', 'link'), step('config')],
     cwd={'.papersconfig.json': json.dumps({'bibtex': 'x.bib', 'local': True, 'future': 1, 'absolute_paths': True})})
# a wrong value in the file is an error that names the file and the setting
INVALID = [{'auto-push': 'no'}, {'auto-push': 1}, {'history-size': True}, {'history-size': 1.5}, {'history-size': '3'},
           {'files': 'keep'}, {'files': ['move']}, {'paths': 'relativ'}, {'paths': 1}, {'pool': 3}, {'pool': True},
           {'bibtex': 5}, {'bibtex': ['a.bib']}, {'filesdir': {}}, {'editor': ['vim']}, {'editor': 3},
           {'checksums': 'yes'}, {'history-separate': 0}, {'history-dir': 3}, {'keep-deleted-files': 'x'},
           {'nameformat': []}, {'nameformat': 'x'}, {'keyformat': 3}, {'nameformat': {'author_num': '2'}},
           {'nameformat': {'author_num': 2.5}}, {'nameformat': {'unknown_strict': 1}}, {'keyformat': {'template': 3}},
           {'keyformat': {'title_sep': None, 'author_sep': 4}}, {'auto-push': None, 'files': None}, {'version': 3}]
case('config-invalid-values',
     [step('config', write={'cwd/.papers/config.json': cfg(**{'bibtex': 'x.bib', **js})}) for js in INVALID]
     + [step('config', 'files', 'link', write={'cwd/.papers/config.json': cfg(bibtex='x.bib', **{'auto-push': 'no'})}),
        step('config', '--unset', 'files'), step('config', '--prune'), step('init', 'sub'), step('-C', 'sub', 'config')])
case('config-invalid-file',
     [step('config', write={'cwd/.papers/config.json': text})
      for text in ['{bad', '[1]', '"x"', 'null', '1', '', '{"version": 3,}', '﻿{"version": 3, "editor": "é"}',
                   '{"version": 3, "bibtex": "réf.bib"}']]
     + [step('config', 'editor', 'x', write={'cwd/.papers/config.json': '{bad'}), step('config', '--prune'),
        step('init', 'sub')])
case('config-global-invalid-values',
     [step('config', '--global', write={'home/<MC>': json.dumps(js)}) for js in
      [{'auto-push': 'no'}, {'default': 3}, {'history-size': True}, {'files': 'keep'}, {'nameformat': {'title_length': '10'}},
       {'keyformat': 1}, {'paths': 'x'}, {'editor': 1}, {'bibtex': 1}, {'filesdir': 2}, {'default': ['x']},
       {'history-dir': 1}, {'keep-deleted-files': 'no'}, {'pool': 3}, {'pool': 'none'}, {'pool': '/x'}, {'version': 3, 'default': '~/x'}]]
     + [step('-C', 'v', 'config', write={'home/<MC>': json.dumps({'editor': 4})}), step('config', '--global', 'editor', 'vim'),
        step('config', '--global', '--unset', 'editor'), step('init', 'w')],
     cwd={'v/.papers/config.json': cfg(bibtex='x.bib')})
case('config-global-invalid-file',
     [step('config', '--global', write={'home/<MC>': text}) for text in ['[1]', '"x"', '{bad', '', '1', '{"a": 1}']]
     + [step('config', '--global', 'editor', 'vim'), step('init', 'lib'), step('-C', 'v', 'config'), step('status')],
     cwd={'v/.papers/config.json': cfg(bibtex='x.bib')})
# the global configuration is written whole: its unknown keys and the pool names of earlier builds are kept
case('config-global-keeps-keys',
     [step('config', '--global', 'files', 'link'), step('config', '--global'), step('config', '--global', '--unset', 'files'),
      step('config', '--global', 'default', 'lib'), step('config', '--global', 'default', 'none'),
      step('-C', 'v', 'config', 'pool'), step('-C', 'w', 'config', 'pool')],
     home={MC: jd(version=3, future={'a': 1}, pools={'shared': '/x/shared', 'other': '../o'}, editor='vi')},
     cwd={'lib/.papers/config.json': cfg(bibtex='x.bib'), 'v/.papers/config.json': cfg(bibtex='x.bib', pools=['shared']),
          'w/.papers/config.json': cfg(bibtex='x.bib', pools=['default', 'shared'])})
case('config-global-unicode',
     [step('config', '--global', 'editor', 'édit \U0001F600'), step('config', '--global'),
      step('config', '--global', 'editor')],
     home={MC: '﻿' + jd(version=3, default='~/lib')})
# ~ in the paths of a library configuration; relative paths of the global configuration are relative to its folder
case('config-tilde-paths',
     [step('config'), step('config', 'bibtex'), step('config', 'filesdir'), step('config', 'files', 'link'),
      step('config', 'bibtex', '~/other.bib'), step('config', 'filesdir', '~/p'), step('config'),
      step('config', 'paths', 'absolute'), step('config')],
     cwd={'.papers/config.json': cfg(bibtex='~/docs/x.bib', filesdir='~/pdfs', pool='~/pool')})
case('config-global-relative-paths',
     [step('config', 'bibtex'), step('config', '-v'), step('config', '--global'), step('-C', 'sub', 'config', 'bibtex'),
      step('config', '--global', 'default', 'lib'), step('config', '--global', 'default'),
      step('config', '--global', 'history-dir', 'hist'), step('config', '--global', 'history-dir', '~/h'),
      step('-C', 'sub', 'config', '--global', 'history-dir', '../h2'), step('config', '--global', 'history-dir'),
      step('config', '--global', 'default', '../cwd/lib'), step('config', '--global'),
      step('-C', 'sub', 'config', '--global', 'default', '../lib'), step('-C', 'sub', 'config', '--global', 'default'),
      step('init', 'x', '--shallow'), step('-C', 'x', 'config', 'pool'), step('-C', 'x', 'config')],
     home={MC: jd(version=3, default='../../../../cwd/lib', **{'history-dir': 'hh'})},
     cwd={'lib/.papers/config.json': cfg(bibtex='x.bib', filesdir='files'), 'sub/': None}, platform='macos')
case('config-global-default-home',
     [step('config', 'bibtex'), step('config', '--global', 'default', '~/lib'), step('config', '--global', 'default'),
      step('config', '--global', 'default', '~/lib/../lib2'), step('config', '--global')],
     home={'lib/.papers/config.json': cfg(bibtex='x.bib'), 'lib2/.papers/config.json': cfg(bibtex='y.bib'),
           MC: jd(version=3, default='~/lib')})
# paths on the command line are relative to the current directory
case('init-relative-to-the-current-directory',
     [step('init', 'lib', '--bibtex', 'refs/my.bib', '--filesdir', 'docs/pdfs'),
      step('init', 'lib2', '--bibtex', '../x.bib', '--filesdir', '../pdfs'), step('-C', 'lib2', 'config'),
      step('init', 'sub/lib3', '--bibtex', 'sub/x.bib', '--filesdir', 'sub/pdfs', '--pool', 'lib'),
      step('-C', 'sub/lib3', 'config'), step('init', 'lib4', '--pool', '../lib'), step('init', 'sub/lib5', '--pool', '../../lib'),
      step('init', 'sub/lib6', '--pool', 'lib'), step('-C', 'sub/lib6', 'config', 'pool'),
      step('init', 'lib7', '--pool', 'sub/..//lib'), step('-C', 'lib7', 'config', 'pool'),
      step('init', 'sub/lib8', '--pool', '~/pool', '--bibtex', '~/b.bib'), step('-C', 'sub/lib8', 'config'),
      step('init', 'sub/lib9', '--pool', 'lib/'), step('-C', 'sub/lib9', 'config', 'pool'),
      step('init', 'lib10', '--pool', 'lib/files/../../lib'), step('-C', 'lib10', 'config', 'pool'),
      step('init', 'lib/inside', '--pool', 'lib'), step('init', 'lib11', '--pool', 'lib2', '--bibtex', 'none', '--filesdir', 'NONE'),
      step('-C', 'lib11', 'config')],
     cwd={'sub/': None}, home={'pool/.papers/config.json': cfg(bibtex='p.bib')})
case('init-pool-itself',
     [step('init', 'a'), step('init', 'a2', '--pool', 'a2'), step('init', 'b', '--pool', '../b'), step('init', 'c', '--pool', 'c/../c'),
      step('init', 'd', '--pool', '.'), step('init', 'e', '--pool', 'e'), step('init', 'f', '--shallow'),
      step('-C', 'a', 'config', 'pool', '.'), step('-C', 'a', 'config', 'pool', '../a'), step('-C', 'a', 'config', 'pool', 'a'),
      step('-C', 'a', 'config', 'pool', '../a/'), step('-C', 'a', 'config', 'pool', '/<TMP>/cwd/a'.replace('//', '/')),
      step('-C', 'a', 'config', 'pool', '~/a'), step('-C', 'a', 'config'), step('config', '--global', 'default', 'a'),
      step('init', 'g', '--shallow'), step('init', 'a', '--shallow'), step('-C', 'a', 'config', 'pool', '../a', '--dry-run')],
     cwd={'e/': None}, home={})
case('init-shallow-default-written',
     [step('init', 'lib', '--shallow'), step('-C', 'lib', 'config', 'pool'), step('init', 'sub/lib2', '--shallow'),
      step('-C', 'sub/lib2', 'config', 'pool'), step('init', 'lib3', '--shallow', '--bibtex', 'z.bib')],
     home={MC: jd(version=3, default='~/def'), 'def/.papers/config.json': cfg(bibtex='d.bib')}, cwd={'sub/': None})
case('init-shallow-default-relative',
     [step('init', 'lib', '--shallow'), step('-C', 'lib', 'config', 'pool'), step('init', 'lib2', '--shallow', '--pool', 'x'),
      step('init', 'lib', '--shallow'), step('config', '--global')],
     home={MC: jd(version=3, default='../../../../cwd/def')}, cwd={'def/.papers/config.json': cfg(bibtex='d.bib')},
     platform='macos')
case('init-stores-paths',
     [step('init', 'a'), step('init', 'b', '--paths', 'relative'), step('init', 'c', '--paths', 'absolute'),
      step('init', 'd', '--editor', 'vim'), step('init', 'e', '--name-author-num', '2'), step('init', 'f', '--key-unknown-strict'),
      step('-C', 'a', 'config', '--unset', 'paths'), step('-C', 'a', 'config')],
     home={MC: jd(version=3, paths='absolute')})
case('init-stores-paths-default',
     [step('init', 'a'), step('-C', 'a', 'config', 'paths'), step('-C', 'a', 'config', '--prune'), step('-C', 'a', 'config')])
# all of .papers/ is in the library folder, also when the bibtex is in a subfolder
BIB_DISTINCT = '{"distinct": []}\n'
case('layout-bibtex-in-subfolder',
     [step('-C', 'a', 'config', '-v'), step('-C', 'a', 'config', '-v'), step('-C', 'b', 'config', '-v'), step('-C', 'c', 'config', '-v'),
      step('-C', 'd', 'config', '-v'), step('-C', 'e/lib', 'config', '-v'), step('-C', 'f', 'config', '-v'),
      step('-C', 'g', 'config', '-v'), step('-C', 'h', 'config', '-v', '--dry-run'), step('-C', 'h', 'config', '-v'),
      step('-C', 'i', 'config', '-v')],
     cwd={# moved: distinct.json, origin/ (a folder) and origin.bib; the other files stay
          'a/.papers/config.json': cfg(bibtex='bib/papers.bib'), 'a/bib/.papers/distinct.json': BIB_DISTINCT,
          'a/bib/.papers/origin/pool-123456.bib': '@comment{papers-pool ../pool}\n', 'a/bib/.papers/origin.bib': '@comment{x}\n',
          'a/bib/.papers/files.json': '{}', 'a/bib/.papers/lock': 'x', 'a/bib/.papers/other.txt': 'x',
          # not moved over a file of the library folder
          'b/.papers/config.json': cfg(bibtex='s/papers.bib'), 'b/.papers/distinct.json': '{"distinct": [[1, 2]]}\n',
          'b/s/.papers/distinct.json': BIB_DISTINCT,
          # the folder of the bibtex is another library: left alone
          'c/.papers/config.json': cfg(bibtex='../o/papers.bib'), 'o/.papers/config.json': cfg(bibtex='papers.bib'),
          'o/.papers/distinct.json': BIB_DISTINCT,
          # a bibtex in the library folder
          'd/.papers/config.json': cfg(bibtex='papers.bib'), 'd/.papers/distinct.json': BIB_DISTINCT,
          # a bibtex above the library folder
          'e/lib/.papers/config.json': cfg(bibtex='../papers.bib'), 'e/.papers/distinct.json': BIB_DISTINCT,
          'e/.papers/origin/a.bib': '@comment{papers-pool x}\n',
          # a library configuration of papers 2
          'f/.papersconfig.json': json.dumps({'bibtex': 'sub/x.bib', 'local': True}), 'f/sub/.papers/distinct.json': BIB_DISTINCT,
          'g/.papers/config.json': cfg(bibtex='nosub/x.bib'),
          # a dry run moves nothing
          'h/.papers/config.json': cfg(bibtex='hb/x.bib'), 'h/hb/.papers/distinct.json': BIB_DISTINCT,
          'h/hb/.papers/origin.bib': '@comment{x}\n',
          # a bibtex given with ~
          'i/.papers/config.json': cfg(bibtex='~/ib/x.bib'), 'i/.papers/origin.bib': '@comment{y}\n',
          'i/.papers/distinct.json': BIB_DISTINCT}, home={'ib/.papers/distinct.json': BIB_DISTINCT, 'ib/.papers/origin/z.bib': ''})
case('layout-dry-run-moves-nothing',
     [step('list', '--dry-run'), step('config', '--dry-run'), step('config'), step('config', '--dry-run', 'files', 'link')],
     cwd={'.papers/config.json': cfg(bibtex='bib/papers.bib'), 'bib/.papers/distinct.json': BIB_DISTINCT})
# the pool is checked again for a pool of its own when the library exchanges with it
case('pool-with-a-pool-of-its-own',
     [step('-C', 'B', 'status'), step('-C', 'B', 'pull'), step('-C', 'B', 'pull', '--all'), step('-C', 'B', 'pull', '--check'),
      step('-C', 'B', 'push'), step('-C', 'B', 'push', '--all'), step('-C', 'B', 'sync'), step('-C', 'B', 'sync', '--all'),
      step('-C', 'B', 'sync', '--pool', '../A'), step('-C', 'B', 'pull', '--pool', '../C'), step('-C', 'B', 'push', '--pool', '../C'),
      step('-C', 'B', 'pull', '--pool', '../B'), step('-C', 'B', 'config', 'pool', '../A', '--dry-run'), step('-C', 'D', 'config', 'pool', '../A'),
      step('-C', 'B', 'list', '--pull', '--dry-run'), step('-C', 'A', 'pull', '--check'), step('-C', 'A', 'status'),
      step('-C', 'D', 'status')],
     cwd={'C/.papers/config.json': cfg(bibtex='c.bib', filesdir='files', pool='none'), 'C/c.bib': '',
          'A/.papers/config.json': cfg(bibtex='a.bib', filesdir='files', pool='../C'), 'A/a.bib': '',
          'B/.papers/config.json': cfg(bibtex='b.bib', filesdir='files', pool='../A'),
          'B/b.bib': '@article{x2000,\n author = {X, Y},\n title = {T},\n year = {2000}\n}\n',
          'D/.papers/config.json': cfg(bibtex='d.bib', filesdir='files', pool='../B'), 'D/d.bib': ''})


def normalise(text, tmp):
    mc = os.path.join(tmp, 'home', MC)
    v2 = os.path.join(tmp, 'home', V2)
    text = text.replace(mc, '<MC>').replace('~/' + MC, '<MC~>').replace(v2, '<V2>').replace('~/' + V2, '<V2~>')
    return text.replace(tmp, '<TMP>')


def expand(text, tmp):
    return text.replace('<TMP>', tmp)


def key_of(path):
    "the tree key of a path under the case directory"
    if path == 'home/' + MC:
        return 'home/<MC>'
    if path == 'home/' + V2:
        return 'home/<V2>'
    return path


def real_of(key):
    if key == 'home/<MC>':
        return 'home/' + MC
    if key == 'home/<V2>':
        return 'home/' + V2
    return key


def snapshot(tmp):
    tree = {}
    for base, dirs, files in os.walk(tmp):
        dirs.sort()
        for name in sorted(files):
            full = os.path.join(base, name)
            rel = os.path.relpath(full, tmp)
            if any(rel.startswith(p) for p in IGNORED) or rel.endswith('.papers/lock'):
                continue
            if os.path.islink(full):
                tree[key_of(rel)] = '-> ' + os.readlink(full)
                continue
            try:
                with open(full, encoding='utf-8') as f:
                    tree[key_of(rel)] = normalise(f.read(), tmp)
            except UnicodeDecodeError:
                tree[key_of(rel)] = '<binary>'
        for name in dirs:
            full = os.path.join(base, name)
            rel = os.path.relpath(full, tmp)
            if os.path.islink(full):
                tree[key_of(rel)] = '-> ' + os.readlink(full)
            elif not os.listdir(full) and rel not in ('home', 'cwd') and not any(rel.startswith(p.rstrip('/')) for p in IGNORED):
                tree[key_of(rel) + '/'] = None
    return tree


def setup(tmp, case):
    os.makedirs(os.path.join(tmp, 'home'))
    os.makedirs(os.path.join(tmp, 'cwd'))
    for rel, content in case['home'].items():
        write(os.path.join(tmp, 'home', rel), content, tmp)
    for rel, content in case['cwd'].items():
        write(os.path.join(tmp, 'cwd', rel), content, tmp)


def write(path, content, tmp):
    if isinstance(content, (list, tuple)):  # a symbolic link
        os.symlink(content[1], path)
        return
    if path.endswith('/') or content is None:
        os.makedirs(path, exist_ok=True)
        return
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w', encoding='utf-8') as f:
        f.write(expand(content, tmp))


def run_case(case):
    tmp = os.path.realpath(tempfile.mkdtemp(prefix='papers-case-'))
    try:
        setup(tmp, case)
        steps = []
        for st in case['steps']:
            for rel, content in st['write'].items():
                write(os.path.join(tmp, real_of(rel)), content, tmp)
            for rel in st['rm']:
                os.remove(os.path.join(tmp, real_of(rel)))
            args = [expand(a, tmp) for a in st['args']]
            env = {'PATH': os.environ['PATH'], 'HOME': os.path.join(tmp, 'home'), 'LC_ALL': 'C.UTF-8', **st['env']}
            result = subprocess.run([PAPERS, *args], cwd=os.path.join(tmp, 'cwd', st['cwd']), env=env,
                                    capture_output=True, text=True, stdin=subprocess.DEVNULL)
            out = dict(st)
            out.update(rc=result.returncode, stdout=normalise(result.stdout, tmp), stderr=normalise(result.stderr, tmp),
                       tree=snapshot(tmp))
            assert 'Traceback' not in result.stderr, (case['name'], args, result.stderr)
            steps.append(out)
        home = {('<MC>' if k == MC else '<V2>' if k == V2 else k): v for k, v in case['home'].items()}
        return dict(name=case['name'], home=home, cwd={k: (list(v) if isinstance(v, tuple) else v) for k, v in case['cwd'].items()},
                    platform=case['platform'], steps=steps)
    finally:
        shutil.rmtree(tmp)


def main():
    os.makedirs(OUT, exist_ok=True)
    for case in CASES:
        if ONLY and case['name'] not in ONLY:
            continue
        with open(os.path.join(OUT, case['name'] + '.json'), 'w', encoding='utf-8') as f:
            json.dump(run_case(case), f, indent=1, ensure_ascii=False, sort_keys=False)
            f.write('\n')
        print(case['name'])


main()
