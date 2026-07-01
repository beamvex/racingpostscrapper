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

  SQL_FILE="${OUT_DIR}/racingpost-racecards-${Y}-${M}-${D}-history.sql"
  if [ -f "${SQL_FILE}" ]; then
    echo "generated sql file ${SQL_FILE}"
    if [ -n "${ATHENA_OUTPUT_LOCATION:-}" ]; then
      echo "running athena query"
      AWS_REGION_USED="${AWS_REGION:-eu-west-2}"
      AWS_PROFILE_USED="${AWS_PROFILE:-}"

      athena_start() {
        local q="$1"
        if [ -n "${AWS_PROFILE_USED}" ]; then
          aws athena start-query-execution \
            --region "${AWS_REGION_USED}" \
            --profile "${AWS_PROFILE_USED}" \
            --work-group primary \
            --query-string "${q}" \
            --query-execution-context Database=racingpost \
            --result-configuration "OutputLocation=${ATHENA_OUTPUT_LOCATION}" \
            --output text \
            --query 'QueryExecutionId'
        else
          aws athena start-query-execution \
            --region "${AWS_REGION_USED}" \
            --work-group primary \
            --query-string "${q}" \
            --query-execution-context Database=racingpost \
            --result-configuration "OutputLocation=${ATHENA_OUTPUT_LOCATION}" \
            --output text \
            --query 'QueryExecutionId'
        fi
      }

      athena_wait() {
        local id="$1"
        local state=""
        local i=0
        while true; do
          if [ -n "${AWS_PROFILE_USED}" ]; then
            state=$(aws athena get-query-execution \
              --region "${AWS_REGION_USED}" \
              --profile "${AWS_PROFILE_USED}" \
              --query-execution-id "${id}" \
              --output text \
              --query 'QueryExecution.Status.State')
          else
            state=$(aws athena get-query-execution \
              --region "${AWS_REGION_USED}" \
              --query-execution-id "${id}" \
              --output text \
              --query 'QueryExecution.Status.State')
          fi

          if [ "${state}" = "SUCCEEDED" ]; then
            return 0
          fi
          if [ "${state}" = "FAILED" ] || [ "${state}" = "CANCELLED" ]; then
            if [ -n "${AWS_PROFILE_USED}" ]; then
              aws athena get-query-execution \
                --region "${AWS_REGION_USED}" \
                --profile "${AWS_PROFILE_USED}" \
                --query-execution-id "${id}" \
                --query 'QueryExecution.Status.StateChangeReason' \
                --output text >&2 || true
            else
              aws athena get-query-execution \
                --region "${AWS_REGION_USED}" \
                --query-execution-id "${id}" \
                --query 'QueryExecution.Status.StateChangeReason' \
                --output text >&2 || true
            fi
            return 1
          fi

          i=$((i+1))
          if [ $i -gt 900 ]; then
            echo "athena query timed out id=${id}" >&2
            return 1
          fi
          sleep 2
        done
      }

      while IFS= read -r -d '' stmt; do
        stmt_trim=$(echo "${stmt}" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
        if [ -z "${stmt_trim}" ]; then
          continue
        fi
        first_line=$(printf "%s" "${stmt_trim}" | awk 'NR==1{print; exit}')
        if printf "%s" "${stmt_trim}" | grep -q "external_location"; then
          echo "athena executing: ${first_line} (has external_location)"

          if [ "${ATHENA_CLEAN_EXTERNAL_LOCATION:-1}" = "1" ]; then
            external_loc=$(printf "%s" "${stmt_trim}" | sed -n "s/.*external_location[[:space:]]*=[[:space:]]*'\([^']*\)'.*/\1/p")
            if [ -n "${external_loc}" ]; then
              echo "cleaning external_location prefix before CTAS: ${external_loc}"
              if [ -n "${AWS_PROFILE_USED}" ]; then
                aws s3 rm --recursive "${external_loc}" \
                  --region "${AWS_REGION_USED}" \
                  --profile "${AWS_PROFILE_USED}" >/dev/null
              else
                aws s3 rm --recursive "${external_loc}" \
                  --region "${AWS_REGION_USED}" >/dev/null
              fi
            fi
          fi
        else
          echo "athena executing: ${first_line}"
        fi
        if ! qid=$(athena_start "${stmt_trim}"); then
          echo "athena start-query-execution failed" >&2
          exit 1
        fi
        if [ -z "${qid}" ]; then
          echo "athena start-query-execution returned empty QueryExecutionId" >&2
          exit 1
        fi
        echo "athena query id=${qid}"
        athena_wait "${qid}"
      done < <(awk 'BEGIN{RS=";"; ORS="\0"} {print}' "${SQL_FILE}")
    else
      echo "ATHENA_OUTPUT_LOCATION not set; skipping athena execution"
    fi
  else
    echo "sql file not found at ${SQL_FILE}"
  fi
fi

exit ${SCRAPER_EXIT}
