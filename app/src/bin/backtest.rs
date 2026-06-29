use anyhow::Context;
use std::fs;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

fn list_parquet_files(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();

    let mut stack = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        let md = fs::metadata(&p).with_context(|| format!("stat {}", p.display()))?;
        if md.is_dir() {
            for ent in fs::read_dir(&p).with_context(|| format!("read_dir {}", p.display()))? {
                let ent = ent.with_context(|| format!("read_dir entry {}", p.display()))?;
                stack.push(ent.path());
            }
        } else if md.is_file() {
            if p.extension().and_then(|s| s.to_str()) == Some("parquet") {
                out.push(p);
            }
        }
    }

    out.sort();
    Ok(out)
}

fn validate_and_count_rows(path: &Path) -> anyhow::Result<usize> {
    let f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let builder = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(f)
        .context("ParquetRecordBatchReaderBuilder::try_new")?;

    // Force reading all record batches to validate the file isn't corrupt.
    let mut reader = builder.with_batch_size(8192).build()?;

    let mut rows = 0usize;
    while let Some(batch) = reader.next() {
        let batch = batch.with_context(|| format!("read batch {}", path.display()))?;
        rows += batch.num_rows();
    }

    Ok(rows)
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HorseContextKey {
    horse: String,
    course: String,
    going: String,
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

fn shrink_feature(value: f64, n: f64, prior_n: f64) -> f64 {
    if !value.is_finite() || n <= 0.0 {
        return 0.0;
    }
    let w = n / (n + prior_n.max(1.0));
    w * value
}

#[derive(Clone)]
struct RunnerRow {
    course: String,
    title: String,
    going: String,
    position: Option<i64>,
    horse: String,
    jockey: String,
    trainer: String,
    rpr: Option<f64>,
}

fn get_string(b: &arrow::record_batch::RecordBatch, col: &str, row: usize) -> Option<String> {
    let idx = b.schema().index_of(col).ok()?;
    let arr = b.column(idx);
    if arr.is_null(row) {
        return None;
    }
    match arr.data_type() {
        arrow::datatypes::DataType::Utf8 => {
            let a = arr.as_any().downcast_ref::<arrow::array::StringArray>()?;
            Some(a.value(row).trim().to_string())
        }
        arrow::datatypes::DataType::LargeUtf8 => {
            let a = arr
                .as_any()
                .downcast_ref::<arrow::array::LargeStringArray>()?;
            Some(a.value(row).trim().to_string())
        }
        _ => None,
    }
}

fn parse_i64(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok()
}

fn parse_f64(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok()
}

fn extract_partition_ymd(path: &Path) -> Option<chrono::NaiveDate> {
    let s = path.to_string_lossy().replace('\\', "/");
    let parts: Vec<&str> = s.split('/').collect();
    let mut y: Option<i32> = None;
    let mut m: Option<u32> = None;
    let mut d: Option<u32> = None;
    for p in parts {
        if let Some(rest) = p.strip_prefix("year=") {
            y = rest.parse::<i32>().ok();
        } else if let Some(rest) = p.strip_prefix("month=") {
            m = rest.parse::<u32>().ok();
        } else if let Some(rest) = p.strip_prefix("day=") {
            d = rest.parse::<u32>().ok();
        }
    }
    Some(chrono::NaiveDate::from_ymd_opt(y?, m?, d?)?)
}

fn parse_time_minutes_from_title(title: &str) -> Option<i32> {
    let t = title.trim();
    let bytes = t.as_bytes();
    for i in 0..bytes.len().saturating_sub(4) {
        let c1 = bytes[i] as char;
        if !c1.is_ascii_digit() {
            continue;
        }
        let mut j = i;
        while j < bytes.len() && (bytes[j] as char).is_ascii_digit() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] as char != ':' {
            continue;
        }
        let hh = t.get(i..j)?.parse::<i32>().ok()?;
        let mm = t.get(j + 1..j + 3)?.parse::<i32>().ok()?;
        if (0..24).contains(&hh) && (0..60).contains(&mm) {
            return Some(hh * 60 + mm);
        }
    }
    None
}

trait HasScore {
    fn score(&self) -> f64;
    fn set_prob(&mut self, p: f64);
}

#[derive(Clone)]
struct Pred {
    horse: String,
    prob: f64,
    score: f64,
}

impl HasScore for Pred {
    fn score(&self) -> f64 {
        self.score
    }
    fn set_prob(&mut self, p: f64) {
        self.prob = p;
    }
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
            p.set_prob(prob);
        }
        return;
    }

    for p in preds.iter_mut() {
        let prob = ((p.score() - max_s) / temperature).exp() / denom;
        p.set_prob(prob);
    }
}

