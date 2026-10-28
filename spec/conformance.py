"""Shared by record.py and run.py: the case format, the sandbox, the normalization.

Python standard library only. See README.md for the format.
"""
import difflib
import fnmatch
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
CASES = os.path.join(HERE, 'cases')

EMPTY_DIR = '.emptydir'          # marker file of an empty directory in a stored tree
STORED = {'.gitignore': '.gitignore.spec'}   # files git would act on, stored under another name
TIMEOUT = 120

TIME_IN_TEXT = re.compile(r'\(\d{4}-\d\d-\d\d \d\d:\d\d\)')
TIME_IN_JSON = re.compile(r'("time":\s*)[0-9.eE+-]+')
PLACEHOLDERS = ('$WORK', '$HOME', '$CONFIG', '$DATA', '$CACHE')


# platform directories (what platformdirs gives papers-cli)
# ===========================================================

def platform_dirs(home):
    "{'config', 'data', 'cache'}: the papers directories of the user, for HOME=home"
    if sys.platform == 'darwin':
        support = os.path.join(home, 'Library', 'Application Support', 'papers')
        return {'config': support, 'data': support,
                'cache': os.path.join(home, 'Library', 'Caches', 'papers')}
    if sys.platform.startswith('linux'):
        return {'config': os.path.join(home, '.config', 'papers'),
                'data': os.path.join(home, '.local', 'share', 'papers'),
                'cache': os.path.join(home, '.cache', 'papers')}
    raise SystemExit(f'unsupported platform: {sys.platform}')


# cases
# =====

class Case:
    def __init__(self, path):
        self.path = path
        self.name = os.path.relpath(path, CASES).replace(os.sep, '/')
        with open(os.path.join(path, 'case.json'), encoding='utf-8') as f:
            spec = json.load(f)
        self.description = spec.get('description', '')
        self.requires = spec.get('requires', [])
        self.steps = spec['steps']
        self.before = os.path.join(path, 'before')
        self.before_home = os.path.join(path, 'before-home')
        self.expected = os.path.join(path, 'expected')


def find_cases(selectors=()):
    "the cases of spec/cases whose name (group/name) matches one of the glob selectors (all by default)"
    found = []
    for root, dirs, files in os.walk(CASES):
        dirs.sort()
        if 'case.json' in files:
            dirs[:] = []
            found.append(Case(root))
    found.sort(key=lambda c: c.name)
    if selectors:
        found = [c for c in found
                 if any(fnmatch.fnmatch(c.name, s) or c.name.startswith(s.rstrip('/') + '/') for s in selectors)]
    return found


# trees
# =====

def _read_tree(root):
    "{path: bytes} of the files under root ('/' separators); an empty directory is its EMPTY_DIR marker"
    tree = {}
    if not os.path.isdir(root):
        return tree
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames.sort()
        rel = os.path.relpath(dirpath, root)
        rel = '' if rel == '.' else rel.replace(os.sep, '/') + '/'
        if not dirnames and not filenames and rel:
            tree[rel + EMPTY_DIR] = b''
        for name in filenames:
            if name == '.DS_Store':
                continue
            with open(os.path.join(dirpath, name), 'rb') as f:
                tree[rel + name] = f.read()
    return tree


def _stored_name(path):
    head, _, name = path.rpartition('/')
    name = STORED.get(name, name)
    return head + '/' + name if head else name


def _logical_name(path):
    head, _, name = path.rpartition('/')
    name = {v: k for k, v in STORED.items()}.get(name, name)
    return head + '/' + name if head else name


def _write_tree(root, tree, stored=True):
    "write {path: bytes} under root; the markers make empty directories"
    os.makedirs(root, exist_ok=True)
    for path, data in sorted(tree.items()):
        target = os.path.join(root, *(_stored_name(path) if stored else path).split('/'))
        if path.rsplit('/', 1)[-1] == EMPTY_DIR:
            os.makedirs(os.path.dirname(target), exist_ok=True)
            if stored:
                with open(target, 'wb') as f:   # the marker is kept: git cannot track a directory
                    pass
            continue
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with open(target, 'wb') as f:
            f.write(data)


def _load_stored(root):
    "the logical tree of a stored tree (before/, expected/work/...)"
    return {_logical_name(p): d for p, d in _read_tree(root).items()}


def _remove_dir_contents(path):
    if os.path.isdir(path):
        shutil.rmtree(path)


# the sandbox
# ===========

