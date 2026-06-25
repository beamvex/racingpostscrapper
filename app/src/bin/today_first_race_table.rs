use anyhow::Context;
use arrow::array::Array;
use serde::Deserialize;
use std::fs::File;
use std::fs;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Default, Clone)]
struct RunnerMini {
    horse: String,
    jockey: String,
    trainer: String,
}

#[derive(Clone)]
struct PredRow {
    horse: String,
    jockey: String,
    trainer: String,
    score: f64,
    prob: f64,
    fair_odds: f64,
}

#[derive(Default, Clone, PartialEq, Eq, Hash)]
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

    let mut in_path_arg: Option<String> = None;
    let mut history_dir_arg: Option<String> = None;
    for a in std::env::args().skip(1) {
        if let Some(p) = a.strip_prefix("--in=") {
            in_path_arg = Some(p.to_string());
            continue;
        }
        if let Some(p) = a.strip_prefix("--history-dir=") {
            history_dir_arg = Some(p.to_string());
            continue;
        }
    }

    let y = &today[0..4];
    let m = &today[5..7];
    let d = &today[8..10];

    let in_path = in_path_arg.unwrap_or_else(|| {
        format!(
            "/data/racecards/{}/{}/{}/racingpost-racecards-{}-runners.jsonl",
            y, m, d, today
        )
    });

    let history_dir = history_dir_arg
        .unwrap_or_else(|| format!("/data/athena/history/{}/{}/{}/", y, m, d));

    eprintln!("today-first-race: reading {in_path}");
    let bytes = fs::read(&in_path).with_context(|| format!("read {in_path}"))?;
    let s = String::from_utf8(bytes).context("decode input as utf-8")?;

    let races = extract_all_races_from_jsonl(&s)?;
    eprintln!("today-first-race: races={}", races.len());

    println!();
    println!("history parquet preview:");
    preview_history_parquet_dir(&history_dir, 10)?;

    let (horse_agg, jockey_agg, trainer_agg) = build_history_aggs(&history_dir, &races)?;

    for (race, runners) in races {
        println!();
        println!(
            "race: course='{}' time='{}' race_name='{}' runners={}",
            race.course,
            race.time,
            race.race_name,
            runners.len()
        );
        print_table(&runners);

        println!();
        println!("odds (heuristic, from history parquet):");
        predict_odds_for_runners(&runners, &horse_agg, &jockey_agg, &trainer_agg);
    }

    Ok(())
}

#[derive(Default, Clone, Copy)]
struct Agg {
    count: u64,
    win_count: u64,
    sum_rpr: f64,
    max_rpr: f64,
}

impl Agg {
    fn add(&mut self, rpr: Option<f64>, position: Option<i64>) {
        self.count += 1;
        if let Some(p) = position {
            if p == 1 {
                self.win_count += 1;
            }
        }
        if let Some(x) = rpr {
            self.sum_rpr += x;
            if x > self.max_rpr {
                self.max_rpr = x;
            }
        }
    }

    fn avg_rpr(&self) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        Some(self.sum_rpr / (self.count as f64))
    }

    fn win_rate(&self) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        Some((self.win_count as f64) / (self.count as f64))
    }
}

fn build_history_aggs(
    history_dir: &str,
    races: &[(RaceKey, Vec<RunnerMini>)],
) -> anyhow::Result<(HashMap<String, Agg>, HashMap<String, Agg>, HashMap<String, Agg>)> {
    let files = list_parquet_files(history_dir)?;
    if files.is_empty() {
        return Ok((HashMap::new(), HashMap::new(), HashMap::new()));
    }

    let mut horses = HashSet::<String>::new();
    let mut jockeys = HashSet::<String>::new();
    let mut trainers = HashSet::<String>::new();
    for (_race, rs) in races {
        for r in rs {
            if !r.horse.trim().is_empty() {
                horses.insert(r.horse.clone());
            }
            if !r.jockey.trim().is_empty() {
                jockeys.insert(r.jockey.clone());
            }
            if !r.trainer.trim().is_empty() {
                trainers.insert(r.trainer.clone());
            }
        }
    }

    let horses_ref: HashSet<&str> = horses.iter().map(|s| s.as_str()).collect();
    let jockeys_ref: HashSet<&str> = jockeys.iter().map(|s| s.as_str()).collect();
    let trainers_ref: HashSet<&str> = trainers.iter().map(|s| s.as_str()).collect();

    let mut horse_agg: HashMap<String, Agg> = HashMap::new();
    let mut jockey_agg: HashMap<String, Agg> = HashMap::new();
    let mut trainer_agg: HashMap<String, Agg> = HashMap::new();

    for f in &files {
        let batches = read_parquet_all_batches(f)
            .with_context(|| format!("read parquet batches {}", f.display()))?;
        for b in batches {
            update_aggs_from_batch(
                &b,
                &horses_ref,
                &jockeys_ref,
                &trainers_ref,
                &mut horse_agg,
                &mut jockey_agg,
                &mut trainer_agg,
            )?;
        }
    }

    Ok((horse_agg, jockey_agg, trainer_agg))
}

