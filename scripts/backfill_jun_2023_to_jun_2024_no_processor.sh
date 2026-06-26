#!/usr/bin/env bash
set -euo pipefail

AWS_REGION="${AWS_REGION:-eu-west-2}"
CLUSTER_NAME="${CLUSTER_NAME:-racingpost-scraper}"
TERRAFORM_DIR="${TERRAFORM_DIR:-terraform}"

# Fixed date range (UTC): 2023-06-01..2024-06-30
START_DATE="${START_DATE:-2023-06-01}"
END_DATE="${END_DATE:-2024-06-30}"

# Controls:
SLEEP_BETWEEN_DAYS_SECONDS="${SLEEP_BETWEEN_DAYS_SECONDS:-2}"
POLL_SECONDS="${POLL_SECONDS:-10}"

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 1
  }
}

require_cmd aws
require_cmd terraform
require_cmd python3

tf_output_raw() {
  terraform -chdir="$TERRAFORM_DIR" output -raw "$1"
}

tf_output_json() {
  terraform -chdir="$TERRAFORM_DIR" output -json "$1"
}

get_network_configuration() {
  local subnets_json
  subnets_json="$(tf_output_json public_subnet_ids)"

  local sg
  sg="$(tf_output_raw ecs_tasks_security_group_id)"

  python3 - <<PY
import json
subs = json.loads('''$subnets_json''')
print(','.join(subs))
print('$sg')
PY
}

compute_date_range() {
  START_DATE="$START_DATE" END_DATE="$END_DATE" python3 - <<'PY'
import os
from datetime import datetime, timedelta, timezone

start = os.environ.get('START_DATE') or ''
end = os.environ.get('END_DATE') or ''

s = datetime.strptime(start, '%Y-%m-%d').replace(tzinfo=timezone.utc)
e = datetime.strptime(end, '%Y-%m-%d').replace(tzinfo=timezone.utc)

if s > e:
    raise SystemExit('START_DATE must be <= END_DATE')

d = s
while d <= e:
    print(d.strftime('%Y-%m-%d'))
    d += timedelta(days=1)
PY
}

run_task_for_date() {
  local date="$1"
  local task_def_arn
  task_def_arn="$(tf_output_raw ecs_task_definition_arn)"

  readarray -t net < <(get_network_configuration)
  local subnets_csv="${net[0]}"
  local sg_id="${net[1]}"

  # IMPORTANT:
  # We explicitly set ATHENA_OUTPUT_LOCATION to an empty string to prevent the
  # in-container scripts from running the Athena/processor step per day.
  local overrides
  overrides=$(python3 - <<PY
import json
print(json.dumps({
  "containerOverrides": [
    {
      "name": "scraper",
      "environment": [
        {"name": "RESULTS_DATE", "value": "$date"},
        {"name": "ATHENA_OUTPUT_LOCATION", "value": ""}
      ]
    }
  ]
}))
PY
)

  local task_arn
  task_arn=$(aws ecs run-task \
    --region "$AWS_REGION" \
    --cluster "$CLUSTER_NAME" \
    --launch-type FARGATE \
    --task-definition "$task_def_arn" \
    --network-configuration "awsvpcConfiguration={subnets=[${subnets_csv}],securityGroups=[${sg_id}],assignPublicIp=ENABLED}" \
    --overrides "$overrides" \
    --query 'tasks[0].taskArn' \
    --output text
  )

  if [[ -z "$task_arn" || "$task_arn" == "None" ]]; then
    echo "run-task failed for date=$date (no taskArn returned)" >&2
    return 1
  fi

  echo "date=$date taskArn=$task_arn"

  while true; do
    local last_status
    last_status=$(aws ecs describe-tasks \
      --region "$AWS_REGION" \
      --cluster "$CLUSTER_NAME" \
      --tasks "$task_arn" \
      --query 'tasks[0].lastStatus' \
      --output text
    )

    if [[ "$last_status" == "STOPPED" ]]; then
      local exit_code
      exit_code=$(aws ecs describe-tasks \
        --region "$AWS_REGION" \
        --cluster "$CLUSTER_NAME" \
        --tasks "$task_arn" \
        --query 'tasks[0].containers[0].exitCode' \
        --output text
      )

      local reason
      reason=$(aws ecs describe-tasks \
        --region "$AWS_REGION" \
        --cluster "$CLUSTER_NAME" \
        --tasks "$task_arn" \
        --query 'tasks[0].stoppedReason' \
        --output text
      )

      echo "date=$date stopped exitCode=$exit_code reason=${reason}"

      if [[ "$exit_code" != "0" ]]; then
        return 1
      fi
      return 0
    fi

    echo "date=$date status=$last_status waiting..."
    sleep "$POLL_SECONDS"
  done
}

main() {
  echo "backfill: region=$AWS_REGION cluster=$CLUSTER_NAME"
  echo "backfill: terraform_dir=$TERRAFORM_DIR"
  echo "backfill: START_DATE=$START_DATE END_DATE=$END_DATE"

  echo "backfill: terraform init"
  terraform -chdir="$TERRAFORM_DIR" init -input=false >/dev/null

  local failures=0

  local -a dates
  mapfile -t dates < <(compute_date_range)

  for d in "${dates[@]}"; do
    echo "backfill: running date=$d"
    if ! run_task_for_date "$d"; then
      echo "backfill: FAILED date=$d" >&2
      failures=$((failures+1))
    fi
    sleep "$SLEEP_BETWEEN_DAYS_SECONDS"
  done

  if [[ "$failures" -gt 0 ]]; then
    echo "backfill: completed with failures=$failures" >&2
    exit 1
  fi

  echo "backfill: completed successfully"
}

main "$@"
