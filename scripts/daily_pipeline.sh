#!/bin/bash
set -euo pipefail
export DISPLAY=:99

echo "=== daily pipeline starting ==="

# --- shared browser setup ---
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

# --- determine date ---
if [ -n "${1:-}" ]; then
  RESULTS_DATE_USED="${1}"
elif [ -n "${RESULTS_DATE:-}" ]; then
  RESULTS_DATE_USED="${RESULTS_DATE}"
else
  RESULTS_DATE_USED="$(date -u +%F)"
fi
echo "date=${RESULTS_DATE_USED}"

Y="${RESULTS_DATE_USED%%-*}"
REST="${RESULTS_DATE_USED#*-}"
M="${REST%%-*}"
D="${REST#*-}"
OUT_DIR="/data/${Y}/${M}/${D}"
AWS_REGION_USED="${AWS_REGION:-eu-west-2}"
AWS_PROFILE_USED="${AWS_PROFILE:-}"

aws_cp() {
  if [ -n "${AWS_PROFILE_USED}" ]; then
    aws s3 cp "$@" --region "${AWS_REGION_USED}" --profile "${AWS_PROFILE_USED}"
  else
    aws s3 cp "$@" --region "${AWS_REGION_USED}"
  fi
}

aws_sync() {
  if [ -n "${AWS_PROFILE_USED}" ]; then
    aws s3 sync "$@" --region "${AWS_REGION_USED}" --profile "${AWS_PROFILE_USED}"
  else
    aws s3 sync "$@" --region "${AWS_REGION_USED}"
  fi
}

# ============================================================
# STEP 1: Scrape today's results
# ============================================================
echo "=== step 1: scraping results ==="
/app/target/release/racingpost_scraper "${RESULTS_DATE_USED}"
RESULTS_EXIT=$?

if [ "${RESULTS_EXIT}" -ne 0 ]; then
  echo "results scraper exited with ${RESULTS_EXIT}" >&2
fi

# ============================================================
# STEP 2: Upload raw data to S3
# ============================================================
if [ -n "${SCRAPER_DATA_BUCKET_NAME:-}" ]; then
  echo "=== step 2: uploading raw data to s3 ==="
  aws_sync /data/ "s3://${SCRAPER_DATA_BUCKET_NAME}/"
fi

# ============================================================
# STEP 3: Process results into parquet
# ============================================================
if [ -n "${SCRAPER_DATA_BUCKET_NAME:-}" ]; then
  echo "=== step 3: processing results into parquet ==="
  PROCESS_MONTH="${Y}-${M}"
  /app/process_captured_s3.sh "${PROCESS_MONTH}" || echo "process_captured_s3.sh failed (may be ok if no new data)" >&2
fi

# ============================================================
# STEP 4: Restart Chromium (results scraper closed it)
# ============================================================
echo "=== restarting chromium for racecard scrape ==="
chromium \
  --no-sandbox \
  --disable-dev-shm-usage \
  --disable-gpu \
  --disable-software-rasterizer \
  --remote-debugging-address=0.0.0.0 \
  --remote-debugging-port=9222 \
  about:blank &
CHROMIUM_PID=$!

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

# ============================================================
# STEP 5: Scrape today's racecard
# ============================================================
echo "=== step 5: scraping racecard ==="
if [ -n "${RACE_URL:-}" ]; then
  echo "scraping specific race URL: ${RACE_URL}"
  /app/target/release/racecards_time_order_scraper "${RESULTS_DATE_USED}" --url "${RACE_URL}"
else
  /app/target/release/racecards_time_order_scraper "${RESULTS_DATE_USED}"
fi
RACECARD_EXIT=$?

if [ "${RACECARD_EXIT}" -ne 0 ]; then
  echo "racecard scraper exited with ${RACECARD_EXIT}" >&2
  kill ${CHROMIUM_PID} >/dev/null 2>&1 || true
  kill ${XVFB_PID} >/dev/null 2>&1 || true
  exit ${RACECARD_EXIT}
fi

# ============================================================
# STEP 6: Parse racecard HTML into runners parquet
# ============================================================
HTML_DIR="${OUT_DIR}/racingpost-racecards-${RESULTS_DATE_USED}-racecards-html"
if [ -n "${RACE_TIME:-}" ]; then
  RUNNERS_OUT="${OUT_DIR}/racingpost-racecards-${RESULTS_DATE_USED}-${RACE_TIME}-runners.parquet"
else
  RUNNERS_OUT="${OUT_DIR}/racingpost-racecards-${RESULTS_DATE_USED}-runners.parquet"
fi

echo "=== step 6: parsing racecard html ==="
/app/target/release/racecard_html_dir_parser --html-dir "${HTML_DIR}" --out "${RUNNERS_OUT}"

# ============================================================
# STEP 7: Upload runners parquet to S3
# ============================================================
if [ -n "${SCRAPER_DATA_BUCKET_NAME:-}" ] && [ -f "${RUNNERS_OUT}" ]; then
  echo "=== step 7: uploading runners parquet ==="
  S3_RACECARDS_PREFIX="s3://${SCRAPER_DATA_BUCKET_NAME}/racecards/${Y}/${M}/${D}/"
  aws_cp "${RUNNERS_OUT}" "${S3_RACECARDS_PREFIX}$(basename "${RUNNERS_OUT}")"
fi

# ============================================================
# STEP 8: Download parquet files from S3
# ============================================================
HISTORY_DIR="${OUT_DIR}/history_parquet"
mkdir -p "${HISTORY_DIR}"
if [ -n "${SCRAPER_DATA_BUCKET_NAME:-}" ]; then
  echo "=== step 8: downloading parquet files ==="
  S3_PROCESSED_PREFIX="s3://${SCRAPER_DATA_BUCKET_NAME}/processed/"
  aws_sync "${S3_PROCESSED_PREFIX}" "${HISTORY_DIR}" \
    --exclude "*" \
    --include "*.parquet"
fi

# ============================================================
# STEP 9: Compute probabilities and write parquet report
# ============================================================
RUN_TS=$(TZ="Europe/London" date +%H%M%S)
if [ -n "${RACE_TIME:-}" ]; then
  PROBABILITIES_PARQUET="${OUT_DIR}/racecard-probabilities-${RESULTS_DATE_USED}-${RACE_TIME}.parquet"
else
  PROBABILITIES_PARQUET="${OUT_DIR}/racecard-probabilities-${RESULTS_DATE_USED}-${RUN_TS}.parquet"
fi
echo "=== step 9: computing probabilities ==="
/app/target/release/today_first_race_table \
  --in="${RUNNERS_OUT}" \
  --history-dir="${HISTORY_DIR}" \
  --out="${PROBABILITIES_PARQUET}"

# ============================================================
# STEP 10: Upload probabilities parquet to S3
# ============================================================
if [ -n "${SCRAPER_DATA_BUCKET_NAME:-}" ] && [ -f "${PROBABILITIES_PARQUET}" ]; then
  echo "=== step 10: uploading probabilities parquet ==="
  S3_PROBABILITIES_PREFIX="s3://${SCRAPER_DATA_BUCKET_NAME}/probabilities/${Y}/${M}/${D}/"
  aws_cp "${PROBABILITIES_PARQUET}" "${S3_PROBABILITIES_PREFIX}$(basename "${PROBABILITIES_PARQUET}")"
fi

# --- cleanup ---
kill ${CHROMIUM_PID} >/dev/null 2>&1 || true
kill ${XVFB_PID} >/dev/null 2>&1 || true

echo "=== daily pipeline complete ==="
exit 0