fn predict_odds_for_runners(
    runners: &[RunnerMini],
    horse_agg: &HashMap<String, Agg>,
    jockey_agg: &HashMap<String, Agg>,
    trainer_agg: &HashMap<String, Agg>,
) {
    let mut preds: Vec<PredRow> = Vec::new();
    for r in runners {
        let h = horse_agg.get(&r.horse).copied().unwrap_or_default();
        let j = jockey_agg.get(&r.jockey).copied().unwrap_or_default();
        let t = trainer_agg.get(&r.trainer).copied().unwrap_or_default();

        let h_avg = h.avg_rpr().unwrap_or(0.0);
        let j_avg = j.avg_rpr().unwrap_or(0.0);
        let t_avg = t.avg_rpr().unwrap_or(0.0);
        let h_wr = h.win_rate().unwrap_or(0.0);

        let score = 1.0 + (0.75 * h_avg) + (0.15 * j_avg) + (0.10 * t_avg) + (20.0 * h_wr);

        preds.push(PredRow {
            horse: r.horse.clone(),
            jockey: r.jockey.clone(),
            trainer: r.trainer.clone(),
            score,
            prob: 0.0,
            fair_odds: 0.0,
        });
    }

    softmax_preds(&mut preds, 10.0);

    let sum_prob: f64 = preds.iter().map(|p| p.prob).sum();

    let mut w_h = "horse".len();
    let mut w_p = "prob".len();
    let mut w_o = "fair_odds".len();
    for p in &preds {
        w_h = w_h.max(p.horse.len());
        w_p = w_p.max(format!("{:.3}", p.prob).len());
        w_o = w_o.max(format!("{:.2}", p.fair_odds).len());
    }

    println!(
        "{:<w_h$}  {:>w_p$}  {:>w_o$}",
        "horse",
        "prob",
        "fair_odds",
        w_h = w_h,
        w_p = w_p,
        w_o = w_o
    );
    println!(
        "{}  {}  {}",
        "-".repeat(w_h),
        "-".repeat(w_p),
        "-".repeat(w_o)
    );

    preds.sort_by(|a, b| b.prob.partial_cmp(&a.prob).unwrap_or(std::cmp::Ordering::Equal));
    for p in preds {
        println!(
            "{:<w_h$}  {:>w_p$.3}  {:>w_o$.2}",
            p.horse,
            p.prob,
            p.fair_odds,
            w_h = w_h,
            w_p = w_p,
            w_o = w_o
        );
    }

    println!("sum_prob={:.6} ({:.2}%)", sum_prob, sum_prob * 100.0);
}

fn softmax_preds(preds: &mut [impl HasScore], temperature: f64) {
    if preds.is_empty() {
        return;
    }
    let max_s = preds
        .iter()
        .map(|p| p.score())
        .fold(f64::NEG_INFINITY, f64::max);
    let mut denom = 0.0;
    for p in preds.iter() {
        denom += ((p.score() - max_s) / temperature).exp();
    }

    if denom <= 0.0 {
        let prob = 1.0 / (preds.len() as f64);
        for p in preds.iter_mut() {
            p.set_prob_and_odds(prob);
        }
        return;
    }

    for p in preds.iter_mut() {
        let prob = ((p.score() - max_s) / temperature).exp() / denom;
        p.set_prob_and_odds(prob);
    }
}

trait HasScore {
    fn score(&self) -> f64;
    fn set_prob_and_odds(&mut self, prob: f64);
}

impl HasScore for PredRow {
    fn score(&self) -> f64 {
        self.score
    }
    fn set_prob_and_odds(&mut self, prob: f64) {
        self.prob = prob;
        self.fair_odds = if prob > 0.0 { 1.0 / prob } else { f64::INFINITY };
    }
}

