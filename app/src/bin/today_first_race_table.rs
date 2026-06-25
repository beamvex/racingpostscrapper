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
    let mut out_path_arg: Option<String> = None;
    for a in std::env::args().skip(1) {
        if let Some(p) = a.strip_prefix("--in=") {
            in_path_arg = Some(p.to_string());
            continue;
        }
        if let Some(p) = a.strip_prefix("--history-dir=") {
            history_dir_arg = Some(p.to_string());
            continue;
        }
        if let Some(p) = a.strip_prefix("--out=") {
            out_path_arg = Some(p.to_string());
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

    let out_path = out_path_arg.unwrap_or_else(|| {
        format!(
            "/data/racecards/{}/{}/{}/racecard-report-{}.html",
            y, m, d, today
        )
    });

    eprintln!("today-first-race: reading {in_path}");
    let bytes = fs::read(&in_path).with_context(|| format!("read {in_path}"))?;
    let s = String::from_utf8(bytes).context("decode input as utf-8")?;

    let races = extract_all_races_from_jsonl(&s)?;
    eprintln!("today-first-race: races={}", races.len());

    let (horse_agg, jockey_agg, trainer_agg) = build_history_aggs(&history_dir, &races)?;

    let html = build_html_report(
        &today,
        &in_path,
        &history_dir,
        &races,
        &horse_agg,
        &jockey_agg,
        &trainer_agg,
    );

    if let Some(parent) = Path::new(&out_path).parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create_dir_all {}", parent.display()))?;
    }
    fs::write(&out_path, html).with_context(|| format!("write {out_path}"))?;
    eprintln!("today-first-race: wrote report {out_path}");

    Ok(())
}

fn build_html_report(
    day: &str,
    in_path: &str,
    history_dir: &str,
    races: &[(RaceKey, Vec<RunnerMini>)],
    horse_agg: &HashMap<String, Agg>,
    jockey_agg: &HashMap<String, Agg>,
    trainer_agg: &HashMap<String, Agg>,
) -> String {
    let mut out = String::new();

    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<link href=\"https://cdn.jsdelivr.net/npm/bootstrap@5.3.3/dist/css/bootstrap.min.css\" rel=\"stylesheet\" integrity=\"sha384-QWTKZyjpPEjISv5WaRU9OFeRpok6YctnYmDr5pNlyT2bRjXh0JMhjY6hW+ALEwIH\" crossorigin=\"anonymous\">\n");
    out.push_str("<title>");
    out.push_str(&html_escape(day));
    out.push_str(" Racecard Report</title>\n");
    out.push_str("</head>\n<body>\n");
    out.push_str("<div class=\"container my-4\">\n");

    out.push_str("<div class=\"d-flex justify-content-between align-items-end\">\n");
    out.push_str("<div>\n");
    out.push_str("<h1 class=\"h3 mb-1\">Racecard Report</h1>\n");
    out.push_str("<div class=\"text-muted\">Date: ");
    out.push_str(&html_escape(day));
    out.push_str("</div>\n");
    out.push_str("</div>\n");
    out.push_str("</div>\n");

    out.push_str("<hr class=\"my-3\">\n");
    out.push_str("<div class=\"small text-muted\">\n");
    out.push_str("<div><strong>Racecard JSONL:</strong> ");
    out.push_str(&html_escape(in_path));
    out.push_str("</div>\n");
    out.push_str("<div><strong>History dir:</strong> ");
    out.push_str(&html_escape(history_dir));
    out.push_str("</div>\n");
    out.push_str("</div>\n");

    out.push_str("<div class=\"accordion my-4\" id=\"racesAccordion\">\n");
    for (idx, (race, runners)) in races.iter().enumerate() {
        let race_id = format!("race{}", idx + 1);
        let heading_id = format!("heading{}", idx + 1);
        let collapse_id = format!("collapse{}", idx + 1);

        let odds = compute_odds_rows(runners, horse_agg, jockey_agg, trainer_agg);
        let sum_prob: f64 = odds.iter().map(|o| o.prob).sum();

        out.push_str("<div class=\"accordion-item\">\n");
        out.push_str("<h2 class=\"accordion-header\" id=\"");
        out.push_str(&heading_id);
        out.push_str("\">\n");
        out.push_str("<button class=\"accordion-button");
        if idx != 0 {
            out.push_str(" collapsed");
        }
        out.push_str("\" type=\"button\" data-bs-toggle=\"collapse\" data-bs-target=\"#");
        out.push_str(&collapse_id);
        out.push_str("\" aria-expanded=\"");
        out.push_str(if idx == 0 { "true" } else { "false" });
        out.push_str("\" aria-controls=\"");
        out.push_str(&collapse_id);
        out.push_str("\">\n");

        out.push_str(&html_escape(&race.course));
        out.push_str(" — ");
        out.push_str(&html_escape(&race.time));
        if !race.race_name.trim().is_empty() {
            out.push_str(" — ");
            out.push_str(&html_escape(&race.race_name));
        }
        out.push_str("</button>\n</h2>\n");

        out.push_str("<div id=\"");
        out.push_str(&collapse_id);
        out.push_str("\" class=\"accordion-collapse collapse");
        if idx == 0 {
            out.push_str(" show");
        }
        out.push_str("\" aria-labelledby=\"");
        out.push_str(&heading_id);
        out.push_str("\" data-bs-parent=\"#racesAccordion\">\n");

        out.push_str("<div class=\"accordion-body\">\n");

        out.push_str("<div class=\"row g-4\">\n");
        out.push_str("<div class=\"col-12 col-lg-7\">\n");
        out.push_str("<h3 class=\"h6\">Runners</h3>\n");
        out.push_str(&runners_table_html(runners, &race_id));
        out.push_str("</div>\n");

        out.push_str("<div class=\"col-12 col-lg-5\">\n");
        out.push_str("<div class=\"d-flex justify-content-between align-items-baseline\">\n");
        out.push_str("<h3 class=\"h6 mb-0\">Fair odds (heuristic)</h3>\n");
        out.push_str("<div class=\"text-muted small\">sum prob: ");
        out.push_str(&format!("{:.2}%", sum_prob * 100.0));
        out.push_str("</div>\n</div>\n");
        out.push_str(&odds_table_html(&odds));
        out.push_str("</div>\n");
        out.push_str("</div>\n");

        out.push_str("</div>\n</div>\n</div>\n");
        out.push_str("</div>\n");
    }
    out.push_str("</div>\n");

    out.push_str("</div>\n");
    out.push_str("<script src=\"https://cdn.jsdelivr.net/npm/bootstrap@5.3.3/dist/js/bootstrap.bundle.min.js\" integrity=\"sha384-YvpcrYf0tY3lHB60NNkmXc5s9fDVZLESaAA55NDzOxhy9GkcIdslK1eN7N6jIeHz\" crossorigin=\"anonymous\"></script>\n");
    out.push_str("</body>\n</html>\n");

    out
}

