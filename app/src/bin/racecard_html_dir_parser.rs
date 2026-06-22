use anyhow::Context;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (html_dir, out_path, athena_db, athena_racecard_table, athena_results_table) = parse_args();

    eprintln!("racecard-parser: html_dir={html_dir}");
    eprintln!("racecard-parser: out_path={out_path}");

    let mut html_paths: Vec<std::path::PathBuf> = std::fs::read_dir(&html_dir)
        .with_context(|| format!("read_dir {html_dir}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("html"))
        .collect();
    html_paths.sort();

    let mut lines = Vec::<String>::new();
    let mut races: Vec<(RaceMeta, Vec<RunnerRec>)> = Vec::new();
    let mut horses: HashSet<String> = HashSet::new();
    let mut jockeys: HashSet<String> = HashSet::new();
    let mut trainers: HashSet<String> = HashSet::new();
    let mut failed = 0usize;

    for path in html_paths {
        let path_str = path.display().to_string();
        let html = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                failed += 1;
                eprintln!("racecard-parser: read failed path={path_str} err={e}");
                continue;
            }
        };

        match parse_one_racecard_html(&html) {
            Ok((meta, runners)) => {
                if !out_path.ends_with(".json") {
                    for r in build_lines(meta.clone(), runners.clone()) {
                        lines.push(r);
                    }
                } else {
                    races.push((meta.clone(), runners.clone()));
                }

                for rr in runners {
                    if let Some(h) = rr.horse.as_deref() {
                        let h = h.trim();
                        if !h.is_empty() {
                            horses.insert(h.to_string());
                        }
                    }
                    if let Some(j) = rr.jockey.as_deref() {
                        let j = j.trim();
                        if !j.is_empty() {
                            jockeys.insert(j.to_string());
                        }
                    }
                    if let Some(t) = rr.trainer.as_deref() {
                        let t = t.trim();
                        if !t.is_empty() {
                            trainers.insert(t.to_string());
                        }
                    }
                }
            }
            Err(e) => {
                failed += 1;
                eprintln!("racecard-parser: parse failed path={path_str} err={e:#}");
            }
        }
    }

    eprintln!(
        "racecard-parser: writing {} runner records (failed {})",
        lines.len(),
        failed
    );

    if out_path.ends_with(".json") {
        let mut races_json = Vec::<Value>::new();
        for (meta, runners) in races {
            races_json.push(race_to_json_value(&meta, &runners));
        }
        let s = serde_json::to_string_pretty(&Value::Array(races_json))
            .with_context(|| "serialize races json")?;
        std::fs::write(&out_path, s).with_context(|| format!("write {out_path}"))?;
    } else {
        std::fs::write(&out_path, lines.join("\n")).with_context(|| format!("write {out_path}"))?;
    }

    if let Some((year, month, day)) =
        infer_ymd_from_path(&html_dir).or_else(|| infer_ymd_from_path(&out_path))
    {
        let sql = build_athena_history_sql_for_day(
            &athena_db,
            &athena_racecard_table,
            &athena_results_table,
            &year,
            &month,
            &day,
            &horses,
            &jockeys,
            &trainers,
        );
        let out_dir = std::path::Path::new(&out_path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("/data")
            .trim_end_matches('/');
        let sql_path = format!(
            "{}/racingpost-racecards-{}-{}-{}-history.sql",
            out_dir, year, month, day
        );
        std::fs::write(&sql_path, sql).with_context(|| format!("write {sql_path}"))?;
        eprintln!("racecard-parser: wrote athena sql to {sql_path}");
    }
    Ok(())
}

