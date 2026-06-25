# Probabilities Whitepaper (Simple Version)

## What this document explains
The `today_first_race_table` report shows a **probability of winning** for each horse in each race, and a matching **fair odds** number.

This document explains, in plain English, how those numbers are calculated.

## First: what does “heuristic” mean?
“Heuristic” just means:

- **A sensible rule-of-thumb.**
- It uses a few simple signals that usually matter.
- It is **not** a perfect scientific model.

So when you see “heuristic probability”, read it as:

- “a best-effort estimate using simple rules and past data”.

## Important warning
These probabilities are:

- **Not bookmaker odds**
- **Not guaranteed to be accurate**
- **Not calibrated** (a 30% prediction is not proven to win 30% of the time)

They are mainly useful for:

- ranking runners within a race
- spotting runners the rules think are stronger/weaker

## What data do we use?

### 1) Today’s racecard runners (JSONL)
For each runner we read:

- `horse`
- `jockey`
- `trainer`
- `time` (race time)
- `race_name`

We also drop any row where the horse/jockey/trainer says `NON-RUNNER`.

### 2) Past results (Parquet “history” files)
From past races we build simple summaries for:

- each horse
- each jockey
- each trainer

The two main things we look at are:

- a rating number (RPR-like)
- finishing position (so we can count wins)

## Step-by-step: how we turn past data into probabilities

### Step A: Build “stats cards” for horses/jockeys/trainers
For each horse/jockey/trainer we store:

- **how many past runs we saw** (`count`)
- **how many wins** (`win_count`, when finishing position was 1)
- **sum of ratings** (`sum_rpr`) so we can later compute an average

From that we compute:

- **Average rating** = `sum_rpr / count`
- **Win rate (horse only)** = `win_count / count`

If we have never seen an entity before (new horse etc.), we treat its stats as zero.

### Step B: Give each runner a “score”
For each runner, we look up:

- the horse’s average rating
- the jockey’s average rating
- the trainer’s average rating
- the horse’s win rate

Then we combine them into one number called `score`.

The code uses this formula:

```text
score = 1.0
      + 0.75 * horse_avg_rating
      + 0.15 * jockey_avg_rating
      + 0.10 * trainer_avg_rating
      + 20.0 * horse_win_rate
```

Plain-English meaning:

- **Horse rating matters most**.
- Jockey and trainer matter a bit.
- Horses that win a lot get a bonus.

### Step C: Turn scores into probabilities (the “softmax” step)
Now we have a score for each horse in the race. But a score is not a probability.

We need probabilities that:

- are all between 0 and 1
- add up to 1.0 (100%) inside the race

To do that we use a standard math trick called **softmax**.

You do not need to remember the math. The important part is:

- higher score => higher probability
- all probabilities in the race sum to ~100%

The code also uses a “sharpening” number (`k = 10.0`) that makes the top runners get more of the probability.

If `k` was smaller, the probabilities would be flatter and more uncertain.

### Step D: Convert probability into “fair odds”
The report shows:

- `prob` (probability)
- `fair_odds`

Fair odds are just:

- `fair_odds = 1 / prob`

Examples:

- probability 0.50 (50%) => fair odds 2.00
- probability 0.25 (25%) => fair odds 4.00
- probability 0.10 (10%) => fair odds 10.00

These are called “fair” because there is no bookmaker margin added.

## Why do the probabilities usually sum to 100%?
Because softmax always normalizes the numbers within each race.

The report also prints the sum so you can sanity-check it.

## What this method does NOT do
This scoring method does not currently include things like:

- distance suitability
- going/ground suitability
- draw bias
- pace
- recent form trends

It is intentionally simple.

## If you want to improve it later
Common next steps are:

- **calibrate** the probabilities against real outcomes
- use more features (distance/going/class/etc.)
- reduce over-confidence by lowering the softmax sharpening factor
