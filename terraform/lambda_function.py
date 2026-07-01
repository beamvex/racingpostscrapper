import json
import os
import boto3
from datetime import datetime, timezone

s3 = boto3.client('s3')

def lambda_handler(event, context):
    bucket_name = os.environ.get('SCRAPER_DATA_BUCKET_NAME')
    if not bucket_name:
        return {
            'statusCode': 500,
            'headers': {'Content-Type': 'text/html'},
            'body': '<h1>Configuration error: SCRAPER_DATA_BUCKET_NAME not set</h1>'
        }

    # Get current date in UTC
    now = datetime.now(timezone.utc)
    y = now.strftime('%Y')
    m = now.strftime('%m')
    d = now.strftime('%d')
    date_str = now.strftime('%Y-%m-%d')

    # Construct S3 key for today's probabilities HTML
    key = f'probabilities/{y}/{m}/{d}/racecard-report-{date_str}.html'

    try:
        response = s3.get_object(Bucket=bucket_name, Key=key)
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
            'body': f'<h1>Probabilities not yet available for {date_str}</h1><p>Check back later after racecard processing completes.</p>'
        }
    except Exception as e:
        return {
            'statusCode': 500,
            'headers': {'Content-Type': 'text/html'},
            'body': f'<h1>Error fetching probabilities</h1><p>{str(e)}</p>'
        }
