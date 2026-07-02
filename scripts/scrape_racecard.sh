#!/bin/bash
set -euo pipefail
export DISPLAY=:99

echo "starting Xvfb on ${DISPLAY}"
Xvfb ${DISPLAY} -screen 0 1280x720x24 -nolisten tcp &
XVFB_PID=$!

i=0
while [ ! -S "/tmp/.X11-unix/X${DISPLAY#:}" ] && [ $i -lt 50 ]; do
  i=$((i+1))
  sleep 0.1
done

echo "starting chromium"

chromium \
  --no-sandbox \
  --disable-dev-shm-usage \
  --disable-gpu \
  --disable-software-rasterizer \
  --remote-debugging-address=0.0.0.0 \
  --remote-debugging-port=9222 \
  about:blank &

CHROMIUM_PID=$!

echo "chromium started"
echo "waiting for devtools port"
i=0
until curl -fsS "http://127.0.0.1:9222/json/version" >/dev/null 2>&1; do
  i=$((i+1))
  if [ $i -gt 100 ]; then
    echo "chromium devtools did not come up"
    kill ${CHROMIUM_PID} >/dev/null 2>&1 || true
    kill ${XVFB_PID} >/dev/null 2>&1 || true
    exit 1
  fi
  sleep 0.1
done

echo "running scraper"
if [ -n "${1:-}" ]; then
  echo "date arg=${1}"
  RESULTS_DATE_USED="${1}"
  /app/target/release/racecards_time_order_scraper "${1}"
else
  if [ -n "${RESULTS_DATE:-}" ]; then
    echo "RESULTS_DATE=${RESULTS_DATE}"
    RESULTS_DATE_USED="${RESULTS_DATE}"
  else
    echo "no date arg and RESULTS_DATE not set (default will be used)"
    RESULTS_DATE_USED="$(date -u +%F)"
  fi
  /app/target/release/racecards_time_order_scraper "${RESULTS_DATE_USED}"
fi

SCRAPER_EXIT=$?

kill ${CHROMIUM_PID} >/dev/null 2>&1 || true
kill ${XVFB_PID} >/dev/null 2>&1 || true

if [ "${SCRAPER_EXIT}" -eq 0 ]; then
  Y="${RESULTS_DATE_USED%%-*}"
  REST="${RESULTS_DATE_USED#*-}"
  M="${REST%%-*}"
  D="${REST#*-}"
  OUT_DIR="/data/${Y}/${M}/${D}"
  HTML_DIR="${OUT_DIR}/racingpost-racecards-${RESULTS_DATE_USED}-racecards-html"
  RUNNERS_OUT="${OUT_DIR}/racingpost-racecards-${RESULTS_DATE_USED}-runners.jsonl"

  echo "running racecard html parser"
  /app/target/release/racecard_html_dir_parser --html-dir "${HTML_DIR}" --out "${RUNNERS_OUT}"

  if [ -n "${SCRAPER_DATA_BUCKET_NAME:-}" ] && [ -f "${RUNNERS_OUT}" ]; then
    S3_RACECARDS_PREFIX="s3://${SCRAPER_DATA_BUCKET_NAME}/racecards/${Y}/${M}/${D}/"
    echo "uploading racecard runners to ${S3_RACECARDS_PREFIX}"
    AWS_REGION_USED="${AWS_REGION:-eu-west-2}"
    AWS_PROFILE_USED="${AWS_PROFILE:-}"
    if [ -n "${AWS_PROFILE_USED}" ]; then
      aws s3 cp "${RUNNERS_OUT}" "${S3_RACECARDS_PREFIX}$(basename "${RUNNERS_OUT}")" \
        --region "${AWS_REGION_USED}" \
        --profile "${AWS_PROFILE_USED}"
    else
      aws s3 cp "${RUNNERS_OUT}" "${S3_RACECARDS_PREFIX}$(basename "${RUNNERS_OUT}")" \
        --region "${AWS_REGION_USED}"
    fi
  fi

  # Download parquet files from S3 for probability computation
  HISTORY_DIR="${OUT_DIR}/history_parquet"
  mkdir -p "${HISTORY_DIR}"
  if [ -n "${SCRAPER_DATA_BUCKET_NAME:-}" ]; then
    S3_PROCESSED_PREFIX="s3://${SCRAPER_DATA_BUCKET_NAME}/processed/"
    echo "downloading parquet files from ${S3_PROCESSED_PREFIX} to ${HISTORY_DIR}"
    if [ -n "${AWS_PROFILE_USED}" ]; then
      aws s3 sync "${S3_PROCESSED_PREFIX}" "${HISTORY_DIR}" \
        --region "${AWS_REGION_USED}" \
        --profile "${AWS_PROFILE_USED}" \
        --exclude "*" \
        --include "*.parquet"
    else
      aws s3 sync "${S3_PROCESSED_PREFIX}" "${HISTORY_DIR}" \
        --region "${AWS_REGION_USED}" \
        --exclude "*" \
        --include "*.parquet"
    fi
  fi

  # Generate probabilities HTML using today_first_race_table
  PROBABILITIES_HTML="${OUT_DIR}/racecard-report-${RESULTS_DATE_USED}.html"
  echo "generating probabilities HTML to ${PROBABILITIES_HTML}"
  /app/target/release/today_first_race_table \
    --in="${RUNNERS_OUT}" \
    --history-dir="${HISTORY_DIR}" \
    --out="${PROBABILITIES_HTML}"

  # Upload probabilities HTML to S3 /probabilities
  if [ -n "${SCRAPER_DATA_BUCKET_NAME:-}" ] && [ -f "${PROBABILITIES_HTML}" ]; then
    S3_PROBABILITIES_PREFIX="s3://${SCRAPER_DATA_BUCKET_NAME}/probabilities/${Y}/${M}/${D}/"
    echo "uploading probabilities HTML to ${S3_PROBABILITIES_PREFIX}"
    if [ -n "${AWS_PROFILE_USED}" ]; then
      aws s3 cp "${PROBABILITIES_HTML}" "${S3_PROBABILITIES_PREFIX}$(basename "${PROBABILITIES_HTML}")" \
        --region "${AWS_REGION_USED}" \
        --profile "${AWS_PROFILE_USED}"
    else
      aws s3 cp "${PROBABILITIES_HTML}" "${S3_PROBABILITIES_PREFIX}$(basename "${PROBABILITIES_HTML}")" \
        --region "${AWS_REGION_USED}"
    fi
  fi

  echo "skipping athena sql execution"
fi

exit ${SCRAPER_EXIT}
