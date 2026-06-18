#!/bin/bash
set -euo pipefail

if [ -z "${SCRAPER_DATA_BUCKET_NAME:-}" ]; then
  echo "SCRAPER_DATA_BUCKET_NAME is not set" >&2
  exit 1
fi

BUCKET="${SCRAPER_DATA_BUCKET_NAME}"
PROCESSED_PREFIX="${PROCESSED_PREFIX:-processed}"

mkdir -p /data/captured

echo "listing captured full-results html dirs in s3://${BUCKET}/"
DIRS=$(aws s3 ls "s3://${BUCKET}/" --recursive | awk '{print $4}' | grep -- '-time-order-full-results-html/[^/]*\.html$' | sed 's#[^/]*\.html$##' | sort -u || true)

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
  processed_key="${PROCESSED_PREFIX}/${rel_dir}/${json_name}"

  if aws s3 ls "s3://${BUCKET}/${processed_key}" >/dev/null 2>&1; then
    echo "skipping (already processed) s3://${BUCKET}/${processed_key}"
    continue
  fi

  echo "parsing ${local_dir}/${html_dir_name}"
  /app/target/release/full_result_html_dir_parser --html-dir "${local_dir}/${html_dir_name}" --out-dir "${local_dir}"

  echo "uploading ${json_out} -> s3://${BUCKET}/${processed_key}"
  aws s3 cp "${json_out}" "s3://${BUCKET}/${processed_key}"
done
