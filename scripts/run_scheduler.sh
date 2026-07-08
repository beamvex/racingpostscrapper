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
echo "scraping time-order page for ${RESULTS_DATE}"

# Only scrape the time-order page (not individual racecards)
# The scheduler just needs race times, not runner details
/app/target/release/racecards_time_order_scraper "${RESULTS_DATE}"

kill ${CHROMIUM_PID} >/dev/null 2>&1 || true
kill ${XVFB_PID} >/dev/null 2>&1 || true

echo "running scheduler"
Y="${RESULTS_DATE%%-*}"
REST="${RESULTS_DATE#*-}"
M="${REST%%-*}"
D="${REST#*-}"
export TIME_ORDER_HTML="/data/${Y}/${M}/${D}/racingpost-racecards-${RESULTS_DATE}.html"
python3 /app/schedule_today.py
