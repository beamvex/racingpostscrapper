use anyhow::Context;
use arrow::array::{Array, ArrayRef, Float64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use chrono::{DateTime, Duration, Utc};
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::{WriterProperties, WriterVersion};
use std::fs::File;
use std::fs;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Default, Clone)]
struct RunnerMini {
    horse: String,
    jockey: String,
    trainer: String,
    odds: Option<f64>,
}

#[derive(Clone)]
struct PredExplainRow {
    horse: String,
    score: f64,
    prob: f64,
    fair_odds: f64,
    bookie_odds: Option<f64>,
}

#[derive(Default, Clone, PartialEq, Eq, Hash)]
struct RaceKey {
    course: String,
    time: String,
    race_name: String,
    going: String,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct HorseContextKey {
    horse: String,
    course: String,
    going: String,
}

struct ProbRow {
    date: String,
    course: String,
    time: String,
    race_name: String,
    going: String,
    horse: String,
    jockey: String,
    trainer: String,
    score: f64,
    prob: f64,
    fair_odds: f64,
    bookie_odds: Option<f64>,
}

fn main() -> anyhow::Result<()> {
    let today = racingpost_scraper::utils::current_utc_date_yyyy_mm_dd();
    eprintln!("today-first-race: date={today}");

    let mut root_arg: Option<String> = None;
    let mut in_path_arg: Option<String> = None;
    let mut history_dir_arg: Option<String> = None;
    let mut out_path_arg: Option<String> = None;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(p) = a.strip_prefix("--root=") {
            root_arg = Some(p.to_string());
        } else if a == "--root" && i + 1 < args.len() {
            i += 1;
            root_arg = Some(args[i].clone());
        } else if let Some(p) = a.strip_prefix("--in=") {
            in_path_arg = Some(p.to_string());
        } else if a == "--in" && i + 1 < args.len() {
            i += 1;
            in_path_arg = Some(args[i].clone());
        } else if let Some(p) = a.strip_prefix("--history-dir=") {
            history_dir_arg = Some(p.to_string());
        } else if a == "--history-dir" && i + 1 < args.len() {
            i += 1;
            history_dir_arg = Some(args[i].clone());
        } else if let Some(p) = a.strip_prefix("--out=") {
            out_path_arg = Some(p.to_string());
        } else if a == "--out" && i + 1 < args.len() {
            i += 1;
            out_path_arg = Some(args[i].clone());
        }
        i += 1;
    }

    let y = &today[0..4];
    let m = &today[5..7];
    let d = &today[8..10];

    let root = root_arg.unwrap_or_else(|| "/data".to_string());

    let in_path = in_path_arg.unwrap_or_else(|| {
        format!(
            "{}/racecards/{}/{}/{}/racingpost-racecards-{}-runners.parquet",
            root, y, m, d, today
        )
    });

    let history_dir = history_dir_arg
        .unwrap_or_else(|| format!("{}/processed/", root));

    let out_path = out_path_arg.unwrap_or_else(|| {
        format!(
            "{}/racecards/{}/{}/{}/racecard-probabilities-{}.parquet",
            root, y, m, d, today
        )
    });

    eprintln!("today-first-race: reading {in_path}");

    let races_all = extract_all_races_from_parquet(&in_path)?;
    let races = filter_recent_races(races_all.clone(), 30);
    eprintln!(
        "today-first-race: races_total={} races_recent={}",
        races_all.len(),
        races.len()
    );

    if races.is_empty() {
        anyhow::bail!("no races in the last 30 minutes or upcoming")
    }

    let (horse_agg, horse_ctx_agg, jockey_agg, trainer_agg) =
        build_history_aggs(&history_dir, &races)?;

