#!/bin/bash
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
if [ -n "$1" ]; then
  echo "date arg=$1"
  RESULTS_DATE_USED="$1"
  /app/target/release/racecards_time_order_scraper "$1"
else
  if [ -n "${RESULTS_DATE}" ]; then
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

  SQL_FILE="${OUT_DIR}/racingpost-racecards-${Y}-${M}-${D}-history.sql"
  if [ -f "${SQL_FILE}" ]; then
    echo "generated sql file ${SQL_FILE}"
    if [ -n "${ATHENA_OUTPUT_LOCATION:-}" ]; then
      echo "running athena query"
      AWS_REGION_USED="${AWS_REGION:-eu-west-2}"
      AWS_PROFILE_USED="${AWS_PROFILE:-}"
      if [ -n "${AWS_PROFILE_USED}" ]; then
        aws athena start-query-execution \
          --region "${AWS_REGION_USED}" \
          --profile "${AWS_PROFILE_USED}" \
          --query-string "$(cat "${SQL_FILE}")" \
          --query-execution-context Database=racingpost \
          --result-configuration "OutputLocation=${ATHENA_OUTPUT_LOCATION}"
      else
        aws athena start-query-execution \
          --region "${AWS_REGION_USED}" \
          --query-string "$(cat "${SQL_FILE}")" \
          --query-execution-context Database=racingpost \
          --result-configuration "OutputLocation=${ATHENA_OUTPUT_LOCATION}"
      fi
    else
      echo "ATHENA_OUTPUT_LOCATION not set; skipping athena execution"
    fi
  else
    echo "sql file not found at ${SQL_FILE}"
  fi
fi

exit ${SCRAPER_EXIT}
