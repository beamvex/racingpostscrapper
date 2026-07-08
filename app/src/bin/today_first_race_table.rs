use anyhow::Context;
use arrow::array::Array;
use chrono::{DateTime, Duration, Utc};
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

#[derive(Clone)]
struct PredExplainRow {
    horse: String,
    score: f64,
    prob: f64,
    fair_odds: f64,
    h_count_overall: u64,
    h_count_ctx: u64,
    h_avg_rpr_raw: f64,
    h_avg_rpr_shrunk: f64,
    h_avg_points_raw: f64,
    h_avg_points_shrunk: f64,
    j_count: u64,
    j_avg_rpr_raw: f64,
    j_avg_rpr_shrunk: f64,
    t_count: u64,
    t_avg_rpr_raw: f64,
    t_avg_rpr_shrunk: f64,
    ctx_weight: f64,
    prior_runs_horse: f64,
    prior_runs_jockey: f64,
    prior_runs_trainer: f64,
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
            "{}/racecards/{}/{}/{}/racecard-report-{}.html",
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

    let html = build_html_report(
        &today,
        &in_path,
        &history_dir,
        &races,
        &horse_agg,
        &horse_ctx_agg,
        &jockey_agg,
        &trainer_agg,
    );

    if let Some(parent) = Path::new(&out_path).parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create_dir_all {}", parent.display()))?;
    }
    fs::write(&out_path, html).with_context(|| format!("write {out_path}"))?;
    eprintln!("today-first-race: wrote report {out_path}");

    if let Some(parent) = Path::new(&out_path).parent() {
        for (idx, (race, runners)) in races.iter().enumerate() {
            let odds = compute_odds_rows_explained(
                race,
                runners,
                &horse_agg,
                &horse_ctx_agg,
                &jockey_agg,
                &trainer_agg,
            );
            let details_html = build_race_details_html(&today, race, &odds);
            let details_path = parent.join(race_details_filename(idx, race));
            fs::write(&details_path, details_html)
                .with_context(|| format!("write {}", details_path.display()))?;
        }
        eprintln!("today-first-race: wrote race detail pages");
    }

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

fn build_html_report(
    day: &str,
    in_path: &str,
    history_dir: &str,
    races: &[(RaceKey, Vec<RunnerMini>)],
    horse_agg: &HashMap<String, Agg>,
    horse_ctx_agg: &HashMap<HorseContextKey, Agg>,
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
    out.push_str("<div><strong>Racecard parquet:</strong> ");
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

        let odds = compute_odds_rows(race, runners, horse_agg, horse_ctx_agg, jockey_agg, trainer_agg);
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

        let hhmm = time_hhmm(&race.time);
        out.push_str(&html_escape(&hhmm));
        out.push_str(" — ");
        out.push_str(&html_escape(&race.course));
        if !race.race_name.trim().is_empty() {
            out.push_str(" — ");
            out.push_str(&html_escape(&race.race_name));
        }
        out.push_str("</button>\n</h2>\n");

        out.push_str("<div class=\"px-3 pb-2\"><a class=\"small\" href=\"");
        out.push_str(&html_escape(&race_details_filename(idx, race)));
        out.push_str("\">How the score was calculated</a></div>\n");

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
    race: &RaceKey,
    runners: &[RunnerMini],
    horse_agg: &HashMap<String, Agg>,
    horse_ctx_agg: &HashMap<HorseContextKey, Agg>,
    jockey_agg: &HashMap<String, Agg>,
    trainer_agg: &HashMap<String, Agg>,
) -> Vec<PredRow> {
    compute_odds_rows_explained(race, runners, horse_agg, horse_ctx_agg, jockey_agg, trainer_agg)
        .into_iter()
        .map(|x| PredRow {
            horse: x.horse,
            score: x.score,
            prob: x.prob,
            fair_odds: x.fair_odds,
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
            h_count_overall: h_overall.count,
            h_count_ctx: h_ctx.count,
            h_avg_rpr_raw: h_avg_raw,
            h_avg_rpr_shrunk: h_avg,
            h_avg_points_raw: h_pts_raw,
            h_avg_points_shrunk: h_pts,
            j_count: j.count,
            j_avg_rpr_raw: j_avg_raw,
            j_avg_rpr_shrunk: j_avg,
            t_count: t.count,
            t_avg_rpr_raw: t_avg_raw,
            t_avg_rpr_shrunk: t_avg,
            ctx_weight,
            prior_runs_horse,
            prior_runs_jockey,
            prior_runs_trainer,
        });
    }

    softmax_preds(&mut preds, 30.0);
    preds.sort_by(|a, b| b.prob.partial_cmp(&a.prob).unwrap_or(std::cmp::Ordering::Equal));
    preds
}

