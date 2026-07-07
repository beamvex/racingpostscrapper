import json
import os
import re
import boto3
from datetime import datetime, timezone

s3 = boto3.client('s3')

DATE_RE = re.compile(r'^\d{4}-\d{2}-\d{2}$')


def _parse_date(date_str: str) -> tuple[str, str, str, str] | None:
    """Parse YYYY-MM-DD, return (y, m, d, date_str) or None."""
    if not DATE_RE.match(date_str):
        return None
    y, m, d = date_str.split('-')
    return y, m, d, date_str


def _fetch_html(bucket: str, y: str, m: str, d: str, date_str: str) -> dict:
    key = f'probabilities/{y}/{m}/{d}/racecard-report-{date_str}.html'
    try:
        response = s3.get_object(Bucket=bucket, Key=key)
        html_content = response['Body'].read().decode('utf-8')
        return {
            'statusCode': 200,
            'headers': {
                'Content-Type': 'text/html',
                'Cache-Control': 'no-cache'
            },
            'body': html_content
        }
    except s3.exceptions.NoSuchKey:
        return {
            'statusCode': 404,
            'headers': {'Content-Type': 'text/html'},
            'body': f'<h1>Probabilities not available for {date_str}</h1><p>Check back later after racecard processing completes.</p>'
        }
    except Exception as e:
        return {
            'statusCode': 500,
            'headers': {'Content-Type': 'text/html'},
            'body': f'<h1>Error fetching probabilities</h1><p>{str(e)}</p>'
        }


def lambda_handler(event, context):
    bucket_name = os.environ.get('SCRAPER_DATA_BUCKET_NAME')
    if not bucket_name:
        return {
            'statusCode': 500,
            'headers': {'Content-Type': 'text/html'},
            'body': '<h1>Configuration error: SCRAPER_DATA_BUCKET_NAME not set</h1>'
        }

    # Check for date path parameter
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
        return _fetch_html(bucket_name, *parsed)

    # Default: today
    now = datetime.now(timezone.utc)
    y = now.strftime('%Y')
    m = now.strftime('%m')
    d = now.strftime('%d')
    date_str = now.strftime('%Y-%m-%d')
    return _fetch_html(bucket_name, y, m, d, date_str)