fn parse_args() -> (String, String, String, String, String) {
    let mut html_dir: Option<String> = None;
    let mut out_path: Option<String> = None;
    let mut athena_db: Option<String> = None;
    let mut athena_racecard_table: Option<String> = None;
    let mut athena_results_table: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--html-dir" | "-d" => html_dir = args.next(),
            "--out" | "-o" => out_path = args.next(),
            "--athena-db" => athena_db = args.next(),
            "--athena-racecard-table" => athena_racecard_table = args.next(),
            "--athena-results-table" => athena_results_table = args.next(),
            _ => {}
        }
    }

    (
        html_dir.unwrap_or_else(|| "/data".to_string()),
        out_path.unwrap_or_else(|| "/data/racecards-runners.jsonl".to_string()),
        athena_db.unwrap_or_else(|| "racingpost".to_string()),
        athena_racecard_table.unwrap_or_else(|| "racecard_runners".to_string()),
        athena_results_table.unwrap_or_else(|| "processed_full_results_runners".to_string()),
    )
}

fn infer_ymd_from_path(path: &str) -> Option<(String, String, String)> {
    let p = path.replace('\\', "/");
    let parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
    for i in 0..parts.len().saturating_sub(2) {
        let y = parts[i];
        let m = parts[i + 1];
        let d = parts[i + 2];
        if y.len() == 4
            && m.len() == 2
            && d.len() == 2
            && y.chars().all(|c| c.is_ascii_digit())
            && m.chars().all(|c| c.is_ascii_digit())
            && d.chars().all(|c| c.is_ascii_digit())
        {
            return Some((y.to_string(), m.to_string(), d.to_string()));
        }
    }
    None
}

fn build_athena_history_sql_for_day(
    db: &str,
    racecard_table: &str,
    results_table: &str,
    year: &str,
    month: &str,
    day: &str,
    horses: &HashSet<String>,
    jockeys: &HashSet<String>,
    trainers: &HashSet<String>,
) -> String {
    let _ = (racecard_table, year, month, day);

    let horse_in = build_in_list_sql(horses);
    let jockey_in = build_in_list_sql(jockeys);
    let trainer_in = build_in_list_sql(trainers);

    let mut preds: Vec<String> = Vec::new();
    if let Some(v) = horse_in {
        preds.push(format!("r.horse IN ({})", v));
    }
    if let Some(v) = jockey_in {
        preds.push(format!("r.jockey IN ({})", v));
    }
    if let Some(v) = trainer_in {
        preds.push(format!("r.trainer IN ({})", v));
    }

    let where_clause = if preds.is_empty() {
        "1 = 0".to_string()
    } else {
        preds.join("\n   OR ")
    };

    format!(
        "SELECT\n  r.*\nFROM {db}.{results_table} r\nWHERE {where_clause}\nORDER BY year, month, day, course, title, position;\n"
    )
}

