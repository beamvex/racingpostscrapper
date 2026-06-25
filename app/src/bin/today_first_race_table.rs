use anyhow::Context;
use serde::Deserialize;
use std::fs;

#[derive(Default, Clone)]
struct RunnerMini {
    horse: String,
    jockey: String,
    trainer: String,
}

#[derive(Default, Clone, PartialEq, Eq)]
struct RaceKey {
    course: String,
    time: String,
    race_name: String,
}

#[derive(Deserialize)]
struct RunnerRow {
    #[serde(default)]
    course: String,
    #[serde(default)]
    time: String,
    #[serde(default)]
    race_name: String,
    #[serde(default)]
    horse: String,
    #[serde(default)]
    jockey: String,
    #[serde(default)]
    trainer: String,
}

fn main() -> anyhow::Result<()> {
    let today = racingpost_scraper::utils::current_utc_date_yyyy_mm_dd();
    eprintln!("today-first-race: date={today}");

    let in_path = std::env::args()
        .skip(1)
        .find_map(|a| a.strip_prefix("--in=").map(|p| p.to_string()))
        .unwrap_or_else(|| {
            let y = &today[0..4];
            let m = &today[5..7];
            let d = &today[8..10];
            format!(
                "/data/racecards/{}/{}/{}/racingpost-racecards-{}-runners.jsonl",
                y, m, d, today
            )
        });

    eprintln!("today-first-race: reading {in_path}");
    let bytes = fs::read(&in_path).with_context(|| format!("read {in_path}"))?;
    let s = String::from_utf8(bytes).context("decode input as utf-8")?;

    let (race, runners) = extract_first_race_from_jsonl(&s)?;
    eprintln!(
        "today-first-race: first race course='{}' time='{}' race_name='{}' runners={}",
        race.course,
        race.time,
        race.race_name,
        runners.len()
    );

    print_table(&runners);

    Ok(())
}

fn extract_first_race_from_jsonl(s: &str) -> anyhow::Result<(RaceKey, Vec<RunnerMini>)> {
    let mut first_key: Option<RaceKey> = None;
    let mut out: Vec<RunnerMini> = Vec::new();

    for (i, line) in s.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let row: RunnerRow = serde_json::from_str(line)
            .with_context(|| format!("parse jsonl line {}", i + 1))?;

        let key = RaceKey {
            course: row.course,
            time: row.time,
            race_name: row.race_name,
        };

        if first_key.is_none() {
            first_key = Some(key.clone());
        }

        if Some(&key) != first_key.as_ref() {
            continue;
        }

        if row.horse.trim().is_empty() {
            continue;
        }

        out.push(RunnerMini {
            horse: row.horse,
            jockey: row.jockey,
            trainer: row.trainer,
        });
    }

    let Some(first_key) = first_key else {
        anyhow::bail!("no jsonl records found")
    };

    Ok((first_key, out))
}

fn print_table(rows: &[RunnerMini]) {
    let mut w_h = "horse".len();
    let mut w_j = "jockey".len();
    let mut w_t = "trainer".len();

    for r in rows {
        w_h = w_h.max(r.horse.len());
        w_j = w_j.max(r.jockey.len());
        w_t = w_t.max(r.trainer.len());
    }

    println!(
        "{:<w_h$}  {:<w_j$}  {:<w_t$}",
        "horse",
        "jockey",
        "trainer",
        w_h = w_h,
        w_j = w_j,
        w_t = w_t
    );
    println!(
        "{}  {}  {}",
        "-".repeat(w_h),
        "-".repeat(w_j),
        "-".repeat(w_t)
    );

    for r in rows {
        println!(
            "{:<w_h$}  {:<w_j$}  {:<w_t$}",
            r.horse,
            r.jockey,
            r.trainer,
            w_h = w_h,
            w_j = w_j,
            w_t = w_t
        );
    }
}
