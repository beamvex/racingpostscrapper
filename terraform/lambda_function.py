import io
import os
import re
import boto3
import pyarrow.parquet as pq
from datetime import datetime, timezone

s3 = boto3.client('s3')

DATE_RE = re.compile(r'^\d{4}-\d{2}-\d{2}$')


def _parse_date(date_str: str) -> tuple[str, str, str, str] | None:
    if not DATE_RE.match(date_str):
        return None
    y, m, d = date_str.split('-')
    return y, m, d, date_str


def _esc(s: str) -> str:
    return (str(s)
            .replace('&', '&amp;')
            .replace('<', '&lt;')
            .replace('>', '&gt;')
            .replace('"', '&quot;'))


def _hhmm(time_str: str) -> str:
    t = (time_str or '').strip()
    idx = t.find('T')
    if idx != -1 and len(t) >= idx + 6:
        return t[idx + 1:idx + 6]
    return t


def _fmt_odds(val) -> str:
    if val is None:
        return '—'
    try:
        f = float(val)
        return f'{f:.2f}' if f > 0 else '—'
    except (TypeError, ValueError):
        return '—'


def _fmt_prob(val) -> str:
    try:
        return f'{float(val) * 100:.1f}%'
    except (TypeError, ValueError):
        return '—'


def _build_html(date_str: str, rows: list[dict]) -> str:
    races: dict[tuple, list[dict]] = {}
    race_order: list[tuple] = []
    for r in rows:
        key = (r.get('course', ''), r.get('time', ''), r.get('race_name', ''), r.get('going', ''))
        if key not in races:
            races[key] = []
            race_order.append(key)
        races[key].append(r)

    race_order.sort(key=lambda k: _hhmm(k[1]))

    out = []
    out.append('<!doctype html>')
    out.append('<html lang="en"><head>')
    out.append('<meta charset="utf-8">')
    out.append('<meta name="viewport" content="width=device-width, initial-scale=1">')
    out.append('<link href="https://cdn.jsdelivr.net/npm/bootstrap@5.3.3/dist/css/bootstrap.min.css" '
               'rel="stylesheet" crossorigin="anonymous">')
    out.append(f'<title>{_esc(date_str)} Probabilities</title>')
    out.append('</head><body><div class="container my-4">')
    out.append(f'<h1 class="h3 mb-1">Race Probabilities</h1>')
    out.append(f'<p class="text-muted">Date: {_esc(date_str)}</p><hr class="my-3">')

    out.append('<div class="accordion" id="racesAccordion">')
    for idx, key in enumerate(race_order):
        course, time_str, race_name, going = key
        runners = sorted(races[key], key=lambda r: -(r.get('prob') or 0))
        hhmm = _hhmm(time_str)
        heading = f'{_esc(hhmm)} — {_esc(course)}'
        if race_name.strip():
            heading += f' — {_esc(race_name)}'

        collapse_id = f'collapse{idx}'
        heading_id = f'heading{idx}'
        expanded = 'true' if idx == 0 else 'false'
        show_cls = ' show' if idx == 0 else ''
        collapsed_cls = '' if idx == 0 else ' collapsed'

        out.append('<div class="accordion-item">')
        out.append(f'<h2 class="accordion-header" id="{heading_id}">')
        out.append(f'<button class="accordion-button{collapsed_cls}" type="button" '
                   f'data-bs-toggle="collapse" data-bs-target="#{collapse_id}" '
                   f'aria-expanded="{expanded}" aria-controls="{collapse_id}">')
        out.append(heading)
        out.append('</button></h2>')
        out.append(f'<div id="{collapse_id}" class="accordion-collapse collapse{show_cls}" '
                   f'aria-labelledby="{heading_id}" data-bs-parent="#racesAccordion">')
        out.append('<div class="accordion-body">')

        if going.strip():
            out.append(f'<p class="text-muted small mb-2">Going: {_esc(going)}</p>')

        out.append('<div class="table-responsive">')
        out.append('<table class="table table-sm table-hover align-middle">')
        out.append('<thead><tr>'
                   '<th>Horse</th>'
                   '<th>Jockey</th>'
                   '<th>Trainer</th>'
                   '<th class="text-end">Bookie odds</th>'
                   '<th class="text-end">Model prob</th>'
                   '<th class="text-end">Fair odds</th>'
                   '</tr></thead><tbody>')
        for r in runners:
            out.append(
                f'<tr>'
                f'<td>{_esc(r.get("horse",""))}</td>'
                f'<td>{_esc(r.get("jockey",""))}</td>'
                f'<td>{_esc(r.get("trainer",""))}</td>'
                f'<td class="text-end">{_fmt_odds(r.get("bookie_odds"))}</td>'
                f'<td class="text-end">{_fmt_prob(r.get("prob"))}</td>'
                f'<td class="text-end">{_fmt_odds(r.get("fair_odds"))}</td>'
                f'</tr>'
            )
        out.append('</tbody></table></div>')
        out.append('</div></div></div>')

    out.append('</div>')
    out.append('<script src="https://cdn.jsdelivr.net/npm/bootstrap@5.3.3/dist/js/bootstrap.bundle.min.js" '
               'crossorigin="anonymous"></script>')
    out.append('</div></body></html>')
    return '\n'.join(out)