fn build_in_list_sql(values: &HashSet<String>) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    let mut items: Vec<&String> = values.iter().collect();
    items.sort();
    Some(
        items
            .into_iter()
            .map(|s| sql_string_literal(s))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

fn sql_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push('\'');
            out.push('\'');
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

fn parse_one_racecard_html(html: &str) -> anyhow::Result<(RaceMeta, Vec<RunnerRec>)> {
    // Prefer structured Next.js data if available, but fall back to HTML parsing.
    if let Some(next_data) = extract_next_data_json(html) {
        if let Ok(v) = serde_json::from_str::<Value>(&next_data) {
            let meta = extract_race_meta(&v);
            let mut runners = extract_runners(&v);
            if !runners.is_empty() {
                // Next.js runner objects sometimes omit weight, but it is present in the rendered HTML.
                // Backfill missing weights by matching on horse name.
                if runners.iter().any(|r| r.weight_st.as_deref().unwrap_or("").is_empty()) {
                    let html_runners = extract_runners_from_html(html);
                    let mut by_horse: HashMap<String, (Option<String>, Option<String>, Option<String>)> =
                        HashMap::new();
                    for hr in html_runners {
                        if let Some(h) = hr.horse.as_deref() {
                            if !h.trim().is_empty() {
                                by_horse.insert(
                                    h.trim().to_lowercase(),
                                    (hr.weight, hr.weight_st, hr.weight_lb),
                                );
                            }
                        }
                    }
                    for r in runners.iter_mut() {
                        let missing = r.weight_st.as_deref().unwrap_or("").is_empty()
                            || r.weight_lb.as_deref().unwrap_or("").is_empty();
                        if !missing {
                            continue;
                        }
                        let Some(h) = r.horse.as_deref() else { continue };
                        let key = h.trim().to_lowercase();
                        if let Some((w, st, lb)) = by_horse.get(&key) {
                            if r.weight.is_none() {
                                r.weight = w.clone();
                            }
                            if r.weight_st.is_none() {
                                r.weight_st = st.clone();
                            }
                            if r.weight_lb.is_none() {
                                r.weight_lb = lb.clone();
                            }
                        }
                    }
                }

                return Ok((meta, runners));
            }
        }
    }

    let canonical = extract_canonical_racecard_url(html);
    let mut meta = extract_race_meta_from_html(html);
    if meta.course.is_none() {
        if let Some(c) = canonical.as_deref().and_then(course_from_canonical_url) {
            meta.course = Some(c);
        }
    }
    if meta.race_name.is_none() {
        meta.race_name = extract_race_name_from_title(html);
    }

    let runners = extract_runners_from_html(html);
    if runners.is_empty() {
        anyhow::bail!("no runners found")
    }

    Ok((meta, runners))
}

fn race_to_json_value(meta: &RaceMeta, runners: &[RunnerRec]) -> Value {
    let mut obj = Map::<String, Value>::new();
    obj.insert(
        "course".to_string(),
        Value::String(meta.course.clone().unwrap_or_default()),
    );
    obj.insert(
        "time".to_string(),
        Value::String(meta.time.clone().unwrap_or_default()),
    );
    obj.insert(
        "race_name".to_string(),
        Value::String(meta.race_name.clone().unwrap_or_default()),
    );
    obj.insert(
        "going".to_string(),
        Value::String(meta.going.clone().unwrap_or_default()),
    );

    let mut runners_out = Vec::<Value>::with_capacity(runners.len());
    for r in runners {
        let mut ro = Map::<String, Value>::new();
        ro.insert(
            "horse".to_string(),
            Value::String(r.horse.clone().unwrap_or_default()),
        );
        ro.insert(
            "jockey".to_string(),
            Value::String(r.jockey.clone().unwrap_or_default()),
        );
        ro.insert(
            "trainer".to_string(),
            Value::String(r.trainer.clone().unwrap_or_default()),
        );
        ro.insert(
            "age".to_string(),
            Value::String(r.age.clone().unwrap_or_default()),
        );
        ro.insert(
            "weight".to_string(),
            Value::String(r.weight.clone().unwrap_or_default()),
        );
        ro.insert(
            "weight_st".to_string(),
            Value::String(r.weight_st.clone().unwrap_or_default()),
        );
        ro.insert(
            "weight_lb".to_string(),
            Value::String(r.weight_lb.clone().unwrap_or_default()),
        );
        runners_out.push(Value::Object(ro));
    }
    obj.insert("runners".to_string(), Value::Array(runners_out));

    Value::Object(obj)
}

fn build_lines(meta: RaceMeta, runners: Vec<RunnerRec>) -> Vec<String> {
    let mut out = Vec::with_capacity(runners.len());
    for r in runners {
        out.push(format!(
            "{{\"course\":\"{}\",\"time\":\"{}\",\"race_name\":\"{}\",\"going\":\"{}\",\"horse\":\"{}\",\"jockey\":\"{}\",\"trainer\":\"{}\",\"age\":\"{}\",\"weight\":\"{}\",\"weight_st\":\"{}\",\"weight_lb\":\"{}\"}}",
            json_escape(meta.course.as_deref().unwrap_or("")),
            json_escape(meta.time.as_deref().unwrap_or("")),
            json_escape(meta.race_name.as_deref().unwrap_or("")),
            json_escape(meta.going.as_deref().unwrap_or("")),
            json_escape(r.horse.as_deref().unwrap_or("")),
            json_escape(r.jockey.as_deref().unwrap_or("")),
            json_escape(r.trainer.as_deref().unwrap_or("")),
            json_escape(r.age.as_deref().unwrap_or("")),
            json_escape(r.weight.as_deref().unwrap_or("")),
            json_escape(r.weight_st.as_deref().unwrap_or("")),
            json_escape(r.weight_lb.as_deref().unwrap_or(""))
        ));
    }
    out
}

fn extract_next_data_json(html: &str) -> Option<String> {
    let marker = "<script id=\"__NEXT_DATA__\" type=\"application/json\">";
    let start = html.find(marker)? + marker.len();
    let end = html[start..].find("</script>").map(|i| start + i)?;
    Some(html[start..end].to_string())
}

fn extract_canonical_racecard_url(html: &str) -> Option<String> {
    let marker = "<link rel=\"canonical\" href=\"";
    let start = html.find(marker)? + marker.len();
    let end = html[start..].find('"').map(|i| start + i)?;
    Some(html[start..end].to_string())
}

fn course_from_canonical_url(url: &str) -> Option<String> {
    // https://www.racingpost.com/racecards/<course_no>/<course_slug>/<date>/<race_id>/
    let u = url.trim_end_matches('/');
    let parts: Vec<&str> = u.split('/').filter(|p| !p.is_empty()).collect();
    let idx = parts.iter().position(|p| *p == "racecards")?;
    parts.get(idx + 2).map(|s| s.to_string())
}

fn extract_race_name_from_title(html: &str) -> Option<String> {
    let title = find_between(html, "<title", "</title>")?;
    let text = extract_text_after(&title, ">")?;
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    Some(t.replace(" Racecard", "").trim().to_string())
}

fn extract_race_meta_from_html(html: &str) -> RaceMeta {
    let mut meta = RaceMeta::default();

    // Meta description often contains: "... at 15:40 Ascot, including ..."
    if let Some(desc) = extract_meta_content(html, "name=\"description\"") {
        if meta.time.is_none() {
            meta.time = find_first_time_hh_mm(&desc);
        }
        if meta.course.is_none() {
            if let Some(t) = meta.time.as_deref() {
                if let Some(c) = course_after_time(&desc, t) {
                    meta.course = Some(c);
                }
            }
        }
    }

    meta
}

fn extract_meta_content(html: &str, needle: &str) -> Option<String> {
    let idx = html.find(needle)?;
    let slice = &html[idx..html.len().min(idx + 2000)];
    let m = "content=\"";
    let start = slice.find(m)? + m.len();
    let end = slice[start..].find('"').map(|i| start + i)?;
    Some(slice[start..end].to_string())
}

fn find_first_time_hh_mm(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    for i in 0..bytes.len().saturating_sub(4) {
        if i + 4 >= bytes.len() {
            break;
        }
        if bytes[i].is_ascii_digit()
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2] == b':'
            && bytes[i + 3].is_ascii_digit()
            && bytes[i + 4].is_ascii_digit()
        {
            return Some(s[i..i + 5].to_string());
        }
    }
    None
}