fn update_aggs_from_batch(
    b: &arrow::record_batch::RecordBatch,
    horses: &std::collections::HashSet<&str>,
    jockeys: &std::collections::HashSet<&str>,
    trainers: &std::collections::HashSet<&str>,
    horse_agg: &mut HashMap<String, Agg>,
    jockey_agg: &mut HashMap<String, Agg>,
    trainer_agg: &mut HashMap<String, Agg>,
) -> anyhow::Result<()> {
    let schema = b.schema();

    let idx_horse = schema.index_of("horse").ok();
    let idx_jockey = schema.index_of("jockey").ok();
    let idx_trainer = schema.index_of("trainer").ok();
    let idx_rpr = schema.index_of("rpr").ok();
    let idx_pos = schema.index_of("position").ok();

    let Some(idx_horse) = idx_horse else {
        return Ok(());
    };

    for row in 0..b.num_rows() {
        let horse = get_string(b.column(idx_horse), row).unwrap_or_default();
        if horse.is_empty() {
            continue;
        }
        if !horses.contains(horse.as_str()) {
            continue;
        }

        let jockey = idx_jockey
            .and_then(|i| get_string(b.column(i), row))
            .unwrap_or_default();
        let trainer = idx_trainer
            .and_then(|i| get_string(b.column(i), row))
            .unwrap_or_default();
        let rpr = idx_rpr.and_then(|i| get_f64(b.column(i), row));
        let pos = idx_pos.and_then(|i| get_i64(b.column(i), row));

        horse_agg.entry(horse.clone()).or_default().add(rpr, pos);
        if !jockey.is_empty() && jockeys.contains(jockey.as_str()) {
            jockey_agg.entry(jockey).or_default().add(rpr, pos);
        }
        if !trainer.is_empty() && trainers.contains(trainer.as_str()) {
            trainer_agg.entry(trainer).or_default().add(rpr, pos);
        }
    }

    Ok(())
}

fn get_string(col: &dyn Array, row: usize) -> Option<String> {
    if col.is_null(row) {
        return None;
    }
    use arrow::datatypes::DataType;
    match col.data_type() {
        DataType::Utf8 => {
            let a = col.as_any().downcast_ref::<arrow::array::StringArray>()?;
            Some(a.value(row).trim().to_string())
        }
        DataType::LargeUtf8 => {
            let a = col
                .as_any()
                .downcast_ref::<arrow::array::LargeStringArray>()?;
            Some(a.value(row).trim().to_string())
        }
        _ => None,
    }
}

fn get_i64(col: &dyn Array, row: usize) -> Option<i64> {
    if col.is_null(row) {
        return None;
    }
    use arrow::datatypes::DataType;
    match col.data_type() {
        DataType::Int64 => {
            let a = col.as_any().downcast_ref::<arrow::array::Int64Array>()?;
            Some(a.value(row))
        }
        DataType::Int32 => {
            let a = col.as_any().downcast_ref::<arrow::array::Int32Array>()?;
            Some(a.value(row) as i64)
        }
        DataType::Utf8 | DataType::LargeUtf8 => get_string(col, row).and_then(|s| s.parse().ok()),
        _ => None,
    }
}

fn get_f64(col: &dyn Array, row: usize) -> Option<f64> {
    if col.is_null(row) {
        return None;
    }
    use arrow::datatypes::DataType;
    match col.data_type() {
        DataType::Float64 => {
            let a = col.as_any().downcast_ref::<arrow::array::Float64Array>()?;
            Some(a.value(row))
        }
        DataType::Float32 => {
            let a = col.as_any().downcast_ref::<arrow::array::Float32Array>()?;
            Some(a.value(row) as f64)
        }
        DataType::Int64 => {
            let a = col.as_any().downcast_ref::<arrow::array::Int64Array>()?;
            Some(a.value(row) as f64)
        }
        DataType::Int32 => {
            let a = col.as_any().downcast_ref::<arrow::array::Int32Array>()?;
            Some(a.value(row) as f64)
        }
        DataType::Utf8 | DataType::LargeUtf8 => get_string(col, row).and_then(|s| s.parse().ok()),
        _ => None,
    }
}

