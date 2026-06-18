#!/bin/bash
set -euo pipefail

: "${AWS_REGION:=eu-west-2}"

: "${ECS_CLUSTER_ARN:?set ECS_CLUSTER_ARN}"
: "${PROCESSOR_TASK_DEFINITION_ARN:?set PROCESSOR_TASK_DEFINITION_ARN}"
: "${ECS_SUBNETS_CSV:?set ECS_SUBNETS_CSV (e.g. subnet-aaa,subnet-bbb)}"
: "${ECS_SECURITY_GROUP_ID:?set ECS_SECURITY_GROUP_ID}"

: "${PROCESS_START_MONTH:=2024-06}"
: "${PROCESS_END_MONTH:=2026-06}"

: "${LAUNCH_TYPE:=FARGATE}"
: "${ASSIGN_PUBLIC_IP:=ENABLED}"

months=$(python3 - <<PY
from datetime import date

def parse_ym(s: str):
    y, m = s.split("-", 1)
    return int(y), int(m)

sy, sm = parse_ym("${PROCESS_START_MONTH}")
ey, em = parse_ym("${PROCESS_END_MONTH}")

y, m = sy, sm
out = []
while (y, m) <= (ey, em):
    out.append(f"{y:04d}-{m:02d}")
    m += 1
    if m == 13:
        m = 1
        y += 1

print("\n".join(out))
PY
)

for month in ${months}; do
  echo "trigger: running processor for ${month}"

  aws ecs run-task \
    --region "${AWS_REGION}" \
    --cluster "${ECS_CLUSTER_ARN}" \
    --launch-type "${LAUNCH_TYPE}" \
    --task-definition "${PROCESSOR_TASK_DEFINITION_ARN}" \
    --network-configuration "awsvpcConfiguration={subnets=[${ECS_SUBNETS_CSV}],securityGroups=[${ECS_SECURITY_GROUP_ID}],assignPublicIp=${ASSIGN_PUBLIC_IP}}" \
    --overrides "$(cat <<JSON
{
  "containerOverrides": [
    {
      "name": "processor",
      "command": ["${month}"]
    }
  ]
}
JSON
)"
done