class Sandbox:
    """a temporary directory with `work/` (the case's before/) and `home/` (HOME of the run)"""

    def __init__(self, case, keep=False):
        self.case = case
        self.keep = keep
        self.tmp = tempfile.mkdtemp(prefix='papers-spec-')
        self.root = os.path.realpath(self.tmp)
        self.work = os.path.join(self.root, 'work')
        self.home = os.path.join(self.root, 'home')
        self.dirs = platform_dirs(self.home)
        self.version = None
        os.makedirs(self.work)
        os.makedirs(self.home)
        self._subs = self._build_subs()
        self.projects = {}

    def close(self):
        if not self.keep:
            shutil.rmtree(self.tmp, ignore_errors=True)

    # placeholders
    # ------------

    def _build_subs(self):
        d = self.dirs
        pairs = []   # (needle, placeholder), the first wins for equal needles
        named = [(os.path.join(d['data'], 'history'), '$DATA/history'),
                 (d['config'], '$CONFIG'), (d['data'], '$DATA'), (d['cache'], '$CACHE'),
                 (self.work, '$WORK'), (self.home, '$HOME')]
        for path, ph in named:
            pairs.append((path, ph))
            if path.startswith(self.home + os.sep):   # as papers shows it
                pairs.append(('~' + path[len(self.home):], ph))
        pairs.append((self.root, '$TMP'))
        # the same with the unresolved name of the temporary directory
        if self.tmp != self.root:
            for needle, ph in list(pairs):
                if needle.startswith(self.root):
                    pairs.append((self.tmp + needle[len(self.root):], ph))
        subs = {}
        for needle, ph in pairs:
            subs.setdefault(needle, ph)
        return sorted(subs.items(), key=lambda kv: -len(kv[0]))

    def normalize(self, text):
        for needle, ph in self._subs:
            text = text.replace(needle, ph)
        if self.version:
            text = text.replace(self.version, '$VERSION')
        return text

    def expand(self, text):
        "the placeholders of a case file or argument, as paths of this sandbox"
        d = self.dirs
        for ph, path in (('$WORK', self.work), ('$HOME', self.home), ('$CONFIG', d['config']),
                         ('$DATA', d['data']), ('$CACHE', d['cache'])):
            text = text.replace(ph, path)
        return text

    def _normalize_bytes(self, data, time_json=False):
        try:
            text = data.decode('utf-8')
        except UnicodeDecodeError:
            return data
        text = self.normalize(text)
        for pid, name in self.projects.items():
            text = text.replace(pid, name)
        if time_json:
            text = TIME_IN_JSON.sub(r'\1"$TIME"', text)
        return text.encode('utf-8')

    def _expand_bytes(self, data):
        try:
            text = data.decode('utf-8')
        except UnicodeDecodeError:
            return data
        return self.expand(text).encode('utf-8')

    # before
    # ------

    def populate(self):
        "write before/ to work/ and before-home/ to home/"
        tree = {p: self._expand_bytes(d) for p, d in _load_stored(self.case.before).items()}
        _write_tree(self.work, tree, stored=False)
        d = self.dirs
        roots = {'config': d['config'], 'data': d['data'], 'home': self.home}
        for path, data in _load_stored(self.case.before_home).items():
            top, _, rest = path.partition('/')
            if top not in roots:
                raise SystemExit(f'{self.case.name}: before-home/{path}: the top folders are config, data and home')
            target = os.path.join(roots[top], *rest.split('/'))
            os.makedirs(os.path.dirname(target), exist_ok=True)
            if rest.rsplit('/', 1)[-1] != EMPTY_DIR:
                with open(target, 'wb') as f:
                    f.write(self._expand_bytes(data))

    # running
    # -------

    def env(self):
        return {'PATH': os.environ.get('PATH', ''), 'HOME': self.home, 'LANG': 'C.UTF-8', 'LC_ALL': 'C.UTF-8',
                'TZ': 'UTC', 'COLUMNS': '80', 'LINES': '24', 'NO_COLOR': '1', 'TERM': 'dumb'}

    def run_step(self, papers, step):
        cwd = os.path.normpath(os.path.join(self.work, step.get('cwd', '.')))
        argv = [self.expand(a) for a in step['argv']]
        if 'stdin_file' in step:
            with open(os.path.join(self.case.path, step['stdin_file']), 'rb') as f:
                stdin = self._expand_bytes(f.read())
        elif 'stdin' in step:
            stdin = self.expand(step['stdin']).encode('utf-8')
        else:
            stdin = None
        if not os.path.isdir(cwd):
            return {'stdout': '', 'stderr': f'spec: the directory {step.get("cwd")} of the step does not exist\n', 'exit': 125}
        try:
            p = subprocess.run(papers + argv, cwd=cwd, env=self.env(), input=stdin, capture_output=True,
                               stdin=None if stdin is not None else subprocess.DEVNULL, timeout=TIMEOUT)
        except subprocess.TimeoutExpired:
            return {'stdout': '', 'stderr': f'spec: timeout after {TIMEOUT} s\n', 'exit': 124}
        # the first stdin, the encoding of the text: utf-8, undecodable bytes kept
        return {'stdout': p.stdout.decode('utf-8', 'surrogateescape'),
                'stderr': p.stderr.decode('utf-8', 'surrogateescape'), 'exit': p.returncode}

    def normalize_steps(self, steps):
        "the outputs of the steps, normalized (after snapshot(): it knows the identifiers of the projects)"
        for step in steps:
            for stream in ('stdout', 'stderr'):
                text = self.normalize(step[stream])
                for pid, name in self.projects.items():
                    text = text.replace(pid, name)
                step[stream] = TIME_IN_TEXT.sub('($TIME)', text)

    def detect_version(self, papers):
        "the version the implementation reports: its output is replaced by $VERSION"
        try:
            p = subprocess.run(papers + ['--version'], cwd=self.work, env=self.env(), capture_output=True,
                               stdin=subprocess.DEVNULL, timeout=TIMEOUT)
        except (OSError, subprocess.SubprocessError):
            return
        text = p.stdout.decode('utf-8', 'replace').strip()
        if p.returncode == 0 and text:
            self.version = text

    # after
    # -----

    def snapshot(self):
        "the normalized trees after the run: {'work': {...}, 'home': {...}}"
        raw = _read_tree(self.home)
        d = self.dirs
        history = os.path.relpath(d['data'], self.home).replace(os.sep, '/') + '/history/'
        self.projects = self._project_names(raw, history)
        work = {p: self._normalize_bytes(data) for p, data in _read_tree(self.work).items()}
        return {'work': work, 'home': self._snapshot_home(raw)}

    def _snapshot_home(self, raw):
        d = self.dirs
        cache = os.path.relpath(d['cache'], self.home).replace(os.sep, '/') + '/'
        config = os.path.relpath(d['config'], self.home).replace(os.sep, '/') + '/'
        data = os.path.relpath(d['data'], self.home).replace(os.sep, '/') + '/'
        history = data + 'history/'
        projects = self.projects
        tree = {}
        for path, content in raw.items():
            if path.startswith(cache):
                continue   # the parse cache: private to each implementation
            if path.startswith(history):
                rel = 'data/history/' + path[len(history):]
                parts = rel.split('/')
                if len(parts) > 3 and parts[2] == 'projects':
                    parts[3] = projects.get(parts[3], parts[3])
                    rel = '/'.join(parts)
                    content = self._normalize_bytes(content, time_json=True)
                elif len(parts) > 3 and parts[2] == 'objects':
                    try:
                        content = zlib.decompress(content)   # the compressor is private; the text is shared
                    except zlib.error:
                        pass
                    content = self._normalize_bytes(content)
                else:
                    content = self._normalize_bytes(content)
            elif path.startswith(config):
                rel = 'config/' + path[len(config):]
                content = self._normalize_bytes(content)
            elif path.startswith(data):
                rel = 'data/' + path[len(data):]
                content = self._normalize_bytes(content)
            else:
                rel = 'home/' + path
                content = self._normalize_bytes(content)
            tree[rel] = content
        return tree

    def _project_names(self, raw, history):
        "the random identifiers of the projects of the history, numbered by the places of their libraries"
        prefix = history + 'projects/'
        ids = sorted({p[len(prefix):].split('/')[0] for p in raw if p.startswith(prefix)})
        keyed = []
        for pid in ids:
            heads = raw.get(prefix + pid + '/heads.json', b'')
            try:
                places = sorted(self.normalize(k) for k in json.loads(heads.decode('utf-8')))
            except ValueError:
                places = []
            keyed.append((places, pid))
        keyed.sort()
        return {pid: f'$PROJECT{i + 1}' for i, (_, pid) in enumerate(keyed)}


