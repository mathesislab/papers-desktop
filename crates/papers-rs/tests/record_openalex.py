#!/usr/bin/env python3
"""Record answers of OpenAlex into `tests/fixtures/openalex.json`, once, for the tests of
`fetch::openalex` (an extension of papers-rs, not in papers). Two searches go to the network
(10 credits each of the daily budget of OpenAlex); the texts are made of the public metadata
of two works (title, authors, the start of the abstract), as the start of a PDF would be.

    python3 tests/record_openalex.py      # with the python that has papers installed (for query_text)

The query of a search is what papers' `query_text` makes of the text, at most 1500 characters (what
the semantic search of OpenAlex uses). The affiliations of the
authors are left out of the answers (the program does not read them; they are most of its size),
as are the notice and the query of `meta`.
"""
import json
import os
import urllib.parse
import urllib.request

from papers.extract import query_text

HERE = os.path.dirname(os.path.abspath(__file__))
SELECT = ('id,doi,title,display_name,type,type_crossref,publication_year,biblio,primary_location,'
          'authorships,abstract_inverted_index')

TEXTS = {
    # a journal article, with a DOI (the answer of crossref for it is in network.json)
    'fettweis': (
        'Reconstructions of the 1900-2015 Greenland ice sheet surface mass balance using the regional '
        'climate MAR model\n\nXavier Fettweis, Jason E. Box, Cecile Agosta, Charles Amory, Christoph Kittel, '
        'Charlotte Lang, Dirk van As, Horst Machguth, and Hubert Gallee\n\nAbstract. With the aim of studying '
        'the recent Greenland ice sheet (GrIS) surface mass balance (SMB) decrease relative to the last century, '
        'we have forced the regional climate MAR (Modele Atmospherique Regional; version 3.5.2) model with the '
        'ERA-Interim (ECMWF Interim Re-Analysis; 1979-2015), ERA-40 (1958-2001), NCEP-NCARv1 (National Centers '
        'for Environmental Prediction-National Center for Atmospheric Research Reanalysis version 1; 1948-2015) '
        'reanalyses. While all these forcing products are reanalyses that are assumed to represent the same '
        'climate, they produce significant differences in the MAR-simulated SMB over their common period.'),
    # a thesis, without DOI
    'angelen': (
        'The Greenland ice sheet in a warming climate\n\nJ. H. van Angelen\n\nUtrecht University\n\nIn this thesis we '
        'assess multiple aspects of the Greenland climate, including the surface energy and mass balance of the '
        'ice sheet for the contemporary and near future climate. For these purposes we used output of the '
        'extensively and well-evaluated regional atmospheric climate model RACMO2. The relatively high horizontal '
        'resolution (11 km) enables us to explicitly solve the atmospheric momentum budget and explain both small '
        'and large-scale wind patterns over the Greenland ice sheet and its surrounding seas. In the surface layer '
        'the katabatic pressure gradient force dominates the momentum budget resulting in strong and persistent '
        'cross-slope winds.'),
}


def trimmed(body):
    meta = body.get('meta', {})
    body['meta'] = {k: v for k, v in meta.items() if k in ('count', 'page', 'per_page')}
    for work in body['results']:
        for a in work.get('authorships', []):
            for k in ('institutions', 'affiliations', 'countries', 'raw_affiliation_strings'):
                a.pop(k, None)
    return body


def main():
    answers = []
    for name, text in TEXTS.items():
        query = query_text(text, 200)
        params = [('search.semantic', query[:1500]), ('per-page', '10'), ('select', SELECT)]
        url = 'https://api.openalex.org/works?' + urllib.parse.urlencode(params)
        with urllib.request.urlopen(urllib.request.Request(url, headers={'User-Agent': 'papers-rs-test'}), timeout=30) as r:
            body = json.load(r)
        answers.append({'name': name, 'search': query, 'status': 200, 'body': trimmed(body)})
    out = os.path.join(HERE, 'fixtures', 'openalex.json')
    with open(out, 'w') as f:
        json.dump({'texts': TEXTS, 'answers': answers}, f, indent=1, ensure_ascii=False)
        f.write('\n')


if __name__ == '__main__':
    main()
