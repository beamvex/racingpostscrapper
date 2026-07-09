import json
import os
import re
import subprocess
from datetime import datetime, timedelta, timezone
from zoneinfo import ZoneInfo

TZ_LONDON = ZoneInfo("Europe/London")


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
    dt = datetime.fromisoformat(t)
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=TZ_LONDON)
    return dt.astimezone(timezone.utc)


def _cron_expr(dt: datetime) -> str:
    dt = dt.astimezone(timezone.utc)
    return f"cron({dt.minute} {dt.hour} {dt.day} {dt.month} ? {dt.year})"


def _run(cmd: list[str]) -> None:
    subprocess.run(cmd, check=True)


def _run_quiet(cmd: list[str]) -> bool:
    """Run a command, return True if it succeeded (exit 0)."""
    result = subprocess.run(cmd, capture_output=True)
    return result.returncode == 0


def _rule_exists(name: str) -> bool:
    region = os.environ.get("AWS_REGION", "eu-west-2")
    return _run_quiet(
        ["aws", "events", "describe-rule", "--name", name, "--region", region]
    )


def _list_rule_names(prefix: str) -> list[str]:
    region = os.environ.get("AWS_REGION", "eu-west-2")
    result = subprocess.run(
        ["aws", "events", "list-rules", "--name-prefix", prefix, "--region", region, "--output", "json"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        return []
    try:
        data = json.loads(result.stdout)
    except json.JSONDecodeError:
        return []
    return [r["Name"] for r in data.get("Rules", [])]


def _delete_rule(name: str) -> None:
    region = os.environ.get("AWS_REGION", "eu-west-2")
    # Remove targets first
    _run_quiet(
        ["aws", "events", "remove-targets", "--rule", name, "--ids", "ecs-run-task", "--region", region]
    )
    _run_quiet(
        ["aws", "events", "delete-rule", "--name", name, "--region", region]
    )


def _cleanup_old_rules(prefix: str, today_str: str) -> int:
    """Delete rules with the given prefix whose names contain a date before today_str.
    Returns the number of rules deleted."""
    names = _list_rule_names(prefix)
    deleted = 0
    for name in names:
        # Extract date part: rps-pipeline-pre-YYYYMMDD-HHMM or rps-pipeline-post-YYYYMMDD-HHMM
        # After prefix, skip "pre-" or "post-" (4 chars), then 8-char date
        rest = name[len(prefix):]  # e.g. "pre-20260706-1315" or "post-20260706-1315"
        if len(rest) >= 13:  # pre-/post- (4) + YYYYMMDD (8) + at least "-"
            rule_date = rest[4:12]  # skip "pre-" or "post-"
            if rule_date < today_str:
                print(f"  deleting old rule: {name}")
                _delete_rule(name)
                deleted += 1
    return deleted


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
    """Check if course is UK or Irish, handling both display names and URL slugs."""
    c = course.strip().lower()
    if not c:
        return False
    # Display name patterns
    if "(ire)" in c:
        return True
    if "(aw)" in c:
        return True
    # No parentheses in display name = UK
    if "(" not in c and " " in c:
        return True
    # URL slug patterns
    if c.endswith("-aw") or c.endswith("-aw-gb"):
        return True
    # Slugs with hyphens that aren't AW are non-UK/IRE (e.g. happy-valley, santa-anita)
    if "-" in c:
        return False
    # Plain slug without hyphens: likely UK/IRE (e.g. ayr, ascot, roscommon)
    return True


def _try_parse_time(time_str: str, date_yyyy_mm_dd: str) -> datetime | None:
    """Parse a time string that may be ISO 8601 or plain HH:MM."""
    t = time_str.strip()
    if not t:
        return None
    # Try ISO 8601 first
    try:
        return _parse_iso8601(t)
    except Exception:
        pass
    # Try HH:MM
    m = re.match(r'^(\d{1,2}):(\d{2})$', t)
    if m:
        try:
            return datetime(
                int(date_yyyy_mm_dd[:4]),
                int(date_yyyy_mm_dd[5:7]),
                int(date_yyyy_mm_dd[8:10]),
                int(m.group(1)), int(m.group(2)),
                tzinfo=TZ_LONDON,
            ).astimezone(timezone.utc)
        except Exception:
            pass
    return None


def _course_from_url(url: str) -> str:
    """Extract course slug from racecard URL.
    e.g. https://www.racingpost.com/racecards/3/ayr/2026-07-06/922295 -> ayr"""
    parts = url.rstrip("/").split("/")
    # URL: .../racecards/<id>/<course_slug>/<date>/<race_id>
    try:
        idx = parts.index("racecards")
        return parts[idx + 2]
    except (ValueError, IndexError):
        return ""


def _extract_race_times_from_time_order_html(html_path: str, date_yyyy_mm_dd: str) -> list[datetime]:
    """Parse the time-order page HTML to extract race times for UK/IRE courses."""
    with open(html_path, "r", encoding="utf-8") as f:
        html = f.read()

    seen = set()
    out: list[datetime] = []

    # Try Next.js data first
    marker = '<script id="__NEXT_DATA__" type="application/json">'
    idx = html.find(marker)
    if idx != -1:
        start = idx + len(marker)
        end = html.find("</script>", start)
        if end != -1:
            try:
                next_data = json.loads(html[start:end])
            except json.JSONDecodeError:
                next_data = None

            if next_data:
                # Walk the Next.js data for race entries with time + course
                def walk(obj, depth=0):
                    if depth > 20:
                        return
                    if isinstance(obj, dict):
                        course = (obj.get("courseName") or obj.get("course") or
                                  obj.get("meetingName") or obj.get("meeting") or
                                  obj.get("trackName") or obj.get("venue") or "").strip()
                        time_str = (obj.get("raceTime") or obj.get("time") or
                                    obj.get("offTime") or obj.get("startTime") or
                                    obj.get("scheduledTime") or "").strip()
                        if course and time_str and _is_uk_or_ire_course(course):
                            dt = _try_parse_time(time_str, date_yyyy_mm_dd)
                            if dt:
                                key = dt.isoformat()
                                if key not in seen:
                                    seen.add(key)
                                    out.append(dt)
                        for v in obj.values():
                            walk(v, depth + 1)
                    elif isinstance(obj, list):
                        for v in obj:
                            walk(v, depth + 1)
                walk(next_data)

    if out:
        out.sort()
        print(f"  extracted {len(out)} times from Next.js data")
        return out

    # Fallback: extract from racecard URLs (relative or absolute) and nearby times
    url_pattern = re.compile(r'(?:https://www\.racingpost\.com)?/racecards/\d+/[^/"]+/\d{4}-\d{2}-\d{2}/\d+')
    time_pattern = re.compile(r'\b(\d{2}:\d{2})\b')

    url_positions = [(m.start(), m.group()) for m in url_pattern.finditer(html)]
    time_positions = [(m.start(), m.group()) for m in time_pattern.finditer(html)]

    print(f"  fallback: found {len(url_positions)} racecard URLs, {len(time_positions)} HH:MM times")

    # Debug: show unique course slugs
    all_slugs = sorted(set(_course_from_url(u) for _, u in url_positions))
    print(f"  course slugs: {all_slugs}")

    for url_pos, url in url_positions:
        course = _course_from_url(url)
        if not _is_uk_or_ire_course(course):
            continue
        # Find closest time before this URL (within 2000 chars)
        best_time = None
        best_dist = 99999
        for t_pos, t_str in time_positions:
            if t_pos < url_pos:
                dist = url_pos - t_pos
                if dist < best_dist and dist < 2000:
                    best_dist = dist
                    best_time = t_str
        if best_time:
            try:
                h, m = best_time.split(":")
                dt = datetime(
                    int(date_yyyy_mm_dd[:4]),
                    int(date_yyyy_mm_dd[5:7]),
                    int(date_yyyy_mm_dd[8:10]),
                    int(h), int(m),
                    tzinfo=TZ_LONDON,
                ).astimezone(timezone.utc)
            except Exception:
                continue
            key = dt.isoformat()
            if key not in seen:
                seen.add(key)
                out.append(dt)

    out.sort()
    return out


def main() -> None:
    date_yyyy_mm_dd = os.environ.get("RESULTS_DATE", "").strip()
    if not date_yyyy_mm_dd:
        date_yyyy_mm_dd = datetime.now(TZ_LONDON).strftime("%Y-%m-%d")

    y, m, d = date_yyyy_mm_dd.split("-")

    time_order_html = os.environ.get(
        "TIME_ORDER_HTML",
        f"/data/{y}/{m}/{d}/racingpost-racecards-{date_yyyy_mm_dd}.html",
    )

    cluster_arn = _env("ECS_CLUSTER_ARN")
    role_arn = _env("EVENTBRIDGE_ROLE_ARN")
    subnets_csv = _env("ECS_SUBNETS")
    security_groups_csv = _env("ECS_SECURITY_GROUPS")

    taskdef_pipeline = _env("ECS_TASKDEF_PIPELINE_ARN")

    times = _extract_race_times_from_time_order_html(time_order_html, date_yyyy_mm_dd)
    if not times:
        raise RuntimeError(f"no UK/IRE race times found in {time_order_html}")

    print(f"found {len(times)} unique UK/IRE race times")

    # Clean up old rules from before today
    today_stamp = date_yyyy_mm_dd.replace("-", "")
    deleted = _cleanup_old_rules("rps-pipeline-", today_stamp)
    if deleted:
        print(f"cleaned up {deleted} old rule(s)")

    def _lon(dt: datetime) -> datetime:
        return dt.astimezone(TZ_LONDON)

    # Schedule pipeline 10 mins before each race
    for dt in times:
        dt_pre = dt - timedelta(minutes=10)
        stamp = _lon(dt).strftime("%Y%m%d-%H%M")
        rule_name = f"rps-pipeline-pre-{stamp}"

        if _rule_exists(rule_name):
            print(f"  skip pre-race  {_lon(dt_pre).strftime('%H:%M')} London  (rule {rule_name} already exists)")
            continue
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
        print(f"  scheduled pre-race  {_lon(dt_pre).strftime('%H:%M')} London"
              f"  (race at {_lon(dt).strftime('%H:%M')} London"
              f" / cron UTC {_cron_expr(dt_pre)})")

    # Schedule pipeline 30 mins after the last race
    last_dt = times[-1]
    dt_post = last_dt + timedelta(minutes=30)
    stamp = _lon(last_dt).strftime("%Y%m%d-%H%M")
    rule_name = f"rps-pipeline-post-{stamp}"

    if _rule_exists(rule_name):
        print(f"  skip post-race {_lon(dt_post).strftime('%H:%M')} London  (rule {rule_name} already exists)")
    else:
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
        print(f"  scheduled post-race {_lon(dt_post).strftime('%H:%M')} London"
              f"  (30 min after last race at {_lon(last_dt).strftime('%H:%M')} London"
              f" / cron UTC {_cron_expr(dt_post)})")

    print("done")


if __name__ == "__main__":
    main()
