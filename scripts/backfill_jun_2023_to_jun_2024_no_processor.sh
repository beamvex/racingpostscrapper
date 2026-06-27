#!/usr/bin/env bash
set -euo pipefail

AWS_REGION="${AWS_REGION:-eu-west-2}"
CLUSTER_NAME="${CLUSTER_NAME:-racingpost-scraper}"
TERRAFORM_DIR="${TERRAFORM_DIR:-terraform}"

# Fixed date range (UTC): 2022-06-01..2024-06-12
START_DATE="${START_DATE:-2022-06-01}"
END_DATE="${END_DATE:-2024-06-12}"

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

  echo "date=$date taskArn=$task_arn" >&2

  # Print the taskArn as the function's stdout so callers can capture it.
  printf '%s\n' "$task_arn"
}

wait_for_task() {
  local date="$1"
  local task_arn="$2"

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

poll_in_flight() {
  # Mutates the arrays passed by name: removes completed tasks and increments failures.
  local -n _dates_ref="$1"
  local -n _arns_ref="$2"
  local -n _failures_ref="$3"

  if [[ "${#_arns_ref[@]}" -eq 0 ]]; then
    return 0
  fi

  local -a remaining_dates=()
  local -a remaining_arns=()

  for i in "${!_arns_ref[@]}"; do
    local d="${_dates_ref[$i]}"
    local arn="${_arns_ref[$i]}"

    local last_status
    last_status=$(aws ecs describe-tasks \
      --region "$AWS_REGION" \
      --cluster "$CLUSTER_NAME" \
      --tasks "$arn" \
      --query 'tasks[0].lastStatus' \
      --output text
    )

    if [[ "$last_status" == "STOPPED" ]]; then
      local exit_code
      exit_code=$(aws ecs describe-tasks \
        --region "$AWS_REGION" \
        --cluster "$CLUSTER_NAME" \
        --tasks "$arn" \
        --query 'tasks[0].containers[0].exitCode' \
        --output text
      )

      local reason
      reason=$(aws ecs describe-tasks \
        --region "$AWS_REGION" \
        --cluster "$CLUSTER_NAME" \
        --tasks "$arn" \
        --query 'tasks[0].stoppedReason' \
        --output text
      )

      echo "date=$d stopped exitCode=$exit_code reason=${reason}"

      if [[ "$exit_code" != "0" ]]; then
        echo "backfill: FAILED date=$d" >&2
        _failures_ref=$((_failures_ref+1))
      fi
    else
      echo "date=$d status=$last_status"
      remaining_dates+=("$d")
      remaining_arns+=("$arn")
    fi
  done

  _dates_ref=("${remaining_dates[@]}")
  _arns_ref=("${remaining_arns[@]}")
}

main() {
  echo "backfill: region=$AWS_REGION cluster=$CLUSTER_NAME"
  echo "backfill: terraform_dir=$TERRAFORM_DIR"
  echo "backfill: START_DATE=$START_DATE END_DATE=$END_DATE"

  local max_in_flight="${MAX_IN_FLIGHT:-20}"
  echo "backfill: MAX_IN_FLIGHT=$max_in_flight"

  echo "backfill: terraform init"
  terraform -chdir="$TERRAFORM_DIR" init -input=false >/dev/null

  local failures=0

  local -a dates
  mapfile -t dates < <(compute_date_range)

  local -a in_flight_dates=()
  local -a in_flight_arns=()

  local idx=0
  local total="${#dates[@]}"

  while [[ "$idx" -lt "$total" || "${#in_flight_arns[@]}" -gt 0 ]]; do
    while [[ "$idx" -lt "$total" && "${#in_flight_arns[@]}" -lt "$max_in_flight" ]]; do
      local d="${dates[$idx]}"
      idx=$((idx+1))

      echo "backfill: launching date=$d"

      local arn
      if ! arn="$(run_task_for_date "$d")"; then
        echo "backfill: FAILED date=$d" >&2
        failures=$((failures+1))
        continue
      fi

      in_flight_dates+=("$d")
      in_flight_arns+=("$arn")
      sleep "$SLEEP_BETWEEN_DAYS_SECONDS"
    done

    poll_in_flight in_flight_dates in_flight_arns failures

    if [[ "${#in_flight_arns[@]}" -gt 0 ]]; then
      echo "backfill: in_flight=${#in_flight_arns[@]} remaining=$((total-idx)) failures=$failures"
      sleep "$POLL_SECONDS"
    fi
  done

  if [[ "$failures" -gt 0 ]]; then
    echo "backfill: completed with failures=$failures" >&2
    exit 1
  fi

  echo "backfill: completed successfully"
}

main "$@"
