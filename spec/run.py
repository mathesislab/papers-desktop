#!/usr/bin/env python3
"""Run an implementation of papers on the cases and compare it with the recorded results.

    spec/run.py --papers CMD [--contract-only] [--section PATTERN] [CASE ...]

CMD is the implementation: `papers`, `target/debug/papers-rs`... (a program name, or a path, with
optional arguments). CASE is a glob on the name of the case (group/name), all cases by default.
Each case runs in a fresh sandbox (a temporary HOME and working folder). Exit status 0 when all pass.

By default every output is compared byte for byte. With --contract-only only what the specification
makes Contract is: the exit codes, the files after, and the outputs of the steps that are Contract
(see README.md, "Contract and Free"); a difference in a Free output is reported as a note, not a failure.
--section PATTERN keeps the cases that exercise a section of the specification (a glob on its
reference, or a section above it: `formats.md#7` finds the cases of `formats.md#7.2`); it can be repeated.
"""
import argparse
import sys

import conformance as c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--papers', help='the implementation to test')
    ap.add_argument('--with', dest='have', default='', help='comma-separated capabilities available (e.g. network)')
    ap.add_argument('--contract-only', action='store_true',
                    help='compare the Contract parts only; differences in Free outputs are notes')
    ap.add_argument('--section', action='append', default=[], metavar='PATTERN',
                    help='only the cases of this section of the specification (e.g. formats.md#7)')
    ap.add_argument('-a', '--all', action='store_true', help='show every difference, not only the first')
    ap.add_argument('-q', '--quiet', action='store_true', help='only the failures and the summary')
    ap.add_argument('--keep', action='store_true', help='keep the sandboxes (their paths are printed)')
    ap.add_argument('--list', action='store_true', help='list the cases (with their sections) and stop')
    ap.add_argument('--sections', action='store_true', help='list the sections and their cases and stop')
    ap.add_argument('cases', nargs='*', help='glob(s) of case names (default: all)')
    o = ap.parse_args()
    cases = c.find_cases(o.cases, o.section)
    if o.list:
        for case in cases:
            print(case.name + (f'  (needs {", ".join(case.requires)})' if case.requires else '')
                  + '  [' + ', '.join(case.sections) + ']')
        return
    if o.sections:
        by_section = {}
        for case in cases:
            for ref in case.sections:
                by_section.setdefault(ref, []).append(case.name)
        for ref in sorted(by_section):
            print(f'{ref}: {", ".join(by_section[ref])}')
        return
    if not cases:
        sys.exit('no case found')
    if not o.papers:
        ap.error('--papers is required')
    papers = c.resolve_papers(o.papers)
    have = set(filter(None, o.have.split(',')))
    passed = failed = skipped = noted = 0
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
        diff = c.differences(expected, actual, case, first_only=not o.all, contract_only=o.contract_only)
        failures = [d for d in diff if not (d[2] and o.contract_only)]
        notes = [d for d in diff if d[2] and o.contract_only]
        if notes:
            noted += 1
        if not failures:
            passed += 1
            if not o.quiet:
                print(f'PASS {case.name}' + (f' (Free differences: {", ".join(n[0] for n in notes)})' if notes else ''))
            if notes and o.all:
                for label, text, _ in notes:
                    print(f'  -- note, Free: {label}')
                    print(''.join('     ' + line for line in text.splitlines(True)), end='')
            continue
        failed += 1
        print(f'FAIL {case.name}')
        for label, text, free in diff:
            print(f'  -- {label}' + (' (Free: a note)' if free and o.contract_only else ''))
            print(''.join('     ' + line for line in text.splitlines(True)), end='')
    print(f'{passed} passed, {failed} failed' + (f', {skipped} skipped' if skipped else '')
          + f' of {len(cases)} cases' + (f' ({noted} with differences in Free outputs)' if o.contract_only else ''))
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
