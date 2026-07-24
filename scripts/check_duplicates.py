#!/usr/bin/env python3
"""Check for duplicate races across probability parquet files."""
import glob
import pyarrow.parquet as pq
from datetime import datetime
from collections import Counter

DATA = "/Users/robertforster/develop/racingpostscrapper/scripts/data"

files = sorted(glob.glob(f"{DATA}/probabilities/**/*.parquet", recursive=True))
print(f"Total prob parquet files: {len(files)}")

# Show directory structure
import os
subdirs = Counter()
for f in files:
    rel = os.path.relpath(f, f"{DATA}/probabilities")
    parts = rel.split(os.sep)
    subdirs[len(parts)] += 1
print(f"Depth distribution: {dict(subdirs)}")
print("Sample paths:")
for f in files[:5]:
    print(" ", os.path.relpath(f, DATA))

# Count unique races (course+time combos) and see if any appear in multiple files
race_files: dict[tuple, list[str]] = {}
for path in files:
    t = pq.read_table(path, columns=["course", "time"])
    cols = t.to_pydict()
    fname = os.path.relpath(path, DATA)
    for i in range(len(cols["course"])):
        key = (cols["course"][i], cols["time"][i])
        race_files.setdefault(key, []).append(fname)

dups = {k: v for k, v in race_files.items() if len(set(v)) > 1}
print(f"\nUnique races: {len(race_files)}  Races in multiple files: {len(dups)}")
for (course, time), fnames in list(dups.items())[:5]:
    print(f"  {course} {time}: {fnames}")