fn course_after_time(desc: &str, time: &str) -> Option<String> {
    let idx = desc.find(time)?;
    let after = desc[idx + time.len()..].trim_start();
    let after = after.strip_prefix("at")?.trim_start();
    let after = after.strip_prefix(time)?.trim_start();
    let mut out = String::new();
    for c in after.chars() {
        if c == ',' || c == '.' {
            break;
        }
        if out.len() > 64 {
            break;
        }
        out.push(c);
    }
    let out = out.trim();
    if out.is_empty() {
        None
    } else {
        Some(out.to_string())
    }
}

fn extract_runners_from_html(html: &str) -> Vec<RunnerRec> {
    let mut out = Vec::<RunnerRec>::new();
    let marker = "data-testid=\"Link__Horse\"";
    let mut start = 0usize;

    while let Some(rel) = html[start..].find(marker) {
        let idx = start + rel;
        let next = html[idx + marker.len()..]
            .find(marker)
            .map(|r| idx + marker.len() + r)
            .unwrap_or(html.len());
        let block = &html[idx..next];

        let horse = extract_text_after(block, marker)
            .and_then(|s| extract_text_after(&s, ">"))
            .map(|s| s.trim().to_string());

        let jockey = extract_text_after(block, "data-testid=\"Link__Jockey\"")
            .and_then(|s| extract_text_after(&s, ">"))
            .map(|s| s.trim().to_string());

        let trainer = extract_text_after(block, "data-testid=\"Link__Trainer\"")
            .and_then(|s| extract_text_after(&s, ">"))
            .map(|s| s.trim().to_string());

        let age = extract_first_testid_text(block, &["Text__Age", "Text__HorseAge"])
            .or_else(|| find_age_yo(block));
        let weight_text = extract_first_testid_text(
            block,
            &["Text__Weight", "Text__Wgt", "Text__HorseWeight", "Text__WeightValue"],
        )
        .or_else(|| find_weight_st_lb(block))
        .or_else(|| find_weight_dash(block));

        let (weight, weight_st, weight_lb) = match weight_text.as_deref() {
            Some(t) => {
                let (st, lb) = parse_weight_to_st_lb(t);
                (Some(t.to_string()), st, lb)
            }
            None => (None, None, None),
        };

        if horse.as_deref().unwrap_or("").is_empty() {
            start = next;
            continue;
        }

        out.push(RunnerRec {
            horse,
            jockey,
            trainer,
            age,
            weight,
            weight_st,
            weight_lb,
        });

        start = next;
    }

    out
}

