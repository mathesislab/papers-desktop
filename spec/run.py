#!/usr/bin/env python3
"""Run an implementation of papers on the cases and compare it with the recorded results.

    spec/run.py --papers CMD [CASE ...]

CMD is the implementation: `papers`, `target/debug/papers-rs`... (a program name, or a path, with
optional arguments). CASE is a glob on the name of the case (group/name), all cases by default.
Each case runs in a fresh sandbox (a temporary HOME and working folder). Exit status 0 when all pass.
"""
import argparse
import sys

import conformance as c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--papers', help='the implementation to test')
    ap.add_argument('--with', dest='have', default='', help='comma-separated capabilities available (e.g. network)')
    ap.add_argument('-a', '--all', action='store_true', help='show every difference, not only the first')
    ap.add_argument('-q', '--quiet', action='store_true', help='only the failures and the summary')
    ap.add_argument('--keep', action='store_true', help='keep the sandboxes (their paths are printed)')
    ap.add_argument('--list', action='store_true', help='list the cases and stop')
    ap.add_argument('cases', nargs='*', help='glob(s) of case names (default: all)')
    o = ap.parse_args()
    cases = c.find_cases(o.cases)
    if o.list:
        for case in cases:
            print(case.name + (f'  (needs {", ".join(case.requires)})' if case.requires else ''))
        return
    if not cases:
        sys.exit('no case found')
    if not o.papers:
        ap.error('--papers is required')
    papers = c.resolve_papers(o.papers)
    have = set(filter(None, o.have.split(',')))
    passed = failed = skipped = 0
    for case in cases:
        missing = set(case.requires) - have
        if missing:
            skipped += 1
            if not o.quiet:
                print(f'SKIP {case.name} (needs {", ".join(sorted(missing))})')
            continue
        expected = c.read_expected(case)
        if expected is None:
            failed += 1
            print(f'FAIL {case.name}: not recorded (spec/record.py)')
            continue
        actual = c.run_case(case, papers, keep=o.keep)
        diff = c.differences(expected, actual, first_only=not o.all)
        if not diff:
            passed += 1
            if not o.quiet:
                print(f'PASS {case.name}')
            continue
        failed += 1
        print(f'FAIL {case.name}')
        for label, text in diff:
            print(f'  -- {label}')
            print(''.join('     ' + line for line in text.splitlines(True)), end='')
    print(f'{passed} passed, {failed} failed' + (f', {skipped} skipped' if skipped else '')
          + f' of {len(cases)} cases')
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
