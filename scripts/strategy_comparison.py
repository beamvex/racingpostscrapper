#!/usr/bin/env python3
"""
Compare betting strategies across all probability parquets.
One selection per race per strategy; checks result in processed parquets.

Usage:
  python3 strategy_comparison.py            # summary table only
  python3 strategy_comparison.py --detail   # + per-race rows per strategy
  python3 strategy_comparison.py --detail model_fav value_odds_5  # specific strategies
"""
import glob
import sys
import pyarrow.parquet as pq
from datetime import datetime, date
from collections import defaultdict

SHOW_DETAIL = "--detail" in sys.argv
DETAIL_STRATEGIES = [a for a in sys.argv[1:] if not a.startswith("-")] or None

DATA = "/Users/robertforster/develop/racingpostscrapper/scripts/data"

# ── Strategies: (name, selector_fn)
# selector_fn receives a list of runner dicts and returns the chosen one or None
STRATEGIES = [
    ("model_fav",      lambda rs: max(rs, key=lambda r: r["prob"])),
    ("max_abs_edge",   lambda rs: _best_positive(rs, lambda r: r["bookie_odds"] - r["fair_odds"])),
    ("max_rel_edge",   lambda rs: _best_positive(rs, lambda r: r["bookie_odds"] / r["fair_odds"] - 1)),
    ("max_prob_edge",  lambda rs: _best_positive(rs, lambda r: r["prob"] - 1/r["bookie_odds"])),
    ("value_odds_5",   lambda rs: _best_positive([r for r in rs if r["bookie_odds"] <= 5.0],
                                                  lambda r: r["bookie_odds"] / r["fair_odds"] - 1)),
    ("value_odds_10",  lambda rs: _best_positive([r for r in rs if r["bookie_odds"] <= 10.0],
                                                  lambda r: r["bookie_odds"] / r["fair_odds"] - 1)),
    ("value_odds_20",  lambda rs: _best_positive([r for r in rs if r["bookie_odds"] <= 20.0],
                                                  lambda r: r["bookie_odds"] / r["fair_odds"] - 1)),
]

def _best_positive(runners, key_fn):
    """Return runner with highest key_fn score if score > 0, else None."""
    if not runners:
        return None
    best = max(runners, key=key_fn)
    return best if key_fn(best) > 0 else None


# ── 1. Load results lookup
print("Loading results...")
results: dict[tuple, str] = {}
for path in glob.glob(f"{DATA}/processed/**/*.parquet", recursive=True):
    t = pq.read_table(path, columns=["horse", "position", "year", "month", "day"])
    cols = t.to_pydict()
    for i in range(len(cols["horse"])):
        horse = (cols["horse"][i] or "").strip().lower()
        pos   = cols["position"][i] or ""
        yr, mo, dy = cols["year"][i], cols["month"][i], cols["day"][i]
        if horse and yr:
            key = (horse, int(yr), int(mo), int(dy))
            if key not in results or (pos.isdigit() and (not results[key].isdigit() or int(pos) < int(results[key]))):
                results[key] = pos
print(f"  {len(results):,} result entries")

# ── 2. Load & deduplicate runners
print("Loading probabilities...")
all_runners: dict[tuple, dict] = {}
for path in sorted(glob.glob(f"{DATA}/probabilities/**/*.parquet", recursive=True)):
    t = pq.read_table(path)
    cols = t.to_pydict()
    for i in range(len(cols["horse"])):
        bookie = cols["bookie_odds"][i]
        fair   = cols["fair_odds"][i]
        prob   = cols["prob"][i]
        if bookie is None or fair is None or prob is None or bookie <= 1:
            continue
        key = (cols["course"][i], cols["time"][i], (cols["horse"][i] or "").strip().lower())
        all_runners[key] = {
            "horse":       cols["horse"][i],
            "jockey":      cols["jockey"][i],
            "prob":        prob,
            "fair_odds":   fair,
            "bookie_odds": bookie,
            "time":        cols["time"][i],
            "course":      cols["course"][i],
        }

race_runners: dict[tuple, list] = defaultdict(list)
for (course, time, _), runner in all_runners.items():
    race_runners[(course, time)].append(runner)
print(f"  {len(race_runners):,} unique races")

