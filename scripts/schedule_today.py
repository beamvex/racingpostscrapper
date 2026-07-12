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



def _run(cmd: list[str]) -> None:
    subprocess.run(cmd, check=True)


def _run_quiet(cmd: list[str]) -> bool:
    """Run a command, return True if it succeeded (exit 0)."""
    result = subprocess.run(cmd, capture_output=True)
    return result.returncode == 0


def _schedule_exists(name: str) -> bool:
    region = os.environ.get("AWS_REGION", "eu-west-2")
    return _run_quiet(
        ["aws", "scheduler", "get-schedule", "--name", name, "--region", region]
    )


def _list_schedule_names(prefix: str) -> list[str]:
    region = os.environ.get("AWS_REGION", "eu-west-2")
    result = subprocess.run(
        ["aws", "scheduler", "list-schedules", "--name-prefix", prefix, "--region", region, "--output", "json"],
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


def _delete_schedule(name: str) -> None:
    region = os.environ.get("AWS_REGION", "eu-west-2")
    _run_quiet(
        ["aws", "scheduler", "delete-schedule", "--name", name, "--region", region]
    )


def _cleanup_old_schedules(prefix: str, today_str: str) -> int:
    """Delete schedules with the given prefix whose names contain a date before today_str.
    Returns the number of schedules deleted."""
    names = _list_schedule_names(prefix)
    deleted = 0
    for name in names:
        # Extract date part: rps-pipeline-pre-YYYYMMDD-HHMM or rps-pipeline-post-YYYYMMDD-HHMM
        # After prefix, skip "pre-" or "post-" (4 chars), then 8-char date
        rest = name[len(prefix):]  # e.g. "pre-20260706-1315" or "post-20260706-1315"
        if len(rest) >= 13:  # pre-/post- (4) + YYYYMMDD (8) + at least "-"
            schedule_date = rest[4:12]  # skip "pre-" or "post-"
            if schedule_date < today_str:
                print(f"  deleting old schedule: {name}")
                _delete_schedule(name)
                deleted += 1
    return deleted


def _create_schedule(
    name: str,
    dt: datetime,
    lambda_arn: str,
    race_url: str | None = None,
) -> None:
    region = os.environ.get("AWS_REGION", "eu-west-2")

    # Build target for Lambda invocation
    target = json.dumps({
        "Arn": lambda_arn,
        "RoleArn": f"arn:aws:iam::{_account_id()}:role/racingpost-scraper-lambda",
    })

    # Pass race URL in the Lambda input
    if race_url:
        target = json.dumps({
            "Arn": lambda_arn,
            "RoleArn": f"arn:aws:iam::{_account_id()}:role/racingpost-scraper-lambda",
            "Input": json.dumps({"race_url": race_url}),
        })

    # EventBridge Scheduler uses at(yyyy-mm-ddThh:mm:ss) for one-time schedules (UTC)
    at_expr = dt.astimezone(timezone.utc).strftime("at(%Y-%m-%dT%H:%M:%S)")

    _run(
        [
            "aws", "scheduler", "create-schedule",
            "--name", name,
            "--schedule-expression", at_expr,
            "--schedule-expression-timezone", "UTC",
            "--flexible-time-window", json.dumps({"Mode": "OFF"}),
            "--target", target,
            "--action-after-completion", "DELETE",
            "--region", region,
        ]
    )


def _account_id() -> str:
    """Extract AWS account ID from cluster ARN or environment."""
    # Try to get from cluster ARN environment variable
    cluster_arn = os.environ.get("ECS_CLUSTER_ARN", "")
    if cluster_arn:
        parts = cluster_arn.split(":")
        if len(parts) >= 5:
            return parts[4]
    # Fallback to STS call
    result = subprocess.run(
        ["aws", "sts", "get-caller-identity", "--query", "Account", "--output", "text"],
        capture_output=True,
        text=True,
        check=True
    )
    return result.stdout.strip()


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


def _extract_race_times_from_time_order_html(html_path: str, date_yyyy_mm_dd: str) -> list[tuple[datetime, str]]:
    """Parse the time-order page HTML to extract race times and URLs for UK/IRE courses.
    Returns list of (datetime, url) tuples."""
    with open(html_path, "r", encoding="utf-8") as f:
        html = f.read()

    seen = set()
    out: list[tuple[datetime, str]] = []

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
                # Walk the Next.js data for race entries with time + course + url
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
                        url = (obj.get("url") or obj.get("racecardUrl") or
                               obj.get("link") or "").strip()
                        if course and time_str and _is_uk_or_ire_course(course):
                            dt = _try_parse_time(time_str, date_yyyy_mm_dd)
                            if dt:
                                # Only include if we have a valid specific race URL
                                # URL should be: /racecards/<course_no>/<course_slug>/<date>/<race_id>
                                valid_url = None
                                if url and "/racecards/" in url:
                                    url_full = url if url.startswith("http") else f"https://www.racingpost.com{url}"
                                    parts = url_full.rstrip("/").split("/")
                                    # Structure: https://www.racingpost.com/racecards/<no>/<slug>/<date>/<id>
                                    # That's 8 parts when split, racecards at index 3
                                    if len(parts) >= 8 and parts[3] == "racecards":
                                        valid_url = url_full
                                if valid_url:
                                    key = valid_url  # deduplicate by URL, not time
                                    if key not in seen:
                                        seen.add(key)
                                        out.append((dt, valid_url))
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
                # Convert relative URL to absolute if needed
                if url.startswith("/"):
                    url = f"https://www.racingpost.com{url}"
                out.append((dt, url))

    out.sort(key=lambda x: x[0])
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

    lambda_arn = _env("STARTER_LAMBDA_ARN")

    times = _extract_race_times_from_time_order_html(time_order_html, date_yyyy_mm_dd)
    if not times:
        raise RuntimeError(f"no UK/IRE race times found in {time_order_html}")

    print(f"found {len(times)} unique UK/IRE race times")

    # Clean up old schedules from before today
    today_stamp = date_yyyy_mm_dd.replace("-", "")
    deleted = _cleanup_old_schedules("rps-pipeline-", today_stamp)
    if deleted:
        print(f"cleaned up {deleted} old schedule(s)")

    def _lon(dt: datetime) -> datetime:
        return dt.astimezone(TZ_LONDON)

    # Schedule pipeline 10 mins before each race
    for dt, url in times:
        dt_pre = dt - timedelta(minutes=10)
        stamp = _lon(dt).strftime("%Y%m%d-%H%M")
        schedule_name = f"rps-pipeline-pre-{stamp}"

        if _schedule_exists(schedule_name):
            print(f"  skip pre-race  {_lon(dt_pre).strftime('%H:%M')} London  (schedule {schedule_name} already exists)")
            continue
        _create_schedule(
            name=schedule_name,
            dt=dt_pre,
            lambda_arn=lambda_arn,
            race_url=url,
        )
        print(f"  scheduled pre-race  {_lon(dt_pre).strftime('%H:%M')} London"
              f"  (race at {_lon(dt).strftime('%H:%M')} London"
              f" / at UTC {dt_pre.astimezone(timezone.utc).strftime('%H:%M')}"
              f" / url {url})")

    # Schedule pipeline 30 mins after the last race
    last_dt, last_url = times[-1]
    dt_post = last_dt + timedelta(minutes=30)
    stamp = _lon(last_dt).strftime("%Y%m%d-%H%M")
    schedule_name = f"rps-pipeline-post-{stamp}"

    if _schedule_exists(schedule_name):
        print(f"  skip post-race {_lon(dt_post).strftime('%H:%M')} London  (schedule {schedule_name} already exists)")
    else:
        _create_schedule(
            name=schedule_name,
            dt=dt_post,
            lambda_arn=lambda_arn,
        )
        print(f"  scheduled post-race {_lon(dt_post).strftime('%H:%M')} London"
              f"  (30 min after last race at {_lon(last_dt).strftime('%H:%M')} London"
              f" / at UTC {dt_post.astimezone(timezone.utc).strftime('%H:%M')}")

    print("done")


if __name__ == "__main__":
    main()