fn read_parquet_all_batches(
    path: &Path,
) -> anyhow::Result<Vec<arrow::record_batch::RecordBatch>> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let builder = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(f)
        .context("ParquetRecordBatchReaderBuilder::try_new")?;
    let mut reader = builder
        .with_batch_size(8192)
        .build()
        .context("build parquet record batch reader")?;

    let mut out = Vec::<arrow::record_batch::RecordBatch>::new();
    while let Some(batch) = reader.next() {
        let batch = batch.context("read record batch")?;
        out.push(batch);
    }
    Ok(out)
}

fn preview_history_parquet_dir(dir: &str, max_rows: usize) -> anyhow::Result<()> {
    let paths = list_parquet_files(dir)?;
    if paths.is_empty() {
        println!("(no parquet files found in {dir})");
        return Ok(());
    }

    let mut printed = 0usize;
    for p in paths {
        if printed >= max_rows {
            break;
        }
        let (rows, batches) = read_parquet_first_rows(&p, max_rows - printed)
            .with_context(|| format!("read parquet {}", p.display()))?;
        if batches.is_empty() {
            continue;
        }
        println!();
        println!("file: {} (showing {} rows)", p.display(), rows);
        arrow::util::pretty::print_batches(&batches).context("print parquet batches")?;
        printed += rows;
    }

    if printed == 0 {
        println!("(no rows found in parquet files)");
    }

    Ok(())
}

fn list_parquet_files(dir: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::<PathBuf>::new();
    let d = Path::new(dir);
    if !d.exists() {
        return Ok(out);
    }
    for ent in fs::read_dir(d).with_context(|| format!("read_dir {dir}"))? {
        let ent = ent.context("read_dir entry")?;
        let p = ent.path();
        if p.is_file() {
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if name == "_SUCCESS" || name.ends_with(".crc") {
                continue;
            }
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

fn read_parquet_first_rows(
    path: &Path,
    max_rows: usize,
) -> anyhow::Result<(usize, Vec<arrow::record_batch::RecordBatch>)> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let builder = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(f)
        .context("ParquetRecordBatchReaderBuilder::try_new")?;
    let mut reader = builder
        .with_batch_size(1024)
        .build()
        .context("build parquet record batch reader")?;

    let mut out = Vec::<arrow::record_batch::RecordBatch>::new();
    let mut read = 0usize;
    while let Some(batch) = reader.next() {
        let batch = batch.context("read record batch")?;
        if batch.num_rows() == 0 {
            continue;
        }

        if read >= max_rows {
            break;
        }

        let remaining = max_rows - read;
        let take = batch.num_rows().min(remaining);

        let b = if take == batch.num_rows() {
            batch
        } else {
            batch.slice(0, take)
        };

        read += b.num_rows();
        out.push(b);
    }

    Ok((read, out))
}

fn extract_all_races_from_jsonl(s: &str) -> anyhow::Result<Vec<(RaceKey, Vec<RunnerMini>)>> {
    let mut race_order: Vec<RaceKey> = Vec::new();
    let mut map: HashMap<RaceKey, Vec<RunnerMini>> = HashMap::new();

    for (i, line) in s.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let row: RunnerRow = serde_json::from_str(line)
            .with_context(|| format!("parse jsonl line {}", i + 1))?;

        if row.horse.trim().is_empty() {
            continue;
        }
        if is_non_runner(&row.horse) || is_non_runner(&row.jockey) || is_non_runner(&row.trainer) {
            continue;
        }

        let key = RaceKey {
            course: row.course,
            time: row.time,
            race_name: row.race_name,
        };

        if !map.contains_key(&key) {
            race_order.push(key.clone());
        }

        map.entry(key).or_default().push(RunnerMini {
            horse: row.horse,
            jockey: row.jockey,
            trainer: row.trainer,
        });
    }

    let mut out = Vec::<(RaceKey, Vec<RunnerMini>)>::new();
    for k in race_order {
        if let Some(v) = map.remove(&k) {
            if !v.is_empty() {
                out.push((k, v));
            }
        }
    }

    if out.is_empty() {
        anyhow::bail!("no jsonl records found")
    }

    Ok(out)
}

fn is_non_runner(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    let mut norm = String::with_capacity(t.len());
    for c in t.chars() {
        if c.is_ascii_alphanumeric() {
            norm.push(c.to_ascii_lowercase());
        }
    }
    norm == "nonrunner"
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
