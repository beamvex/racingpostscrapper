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
  /app/target/release/racingpost_scraper "$1"
else
  if [ -n "${RESULTS_DATE}" ]; then
    echo "RESULTS_DATE=${RESULTS_DATE}"
    RESULTS_DATE_USED="${RESULTS_DATE}"
  else
    echo "no date arg and RESULTS_DATE not set (default will be used)"
    RESULTS_DATE_USED=""
  fi
  /app/target/release/racingpost_scraper
fi

aws s3 cp /data "s3://${SCRAPER_DATA_BUCKET_NAME}/" --recursive

if [ -z "${RESULTS_DATE_USED}" ]; then
  RESULTS_DATE_USED=$(/app/target/release/racingpost_scraper --help 2>/dev/null || true)
fi

PROCESS_MONTH="${RESULTS_DATE_USED:0:7}"
if [ -n "${PROCESS_MONTH}" ]; then
  echo "running processor for month ${PROCESS_MONTH}"
  /app/process_captured_s3.sh "${PROCESS_MONTH}"
else
  echo "could not infer process month; skipping processor" >&2
fi

SCRAPER_EXIT=$?

kill ${CHROMIUM_PID} >/dev/null 2>&1 || true
kill ${XVFB_PID} >/dev/null 2>&1 || true

exit ${SCRAPER_EXIT}
