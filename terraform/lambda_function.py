import io
import os
import re
import boto3
import pyarrow.parquet as pq
from datetime import datetime, timezone, timedelta

s3 = boto3.client('s3')

DATE_RE = re.compile(r'^\d{4}-\d{2}-\d{2}$')
RUN_TS_RE = re.compile(r'racecard-probabilities-\d{4}-\d{2}-\d{2}-(\d+)\.parquet$')


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


def _list_runs_for_date(bucket: str, y: str, m: str, d: str, date_str: str) -> list[dict]:
    prefix = f'probabilities/{y}/{m}/{d}/'
    try:
        resp = s3.list_objects_v2(Bucket=bucket, Prefix=prefix)
        files = []
        for obj in resp.get('Contents', []):
            key = obj['Key']
            if not key.endswith('.parquet'):
                continue
            m_ts = RUN_TS_RE.search(key)
            ts = m_ts.group(1) if m_ts else '000000'
            files.append({'key': key, 'date': date_str, 'ts': ts})
        files.sort(key=lambda x: x['ts'], reverse=True)
        return files
    except Exception:
        return []


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


def _bookie_prob(bookie_odds) -> float | None:
    try:
        f = float(bookie_odds)
        return (1.0 / f) if f > 0 else None
    except (TypeError, ValueError):
        return None


def _edge_cell(edge: float | None) -> str:
    if edge is None:
        return '<td class="text-end">—</td>'
    pct = edge * 100
    cls = 'text-success fw-semibold' if pct > 0 else 'text-danger'
    sign = '+' if pct > 0 else ''
    return f'<td class="text-end {cls}">{sign}{pct:.1f}pp</td>'


def _run_url(run: dict) -> str:
    ds, ts = run['date'], run['ts']
    return f'/{ds}?run={ts}' if ts != '000000' else f'/{ds}'


def _build_sidebar(all_runs: list[dict], current_key: str,
                   today_str: str, yesterday_str: str) -> str:
    if not all_runs:
        return ''
    seen_dates: list[str] = []
    by_date: dict[str, list[dict]] = {}
    for run in all_runs:
        ds = run['date']
        if ds not in by_date:
            by_date[ds] = []
            seen_dates.append(ds)
        by_date[ds].append(run)
    out = []
    out.append('<nav class="sticky-top" style="top:1rem;max-height:calc(100vh - 2rem);overflow-y:auto">')
    out.append('<p class="fw-semibold mb-1 small text-uppercase text-muted">Runs</p>')
    for ds in seen_dates:
        if ds == today_str:
            day_label = 'Today'
        elif ds == yesterday_str:
            day_label = 'Yesterday'
        else:
            day_label = ds
        out.append(f'<p class="fw-semibold mb-1 mt-3 small text-muted">{_esc(day_label)}</p>')
        out.append('<div class="list-group list-group-flush">')
        for run in by_date[ds]:
            ts = run['ts']
            # ts is HHMMSS (full-day run) or a numeric race_id (per-race run)
            label = f"{ts[:2]}:{ts[2:4]}" if len(ts) == 6 and ts.isdigit() and int(ts[:2]) < 24 else ts
            active = ' active' if run['key'] == current_key else ''
            out.append(f'<a href="{_esc(_run_url(run))}" '
                       f'class="list-group-item list-group-item-action py-1 small{active}">'
                       f'{_esc(label)}</a>')
        out.append('</div>')
    out.append('</nav>')
    return '\n'.join(out)


def _edge_key(r: dict) -> float:
    bp = _bookie_prob(r.get('bookie_odds'))
    mp = r.get('prob')
    if bp is not None and mp is not None:
        return float(mp) - bp
    return float('-inf')