    let mut prob_rows: Vec<ProbRow> = Vec::new();
    for (race, runners) in &races {
        let preds = compute_odds_rows_explained(
            race,
            runners,
            &horse_agg,
            &horse_ctx_agg,
            &jockey_agg,
            &trainer_agg,
        );
        for p in preds {
            let runner = runners.iter().find(|r| r.horse == p.horse);
            prob_rows.push(ProbRow {
                date: today.clone(),
                course: race.course.clone(),
                time: race.time.clone(),
                race_name: race.race_name.clone(),
                going: race.going.clone(),
                horse: p.horse,
                jockey: runner.map(|r| r.jockey.clone()).unwrap_or_default(),
                trainer: runner.map(|r| r.trainer.clone()).unwrap_or_default(),
                score: p.score,
                prob: p.prob,
                fair_odds: p.fair_odds,
                bookie_odds: p.bookie_odds,
            });
        }
    }

    if let Some(parent) = Path::new(&out_path).parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create_dir_all {}", parent.display()))?;
    }
    write_probabilities_parquet(&out_path, &prob_rows)?;
    eprintln!("today-first-race: wrote probabilities parquet {out_path}");

    Ok(())
}

fn filter_recent_races(
    races: Vec<(RaceKey, Vec<RunnerMini>)>,
    minutes_lookback: i64,
) -> Vec<(RaceKey, Vec<RunnerMini>)> {
    let cutoff = Utc::now() - Duration::minutes(minutes_lookback);
    races
        .into_iter()
        .filter(|(rk, _)| {
            if rk.time.trim().is_empty() {
                return true;
            }
            match DateTime::parse_from_rfc3339(rk.time.trim()) {
                Ok(dt) => dt.with_timezone(&Utc) > cutoff,
                Err(_) => true,
            }
        })
        .collect()
}

fn compute_odds_rows_explained(
    race: &RaceKey,
    runners: &[RunnerMini],
    horse_agg: &HashMap<String, Agg>,
    horse_ctx_agg: &HashMap<HorseContextKey, Agg>,
    jockey_agg: &HashMap<String, Agg>,
    trainer_agg: &HashMap<String, Agg>,
) -> Vec<PredExplainRow> {
    let mut preds: Vec<PredExplainRow> = Vec::new();

    let ctx_course = norm_key(&race.course);
    let ctx_going = norm_key(&race.going);
    let ctx_weight = 0.30;

    let prior_runs_horse = 10.0;
    let prior_runs_jockey = 30.0;
    let prior_runs_trainer = 30.0;

    for r in runners {
        let h_overall = horse_agg.get(&r.horse).copied().unwrap_or_default();
        let h_ctx = if !ctx_course.is_empty() || !ctx_going.is_empty() {
            horse_ctx_agg
                .get(&HorseContextKey {
                    horse: r.horse.clone(),
                    course: ctx_course.clone(),
                    going: ctx_going.clone(),
                })
                .copied()
                .unwrap_or_default()
        } else {
            Agg::default()
        };

        let h = blend_aggs(h_overall, h_ctx, ctx_weight);
        let j = jockey_agg.get(&r.jockey).copied().unwrap_or_default();
        let t = trainer_agg.get(&r.trainer).copied().unwrap_or_default();

        let h_avg_raw = h.avg_rpr().unwrap_or(0.0);
        let j_avg_raw = j.avg_rpr().unwrap_or(0.0);
        let t_avg_raw = t.avg_rpr().unwrap_or(0.0);
        let h_pts_raw = h.avg_points().unwrap_or(0.0);

        let h_avg = shrink_feature(h_avg_raw, h.count as f64, prior_runs_horse);
        let j_avg = shrink_feature(j_avg_raw, j.count as f64, prior_runs_jockey);
        let t_avg = shrink_feature(t_avg_raw, t.count as f64, prior_runs_trainer);
        let h_pts = shrink_feature(h_pts_raw, h.count as f64, prior_runs_horse);

        let score = 1.0 + (0.75 * h_avg) + (0.15 * j_avg) + (0.10 * t_avg) + (10.0 * h_pts);

        preds.push(PredExplainRow {
            horse: r.horse.clone(),
            score,
            prob: 0.0,
            fair_odds: 0.0,
            bookie_odds: r.odds,
        });
    }

    softmax_preds(&mut preds, 30.0);
    preds.sort_by(|a, b| b.prob.partial_cmp(&a.prob).unwrap_or(std::cmp::Ordering::Equal));
    preds
}