fn extract_first_testid_text(s: &str, testids: &[&str]) -> Option<String> {
    for t in testids {
        let needle = format!("data-testid=\"{}\"", t);
        if let Some(v) = extract_text_after(s, &needle) {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn find_age_yo(s: &str) -> Option<String> {
    // Look for patterns like '5yo' or '10yo'.
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }

        let start = i;
        i += 1;
        if i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }

        if i + 1 < bytes.len() && bytes[i] == b'y' && bytes[i + 1] == b'o' {
            return Some(s[start..i].to_string());
        }
    }
    None
}

fn find_weight_st_lb(s: &str) -> Option<String> {
    // Look for patterns like '9</span>st <span...>5</span>lb'.
    // Some pages render an *empty* weight placeholder elsewhere in the same runner block
    // ('</span>st </span>lb'), so we must scan for the first valid occurrence.
    let mut search_from = 0usize;
    while let Some(rel_st_idx) = s[search_from..].find("st") {
        let st_idx = search_from + rel_st_idx;
        let head = &s[..st_idx];
        let st_num = head
            .chars()
            .rev()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();

        let after = &s[st_idx..];
        let Some(lb_rel_idx) = after.find("lb") else {
            return None;
        };
        let lb_head = &after[..lb_rel_idx];
        let lb_num = lb_head
            .chars()
            .rev()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();

        if !st_num.is_empty() && !lb_num.is_empty() {
            return Some(format!("{}st {}lb", st_num, lb_num));
        }

        search_from = st_idx + 2;
    }

    None
}

fn find_weight_dash(s: &str) -> Option<String> {
    // Alternative format sometimes used: '9-05' meaning 9st 5lb.
    let bytes = s.as_bytes();
    for i in 0..bytes.len().saturating_sub(3) {
        if !bytes[i].is_ascii_digit() {
            continue;
        }
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'-' {
            continue;
        }
        let st = &s[i..j];
        let mut k = j + 1;
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
        if k == j + 1 {
            continue;
        }
        let lb = &s[j + 1..k];

        // keep it reasonably bounded
        if st.len() <= 2 && lb.len() <= 2 {
            let st = st.trim_start_matches('0');
            let st = if st.is_empty() { "0" } else { st };
            let lb = lb.trim_start_matches('0');
            let lb = if lb.is_empty() { "0" } else { lb };
            return Some(format!("{}st {}lb", st, lb));
        }
    }
    None
}

fn extract_text_after(haystack: &str, needle: &str) -> Option<String> {
    let idx = haystack.find(needle)? + needle.len();
    let rest = &haystack[idx..];
    let start = rest.find('>')? + 1;
    let rest2 = &rest[start..];
    let end = rest2.find('<')?;
    Some(rest2[..end].to_string())
}

