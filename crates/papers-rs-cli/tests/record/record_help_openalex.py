#!/usr/bin/env python3
"""Make the help of `add`, `fetch` and `extract` of papers-rs, which have `--openalex` where
papers has `--scholar` (an extension of papers-rs, not in papers), into
`tests/fixtures/help-openalex/`: papers' own parser (argparse) with the option added after
`--scholar`, so that the wrapping of the usage and of the help is argparse's. The installed papers
is not changed (a copy is patched).

    python3 record_help_openalex.py [PYTHON]

PYTHON is the interpreter that has papers installed (default: this one). The help is that of a run
outside of any library, as in `record_help.py`.
"""
import os
import re
import shutil
import subprocess
import sys
import tempfile

PYTHON = sys.argv[1] if len(sys.argv) > 1 else sys.executable
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, '..', 'fixtures', 'help-openalex')
ENV = '(environment: PAPERS_MAILTO, OPENALEX_API_KEY)'
HELP = {
    'grp': f'(papers-rs) use OpenAlex instead of crossref for the full-text search {ENV}',
    'fetchp': f'(papers-rs) use OpenAlex instead of default crossref for fulltext search {ENV}',
    'extractp': f'(papers-rs) use OpenAlex instead of default crossref for fulltext search {ENV}',
}


def papers_dir():
    out = subprocess.run([PYTHON, '-c', 'import papers, os; print(os.path.dirname(papers.__file__))'],
                         capture_output=True, text=True, check=True)
    return out.stdout.strip()


def main():
    os.makedirs(OUT, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        shutil.copytree(papers_dir(), os.path.join(tmp, 'papers'))
        parser = os.path.join(tmp, 'papers', 'cli', 'parser.py')
        lines = []
        with open(parser) as f:
            for line in f:
                lines.append(line)
                m = re.match(r"(\s*)(grp|fetchp|extractp)\.add_argument\('--scholar'", line)
                if m:
                    lines.append(f"{m.group(1)}{m.group(2)}.add_argument('--openalex', action='store_true', "
                                 f"help={HELP[m.group(2)]!r})\n")
        with open(parser, 'w') as f:
            f.writelines(lines)
        runner = os.path.join(tmp, 'run.py')
        with open(runner, 'w') as f:
            f.write(f"import sys\nsys.path.insert(0, {tmp!r})\nsys.argv[0] = 'papers'\n"
                    "from papers.__main__ import main_clean_exit\nmain_clean_exit()\n")
        for command in ['add', 'fetch', 'extract']:
            with tempfile.TemporaryDirectory() as cwd:
                env = {'PATH': os.environ['PATH'], 'HOME': '/home/papers-help', 'LC_ALL': 'C.UTF-8'}
                result = subprocess.run([PYTHON, runner, command, '-h'], cwd=cwd, env=env,
                                        capture_output=True, text=True)
                assert result.returncode == 0 and not result.stderr, result
                with open(os.path.join(OUT, command + '.txt'), 'w') as f:
                    f.write(result.stdout)


if __name__ == '__main__':
    main()
