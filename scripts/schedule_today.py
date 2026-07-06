import json
import os
import subprocess
from datetime import datetime, timedelta, timezone


def _env(name: str) -> str:
    v = os.environ.get(name, "").strip()
    if not v:
        raise RuntimeError(f"missing env var: {name}")
    return v


def _parse_iso8601(s: str) -> datetime:
    t = s.strip()
    if not t:
        raise ValueError("empty time")

    # Common forms seen in the scraper output.
    if t.endswith("Z"):
        t = t[:-1] + "+00:00"
    return datetime.fromisoformat(t).astimezone(timezone.utc)


def _cron_expr(dt: datetime) -> str:
    dt = dt.astimezone(timezone.utc)
    return f"cron({dt.minute} {dt.hour} {dt.day} {dt.month} ? {dt.year})"


def _run(cmd: list[str]) -> None:
    subprocess.run(cmd, check=True)


def _put_rule(name: str, schedule_expression: str) -> None:
    region = os.environ.get("AWS_REGION", "eu-west-2")
    _run(
        [
            "aws",
            "events",
            "put-rule",
            "--name",
            name,
            "--schedule-expression",
            schedule_expression,
            "--state",
            "ENABLED",
            "--region",
            region,
        ]
    )


def _put_target(
    rule_name: str,
    target_id: str,
    cluster_arn: str,
    role_arn: str,
    task_def_arn: str,
    subnets_csv: str,
    security_groups_csv: str,
) -> None:
    region = os.environ.get("AWS_REGION", "eu-west-2")

    subnets = [s.strip() for s in subnets_csv.split(",") if s.strip()]
    sgs = [s.strip() for s in security_groups_csv.split(",") if s.strip()]

    if not subnets:
        raise RuntimeError("no subnets configured")
    if not sgs:
        raise RuntimeError("no security groups configured")

    ecs_params = {
        "TaskDefinitionArn": task_def_arn,
        "LaunchType": "FARGATE",
        "PlatformVersion": "LATEST",
        "NetworkConfiguration": {
            "awsvpcConfiguration": {
                "Subnets": subnets,
                "SecurityGroups": sgs,
                "AssignPublicIp": "ENABLED",
            }
        },
    }

    target = {
        "Id": target_id,
        "Arn": cluster_arn,
        "RoleArn": role_arn,
        "EcsParameters": ecs_params,
    }

    _run(
        [
            "aws",
            "events",
            "put-targets",
            "--rule",
            rule_name,
            "--targets",
            json.dumps([target]),
            "--region",
            region,
        ]
    )


def _is_uk_or_ire_course(course: str) -> bool:
    """UK courses have no suffix or (AW); Irish have (IRE). Exclude others (e.g. French)."""
    c = course.strip()
    if not c:
        return False
    if "(IRE)" in c:
        return True
    if "(AW)" in c:
        return True
    # No country suffix = UK
    if "(" not in c:
        return True
    return False


def _load_uk_ire_race_times(jsonl_path: str) -> list[datetime]:
    seen = set()
    out: list[datetime] = []

    with open(jsonl_path, "r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue

            course = (obj.get("course") or "").strip()
            if not _is_uk_or_ire_course(course):
                continue

            t = (obj.get("time") or "").strip()
            if not t:
                continue
            try:
                dt = _parse_iso8601(t)
            except Exception:
                continue
            key = dt.isoformat()
            if key in seen:
                continue
            seen.add(key)
            out.append(dt)

    out.sort()
    return out


def main() -> None:
    date_yyyy_mm_dd = os.environ.get("RESULTS_DATE", "").strip()
    if not date_yyyy_mm_dd:
        date_yyyy_mm_dd = datetime.now(timezone.utc).strftime("%Y-%m-%d")

    y, m, d = date_yyyy_mm_dd.split("-")

    runners_jsonl = os.environ.get(
        "RACECARD_RUNNERS_JSONL",
        f"/data/{y}/{m}/{d}/racingpost-racecards-{date_yyyy_mm_dd}-runners.jsonl",
    )

    cluster_arn = _env("ECS_CLUSTER_ARN")
    role_arn = _env("EVENTBRIDGE_ROLE_ARN")
    subnets_csv = _env("ECS_SUBNETS")
    security_groups_csv = _env("ECS_SECURITY_GROUPS")

    taskdef_pipeline = _env("ECS_TASKDEF_PIPELINE_ARN")

    times = _load_uk_ire_race_times(runners_jsonl)
    if not times:
        raise RuntimeError(f"no UK/IRE race times found in {runners_jsonl}")

    print(f"found {len(times)} unique UK/IRE race times")

    # Schedule pipeline 10 mins before each race
    for dt in times:
        dt_pre = dt - timedelta(minutes=10)
        stamp = dt.strftime("%Y%m%d-%H%M")
        rule_name = f"rps-pipeline-pre-{stamp}"

        _put_rule(rule_name, _cron_expr(dt_pre))
        _put_target(
            rule_name=rule_name,
            target_id="ecs-run-task",
            cluster_arn=cluster_arn,
            role_arn=role_arn,
            task_def_arn=taskdef_pipeline,
            subnets_csv=subnets_csv,
            security_groups_csv=security_groups_csv,
        )
        print(f"  scheduled pre-race  {dt_pre.strftime('%H:%M')}  for race at {dt.strftime('%H:%M')}")

    # Schedule pipeline 30 mins after the last race
    last_dt = times[-1]
    dt_post = last_dt + timedelta(minutes=30)
    stamp = last_dt.strftime("%Y%m%d-%H%M")
    rule_name = f"rps-pipeline-post-{stamp}"

    _put_rule(rule_name, _cron_expr(dt_post))
    _put_target(
        rule_name=rule_name,
        target_id="ecs-run-task",
        cluster_arn=cluster_arn,
        role_arn=role_arn,
        task_def_arn=taskdef_pipeline,
        subnets_csv=subnets_csv,
        security_groups_csv=security_groups_csv,
    )
    print(f"  scheduled post-race {dt_post.strftime('%H:%M')}  (30 min after last race at {last_dt.strftime('%H:%M')})")

    print("done")


if __name__ == "__main__":
    main()
