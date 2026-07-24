#!/usr/bin/env python3
"""
Edge analysis: for each race in the probabilities parquets, pick the horse
with the best positive edge (bookie_odds > fair_odds), check if it won in
the results, and display a P&L table.
"""
import glob
import pyarrow.parquet as pq
from datetime import datetime, date
from collections import defaultdict

DATA = "/Users/robertforster/develop/racingpostscrapper/scripts/data"

# ── 1. Load results into a lookup: (horse_lower, year, month, day) -> position
print("Loading results...")
results: dict[tuple, str] = {}
for path in glob.glob(f"{DATA}/processed/**/*.parquet", recursive=True):
    t = pq.read_table(path, columns=["horse", "position", "year", "month", "day"])
    cols = t.to_pydict()
    for i in range(len(cols["horse"])):
        horse = (cols["horse"][i] or "").strip().lower()
        pos = cols["position"][i] or ""
        yr = cols["year"][i]
        mo = cols["month"][i]
        dy = cols["day"][i]
        if horse and yr and mo and dy:
            key = (horse, int(yr), int(mo), int(dy))
            # keep best (lowest) position if duplicate
            if key not in results or (pos.isdigit() and (not results[key].isdigit() or int(pos) < int(results[key]))):
                results[key] = pos
print(f"  {len(results):,} result entries loaded")

# ── 2. Load all runners across all prob files, deduplicated by (course, time, horse)
print("Loading probabilities...")
# Use a dict to deduplicate — same race appears in daily + per-race files.
# Key: (course, time, horse) — keep last-seen entry (per-race files loaded later).
all_runners: dict[tuple, dict] = {}
for path in sorted(glob.glob(f"{DATA}/probabilities/**/*.parquet", recursive=True)):
    t = pq.read_table(path)
    cols = t.to_pydict()
    n = len(cols["horse"])
    for i in range(n):
        bookie = cols["bookie_odds"][i]
        fair   = cols["fair_odds"][i]
        if bookie is None or fair is None:
            continue
        key = (cols["course"][i], cols["time"][i], (cols["horse"][i] or "").strip().lower())
        all_runners[key] = {
            "horse":       cols["horse"][i],
            "jockey":      cols["jockey"][i],
            "prob":        cols["prob"][i],
            "fair_odds":   fair,
            "bookie_odds": bookie,
            "edge":        bookie - fair,
            "time":        cols["time"][i],
            "course":      cols["course"][i],
        }

# Group deduplicated runners by race
race_runners: dict[tuple, list] = defaultdict(list)
for (course, time, _), runner in all_runners.items():
    race_runners[(course, time)].append(runner)

rows = []
for race_key, runners in race_runners.items():
    # Pick runner with highest positive edge
    best = max(runners, key=lambda r: r["edge"])
    if best["edge"] <= 0:
        continue  # no value on this race

    # Parse race date from ISO time field
    try:
        dt = datetime.fromisoformat(best["time"].replace("Z", "+00:00"))
        race_date = date(dt.year, dt.month, dt.day)
    except Exception:
        continue

    horse_lower = (best["horse"] or "").strip().lower()
    result_key = (horse_lower, race_date.year, race_date.month, race_date.day)
    position = results.get(result_key, "?")
    won = position == "1"

    rows.append({**best, "race_date": race_date, "position": position, "won": won})

# ── 3. Sort by date then race time
rows.sort(key=lambda r: (r["race_date"], r["time"]))

# ── 4. Display table
print(f"\n{'Date':<12} {'Course':<16} {'Horse':<28} {'Bookie':>7} {'Fair':>7} {'Edge':>6} {'Pos':>4} {'P&L':>7} {'Running':>8}")
print("-" * 100)

total_pnl = 0.0
wins = 0
losses = 0
no_result = 0

for r in rows:
    pnl = None
    pnl_str = "  n/a"
    if r["position"] == "1":
        pnl = r["bookie_odds"] - 1
        wins += 1
    elif r["position"].isdigit():
        pnl = -1.0
        losses += 1
    else:
        no_result += 1

    if pnl is not None:
        total_pnl += pnl
        pnl_str = f"{pnl:+.2f}"

    print(
        f"{str(r['race_date']):<12} "
        f"{(r['course'] or ''):<16.16} "
        f"{(r['horse'] or ''):<28.28} "
        f"{r['bookie_odds']:>7.2f} "
        f"{r['fair_odds']:>7.2f} "
        f"{r['edge']:>+6.2f} "
        f"{r['position']:>4} "
        f"{pnl_str:>7} "
        f"{total_pnl:>+8.2f}"
    )

# ── 5. Summary
settled = wins + losses
print("-" * 100)
print(f"\nSUMMARY  Races with value bet: {len(rows)}  |  Settled: {settled}  |  No result: {no_result}")
print(f"  Wins: {wins}  Losses: {losses}  Strike rate: {wins/settled*100:.1f}%" if settled else "  No settled bets")
print(f"  Total P&L (£1 stake): {total_pnl:+.2f}")
if settled:
    avg_bookie = sum(r["bookie_odds"] for r in rows if r["position"].isdigit()) / settled
    breakeven_sr = 1 / avg_bookie * 100
    print(f"  Avg bookie odds on selections: {avg_bookie:.2f}  (breakeven strike rate: {breakeven_sr:.1f}%)")
