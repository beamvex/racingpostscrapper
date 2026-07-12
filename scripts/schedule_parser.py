import json
import re
from datetime import datetime, timezone

from bs4 import BeautifulSoup

from schedule_utils import TZ_LONDON, parse_iso8601


def is_uk_or_ire_course(course: str) -> bool:
    c = course.strip().lower()
    if not c:
        return False
    if "(ire)" in c:
        return True
    if "(aw)" in c:
        return True
    if "(" not in c and " " in c:
        return True
    if c.endswith("-aw") or c.endswith("-aw-gb"):
        return True
    if "-" in c:
        return False
    return True


def try_parse_time(time_str: str, date_yyyy_mm_dd: str) -> datetime | None:
    t = time_str.strip()
    if not t:
        return None
    try:
        return parse_iso8601(t)
    except Exception:
        pass
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


def course_from_url(url: str) -> str:
    parts = url.rstrip("/").split("/")
    try:
        idx = parts.index("racecards")
        return parts[idx + 2]
    except (ValueError, IndexError):
        return ""


def extract_race_times(html_path: str, date_yyyy_mm_dd: str) -> list[tuple[datetime, str]]:
    with open(html_path, "r", encoding="utf-8") as f:
        html = f.read()

    soup = BeautifulSoup(html, "html.parser")
    seen: set[str] = set()
    out: list[tuple[datetime, str]] = []

    script_tag = soup.find("script", {"id": "__NEXT_DATA__"})
    if script_tag and script_tag.string:
        try:
            next_data = json.loads(script_tag.string)
        except json.JSONDecodeError:
            next_data = None

        if next_data:
            def walk(obj: object, depth: int = 0) -> None:
                if depth > 20:
                    return
                if isinstance(obj, dict):
                    raw_url = (obj.get("raceUrl") or obj.get("url") or
                               obj.get("racecardUrl") or obj.get("link") or "").strip()
                    valid_url = None
                    if raw_url and "/racecards/" in raw_url:
                        url_full = (raw_url if raw_url.startswith("http")
                                    else f"https://www.racingpost.com{raw_url}")
                        parts = url_full.rstrip("/").split("/")
                        if len(parts) == 8 and parts[3] == "racecards":
                            valid_url = url_full

                    if valid_url:
                        time_str = (obj.get("raceDateTime") or obj.get("raceStart") or
                                    obj.get("raceTime") or obj.get("offTime") or
                                    obj.get("startTime") or obj.get("scheduledTime") or
                                    obj.get("time") or "").strip()
                        if time_str and valid_url not in seen:
                            dt = try_parse_time(time_str, date_yyyy_mm_dd)
                            if dt:
                                seen.add(valid_url)
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

    racecard_re = re.compile(r'/racecards/\d+/[^/]+/\d{4}-\d{2}-\d{2}/\d+')
    time_re = re.compile(r'^\d{1,2}:\d{2}$')

    for a_tag in soup.find_all("a", href=racecard_re):
        href = a_tag.get("href", "")
        url_full = href if href.startswith("http") else f"https://www.racingpost.com{href}"
        parts = url_full.rstrip("/").split("/")
        if len(parts) != 8 or parts[3] != "racecards":
            continue
        course = course_from_url(url_full)
        if not is_uk_or_ire_course(course) or url_full in seen:
            continue
        time_str = None
        for parent in a_tag.parents:
            for text in parent.stripped_strings:
                if time_re.match(text):
                    time_str = text
                    break
            if time_str:
                break
        if time_str:
            dt = try_parse_time(time_str, date_yyyy_mm_dd)
            if dt:
                seen.add(url_full)
                out.append((dt, url_full))

    out.sort(key=lambda x: x[0])
    print(f"  fallback: extracted {len(out)} times from anchor tags")
    return out

if __name__ == "__main__":
    times = extract_race_times("/Users/robertforster/develop/racingpostscrapper/scripts/test-race-times.html", "2026-07-12")
    print(json.dumps([(dt.isoformat(), url) for dt, url in times], indent=2))