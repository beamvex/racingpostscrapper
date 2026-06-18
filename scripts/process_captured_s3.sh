#!/bin/bash
set -euo pipefail

if [ -z "${SCRAPER_DATA_BUCKET_NAME:-}" ]; then
  echo "SCRAPER_DATA_BUCKET_NAME is not set" >&2
  exit 1
fi

BUCKET="${SCRAPER_DATA_BUCKET_NAME}"
PROCESSED_PREFIX="${PROCESSED_PREFIX:-processed}"

TARGET_MONTH="${1:-${PROCESS_MONTH:-}}"
if [ -n "${TARGET_MONTH}" ]; then
  YEAR="${TARGET_MONTH%-*}"
  MONTH="${TARGET_MONTH#*-}"
  if [ -z "${YEAR}" ] || [ -z "${MONTH}" ]; then
    echo "invalid TARGET_MONTH (expected YYYY-MM): ${TARGET_MONTH}" >&2
    exit 1
  fi
  echo "filtering to month ${YEAR}-${MONTH}"
fi

mkdir -p /data/captured

echo "listing captured full-results html dirs in s3://${BUCKET}/"
DIRS=$(aws s3 ls "s3://${BUCKET}/" --recursive | awk '{print $4}' | grep -- '-time-order-full-results-html/[^/]*\.html$' | sed 's#[^/]*\.html$##' | sort -u || true)

if [ -n "${TARGET_MONTH}" ]; then
  DIRS=$(echo "${DIRS}" | grep -E "(^|/)${YEAR}/${MONTH}/" || true)
fi

if [ -z "${DIRS}" ]; then
  echo "no captured full-results html found" >&2
  exit 0
fi

for dir_key in ${DIRS}; do
  rel_dir=$(dirname "${dir_key}")
  html_dir_name=$(basename "${dir_key}")

  local_dir="/data/captured/${rel_dir}"
  mkdir -p "${local_dir}"

  echo "downloading html dir s3://${BUCKET}/${dir_key} -> ${local_dir}/${html_dir_name}/"
  aws s3 cp "s3://${BUCKET}/${dir_key}" "${local_dir}/${html_dir_name}/" --recursive

  json_out="${local_dir}/${html_dir_name/-time-order-full-results-html/-time-order-full-results.json}"
  json_name=$(basename "${json_out}")

  IFS='/' read -r y m d rest <<< "${rel_dir}"
  if [ -z "${y}" ] || [ -z "${m}" ] || [ -z "${d}" ]; then
    echo "could not infer y/m/d from rel_dir=${rel_dir}, uploading to ${PROCESSED_PREFIX}/${rel_dir}/" >&2
    processed_key="${PROCESSED_PREFIX}/${rel_dir}/${json_name}"
  else
    processed_key="${PROCESSED_PREFIX}/year=${y}/month=${m}/day=${d}/${json_name}"
  fi

  echo "parsing ${local_dir}/${html_dir_name}"
  /app/target/release/full_result_html_dir_parser --html-dir "${local_dir}/${html_dir_name}" --out-dir "${local_dir}"

  echo "uploading ${json_out} -> s3://${BUCKET}/${processed_key}"
  aws s3 cp "${json_out}" "s3://${BUCKET}/${processed_key}"
done