def _build_html(date_str: str, rows: list[dict],
                sidebar: str = '', latest_url: str = '/') -> str:
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
    out.append('</head><body><div class="container-fluid my-4">')
    out.append('<div class="row">')
    if sidebar:
        out.append('<div class="col-12 col-md-2 mb-3">')
        out.append(sidebar)
        out.append('</div>')
        out.append('<div class="col-12 col-md-10">')
    else:
        out.append('<div class="col-12">')
    out.append(f'<h1 class="h3 mb-1"><a href="{_esc(latest_url)}" class="text-decoration-none text-reset">Race Probabilities</a></h1>')
    out.append(f'<p class="text-muted">Date: {_esc(date_str)}</p>')
    out.append('<hr class="my-3">')

    for key in race_order:
        course, time_str, race_name, going = key
        runners = sorted(races[key], key=_edge_key, reverse=True)
        hhmm = _hhmm(time_str)
        heading = f'{_esc(hhmm)} — {_esc(course)}'
        if race_name.strip():
            heading += f' — {_esc(race_name)}'

        subtitle = f'Going: {_esc(going)}' if going.strip() else ''
        out.append('<div class="card mt-4">')
        out.append('<div class="card-header">')
        out.append(f'<h5 class="card-title mb-0">{heading}</h5>')
        if subtitle:
            out.append(f'<small class="text-muted">{subtitle}</small>')
        out.append('</div>')
        out.append('<div class="card-body p-0">')

        total_model = sum((r.get('prob') or 0.0) for r in runners)
        total_bookie = sum(_bookie_prob(r.get('bookie_odds')) or 0.0 for r in runners)
        total_edge = total_model - total_bookie if total_bookie > 0 else None

        out.append('<div class="table-responsive">')
        out.append('<table class="table table-sm table-striped table-hover align-middle mb-0">')
        out.append('<thead><tr>'
                   '<th>Horse</th>'
                   '<th>Jockey</th>'
                   '<th>Trainer</th>'
                   '<th class="text-end">Bookie odds</th>'
                   '<th class="text-end">Bookie prob</th>'
                   '<th class="text-end">Model prob</th>'
                   '<th class="text-end">Fair odds</th>'
                   '<th class="text-end">Edge</th>'
                   '</tr></thead><tbody>')
        for r in runners:
            bp = _bookie_prob(r.get('bookie_odds'))
            mp = r.get('prob')
            edge = (float(mp) - bp) if (bp is not None and mp is not None) else None
            out.append(
                f'<tr>'
                f'<td>{_esc(r.get("horse",""))}</td>'
                f'<td>{_esc(r.get("jockey",""))}</td>'
                f'<td>{_esc(r.get("trainer",""))}</td>'
                f'<td class="text-end">{_fmt_odds(r.get("bookie_odds"))}</td>'
                f'<td class="text-end">{_fmt_prob(bp)}</td>'
                f'<td class="text-end">{_fmt_prob(mp)}</td>'
                f'<td class="text-end">{_fmt_odds(r.get("fair_odds"))}</td>'
                + _edge_cell(edge) +
                f'</tr>'
            )
        if total_edge is None:
            edge_total_cell = '<td class="text-end fw-bold border-top">—</td>'
        else:
            pct = total_edge * 100
            cls = 'text-success' if pct > 0 else 'text-danger'
            sign = '+' if pct > 0 else ''
            edge_total_cell = (f'<td class="text-end fw-bold border-top {cls}">'
                               f'{sign}{pct:.1f}pp</td>')
        out.append(
            f'</tbody><tfoot><tr>'
            f'<td colspan="3" class="fw-bold border-top">Totals</td>'
            f'<td class="text-end border-top">—</td>'
            f'<td class="text-end fw-bold border-top">{_fmt_prob(total_bookie)}</td>'
            f'<td class="text-end fw-bold border-top">{_fmt_prob(total_model)}</td>'
            f'<td class="text-end border-top">—</td>'
            + edge_total_cell +
            f'</tr></tfoot></table></div>'
        )
        out.append('</div></div>')
    out.append('</div></div></div></body></html>')
    return '\n'.join(out)


def _fetch_and_render(bucket: str, y: str, m: str, d: str, date_str: str,
                      run_id: str = '', all_runs: list[dict] | None = None,
                      today_str: str = '', yesterday_str: str = '') -> dict:
    all_runs = all_runs or []
    date_runs = [r for r in all_runs if r['date'] == date_str]

    if run_id:
        target = next((r for r in date_runs if r['ts'] == run_id), None)
        if target is None and date_runs:
            target = date_runs[0]
    elif date_runs:
        target = date_runs[0]
    else:
        target = None

    key = target['key'] if target else (
        f'probabilities/{y}/{m}/{d}/racecard-probabilities-{date_str}.parquet'
    )

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
        cols = table.to_pydict()
        n = len(next(iter(cols.values()), []))
        row_list = [{col: cols[col][i] for col in cols} for i in range(n)]
        latest_url = _run_url(all_runs[0]) if all_runs else f'/{date_str}'
        sidebar = _build_sidebar(all_runs, key, today_str or date_str, yesterday_str)
        html = _build_html(date_str, row_list, sidebar=sidebar, latest_url=latest_url)
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

    now = datetime.now(timezone.utc)
    today_str = now.strftime('%Y-%m-%d')
    yesterday_str = (now - timedelta(days=1)).strftime('%Y-%m-%d')

    query_params = event.get('queryStringParameters') or {}
    run_id = (query_params.get('run') or '').strip()

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
        y, m, d, date_str = parsed
    else:
        y, m, d, date_str = now.strftime('%Y'), now.strftime('%m'), now.strftime('%d'), today_str

    all_runs = _list_runs_for_date(bucket_name, *today_str.split('-'), today_str)
    all_runs += _list_runs_for_date(bucket_name, *yesterday_str.split('-'), yesterday_str)
    if date_str not in (today_str, yesterday_str):
        all_runs += _list_runs_for_date(bucket_name, y, m, d, date_str)

    return _fetch_and_render(
        bucket_name, y, m, d, date_str,
        run_id=run_id,
        all_runs=all_runs,
        today_str=today_str,
        yesterday_str=yesterday_str,
    )