fn find_between(haystack: &str, start: &str, end: &str) -> Option<String> {
    let s = haystack.find(start)?;
    let rest = &haystack[s..];
    let e = rest.find(end)?;
    Some(rest[..e].to_string())
}

#[derive(Default, Clone)]
struct RaceMeta {
    course: Option<String>,
    time: Option<String>,
    race_name: Option<String>,
    going: Option<String>,
}

#[derive(Default, Clone)]
struct RunnerRec {
    horse: Option<String>,
    jockey: Option<String>,
    trainer: Option<String>,
    age: Option<String>,
    weight: Option<String>,
    weight_st: Option<String>,
    weight_lb: Option<String>,
}

fn extract_race_meta(v: &Value) -> RaceMeta {
    // Heuristic: grab first occurrences of common keys.
    let mut m = RaceMeta::default();
    let mut found: HashMap<&'static str, bool> = HashMap::new();

    walk(v, &mut |obj| {
        if m.course.is_none() {
            if let Some(s) = first_string(obj, &["courseName", "course", "meetingName"]) {
                m.course = Some(s);
                found.insert("course", true);
            }
        }
        if m.time.is_none() {
            if let Some(s) = first_string(obj, &["raceTime", "time", "offTime"]) {
                m.time = Some(s);
                found.insert("time", true);
            }
        }
        if m.race_name.is_none() {
            if let Some(s) = first_string(obj, &["raceTitle", "raceName", "name", "title"]) {
                // avoid huge generic titles (like page title) by requiring something race-ish
                if s.len() <= 200 {
                    m.race_name = Some(s);
                    found.insert("race_name", true);
                }
            }
        }
        if m.going.is_none() {
            if let Some(s) = first_string(obj, &["going", "goingDescription", "surface"]) {
                m.going = Some(s);
                found.insert("going", true);
            }
        }

        // Stop early if we found everything.
        found.len() >= 4
    });

    m
}

fn extract_runners(v: &Value) -> Vec<RunnerRec> {
    let mut best: Vec<RunnerRec> = Vec::new();

    walk_arrays(v, &mut |arr| {
        if arr.is_empty() {
            return false;
        }
        // candidate: array of objects that look like runner entries
        let mut candidates = Vec::<RunnerRec>::new();
        for el in arr {
            let Some(obj) = el.as_object() else { continue };

            let horse = first_string(obj, &["horseName", "horse", "name"]);
            let jockey = first_string(obj, &["jockeyName", "jockey"]);
            let trainer = first_string(obj, &["trainerName", "trainer"]);
            let age = first_string_or_number(obj, &["age", "horseAge", "ageYears", "ageYrs"]);
            let (weight, weight_st, weight_lb) = extract_weight_any(obj);

            let looks_like_runner = horse.is_some() && (jockey.is_some() || trainer.is_some());
            if looks_like_runner {
                candidates.push(RunnerRec {
                    horse,
                    jockey,
                    trainer,
                    age,
                    weight,
                    weight_st,
                    weight_lb,
                });
            }
        }

        if candidates.len() > best.len() {
            best = candidates;
        }

        false
    });

    best
}

