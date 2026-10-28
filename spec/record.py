#!/usr/bin/env python3
"""Record the expected results of the cases by running the reference implementation (papers-cli).

    spec/record.py [--papers CMD] [CASE ...]

Each case runs twice, in two fresh sandboxes (a temporary HOME and working folder, so the machine's
own configuration and libraries are never touched); it is recorded only if the two runs agree.
CASE is a glob on the name of the case (group/name), all cases by default.
"""
import argparse
import sys

import conformance as c


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--papers', default='papers', help='the reference command (default: papers)')
    ap.add_argument('--with', dest='have', default='', help='comma-separated capabilities available (e.g. network)')
    ap.add_argument('cases', nargs='*', help='glob(s) of case names (default: all)')
    o = ap.parse_args()
    papers = c.resolve_papers(o.papers)
    have = set(filter(None, o.have.split(',')))
    cases = c.find_cases(o.cases)
    if not cases:
        sys.exit('no case found')
    failed = 0
    for case in cases:
        if set(case.requires) - have:
            print(f'skip   {case.name} (needs {", ".join(sorted(set(case.requires) - have))})')
            continue
        first = c.run_case(case, papers)
        second = c.run_case(case, papers)
        diff = c.differences(first, second)
        if diff:
            failed += 1
            print(f'NOT DETERMINISTIC {case.name}: {diff[0][0]}\n{diff[0][1]}', file=sys.stderr)
            continue
        c.write_expected(case, first)
        print(f'record {case.name}')
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