# running a case
# ==============

def resolve_papers(command):
    "the command to run (a list), its program as an absolute path"
    argv = shlex.split(command)
    if not argv:
        raise SystemExit('--papers: empty command')
    prog = argv[0]
    if os.sep in prog:
        prog = os.path.abspath(prog)
    else:
        prog = shutil.which(prog)
    if not prog or not os.path.exists(prog):
        raise SystemExit(f'--papers: {argv[0]}: command not found')
    return [prog] + argv[1:]


def run_case(case, papers, keep=False):
    "run the steps of a case: {'steps': [{stdout, stderr, exit}], 'work': {...}, 'home': {...}}"
    box = Sandbox(case, keep=keep)
    try:
        box.populate()
        box.detect_version(papers)
        steps = [box.run_step(papers, step) for step in case.steps]
        result = box.snapshot()
        box.normalize_steps(steps)
        result['steps'] = steps
        if keep:
            print(f'{case.name}: sandbox kept in {box.root}', file=sys.stderr)
        return result
    finally:
        box.close()


# expected results
# ================

def _step_file(n):
    return f'{n + 1:02d}'


def write_expected(case, result):
    "write expected/ of the case: <NN>.stdout, <NN>.stderr (when not empty), exit.json, work/, home/"
    _remove_dir_contents(case.expected)
    os.makedirs(case.expected)
    for n, step in enumerate(result['steps']):
        for stream in ('stdout', 'stderr'):
            if step[stream]:
                with open(os.path.join(case.expected, f'{_step_file(n)}.{stream}'), 'wb') as f:
                    f.write(step[stream].encode('utf-8', 'surrogateescape'))
    with open(os.path.join(case.expected, 'exit.json'), 'w', encoding='utf-8') as f:
        json.dump([s['exit'] for s in result['steps']], f)
        f.write('\n')
    for tree in ('work', 'home'):
        _write_tree(os.path.join(case.expected, tree), result[tree])


