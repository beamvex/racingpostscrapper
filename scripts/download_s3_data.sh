#!/bin/bash
set -euo pipefail

BUCKET="racingpost-scraper-data-20260612163104863900000001"
DATA_DIR="$(dirname "$0")/data"
PROFILE="${AWS_PROFILE:-512752756525_AdministratorAccess}"

mkdir -p "${DATA_DIR}/processed"
mkdir -p "${DATA_DIR}/racecards"
mkdir -p "${DATA_DIR}/probabilities"

echo "downloading processed/ from s3://${BUCKET}/processed/ to ${DATA_DIR}/processed/"
aws s3 sync "s3://${BUCKET}/processed/" "${DATA_DIR}/processed/" \
  --profile "${PROFILE}" \
  --exclude "*" \
  --include "*.parquet"

echo "downloading racecards/ from s3://${BUCKET}/racecards/ to ${DATA_DIR}/racecards/"
aws s3 sync "s3://${BUCKET}/racecards/" "${DATA_DIR}/racecards/" \
  --profile "${PROFILE}"

echo "downloading probabilities/ from s3://${BUCKET}/probabilities/ to ${DATA_DIR}/probabilities/"
aws s3 sync "s3://${BUCKET}/probabilities/" "${DATA_DIR}/probabilities/" \
  --profile "${PROFILE}" \
  --exclude "*" \
  --include "*.parquet"

echo "done"
