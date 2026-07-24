#!/usr/bin/env python3
"""Check date coverage of results vs probabilities to validate matching quality."""
import glob
import pyarrow.parquet as pq
from datetime import datetime
from collections import Counter

DATA = "/Users/robertforster/develop/racingpostscrapper/scripts/data"

# Results: what years/dates are covered?
print("=== Results coverage ===")
year_counts = Counter()
result_dates = set()
for path in glob.glob(f"{DATA}/processed/**/*.parquet", recursive=True):
    t = pq.read_table(path, columns=["year", "month", "day"])
    cols = t.to_pydict()
    for i in range(len(cols["year"])):
        y, m, d = cols["year"][i], cols["month"][i], cols["day"][i]
        if y:
            year_counts[int(y)] += 1
            result_dates.add((int(y), int(m), int(d)))

for yr, cnt in sorted(year_counts.items()):
    print(f"  {yr}: {cnt:,} rows")

# Probabilities: what race dates are covered?
print("\n=== Probabilities race date coverage ===")
prob_dates = set()
for path in glob.glob(f"{DATA}/probabilities/**/*.parquet", recursive=True):
    t = pq.read_table(path, columns=["time"])
    for ts in t.to_pydict()["time"]:
        if ts:
            try:
                dt = datetime.fromisoformat(ts.replace("Z", "+00:00"))
                prob_dates.add((dt.year, dt.month, dt.day))
            except Exception:
                pass

sorted_prob = sorted(prob_dates)
if sorted_prob:
    print(f"  First: {sorted_prob[0]}  Last: {sorted_prob[-1]}  Unique days: {len(sorted_prob)}")

# Overlap
overlap = prob_dates & result_dates
print(f"\n=== Overlap ===")
print(f"  Prob dates: {len(prob_dates)}  Result dates: {len(result_dates)}  Overlap: {len(overlap)}")
if overlap:
    print(f"  Overlapping dates: {sorted(overlap)}")
else:
    print("  NO OVERLAP — results and probabilities cover different date ranges!")
    print("  This explains false matches: horse names match across different years on same month/day.")