# ── 3. Run each strategy
class Stats:
    def __init__(self):
        self.bets = 0
        self.wins = 0
        self.losses = 0
        self.no_result = 0
        self.pnl = 0.0
        self.bookie_odds_sum = 0.0

    def record(self, selection, position):
        self.bets += 1
        if position == "1":
            profit = selection["bookie_odds"] - 1
            self.wins += 1
            self.pnl += profit
            self.bookie_odds_sum += selection["bookie_odds"]
        elif position.isdigit():
            self.losses += 1
            self.pnl -= 1.0
            self.bookie_odds_sum += selection["bookie_odds"]
        else:
            self.no_result += 1
            self.bets -= 1  # don't count unresolved

    @property
    def settled(self):
        return self.wins + self.losses

    @property
    def strike_rate(self):
        return self.wins / self.settled * 100 if self.settled else 0

    @property
    def roi(self):
        return self.pnl / self.settled * 100 if self.settled else 0

    @property
    def avg_odds(self):
        return self.bookie_odds_sum / self.settled if self.settled else 0


strategy_stats = {name: Stats() for name, _ in STRATEGIES}
# Also track per-race selections for detail view
strategy_rows: dict[str, list] = {name: [] for name, _ in STRATEGIES}

for race_key, runners in race_runners.items():
    course, time_str = race_key
    try:
        dt = datetime.fromisoformat(time_str.replace("Z", "+00:00"))
        race_date = date(dt.year, dt.month, dt.day)
    except Exception:
        continue

    for name, selector in STRATEGIES:
        sel = selector(runners)
        if sel is None:
            continue
        horse_lower = (sel["horse"] or "").strip().lower()
        res_key = (horse_lower, race_date.year, race_date.month, race_date.day)
        position = results.get(res_key, "?")
        strategy_stats[name].record(sel, position)
        strategy_rows[name].append({
            **sel,
            "race_date": race_date,
            "position":  position,
            "edge_abs":  sel["bookie_odds"] - sel["fair_odds"],
            "edge_rel":  sel["bookie_odds"] / sel["fair_odds"] - 1,
        })

# ── 4. Summary table
print()
print(f"{'Strategy':<16} {'Bets':>5} {'Settled':>7} {'Wins':>5} {'SR%':>6} {'AvgOdds':>8} {'P&L':>8} {'ROI%':>7}")
print("-" * 70)
for name, stats in strategy_stats.items():
    print(
        f"{name:<16} {stats.bets:>5} {stats.settled:>7} {stats.wins:>5} "
        f"{stats.strike_rate:>5.1f}% {stats.avg_odds:>8.2f} "
        f"{stats.pnl:>+8.2f} {stats.roi:>+6.1f}%"
    )

# ── 5. Detail view for each strategy (opt-in via --detail)
if not SHOW_DETAIL:
    sys.exit(0)

for name, rows in strategy_rows.items():
    if DETAIL_STRATEGIES and name not in DETAIL_STRATEGIES:
        continue
    rows_sorted = sorted(rows, key=lambda r: (r["race_date"], r["time"]))
    print(f"\n{'='*110}")
    print(f"STRATEGY: {name}  ({len(rows_sorted)} selections)")
    print(f"{'='*110}")
    print(f"{'Date':<12} {'Course':<18} {'Horse':<26} {'Bookie':>7} {'Fair':>7} {'AbsEdge':>8} {'RelEdge':>8} {'Pos':>4} {'P&L':>7} {'Running':>9}")
    print("-" * 110)
    running = 0.0
    for r in rows_sorted:
        pos = r["position"]
        if pos == "1":
            pnl = r["bookie_odds"] - 1
            pnl_s = f"{pnl:+.2f}"
        elif pos.isdigit():
            pnl = -1.0
            pnl_s = f"{pnl:+.2f}"
        else:
            pnl = 0.0
            pnl_s = "  n/a"
        running += pnl
        print(
            f"{str(r['race_date']):<12} "
            f"{(r['course'] or ''):<18.18} "
            f"{(r['horse'] or ''):<26.26} "
            f"{r['bookie_odds']:>7.2f} "
            f"{r['fair_odds']:>7.2f} "
            f"{r['edge_abs']:>+8.2f} "
            f"{r['edge_rel']*100:>+7.1f}% "
            f"{pos:>4} "
            f"{pnl_s:>7} "
            f"{running:>+9.2f}"
        )