fn build_race_details_html(day: &str, race: &RaceKey, odds: &[PredExplainRow]) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<link href=\"https://cdn.jsdelivr.net/npm/bootstrap@5.3.3/dist/css/bootstrap.min.css\" rel=\"stylesheet\" integrity=\"sha384-QWTKZyjpPEjISv5WaRU9OFeRpok6YctnYmDr5pNlyT2bRjXh0JMhjY6hW+ALEwIH\" crossorigin=\"anonymous\">\n");
    out.push_str("<title>");
    out.push_str(&html_escape(day));
    out.push_str(" — ");
    out.push_str(&html_escape(&race.course));
    out.push_str(" ");
    out.push_str(&html_escape(&time_hhmm(&race.time)));
    out.push_str("</title>\n</head>\n<body class=\"container py-4\">\n");

    out.push_str("<div class=\"mb-3\"><a href=\"racecard-report-");
    out.push_str(&html_escape(day));
    out.push_str(".html\">Back to report</a></div>\n");

    out.push_str("<h1 class=\"h4\">");
    out.push_str(&html_escape(&race.course));
    out.push_str(" — ");
    out.push_str(&html_escape(&time_hhmm(&race.time)));
    if !race.going.trim().is_empty() {
        out.push_str(" <span class=\"text-muted\">(");
        out.push_str(&html_escape(&race.going));
        out.push_str(")</span>");
    }
    out.push_str("</h1>\n");
    if !race.race_name.trim().is_empty() {
        out.push_str("<div class=\"text-muted mb-3\">");
        out.push_str(&html_escape(&race.race_name));
        out.push_str("</div>\n");
    }

    out.push_str("<div class=\"table-responsive\">\n<table class=\"table table-sm table-striped align-middle\">\n");
    out.push_str("<thead><tr>");
    out.push_str("<th>Horse</th>");
    out.push_str("<th class=\"text-end\">Score</th>");
    out.push_str("<th class=\"text-end\">Prob</th>");
    out.push_str("<th class=\"text-end\">Fair odds</th>");
    out.push_str("<th class=\"text-end\">Horse runs (overall)</th>");
    out.push_str("<th class=\"text-end\">Horse runs (course+going)</th>");
    out.push_str("<th class=\"text-end\">Horse avg RPR (raw)</th>");
    out.push_str("<th class=\"text-end\">Horse avg RPR (shrunk)</th>");
    out.push_str("<th class=\"text-end\">Horse avg points (raw)</th>");
    out.push_str("<th class=\"text-end\">Horse avg points (shrunk)</th>");
    out.push_str("<th class=\"text-end\">Jockey runs</th>");
    out.push_str("<th class=\"text-end\">Jockey avg RPR (raw)</th>");
    out.push_str("<th class=\"text-end\">Jockey avg RPR (shrunk)</th>");
    out.push_str("<th class=\"text-end\">Trainer runs</th>");
    out.push_str("<th class=\"text-end\">Trainer avg RPR (raw)</th>");
    out.push_str("<th class=\"text-end\">Trainer avg RPR (shrunk)</th>");
    out.push_str("</tr></thead>\n<tbody>\n");

    for r in odds {
        out.push_str("<tr><td>");
        out.push_str(&html_escape(&r.horse));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.4}", r.score));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.3}", r.prob));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.2}", r.fair_odds));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&r.h_count_overall.to_string());
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&r.h_count_ctx.to_string());
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.3}", r.h_avg_rpr_raw));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.3}", r.h_avg_rpr_shrunk));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.3}", r.h_avg_points_raw));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.3}", r.h_avg_points_shrunk));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&r.j_count.to_string());
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.3}", r.j_avg_rpr_raw));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.3}", r.j_avg_rpr_shrunk));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&r.t_count.to_string());
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.3}", r.t_avg_rpr_raw));
        out.push_str("</td><td class=\"text-end\">");
        out.push_str(&format!("{:.3}", r.t_avg_rpr_shrunk));
        out.push_str("</td></tr>\n");
    }

    out.push_str("</tbody></table></div>\n");

    if let Some(first) = odds.first() {
        out.push_str("<div class=\"small text-muted mt-3\">Context blend weight: ");
        out.push_str(&format!("{:.0}%", first.ctx_weight * 100.0));
        out.push_str(". Shrinkage priors (runs): horse=");
        out.push_str(&format!("{:.0}", first.prior_runs_horse));
        out.push_str(", jockey=");
        out.push_str(&format!("{:.0}", first.prior_runs_jockey));
        out.push_str(", trainer=");
        out.push_str(&format!("{:.0}", first.prior_runs_trainer));
        out.push_str(".</div>\n");
    }

    out.push_str("<script src=\"https://cdn.jsdelivr.net/npm/bootstrap@5.3.3/dist/js/bootstrap.bundle.min.js\" integrity=\"sha384-YvpcrYf0tY3lHB60NNkmXc5s9fDVZLESaAA55NDzOxhy9GkcIdslK1eN7N6jIeHz\" crossorigin=\"anonymous\"></script>\n");
    out.push_str("</body>\n</html>\n");
    out
}

fn race_details_filename(idx: usize, race: &RaceKey) -> String {
    let hhmm = time_hhmm(&race.time);
    let course = slugify(&race.course);
    format!("race-details-{:02}-{}-{}.html", idx + 1, hhmm.replace(':', ""), course)
}

fn slugify(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return "race".to_string();
    }
    let mut out = String::with_capacity(t.len());
    let mut last_dash = false;
    for c in t.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "race".to_string()
    } else {
        out
    }
}

fn shrink_feature(value: f64, n: f64, prior_n: f64) -> f64 {
    if !value.is_finite() || n <= 0.0 {
        return 0.0;
    }
    let w = n / (n + prior_n.max(1.0));
    w * value
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

impl HasScore for PredRow {
    fn score(&self) -> f64 {
        self.score
    }
    fn set_prob_and_odds(&mut self, prob: f64) {
        self.prob = prob;
        self.fair_odds = if prob > 0.0 { 1.0 / prob } else { f64::INFINITY };
    }
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

fn time_hhmm(s: &str) -> String {
    let t = s.trim();
    if let Some(idx) = t.find('T') {
        if let Some(hhmm) = t.get(idx + 1..idx + 6) {
            return hhmm.to_string();
        }
    }
    t.to_string()
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

            map.entry(key).or_default().push(RunnerMini {
                horse,
                jockey: jockey_arr[i].clone(),
                trainer: trainer_arr[i].clone(),
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