fn runners_table_html(runners: &[RunnerMini], table_id: &str) -> String {
    let mut out = String::new();
    out.push_str("<div class=\"table-responsive\">\n");
    out.push_str("<table class=\"table table-sm table-striped align-middle\" id=\"");
    out.push_str(&html_escape(table_id));
    out.push_str("\">\n<thead><tr><th>Horse</th><th>Jockey</th><th>Trainer</th></tr></thead>\n<tbody>\n");
    for r in runners {
        out.push_str("<tr><td>");
        out.push_str(&html_escape(&r.horse));
        out.push_str("</td><td>");
        out.push_str(&html_escape(&r.jockey));
        out.push_str("</td><td>");
        out.push_str(&html_escape(&r.trainer));
        out.push_str("</td></tr>\n");
    }
    out.push_str("</tbody></table></div>\n");
    out
}

fn odds_table_html(odds: &[PredRow]) -> String {
    let mut out = String::new();
    out.push_str("<div class=\"table-responsive\">\n");
    out.push_str("<table class=\"table table-sm table-hover align-middle\">\n");
    out.push_str("<thead><tr><th>Horse</th><th class=\"text-end\">Prob</th><th class=\"text-end\">Fair odds</th></tr></thead>\n<tbody>\n");
    for r in odds {
        out.push_str("<tr><td>");
        out.push_str(&html_escape(&r.horse));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.3}", r.prob));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.2}", r.fair_odds));
        out.push_str("</td></tr>\n");
    }
    out.push_str("</tbody></table></div>\n");
    out
}

fn compute_odds_rows(
    runners: &[RunnerMini],
    horse_agg: &HashMap<String, Agg>,
    jockey_agg: &HashMap<String, Agg>,
    trainer_agg: &HashMap<String, Agg>,
) -> Vec<PredRow> {
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
            score,
            prob: 0.0,
            fair_odds: 0.0,
        });
    }

    softmax_preds(&mut preds, 10.0);
    preds.sort_by(|a, b| b.prob.partial_cmp(&a.prob).unwrap_or(std::cmp::Ordering::Equal));
    preds
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
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

        let derived_course = derive_course_from_race_name(&row.course, &row.race_name);
        let key = RaceKey {
            course: derived_course,
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