def _fetch_and_render(bucket: str, y: str, m: str, d: str, date_str: str) -> dict:
    key = f'probabilities/{y}/{m}/{d}/racecard-probabilities-{date_str}.parquet'
    try:
        response = s3.get_object(Bucket=bucket, Key=key)
        data = response['Body'].read()
    except s3.exceptions.NoSuchKey:
        return {
            'statusCode': 404,
            'headers': {'Content-Type': 'text/html'},
            'body': (f'<h1>Probabilities not available for {_esc(date_str)}</h1>'
                     f'<p>Check back later after racecard processing completes.</p>')
        }
    except Exception as e:
        return {
            'statusCode': 500,
            'headers': {'Content-Type': 'text/html'},
            'body': f'<h1>Error fetching probabilities</h1><p>{_esc(str(e))}</p>'
        }

    try:
        table = pq.read_table(io.BytesIO(data))
        rows = table.to_pydict()
        n = len(next(iter(rows.values()), []))
        row_list = [{col: rows[col][i] for col in rows} for i in range(n)]
        html = _build_html(date_str, row_list)
    except Exception as e:
        return {
            'statusCode': 500,
            'headers': {'Content-Type': 'text/html'},
            'body': f'<h1>Error rendering probabilities</h1><p>{_esc(str(e))}</p>'
        }

    return {
        'statusCode': 200,
        'headers': {'Content-Type': 'text/html', 'Cache-Control': 'no-cache'},
        'body': html
    }


def lambda_handler(event, context):
    bucket_name = os.environ.get('SCRAPER_DATA_BUCKET_NAME')
    if not bucket_name:
        return {
            'statusCode': 500,
            'headers': {'Content-Type': 'text/html'},
            'body': '<h1>Configuration error: SCRAPER_DATA_BUCKET_NAME not set</h1>'
        }

    path_params = event.get('pathParameters') or {}
    date_param = (path_params.get('date') or '').strip()

    if date_param:
        parsed = _parse_date(date_param)
        if not parsed:
            return {
                'statusCode': 400,
                'headers': {'Content-Type': 'text/html'},
                'body': '<h1>Invalid date format</h1><p>Use YYYY-MM-DD, e.g. /2026-07-06</p>'
            }
        return _fetch_and_render(bucket_name, *parsed)

    now = datetime.now(timezone.utc)
    return _fetch_and_render(
        bucket_name,
        now.strftime('%Y'),
        now.strftime('%m'),
        now.strftime('%d'),
        now.strftime('%Y-%m-%d'),
    )