fn compute_scores_for_race(
    course: &str,
    going: &str,
    runners: &[RunnerRow],
    horse_agg: &HashMap<String, Agg>,
    horse_ctx_agg: &HashMap<HorseContextKey, Agg>,
    jockey_agg: &HashMap<String, Agg>,
    trainer_agg: &HashMap<String, Agg>,
) -> Vec<(String, f64)> {
    let ctx_course = norm_key(course);
    let ctx_going = norm_key(going);
    let ctx_weight = 0.30;
    let prior_runs_horse = 10.0;
    let prior_runs_jockey = 30.0;
    let prior_runs_trainer = 30.0;

    let mut out = Vec::with_capacity(runners.len());
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
        out.push((r.horse.clone(), score));
    }
    out
}

#[derive(Clone)]
struct Metrics {
    races: u64,
    runners: u64,
    top1_correct: u64,
    logloss_sum: f64,
    brier_sum: f64,
    bet_count_top: u64,
    bet_profit_sum_top: f64,
    bet_avg_fair_odds_sum_top: f64,

    bet_count_mid: u64,
    bet_profit_sum_mid: f64,
    bet_avg_fair_odds_sum_mid: f64,

    bet_count_bottom: u64,
    bet_profit_sum_bottom: f64,
    bet_avg_fair_odds_sum_bottom: f64,
    cal_count: [u64; 20],
    cal_win: [u64; 20],
    cal_sum_p: [f64; 20],
}

impl Metrics {
    fn new() -> Self {
        Self {
            races: 0,
            runners: 0,
            top1_correct: 0,
            logloss_sum: 0.0,
            brier_sum: 0.0,
            bet_count_top: 0,
            bet_profit_sum_top: 0.0,
            bet_avg_fair_odds_sum_top: 0.0,

            bet_count_mid: 0,
            bet_profit_sum_mid: 0.0,
            bet_avg_fair_odds_sum_mid: 0.0,

            bet_count_bottom: 0,
            bet_profit_sum_bottom: 0.0,
            bet_avg_fair_odds_sum_bottom: 0.0,
            cal_count: [0; 20],
            cal_win: [0; 20],
            cal_sum_p: [0.0; 20],
        }
    }

    fn bet_top_roi(&self) -> f64 {
        if self.bet_count_top == 0 {
            return 0.0;
        }
        self.bet_profit_sum_top / (self.bet_count_top as f64)
    }

    fn avg_logloss(&self) -> f64 {
        if self.races == 0 {
            return 0.0;
        }
        self.logloss_sum / (self.races as f64)
    }

    fn avg_brier_per_runner(&self) -> f64 {
        if self.runners == 0 {
            return 0.0;
        }
        self.brier_sum / (self.runners as f64)
    }

    fn record_bet(&mut self, which: &str, horse: &str, prob: f64, winner_horse: &str) {
        let p = prob.max(1e-12);
        let fair_odds = 1.0 / p;
        let won = horse == winner_horse;
        let profit = if won { fair_odds - 1.0 } else { -1.0 };

        match which {
            "top" => {
                self.bet_count_top += 1;
                self.bet_avg_fair_odds_sum_top += fair_odds;
                self.bet_profit_sum_top += profit;
            }
            "mid" => {
                self.bet_count_mid += 1;
                self.bet_avg_fair_odds_sum_mid += fair_odds;
                self.bet_profit_sum_mid += profit;
            }
            "bottom" => {
                self.bet_count_bottom += 1;
                self.bet_avg_fair_odds_sum_bottom += fair_odds;
                self.bet_profit_sum_bottom += profit;
            }
            _ => {}
        }
    }