fn shrink_feature(value: f64, n: f64, prior_n: f64) -> f64 {
    if !value.is_finite() || n <= 0.0 {
        return 0.0;
    }
    let w = n / (n + prior_n.max(1.0));
    w * value
}

#[derive(Default, Clone, Copy)]
struct Agg {
    count: u64,
    win_count: u64,
    sum_rpr: f64,
    max_rpr: f64,
    sum_points: f64,
}

impl Agg {
    fn add(&mut self, rpr: Option<f64>, position: Option<i64>) {
        self.count += 1;
        if let Some(p) = position {
            if p == 1 {
                self.win_count += 1;
            }

            let pts = match p {
                1 => 5.0,
                2 => 3.0,
                3 => 2.0,
                4 => 1.0,
                _ => 0.0,
            };
            self.sum_points += pts;
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

    fn avg_points(&self) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        Some(self.sum_points / (self.count as f64))
    }
}

fn build_history_aggs(
    history_dir: &str,
    races: &[(RaceKey, Vec<RunnerMini>)],
) -> anyhow::Result<(
    HashMap<String, Agg>,
    HashMap<HorseContextKey, Agg>,
    HashMap<String, Agg>,
    HashMap<String, Agg>,
)> {
    let files = list_parquet_files(history_dir)?;
    if files.is_empty() {
        return Ok((HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new()));
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

    let mut contexts = HashSet::<(String, String)>::new();
    for (rk, _rs) in races {
        let c = norm_key(&rk.course);
        let g = norm_key(&rk.going);
        if !c.is_empty() || !g.is_empty() {
            contexts.insert((c, g));
        }
    }

    let horses_ref: HashSet<&str> = horses.iter().map(|s| s.as_str()).collect();
    let jockeys_ref: HashSet<&str> = jockeys.iter().map(|s| s.as_str()).collect();
    let trainers_ref: HashSet<&str> = trainers.iter().map(|s| s.as_str()).collect();

    let mut horse_agg: HashMap<String, Agg> = HashMap::new();
    let mut horse_ctx_agg: HashMap<HorseContextKey, Agg> = HashMap::new();
    let mut jockey_agg: HashMap<String, Agg> = HashMap::new();
    let mut trainer_agg: HashMap<String, Agg> = HashMap::new();

    for f in &files {
        let batches = read_parquet_all_batches(f)
            .with_context(|| format!("read parquet batches {}", f.display()))?;
        for b in batches {
            update_aggs_from_batch(
                &b,
                &contexts,
                &horses_ref,
                &jockeys_ref,
                &trainers_ref,
                &mut horse_agg,
                &mut horse_ctx_agg,
                &mut jockey_agg,
                &mut trainer_agg,
            )?;
        }
    }

    Ok((horse_agg, horse_ctx_agg, jockey_agg, trainer_agg))
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

impl HasScore for PredExplainRow {
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
    contexts: &std::collections::HashSet<(String, String)>,
    horses: &std::collections::HashSet<&str>,
    jockeys: &std::collections::HashSet<&str>,
    trainers: &std::collections::HashSet<&str>,
    horse_agg: &mut HashMap<String, Agg>,
    horse_ctx_agg: &mut HashMap<HorseContextKey, Agg>,
    jockey_agg: &mut HashMap<String, Agg>,
    trainer_agg: &mut HashMap<String, Agg>,
) -> anyhow::Result<()> {
    let schema = b.schema();

    let idx_horse = schema.index_of("horse").ok();
    let idx_jockey = schema.index_of("jockey").ok();
    let idx_trainer = schema.index_of("trainer").ok();
    let idx_rpr = schema.index_of("rpr").ok();
    let idx_pos = schema.index_of("position").ok();
    let idx_course = schema.index_of("course").ok();
    let idx_going = schema.index_of("going").ok();

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

        let course = idx_course
            .and_then(|i| get_string(b.column(i), row))
            .unwrap_or_default();
        let going = idx_going
            .and_then(|i| get_string(b.column(i), row))
            .unwrap_or_default();

        let course_norm = norm_key(&course);
        let going_norm = norm_key(&going);

        horse_agg.entry(horse.clone()).or_default().add(rpr, pos);

        if !contexts.is_empty() && contexts.contains(&(course_norm.clone(), going_norm.clone())) {
            horse_ctx_agg
                .entry(HorseContextKey {
                    horse: horse.clone(),
                    course: course_norm,
                    going: going_norm,
                })
                .or_default()
                .add(rpr, pos);
        }

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

fn blend_aggs(overall: Agg, ctx: Agg, ctx_weight: f64) -> Agg {
    if ctx.count == 0 {
        return overall;
    }

    let w = ctx_weight.clamp(0.0, 1.0);
    Agg {
        count: overall.count,
        win_count: overall.win_count,
        sum_rpr: (1.0 - w) * overall.sum_rpr + w * ctx.sum_rpr,
        max_rpr: overall.max_rpr,
        sum_points: (1.0 - w) * overall.sum_points + w * ctx.sum_points,
    }
}

fn norm_key(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(t.len());
    for c in t.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

fn time_minutes_since_midnight(s: &str) -> i32 {
    let t = s.trim();
    let idx = match t.find('T') {
        Some(i) => i,
        None => return i32::MAX,
    };
    let rest = &t[idx + 1..];
    let hh = rest.get(0..2).and_then(|x| x.parse::<i32>().ok());
    let mm = rest.get(3..5).and_then(|x| x.parse::<i32>().ok());
    match (hh, mm) {
        (Some(h), Some(m)) if (0..24).contains(&h) && (0..60).contains(&m) => (h * 60) + m,
        _ => i32::MAX,
    }
}

fn derive_course_from_race_name(fallback_course: &str, race_name: &str) -> String {
    let rn = race_name.trim();
    if rn.is_empty() {
        return fallback_course.trim().to_string();
    }

    if let Some(left) = rn.split(" Racecard").next() {
        let left = left.trim();
        if let Some((first, rest)) = left.split_once(' ') {
            let first = first.trim();
            let rest = rest.trim();
            if first.len() == 5 && first.as_bytes().get(2) == Some(&b':') && !rest.is_empty() {
                return rest.to_string();
            }
        }
    }

    fallback_course.trim().to_string()
}

fn list_parquet_files(dir: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::<PathBuf>::new();
    let d = Path::new(dir);
    if !d.exists() {
        return Ok(out);
    }
    list_parquet_files_recursive(d, &mut out)?;
    out.sort();
    Ok(out)
}

fn list_parquet_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for ent in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let ent = ent.context("read_dir entry")?;
        let p = ent.path();
        if p.is_dir() {
            list_parquet_files_recursive(&p, out)?;
        } else if p.is_file() {
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
    Ok(())
}

fn extract_all_races_from_parquet(path: &str) -> anyhow::Result<Vec<(RaceKey, Vec<RunnerMini>)>> {
    let batches = read_parquet_all_batches(Path::new(path))
        .with_context(|| format!("read parquet {}", path))?;

    let mut race_order: Vec<RaceKey> = Vec::new();
    let mut map: HashMap<RaceKey, Vec<RunnerMini>> = HashMap::new();

    for batch in &batches {
        let course_arr = column_as_strings(batch, "course")?;
        let time_arr = column_as_strings(batch, "time")?;
        let race_name_arr = column_as_strings(batch, "race_name")?;
        let going_arr = column_as_strings(batch, "going")?;
        let horse_arr = column_as_strings(batch, "horse")?;
        let jockey_arr = column_as_strings(batch, "jockey")?;
        let trainer_arr = column_as_strings(batch, "trainer")?;
        let odds_col = batch.column_by_name("odds");

        for i in 0..batch.num_rows() {
            let horse = horse_arr[i].clone();
            if horse.trim().is_empty() {
                continue;
            }
            if is_non_runner(&horse) || is_non_runner(&jockey_arr[i]) || is_non_runner(&trainer_arr[i]) {
                continue;
            }

            let derived_course = derive_course_from_race_name(&course_arr[i], &race_name_arr[i]);
            let key = RaceKey {
                course: derived_course,
                time: time_arr[i].clone(),
                race_name: race_name_arr[i].clone(),
                going: going_arr[i].clone(),
            };

            if !map.contains_key(&key) {
                race_order.push(key.clone());
            }

            let odds_val = odds_col
                .and_then(|c| get_string(c, i))
                .and_then(|s| s.parse::<f64>().ok());
            map.entry(key).or_default().push(RunnerMini {
                horse,
                jockey: jockey_arr[i].clone(),
                trainer: trainer_arr[i].clone(),
                odds: odds_val,
            });
        }
    }

    let mut out = Vec::<(RaceKey, Vec<RunnerMini>)>::new();
    for k in race_order {
        if let Some(v) = map.remove(&k) {
            if !v.is_empty() {
                out.push((k, v));
            }
        }
    }

    out.sort_by(|(a, _), (b, _)| {
        let am = time_minutes_since_midnight(&a.time);
        let bm = time_minutes_since_midnight(&b.time);
        am.cmp(&bm).then_with(|| a.time.cmp(&b.time))
    });

    if out.is_empty() {
        anyhow::bail!("no racecard records found in parquet")
    }

    Ok(out)
}

fn column_as_strings(batch: &arrow::record_batch::RecordBatch, name: &str) -> anyhow::Result<Vec<String>> {
    let col = batch
        .column_by_name(name)
        .with_context(|| format!("column '{}' not found", name))?;
    let arr = col
        .as_any()
        .downcast_ref::<arrow::array::StringArray>()
        .with_context(|| format!("column '{}' is not StringArray", name))?;
    Ok((0..arr.len()).map(|i| arr.value(i).to_string()).collect())
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

fn write_probabilities_parquet(out_path: &str, rows: &[ProbRow]) -> anyhow::Result<()> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("date", DataType::Utf8, true),
        Field::new("course", DataType::Utf8, true),
        Field::new("time", DataType::Utf8, true),
        Field::new("race_name", DataType::Utf8, true),
        Field::new("going", DataType::Utf8, true),
        Field::new("horse", DataType::Utf8, true),
        Field::new("jockey", DataType::Utf8, true),
        Field::new("trainer", DataType::Utf8, true),
        Field::new("score", DataType::Float64, true),
        Field::new("prob", DataType::Float64, true),
        Field::new("fair_odds", DataType::Float64, true),
        Field::new("bookie_odds", DataType::Float64, true),
    ]));

    let make_str = |f: fn(&ProbRow) -> &str| -> ArrayRef {
        Arc::new(StringArray::from(
            rows.iter()
                .map(|r| {
                    let s = f(r);
                    if s.is_empty() { None } else { Some(s) }
                })
                .collect::<Vec<_>>(),
        ))
    };
    let make_f64 = |f: fn(&ProbRow) -> f64| -> ArrayRef {
        Arc::new(Float64Array::from(
            rows.iter().map(|r| f(r)).collect::<Vec<f64>>(),
        ))
    };
    let bookie_col: ArrayRef = Arc::new(Float64Array::from(
        rows.iter().map(|r| r.bookie_odds).collect::<Vec<Option<f64>>>(),
    ));

    let columns: Vec<ArrayRef> = vec![
        make_str(|r| &r.date),
        make_str(|r| &r.course),
        make_str(|r| &r.time),
        make_str(|r| &r.race_name),
        make_str(|r| &r.going),
        make_str(|r| &r.horse),
        make_str(|r| &r.jockey),
        make_str(|r| &r.trainer),
        make_f64(|r| r.score),
        make_f64(|r| r.prob),
        make_f64(|r| r.fair_odds),
        bookie_col,
    ];

    let batch = RecordBatch::try_new(schema.clone(), columns)
        .context("build record batch")?;

    let file = File::create(out_path).with_context(|| format!("create {out_path}"))?;
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .set_dictionary_enabled(false)
        .set_writer_version(WriterVersion::PARQUET_1_0)
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))
        .context("create parquet writer")?;
    writer.write(&batch).context("write parquet")?;
    writer.close().context("close parquet")?;
    Ok(())
}
