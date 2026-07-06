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

RESULTS_DATE="${RESULTS_DATE:-$(date -u +%F)}"
echo "scraping racecard for ${RESULTS_DATE}"

/app/target/release/racecards_time_order_scraper "${RESULTS_DATE}"

Y="${RESULTS_DATE%%-*}"
REST="${RESULTS_DATE#*-}"
M="${REST%%-*}"
D="${REST#*-}"

HTML_DIR="/data/${Y}/${M}/${D}/racingpost-racecards-${RESULTS_DATE}-racecards-html"
RUNNERS_OUT="/data/${Y}/${M}/${D}/racingpost-racecards-${RESULTS_DATE}-runners.jsonl"

echo "parsing racecard html"
/app/target/release/racecard_html_dir_parser --html-dir "${HTML_DIR}" --out "${RUNNERS_OUT}"

kill ${CHROMIUM_PID} >/dev/null 2>&1 || true
kill ${XVFB_PID} >/dev/null 2>&1 || true

echo "running scheduler"
export RACECARD_RUNNERS_JSONL="${RUNNERS_OUT}"
python3 /app/schedule_today.py