    fn record_race(&mut self, preds: &[Pred], winner_horse: &str) {
        if preds.len() < 2 {
            return;
        }

        let pw = preds
            .iter()
            .find(|p| p.horse == winner_horse)
            .map(|p| p.prob)
            .unwrap_or(0.0)
            .max(1e-12);
        self.logloss_sum += -pw.ln();

        let top1 = preds.first().map(|p| p.horse.as_str()).unwrap_or("");
        let top1_prob = preds.first().map(|p| p.prob).unwrap_or(0.0);

        let mid_idx = preds.len() / 2;
        let mid = preds.get(mid_idx).map(|p| p.horse.as_str()).unwrap_or("");
        let mid_prob = preds.get(mid_idx).map(|p| p.prob).unwrap_or(0.0);

        let bottom = preds.last().map(|p| p.horse.as_str()).unwrap_or("");
        let bottom_prob = preds.last().map(|p| p.prob).unwrap_or(0.0);

        let top1_won = top1 == winner_horse;

        if top1_won {
            self.top1_correct += 1;
        }

        // Hypothetical bets: stake £1 on top/middle/bottom pick at the model fair odds.
        self.record_bet("top", top1, top1_prob, winner_horse);
        self.record_bet("mid", mid, mid_prob, winner_horse);
        self.record_bet("bottom", bottom, bottom_prob, winner_horse);

        for p in preds {
            let y = if p.horse == winner_horse { 1.0 } else { 0.0 };
            let d = p.prob - y;
            self.brier_sum += d * d;

            let mut bi = (p.prob * 20.0).floor() as isize;
            if bi < 0 {
                bi = 0;
            }
            if bi as usize >= 20 {
                bi = 19;
            }
            let bi = bi as usize;
            self.cal_count[bi] += 1;
            self.cal_sum_p[bi] += p.prob;
            if y > 0.0 {
                self.cal_win[bi] += 1;
            }
        }

        self.races += 1;
        self.runners += preds.len() as u64;
    }

    fn print(&self, label: &str) {
        let acc = if self.races > 0 {
            (self.top1_correct as f64) / (self.races as f64)
        } else {
            0.0
        };
        let avg_logloss = if self.races > 0 {
            self.logloss_sum / (self.races as f64)
        } else {
            0.0
        };
        let avg_brier_per_runner = if self.runners > 0 {
            self.brier_sum / (self.runners as f64)
        } else {
            0.0
        };

        let bet_roi_top = if self.bet_count_top > 0 {
            self.bet_profit_sum_top / (self.bet_count_top as f64)
        } else {
            0.0
        };
        let bet_avg_fair_odds_top = if self.bet_count_top > 0 {
            self.bet_avg_fair_odds_sum_top / (self.bet_count_top as f64)
        } else {
            0.0
        };

        let bet_roi_mid = if self.bet_count_mid > 0 {
            self.bet_profit_sum_mid / (self.bet_count_mid as f64)
        } else {
            0.0
        };
        let bet_avg_fair_odds_mid = if self.bet_count_mid > 0 {
            self.bet_avg_fair_odds_sum_mid / (self.bet_count_mid as f64)
        } else {
            0.0
        };

        let bet_roi_bottom = if self.bet_count_bottom > 0 {
            self.bet_profit_sum_bottom / (self.bet_count_bottom as f64)
        } else {
            0.0
        };
        let bet_avg_fair_odds_bottom = if self.bet_count_bottom > 0 {
            self.bet_avg_fair_odds_sum_bottom / (self.bet_count_bottom as f64)
        } else {
            0.0
        };

        println!("label={}", label);
        println!("races={}", self.races);
        println!("runners={}", self.runners);
        println!("top1_accuracy={:.4}", acc);
        println!("avg_logloss={:.6}", avg_logloss);
        println!("avg_brier_per_runner={:.6}", avg_brier_per_runner);
        println!("bet_top_count={}", self.bet_count_top);
        println!("bet_top_profit_sum={:.6}", self.bet_profit_sum_top);
        println!("bet_top_roi={:.6}", bet_roi_top);
        println!("bet_top_avg_fair_odds={:.6}", bet_avg_fair_odds_top);

        let bet_top_profit_sum_fair_plus1 = self.bet_profit_sum_top + (self.top1_correct as f64);
        let bet_top_roi_fair_plus1 = if self.bet_count_top > 0 {
            bet_top_profit_sum_fair_plus1 / (self.bet_count_top as f64)
        } else {
            0.0
        };
        println!(
            "bet_top_profit_sum_fair_plus1={:.6}",
            bet_top_profit_sum_fair_plus1
        );
        println!("bet_top_roi_fair_plus1={:.6}", bet_top_roi_fair_plus1);

        println!("bet_mid_count={}", self.bet_count_mid);
        println!("bet_mid_profit_sum={:.6}", self.bet_profit_sum_mid);
        println!("bet_mid_roi={:.6}", bet_roi_mid);
        println!("bet_mid_avg_fair_odds={:.6}", bet_avg_fair_odds_mid);

        println!("bet_bottom_count={}", self.bet_count_bottom);
        println!("bet_bottom_profit_sum={:.6}", self.bet_profit_sum_bottom);
        println!("bet_bottom_roi={:.6}", bet_roi_bottom);
        println!("bet_bottom_avg_fair_odds={:.6}", bet_avg_fair_odds_bottom);

        println!("calibration_bucket_low,calibration_bucket_high,count,avg_predicted_p,empirical_win_rate");
        for i in 0..20 {
            let low = (i as f64) / 20.0;
            let high = ((i + 1) as f64) / 20.0;
            let n = self.cal_count[i];
            if n == 0 {
                println!("{:.3},{:.3},0,0.000000,0.000000", low, high);
                continue;
            }
            let avg_p = self.cal_sum_p[i] / (n as f64);
            let win_rate = (self.cal_win[i] as f64) / (n as f64);
            println!("{:.3},{:.3},{},{:.6},{:.6}", low, high, n, avg_p, win_rate);
        }
    }
}

