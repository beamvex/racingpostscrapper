import os
from datetime import datetime, timedelta, timezone

from schedule_aws import cleanup_old_schedules, create_schedule, schedule_exists
from schedule_parser import extract_race_times
from schedule_utils import TZ_LONDON, env


def main() -> None:
    date_yyyy_mm_dd = os.environ.get("RESULTS_DATE", "").strip()
    if not date_yyyy_mm_dd:
        date_yyyy_mm_dd = datetime.now(TZ_LONDON).strftime("%Y-%m-%d")

    y, m, d = date_yyyy_mm_dd.split("-")

    time_order_html = os.environ.get(
        "TIME_ORDER_HTML",
        f"/data/{y}/{m}/{d}/racingpost-racecards-{date_yyyy_mm_dd}.html",
    )

    lambda_arn = env("STARTER_LAMBDA_ARN")

    times = extract_race_times(time_order_html, date_yyyy_mm_dd)
    if not times:
        raise RuntimeError(f"no UK/IRE race times found in {time_order_html}")

    print(f"found {len(times)} unique UK/IRE race times")

    today_stamp = date_yyyy_mm_dd.replace("-", "")
    deleted = cleanup_old_schedules("rps-pipeline-", today_stamp)
    if deleted:
        print(f"cleaned up {deleted} old schedule(s)")

    def _lon(dt: datetime) -> datetime:
        return dt.astimezone(TZ_LONDON)

    for dt, url in times:
        dt_pre = dt - timedelta(minutes=10)
        stamp = _lon(dt).strftime("%Y%m%d-%H%M")
        schedule_name = f"rps-pipeline-pre-{stamp}"

        if schedule_exists(schedule_name):
            print(f"  skip pre-race  {_lon(dt_pre).strftime('%H:%M')} London  (schedule {schedule_name} already exists)")
            continue
        race_time = _lon(dt).strftime("%H%M")
        create_schedule(name=schedule_name, dt=dt_pre, lambda_arn=lambda_arn, race_url=url, race_time=race_time)
        print(f"  scheduled pre-race  {_lon(dt_pre).strftime('%H:%M')} London"
              f"  (race at {_lon(dt).strftime('%H:%M')} London"
              f" / at UTC {dt_pre.astimezone(timezone.utc).strftime('%H:%M')}"
              f" / url {url})")

    last_dt, _ = times[-1]
    dt_post = last_dt + timedelta(minutes=30)
    stamp = _lon(last_dt).strftime("%Y%m%d-%H%M")
    schedule_name = f"rps-pipeline-post-{stamp}"

    if schedule_exists(schedule_name):
        print(f"  skip post-race {_lon(dt_post).strftime('%H:%M')} London  (schedule {schedule_name} already exists)")
    else:
        create_schedule(name=schedule_name, dt=dt_post, lambda_arn=lambda_arn)
        print(f"  scheduled post-race {_lon(dt_post).strftime('%H:%M')} London"
              f"  (30 min after last race at {_lon(last_dt).strftime('%H:%M')} London"
              f" / at UTC {dt_post.astimezone(timezone.utc).strftime('%H:%M')}")

    print("done")


if __name__ == "__main__":
    main()
