import json
import os
import subprocess
from datetime import datetime, timezone

from schedule_utils import run, run_quiet


def account_id() -> str:
    cluster_arn = os.environ.get("ECS_CLUSTER_ARN", "")
    if cluster_arn:
        parts = cluster_arn.split(":")
        if len(parts) >= 5:
            return parts[4]
    result = subprocess.run(
        ["aws", "sts", "get-caller-identity", "--query", "Account", "--output", "text"],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip()


def schedule_exists(name: str) -> bool:
    region = os.environ.get("AWS_REGION", "eu-west-2")
    return run_quiet(
        ["aws", "scheduler", "get-schedule", "--name", name, "--region", region]
    )


def list_schedule_names(prefix: str) -> list[str]:
    region = os.environ.get("AWS_REGION", "eu-west-2")
    result = subprocess.run(
        ["aws", "scheduler", "list-schedules", "--name-prefix", prefix,
         "--region", region, "--output", "json"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return []
    try:
        data = json.loads(result.stdout)
    except json.JSONDecodeError:
        return []
    return [s["Name"] for s in data.get("Schedules", [])]


def delete_schedule(name: str) -> None:
    region = os.environ.get("AWS_REGION", "eu-west-2")
    run_quiet(["aws", "scheduler", "delete-schedule", "--name", name, "--region", region])


def cleanup_old_schedules(prefix: str, today_str: str) -> int:
    names = list_schedule_names(prefix)
    deleted = 0
    for name in names:
        rest = name[len(prefix):]
        if len(rest) >= 13:
            schedule_date = rest[4:12]
            if schedule_date < today_str:
                print(f"  deleting old schedule: {name}")
                delete_schedule(name)
                deleted += 1
    return deleted


def create_schedule(
    name: str,
    dt: datetime,
    lambda_arn: str,
    race_url: str | None = None,
    race_time: str | None = None,
) -> None:
    region = os.environ.get("AWS_REGION", "eu-west-2")
    role_arn = f"arn:aws:iam::{account_id()}:role/racingpost-scraper-lambda"

    target: dict = {"Arn": lambda_arn, "RoleArn": role_arn}
    payload: dict = {}
    if race_url:
        payload["race_url"] = race_url
    if race_time:
        payload["race_time"] = race_time
    if payload:
        target["Input"] = json.dumps(payload)

    at_expr = dt.astimezone(timezone.utc).strftime("at(%Y-%m-%dT%H:%M:%S)")

    run(
        [
            "aws", "scheduler", "create-schedule",
            "--name", name,
            "--schedule-expression", at_expr,
            "--schedule-expression-timezone", "UTC",
            "--flexible-time-window", json.dumps({"Mode": "OFF"}),
            "--target", json.dumps(target),
            "--action-after-completion", "DELETE",
            "--region", region,
        ]
    )