fn first_string(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(v) = obj.get(*k) {
            if let Some(s) = v.as_str() {
                let s = s.trim();
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

fn first_string_or_number(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for k in keys {
        let Some(v) = obj.get(*k) else { continue };
        match v {
            Value::String(s) => {
                let s = s.trim();
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
            Value::Number(n) => return Some(n.to_string()),
            _ => {}
        }
    }
    None
}

fn extract_weight_any(obj: &serde_json::Map<String, Value>) -> (Option<String>, Option<String>, Option<String>) {
    extract_weight_any_impl(obj).unwrap_or((None, None, None))
}

fn extract_weight_any_impl(
    obj: &serde_json::Map<String, Value>,
) -> Option<(Option<String>, Option<String>, Option<String>)> {
    // Common fields in Next.js runner objects.
    if let (Some(st), Some(lb)) = (
        first_string_or_number(obj, &["formattedWeightStones", "weightStones", "stones"]),
        first_string_or_number(obj, &["formattedWeightPounds", "weightPounds", "pounds"]),
    ) {
        return Some((Some(format!("{}st {}lb", st, lb)), Some(st), Some(lb)));
    }

    let keys = [
        "weight",
        "weightText",
        "weightStLb",
        "weightDisplay",
        "weightValue",
        "wgt",
        "wgtText",
        "horseWeight",
    ];

    for k in keys {
        let Some(v) = obj.get(k) else { continue };
        match v {
            Value::String(s) => {
                let s = s.trim();
                if !s.is_empty() {
                    let (st, lb) = parse_weight_to_st_lb(s);
                    return Some((Some(s.to_string()), st, lb));
                }
            }
            Value::Number(n) => return Some((Some(n.to_string()), None, None)),
            Value::Object(o) => {
                if let (Some(st), Some(lb)) = (
                    first_string_or_number(o, &["st", "stones", "stone"]),
                    first_string_or_number(o, &["lb", "pounds", "lbs"]),
                ) {
                    return Some((Some(format!("{}st {}lb", st, lb)), Some(st), Some(lb)));
                }
            }
            _ => {}
        }
    }

    for sub in ["weight", "wgt"] {
        if let Some(Value::Object(o)) = obj.get(sub) {
            if let (Some(st), Some(lb)) = (
                first_string_or_number(o, &["st", "stones", "stone"]),
                first_string_or_number(o, &["lb", "pounds", "lbs"]),
            ) {
                return Some((Some(format!("{}st {}lb", st, lb)), Some(st), Some(lb)));
            }
        }
    }

    None
}

fn parse_weight_to_st_lb(s: &str) -> (Option<String>, Option<String>) {
    if let Some((st, lb)) = parse_weight_st_lb_from_st_lb(s) {
        return (Some(st), Some(lb));
    }
    if let Some((st, lb)) = parse_weight_st_lb_from_dash(s) {
        return (Some(st), Some(lb));
    }
    (None, None)
}

fn parse_weight_st_lb_from_st_lb(s: &str) -> Option<(String, String)> {
    let st_idx = s.find("st")?;
    let head = &s[..st_idx];
    let st_num = head
        .chars()
        .rev()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();

    let after = &s[st_idx..];
    let lb_idx = after.find("lb")?;
    let lb_head = &after[..lb_idx];
    let lb_num = lb_head
        .chars()
        .rev()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();

    if st_num.is_empty() || lb_num.is_empty() {
        return None;
    }
    Some((st_num, lb_num))
}

fn parse_weight_st_lb_from_dash(s: &str) -> Option<(String, String)> {
    let bytes = s.as_bytes();
    for i in 0..bytes.len().saturating_sub(3) {
        if !bytes[i].is_ascii_digit() {
            continue;
        }
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'-' {
            continue;
        }
        let st = &s[i..j];
        let mut k = j + 1;
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
        if k == j + 1 {
            continue;
        }
        let lb = &s[j + 1..k];
        if st.len() <= 2 && lb.len() <= 2 {
            let st = st.trim_start_matches('0');
            let st = if st.is_empty() { "0" } else { st };
            let lb = lb.trim_start_matches('0');
            let lb = if lb.is_empty() { "0" } else { lb };
            return Some((st.to_string(), lb.to_string()));
        }
    }
    None
}

fn walk(v: &Value, f: &mut impl FnMut(&serde_json::Map<String, Value>) -> bool) {
    match v {
        Value::Object(o) => {
            if f(o) {
                return;
            }
            for (_k, vv) in o {
                walk(vv, f);
            }
        }
        Value::Array(a) => {
            for vv in a {
                walk(vv, f);
            }
        }
        _ => {}
    }
}

fn walk_arrays(v: &Value, f: &mut impl FnMut(&[Value]) -> bool) {
    match v {
        Value::Array(a) => {
            if f(a) {
                return;
            }
            for vv in a {
                walk_arrays(vv, f);
            }
        }
        Value::Object(o) => {
            for (_k, vv) in o {
                walk_arrays(vv, f);
            }
        }
        _ => {}
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}
