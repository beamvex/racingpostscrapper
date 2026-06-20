#!/bin/bash
bash ./builddocker.sh

docker run --rm -it -p 3001:3001 \
  --name racingpost-scrapper \
  -e AWS_PROFILE=512752756525_AdministratorAccess \
  -e S6_KEEP_ENV=1 \
  -e S3_BUCKET_NAME=racingpost-scrapper \
  -e AWS_REGION=eu-west-2 \
  -v ~/.aws:/config/.aws \
  -v ./data:/data \
  --entrypoint "/app/target/release/racecard_html_dir_parser" \
  racingpost-scrapper:latest \
  --html-dir /data/2026/06/20/racingpost-racecards-2026-06-20-racecards-html \
  --out /data/2026/06/20/racingpost-racecards-2026-06-20-runners.jsonl