def read_expected(case):
    if not os.path.isfile(os.path.join(case.expected, 'exit.json')):
        return None
    with open(os.path.join(case.expected, 'exit.json'), encoding='utf-8') as f:
        exits = json.load(f)
    steps = []
    for n, code in enumerate(exits):
        step = {'exit': code}
        for stream in ('stdout', 'stderr'):
            path = os.path.join(case.expected, f'{_step_file(n)}.{stream}')
            if os.path.exists(path):
                with open(path, 'rb') as f:
                    step[stream] = f.read().decode('utf-8', 'surrogateescape')
            else:
                step[stream] = ''
        steps.append(step)
    result = {'steps': steps}
    for tree in ('work', 'home'):
        result[tree] = _load_stored(os.path.join(case.expected, tree))
    return result


# comparison
# ==========

def _text_diff(label, expected, actual, limit=40):
    if isinstance(expected, bytes) or isinstance(actual, bytes):
        try:
            expected = expected.decode('utf-8') if isinstance(expected, bytes) else expected
            actual = actual.decode('utf-8') if isinstance(actual, bytes) else actual
        except UnicodeDecodeError:
            return f'{label}: binary content differs ({len(expected)} bytes expected, {len(actual)} found)'
    lines = list(difflib.unified_diff(expected.splitlines(True), actual.splitlines(True),
                                      f'expected {label}', f'actual {label}'))
    lines = [l if l.endswith('\n') else l + '\n\\ no newline at end of text\n' for l in lines]
    if len(lines) > limit:
        lines = lines[:limit] + [f'... ({len(lines) - limit} more lines)\n']
    if not lines:
        return f'{label}: differs'
    return ''.join(lines)


def differences(expected, actual, first_only=True):
    "list of (label, unified diff) of what differs in the results, in the order of the run"
    found = []

    def add(label, a, b):
        found.append((label, _text_diff(label, a, b)))
        return first_only

    if len(expected['steps']) != len(actual['steps']):
        add('steps', f'{len(expected["steps"])} steps\n', f'{len(actual["steps"])} steps\n')
        return found
    for n, (e, a) in enumerate(zip(expected['steps'], actual['steps'])):
        for key in ('exit', 'stdout', 'stderr'):
            if e[key] != a[key]:
                label = f'step {n + 1} {key}'
                if key == 'exit':
                    found.append((label, f'expected exit code {e[key]}, found {a[key]}\n'))
                    if first_only:
                        return found
                elif add(label, e[key], a[key]):
                    return found
    for tree in ('work', 'home'):
        for path in sorted(set(expected[tree]) | set(actual[tree])):
            e, a = expected[tree].get(path), actual[tree].get(path)
            if e == a:
                continue
            label = f'{tree}/{path}'
            if e is None:
                found.append((label, 'file not expected, but written\n'))
            elif a is None:
                found.append((label, 'file expected, but not written\n'))
            else:
                found.append((label, _text_diff(label, e, a)))
            if first_only:
                return found
    return found
