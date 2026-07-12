import os
import subprocess
from datetime import datetime, timezone
from zoneinfo import ZoneInfo

TZ_LONDON = ZoneInfo("Europe/London")


def env(name: str) -> str:
    v = os.environ.get(name, "").strip()
    if not v:
        raise RuntimeError(f"missing env var: {name}")
    return v


def parse_iso8601(s: str) -> datetime:
    t = s.strip()
    if not t:
        raise ValueError("empty time")
    if t.endswith("Z"):
        t = t[:-1] + "+00:00"
    dt = datetime.fromisoformat(t)
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=TZ_LONDON)
    return dt.astimezone(timezone.utc)


def run(cmd: list[str]) -> None:
    subprocess.run(cmd, check=True)


def run_quiet(cmd: list[str]) -> bool:
    result = subprocess.run(cmd, capture_output=True)
    return result.returncode == 0
