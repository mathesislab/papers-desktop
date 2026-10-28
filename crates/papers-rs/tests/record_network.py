#!/usr/bin/env python3
"""Record the answers of the network that tests/gen_extract_expected.py replays (the "recorded"
scenarios): a handful of requests, made once with papers' own code (`papers.fetch.http_get`), the
answers kept in tests/fixtures/network.json as they came (but for the list of references of the
crossref record, which is large and is not used).

    /path/to/python3 tests/record_network.py

Seven requests to crossref, doi.org and arXiv. A temporary HOME and cache are used.
"""
import json
import os
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TMP = tempfile.mkdtemp(prefix='papers-rs-record-')
os.environ['HOME'] = TMP
os.environ['XDG_CACHE_HOME'] = os.path.join(TMP, 'cache')
os.environ['XDG_CONFIG_HOME'] = os.path.join(TMP, 'config')

import requests  # noqa: E402

import papers.config  # noqa: E402
import papers.fetch as pf  # noqa: E402

papers.config.CACHE_DIR = os.path.join(TMP, 'papers-cache')
RECORDED = {}
real_get = pf.http_get


def recording_get(url, **kw):
    response = real_get(url, **kw)
    full = requests.Request('GET', url, params=kw.get('params')).prepare().url
    body = response.text
    if 'api.crossref.org/works/' in full and response.ok:
        data = response.json()
        data['message'].pop('reference', None)
        body = json.dumps(data, ensure_ascii=False)
    RECORDED[full] = {'status': response.status_code, 'reason': response.reason,
                      'content_type': response.headers.get('content-type', ''), 'body': body,
                      'accept': (kw.get('headers') or {}).get('Accept')}
    print(response.status_code, full)
    return response


pf.http_get = recording_get

for call, args in [
        (pf.fetch_bibtex_by_doi, ('10.5194/tc-11-1015-2017',)),
        (pf.fetch_bibtex_by_doi, ('10.5281/zenodo.2580252',)),  # DataCite: crossref does not have it, doi.org does
        (pf.fetch_bibtex_by_doi, ('10.9999/not.a.doi',)),
        (pf.fetch_bibtex_by_doi, ('10.48550/arXiv.1706.03762',)),
        (pf.fetch_bibtex_by_fulltext_crossref, ('Reconstructions of the 1900-2015 Greenland ice sheet surface mass balance '
                                                'using the regional climate MAR model',))]:
    try:
        call(*args)
    except Exception as error:
        print('error', type(error).__name__, error)

os.makedirs(os.path.join(HERE, 'fixtures'), exist_ok=True)
with open(os.path.join(HERE, 'fixtures', 'network.json'), 'w', encoding='utf-8') as f:
    f.write('{\n' + ',\n'.join(f'{json.dumps(k)}: {json.dumps(v, ensure_ascii=False)}' for k, v in RECORDED.items()) + '\n}\n')