fn parse_args() -> (String, chrono::NaiveDate, Option<chrono::NaiveDate>) {
    let mut root = "/data/processed".to_string();
    let mut start = "2021-01-01".to_string();
    let mut end: Option<String> = None;
    let mut html_out: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => {
                if let Some(v) = args.next() {
                    root = v;
                }
            }
            "--start-date" => {
                if let Some(v) = args.next() {
                    start = v;
                }
            }
            "--end-date" => {
                end = args.next();
            }
            "--html-out" => {
                html_out = args.next();
            }
            _ => {}
        }
    }

    let start = chrono::NaiveDate::parse_from_str(&start, "%Y-%m-%d")
        .with_context(|| format!("invalid --start-date {start}"))
        .unwrap();
    let end = match end {
        Some(s) => Some(
            chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                .with_context(|| format!("invalid --end-date {s}"))
                .unwrap(),
        ),
        None => None,
    };

    if let Some(p) = html_out {
        std::env::set_var("BACKTEST_HTML_OUT", p);
    }

    (root, start, end)
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

fn write_html_report(
    out_path: &str,
    start_date: chrono::NaiveDate,
    end_date: Option<chrono::NaiveDate>,
    first_date_seen: Option<chrono::NaiveDate>,
    last_date_seen: Option<chrono::NaiveDate>,
    parquet_failures: u64,
    uniform_races: u64,
    uniform_runners: u64,
    uniform_avg_logloss: f64,
    uniform_avg_brier_per_runner: f64,
    temps: &[f64],
    metrics_by_temp: &[Metrics],
    recommended_i: Option<usize>,
) -> anyhow::Result<()> {
    let mut w = std::io::BufWriter::new(
        std::fs::File::create(out_path).with_context(|| format!("create {out_path}"))?,
    );

    let title = "Backtest Summary";
    writeln!(w, "<!doctype html>")?;
    writeln!(w, "<html lang=\"en\">")?;
    writeln!(w, "<head>")?;
    writeln!(w, "<meta charset=\"utf-8\">")?;
    writeln!(w, "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">")?;
    writeln!(w, "<link href=\"https://cdn.jsdelivr.net/npm/bootstrap@5.3.3/dist/css/bootstrap.min.css\" rel=\"stylesheet\" integrity=\"sha384-QWTKZyjpPEjISv5WaRU9OFeRpok6YctnYmDr5pNlyT2bRjXh0JMhjY6hW+ALEwIH\" crossorigin=\"anonymous\">")?;
    writeln!(w, "<title>{}</title>", html_escape(title))?;
    writeln!(w, "</head>")?;
    writeln!(w, "<body class=\"container py-4\">")?;
    writeln!(w, "<h1 class=\"h4\">{}</h1>", html_escape(title))?;

    writeln!(w, "<div class=\"mb-3 small text-muted\">")?;
    writeln!(w, "<div><strong>start_date</strong>: {}</div>", html_escape(&start_date.to_string()))?;
    writeln!(w, "<div><strong>end_date</strong>: {}</div>", html_escape(&end_date.map(|d| d.to_string()).unwrap_or_default()))?;
    writeln!(w, "<div><strong>first_date_seen</strong>: {}</div>", html_escape(&first_date_seen.map(|d| d.to_string()).unwrap_or_default()))?;
    writeln!(w, "<div><strong>last_date_seen</strong>: {}</div>", html_escape(&last_date_seen.map(|d| d.to_string()).unwrap_or_default()))?;
    writeln!(w, "<div><strong>parquet_failures</strong>: {}</div>", parquet_failures)?;
    writeln!(w, "</div>")?;

    writeln!(w, "<h2 class=\"h6 mt-4\">Uniform baseline</h2>")?;
    writeln!(w, "<div class=\"table-responsive\"><table class=\"table table-sm\">")?;
    writeln!(w, "<thead><tr><th>races</th><th>runners</th><th class=\"text-end\">avg_logloss</th><th class=\"text-end\">avg_brier_per_runner</th></tr></thead><tbody>")?;
    writeln!(w, "<tr><td>{}</td><td>{}</td><td class=\"text-end\">{:.6}</td><td class=\"text-end\">{:.6}</td></tr>", uniform_races, uniform_runners, uniform_avg_logloss, uniform_avg_brier_per_runner)?;
    writeln!(w, "</tbody></table></div>")?;

    writeln!(w, "<h2 class=\"h6 mt-4\">Temperature sweep</h2>")?;
    if let Some(i) = recommended_i {
        writeln!(
            w,
            "<div class=\"alert alert-info py-2\"><strong>Recommended temperature</strong>: {:.1} (min |top ROI|)</div>",
            temps[i]
        )?;
    }

    writeln!(w, "<div class=\"table-responsive\"><table class=\"table table-sm table-striped\">")?;
    writeln!(w, "<thead><tr><th>temp</th><th class=\"text-end\">top_roi</th><th class=\"text-end\">avg_logloss</th><th class=\"text-end\">avg_brier_per_runner</th></tr></thead><tbody>")?;
    for (i, temp) in temps.iter().enumerate() {
        let m = &metrics_by_temp[i];
        writeln!(
            w,
            "<tr><td>{:.1}</td><td class=\"text-end\">{:.6}</td><td class=\"text-end\">{:.6}</td><td class=\"text-end\">{:.6}</td></tr>",
            temp,
            m.bet_top_roi(),
            m.avg_logloss(),
            m.avg_brier_per_runner()
        )?;
    }
    writeln!(w, "</tbody></table></div>")?;

    writeln!(w, "<h2 class=\"h6 mt-4\">Details</h2>")?;
    writeln!(w, "<div class=\"accordion\" id=\"acc\">")?;
    for (i, temp) in temps.iter().enumerate() {
        let m = &metrics_by_temp[i];
        let item_id = format!("t{}", i);
        writeln!(w, "<div class=\"accordion-item\">")?;
        writeln!(w, "<h2 class=\"accordion-header\" id=\"h-{id}\">", id = item_id)?;
        writeln!(
            w,
            "<button class=\"accordion-button collapsed\" type=\"button\" data-bs-toggle=\"collapse\" data-bs-target=\"#c-{id}\" aria-expanded=\"false\" aria-controls=\"c-{id}\">temp={:.1} (top_roi={:.4}, logloss={:.4}, brier={:.4})</button>",
            temp,
            m.bet_top_roi(),
            m.avg_logloss(),
            m.avg_brier_per_runner(),
            id = item_id
        )?;
        writeln!(w, "</h2>")?;
        writeln!(w, "<div id=\"c-{id}\" class=\"accordion-collapse collapse\" aria-labelledby=\"h-{id}\" data-bs-parent=\"#acc\">", id = item_id)?;
        writeln!(w, "<div class=\"accordion-body\">")?;

        writeln!(w, "<div class=\"table-responsive\"><table class=\"table table-sm\">")?;
        writeln!(w, "<thead><tr><th></th><th class=\"text-end\">count</th><th class=\"text-end\">profit_sum</th><th class=\"text-end\">roi</th><th class=\"text-end\">avg_fair_odds</th><th class=\"text-end\">profit_sum (fair+1)</th><th class=\"text-end\">roi (fair+1)</th></tr></thead><tbody>")?;
        let top_roi = m.bet_top_roi();
        let top_avg_odds = if m.bet_count_top > 0 { m.bet_avg_fair_odds_sum_top / (m.bet_count_top as f64) } else { 0.0 };
        let top_profit_plus1 = m.bet_profit_sum_top + (m.top1_correct as f64);
        let top_roi_plus1 = if m.bet_count_top > 0 { top_profit_plus1 / (m.bet_count_top as f64) } else { 0.0 };
        let mid_roi = if m.bet_count_mid > 0 { m.bet_profit_sum_mid / (m.bet_count_mid as f64) } else { 0.0 };
        let mid_avg_odds = if m.bet_count_mid > 0 { m.bet_avg_fair_odds_sum_mid / (m.bet_count_mid as f64) } else { 0.0 };
        let bottom_roi = if m.bet_count_bottom > 0 { m.bet_profit_sum_bottom / (m.bet_count_bottom as f64) } else { 0.0 };
        let bottom_avg_odds = if m.bet_count_bottom > 0 { m.bet_avg_fair_odds_sum_bottom / (m.bet_count_bottom as f64) } else { 0.0 };

        writeln!(w, "<tr><td><strong>top</strong></td><td class=\"text-end\">{}</td><td class=\"text-end\">{:.2}</td><td class=\"text-end\">{:.6}</td><td class=\"text-end\">{:.2}</td><td class=\"text-end\">{:.2}</td><td class=\"text-end\">{:.6}</td></tr>", m.bet_count_top, m.bet_profit_sum_top, top_roi, top_avg_odds, top_profit_plus1, top_roi_plus1)?;
        writeln!(w, "<tr><td><strong>mid</strong></td><td class=\"text-end\">{}</td><td class=\"text-end\">{:.2}</td><td class=\"text-end\">{:.6}</td><td class=\"text-end\">{:.2}</td><td class=\"text-end\">-</td><td class=\"text-end\">-</td></tr>", m.bet_count_mid, m.bet_profit_sum_mid, mid_roi, mid_avg_odds)?;
        writeln!(w, "<tr><td><strong>bottom</strong></td><td class=\"text-end\">{}</td><td class=\"text-end\">{:.2}</td><td class=\"text-end\">{:.6}</td><td class=\"text-end\">{:.2}</td><td class=\"text-end\">-</td><td class=\"text-end\">-</td></tr>", m.bet_count_bottom, m.bet_profit_sum_bottom, bottom_roi, bottom_avg_odds)?;
        writeln!(w, "</tbody></table></div>")?;

        writeln!(w, "<div class=\"table-responsive\"><table class=\"table table-sm table-striped\">")?;
        writeln!(w, "<thead><tr><th>p_low</th><th>p_high</th><th class=\"text-end\">count</th><th class=\"text-end\">avg_p</th><th class=\"text-end\">win_rate</th></tr></thead><tbody>")?;
        for bi in 0..20 {
            let low = (bi as f64) / 20.0;
            let high = ((bi + 1) as f64) / 20.0;
            let n = m.cal_count[bi];
            let avg_p = if n > 0 { m.cal_sum_p[bi] / (n as f64) } else { 0.0 };
            let win_rate = if n > 0 { (m.cal_win[bi] as f64) / (n as f64) } else { 0.0 };
            writeln!(w, "<tr><td>{:.3}</td><td>{:.3}</td><td class=\"text-end\">{}</td><td class=\"text-end\">{:.6}</td><td class=\"text-end\">{:.6}</td></tr>", low, high, n, avg_p, win_rate)?;
        }
        writeln!(w, "</tbody></table></div>")?;

        writeln!(w, "</div></div></div></div>")?;
    }
    writeln!(w, "</div>")?;

    writeln!(w, "<script src=\"https://cdn.jsdelivr.net/npm/bootstrap@5.3.3/dist/js/bootstrap.bundle.min.js\" integrity=\"sha384-YvpcrYf0tY3lHB60NNkmXc5s9fDVZLESaAA55NDzOxhy9GkcIdslK1eN7N6jIeHz\" crossorigin=\"anonymous\"></script>")?;
    writeln!(w, "</body></html>")?;
    w.flush().ok();
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let (root, start_date, end_date) = parse_args();
    let root = Path::new(&root);
    eprintln!("backtest: root={} start_date={} end_date={}", root.display(), start_date, end_date.map(|d| d.to_string()).unwrap_or_else(|| "".to_string()));

    if !root.exists() {
        anyhow::bail!("missing directory: {}", root.display());
    }

    let mut files: Vec<PathBuf> = list_parquet_files(root)?
        .into_iter()
        .filter(|p| extract_partition_ymd(p).is_some())
        .collect();

    files.sort_by_key(|p| extract_partition_ymd(p).unwrap());

    let mut horse_agg: HashMap<String, Agg> = HashMap::new();
    let mut horse_ctx_agg: HashMap<HorseContextKey, Agg> = HashMap::new();
    let mut jockey_agg: HashMap<String, Agg> = HashMap::new();
    let mut trainer_agg: HashMap<String, Agg> = HashMap::new();

    let temps: [f64; 9] = [10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0, 50.0, 60.0];
    let mut metrics_by_temp: Vec<Metrics> = temps.iter().map(|_| Metrics::new()).collect();

    let mut uniform_races: u64 = 0;
    let mut uniform_runners: u64 = 0;
    let mut uniform_logloss_sum: f64 = 0.0;
    let mut uniform_brier_sum: f64 = 0.0;

    let mut parquet_failures: u64 = 0;
    let mut first_date_seen: Option<chrono::NaiveDate> = None;
    let mut last_date_seen: Option<chrono::NaiveDate> = None;

    for file in &files {
        let date = extract_partition_ymd(file).unwrap();
        if date < start_date {
            continue;
        }
        if let Some(end) = end_date {
            if date > end {
                continue;
            }
        }

        first_date_seen.get_or_insert(date);
        last_date_seen = Some(date);

        if let Err(e) = validate_and_count_rows(file) {
            parquet_failures += 1;
            eprintln!("backtest: FAILED parquet {}: {:#}", file.display(), e);
            continue;
        }

        let f = std::fs::File::open(file).with_context(|| format!("open {}", file.display()))?;
        let builder = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(f)
            .context("ParquetRecordBatchReaderBuilder::try_new")?;
        let mut reader = builder.with_batch_size(8192).build()?;

        let mut per_race: HashMap<String, Vec<RunnerRow>> = HashMap::new();
        while let Some(batch) = reader.next() {
            let b = batch.context("read record batch")?;
            for row in 0..b.num_rows() {
                let race_id = get_string(&b, "race_id", row).unwrap_or_default();
                if race_id.is_empty() {
                    continue;
                }
                let course = get_string(&b, "course", row).unwrap_or_default();
                let title = get_string(&b, "title", row).unwrap_or_default();
                let going = get_string(&b, "going", row).unwrap_or_default();
                let pos = get_string(&b, "position", row).and_then(|s| parse_i64(&s));
                let horse = get_string(&b, "horse", row).unwrap_or_default();
                let jockey = get_string(&b, "jockey", row).unwrap_or_default();
                let trainer = get_string(&b, "trainer", row).unwrap_or_default();
                let rpr = get_string(&b, "rpr", row).and_then(|s| parse_f64(&s));

                if horse.trim().is_empty() {
                    continue;
                }

                per_race.entry(race_id.clone()).or_default().push(RunnerRow {
                    course,
                    title,
                    going,
                    position: pos,
                    horse,
                    jockey,
                    trainer,
                    rpr,
                });
            }
        }

        let mut race_ids: Vec<String> = per_race.keys().cloned().collect();
        race_ids.sort_by_key(|rid| {
            let rs = per_race.get(rid).map(|v| v.as_slice()).unwrap_or(&[]);
            rs.first()
                .and_then(|r| parse_time_minutes_from_title(&r.title))
                .unwrap_or(i32::MAX)
        });

        for rid in race_ids {
            let rs = per_race.get(&rid).cloned().unwrap_or_default();
            if rs.len() < 2 {
                continue;
            }

            let course = rs.first().map(|r| r.course.as_str()).unwrap_or("");
            let going = rs.first().map(|r| r.going.as_str()).unwrap_or("");

            let winner = rs.iter().find(|r| r.position == Some(1));
            let Some(w) = winner else {
                continue;
            };

            let scores = compute_scores_for_race(
                course,
                going,
                &rs,
                &horse_agg,
                &horse_ctx_agg,
                &jockey_agg,
                &trainer_agg,
            );

            if scores.len() < 2 {
                continue;
            }

            let n = scores.len() as f64;
            let p0 = 1.0 / n;
            uniform_races += 1;
            uniform_runners += scores.len() as u64;
            uniform_logloss_sum += -p0.max(1e-12).ln();
            // mean per-runner brier for uniform is (1/n - 1/n^2)
            uniform_brier_sum += scores.len() as f64 * (p0 - (p0 * p0));

            for (mi, temp) in temps.iter().enumerate() {
                let mut preds: Vec<Pred> = scores
                    .iter()
                    .map(|(h, s)| Pred {
                        horse: h.clone(),
                        prob: 0.0,
                        score: *s,
                    })
                    .collect();
                softmax_preds(&mut preds, *temp);
                preds.sort_by(|a, b| b.prob.partial_cmp(&a.prob).unwrap_or(std::cmp::Ordering::Equal));
                metrics_by_temp[mi].record_race(&preds, &w.horse);
            }

            for r in &rs {
                let horse = r.horse.trim();
                if !horse.is_empty() {
                    horse_agg
                        .entry(horse.to_string())
                        .or_default()
                        .add(r.rpr, r.position);
                }

                let course_norm = norm_key(&r.course);
                let going_norm = norm_key(&r.going);
                if !course_norm.is_empty() || !going_norm.is_empty() {
                    horse_ctx_agg
                        .entry(HorseContextKey {
                            horse: horse.to_string(),
                            course: course_norm,
                            going: going_norm,
                        })
                        .or_default()
                        .add(r.rpr, r.position);
                }

                let jockey = r.jockey.trim();
                if !jockey.is_empty() {
                    jockey_agg
                        .entry(jockey.to_string())
                        .or_default()
                        .add(r.rpr, r.position);
                }

                let trainer = r.trainer.trim();
                if !trainer.is_empty() {
                    trainer_agg
                        .entry(trainer.to_string())
                        .or_default()
                        .add(r.rpr, r.position);
                }
            }
        }
    }

    println!("start_date={}", start_date);
    println!("end_date={}", end_date.map(|d| d.to_string()).unwrap_or_else(|| "".to_string()));
    println!("first_date_seen={}", first_date_seen.map(|d| d.to_string()).unwrap_or_else(|| "".to_string()));
    println!("last_date_seen={}", last_date_seen.map(|d| d.to_string()).unwrap_or_else(|| "".to_string()));
    println!("parquet_failures={}", parquet_failures);

    let uniform_avg_logloss = if uniform_races > 0 {
        uniform_logloss_sum / (uniform_races as f64)
    } else {
        0.0
    };
    let uniform_avg_brier_per_runner = if uniform_runners > 0 {
        uniform_brier_sum / (uniform_runners as f64)
    } else {
        0.0
    };

    if uniform_races > 0 {
        println!("label=uniform");
        println!("races={}", uniform_races);
        println!("runners={}", uniform_runners);
        println!("avg_logloss={:.6}", uniform_avg_logloss);
        println!("avg_brier_per_runner={:.6}", uniform_avg_brier_per_runner);
    }

    println!("temperature,top_roi,avg_logloss,avg_brier_per_runner");
    let mut best_i: Option<usize> = None;
    let mut best_abs_roi = f64::INFINITY;
    for (i, temp) in temps.iter().enumerate() {
        let m = &metrics_by_temp[i];
        let roi = m.bet_top_roi();
        let abs_roi = roi.abs();
        if abs_roi < best_abs_roi {
            best_abs_roi = abs_roi;
            best_i = Some(i);
        }
        println!(
            "{:.1},{:.6},{:.6},{:.6}",
            temp,
            roi,
            m.avg_logloss(),
            m.avg_brier_per_runner()
        );
    }
    if let Some(i) = best_i {
        println!(
            "recommended_temperature={:.1} (min_abs_top_roi={:.6})",
            temps[i],
            best_abs_roi
        );
    }

    let html_out = std::env::var("BACKTEST_HTML_OUT").ok();
    if let Some(p) = html_out.as_deref() {
        if !p.trim().is_empty() {
            write_html_report(
                p,
                start_date,
                end_date,
                first_date_seen,
                last_date_seen,
                parquet_failures,
                uniform_races,
                uniform_runners,
                uniform_avg_logloss,
                uniform_avg_brier_per_runner,
                &temps,
                &metrics_by_temp,
                best_i,
            )?;
            eprintln!("backtest: wrote html report to {}", p);
        }
    }

    for (i, temp) in temps.iter().enumerate() {
        metrics_by_temp[i].print(&format!("temp_{:.0}", temp));
    }

    if parquet_failures > 0 {
        anyhow::bail!("one or more parquet files failed validation")
    }

    Ok(())
}
