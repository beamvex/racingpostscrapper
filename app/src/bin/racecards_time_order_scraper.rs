use anyhow::Context;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Europe::London;
use std::collections::{HashMap, HashSet};
use tokio::time::{timeout, Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("racecards: starting");
    std::fs::create_dir_all("/data").context("create /data")?;

    let mut time_order_only = false;
    let mut results_date: Option<String> = None;
    for arg in std::env::args().skip(1) {
        if arg == "--time-order-only" {
            time_order_only = true;
        } else {
            results_date = Some(arg);
        }
    }

    let results_date = results_date
        .or_else(|| std::env::var("RESULTS_DATE").ok())
        .unwrap_or_else(racingpost_scraper::utils::current_utc_date_yyyy_mm_dd);

    let target_url = "https://www.racingpost.com/racecards/time-order/";
    eprintln!("racecards: target_url={target_url}");

    let out_base_dir = racingpost_scraper::scrape::out_base_dir_for_date(&results_date)?;
    eprintln!("racecards: out_base_dir={out_base_dir}");

    let (mut browser, handler_task) = racingpost_scraper::scrape::connect_browser_and_spawn_handler().await?;

    let out_path = format!("{out_base_dir}racingpost-racecards-{results_date}.html");
    let html = racingpost_scraper::scrape::fetch_and_save_time_order_html(&mut browser, target_url, &out_path).await?;

    let urls_path = format!(
        "{out_base_dir}racingpost-racecards-{results_date}-time-order-racecard-urls.txt"
    );
    let urls = extract_racecard_urls(&html);
    eprintln!("racecards: writing {} urls to {}", urls.len(), urls_path);
    std::fs::write(&urls_path, urls.join("\n")).with_context(|| format!("write {urls_path}"))?;

    // Filter out races that have already started when scraping today's card
    let now_utc = Utc::now();
    let today_london = now_utc.with_timezone(&London).format("%Y-%m-%d").to_string();
    let urls_to_download = if results_date == today_london {
        let race_time_map = build_race_time_map(&html);
        eprintln!("racecards: race time map has {} entries", race_time_map.len());
        let filtered: Vec<String> = urls
            .iter()
            .filter(|url| {
                let race_id = parse_racecard_detail_url(url)
                    .map(|i| i.race_id)
                    .unwrap_or_default();
                if let Some(&race_time) = race_time_map.get(&race_id) {
                    if race_time < now_utc {
                        eprintln!(
                            "racecards: skipping past race id={} time={} London",
                            race_id,
                            race_time.with_timezone(&London).format("%H:%M"),
                        );
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();
        eprintln!(
            "racecards: {} of {} urls are future races",
            filtered.len(),
            urls.len(),
        );
        filtered
    } else {
        eprintln!("racecards: historical date {}, keeping all {} urls", results_date, urls.len());
        urls.clone()
    };

    if time_order_only {
        eprintln!("racecards: --time-order-only, skipping racecard downloads");
    } else {
        let cards_dir = format!("{out_base_dir}racingpost-racecards-{results_date}-racecards-html");
        std::fs::create_dir_all(&cards_dir).with_context(|| format!("create {cards_dir}"))?;
        eprintln!("racecards: downloading {} racecards into {}", urls_to_download.len(), cards_dir);
        let (downloaded, failed) = download_racecards_html(&mut browser, &urls_to_download, &cards_dir).await?;
        eprintln!("racecards: downloaded {} (failed {})", downloaded, failed);
    }

    eprintln!("racecards: closing browser");
    browser.close().await.ok();
    handler_task.abort();

    eprintln!("racecards: done");
    Ok(())
}

async fn download_racecards_html(
    browser: &mut chromiumoxide::browser::Browser,
    urls: &[String],
    out_dir: &str,
) -> anyhow::Result<(usize, usize)> {
    let mut downloaded = 0usize;
    let mut failed = 0usize;

    for (i, url) in urls.iter().enumerate() {
        let seq = i + 1;
        let info = parse_racecard_detail_url(url).unwrap_or_else(|| RacecardInfo {
            course_no: "unknown".to_string(),
            course_slug: "unknown".to_string(),
            race_date: "unknown".to_string(),
            race_id: format!("{seq}"),
        });

        let filename = format!(
            "{}-{}-{}-{}.html",
            info.race_date, info.course_no, info.course_slug, info.race_id
        );
        let path = format!("{}/{}", out_dir.trim_end_matches('/'), filename);

        let mut ok = false;
        for attempt in 1..=3 {
            eprintln!(
                "racecards: seq={} fetching racecard (attempt {}/3) {}",
                seq, attempt, url
            );
            let page = match timeout(Duration::from_secs(30), browser.new_page(url)).await {
                Ok(Ok(p)) => p,
                Ok(Err(e)) => {
                    eprintln!("racecards: seq={} open page failed err={}", seq, e);
                    continue;
                }
                Err(_) => {
                    eprintln!("racecards: seq={} open page timeout", seq);
                    continue;
                }
            };

            tokio::time::sleep(Duration::from_secs(3)).await;
            let html = match timeout(Duration::from_secs(30), page.content()).await {
                Ok(Ok(h)) => h,
                Ok(Err(e)) => {
                    eprintln!("racecards: seq={} fetch html failed err={}", seq, e);
                    page.close().await.ok();
                    continue;
                }
                Err(_) => {
                    eprintln!("racecards: seq={} fetch html timeout", seq);
                    page.close().await.ok();
                    continue;
                }
            };

            if let Err(e) = std::fs::write(&path, &html).with_context(|| format!("write {path}"))
            {
                eprintln!("racecards: seq={} write failed err={}", seq, e);
                page.close().await.ok();
                continue;
            }

            page.close().await.ok();

            ok = true;
            break;
        }

        if ok {
            downloaded += 1;
        } else {
            failed += 1;
        }
    }

    Ok((downloaded, failed))
}

#[derive(Debug, Clone)]
struct RacecardInfo {
    course_no: String,
    course_slug: String,
    race_date: String,
    race_id: String,
}

fn parse_racecard_detail_url(url: &str) -> Option<RacecardInfo> {
    let path = url
        .strip_prefix("https://www.racingpost.com")
        .unwrap_or(url);
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() != 5 {
        return None;
    }
    if parts[0] != "racecards" {
        return None;
    }
    Some(RacecardInfo {
        course_no: parts[1].to_string(),
        course_slug: parts[2].to_string(),
        race_date: parts[3].to_string(),
        race_id: parts[4].to_string(),
    })
}

fn parse_race_time(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // ISO 8601 with offset or Z
    let normalised = if s.ends_with('Z') {
        format!("{}+00:00", &s[..s.len() - 1])
    } else {
        s.to_string()
    };
    if let Ok(dt) = DateTime::parse_from_rfc3339(&normalised) {
        return Some(dt.with_timezone(&Utc));
    }
    // Naive datetime — treat as Europe/London
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            if let Some(dt) = London.from_local_datetime(&ndt).earliest() {
                return Some(dt.with_timezone(&Utc));
            }
        }
    }
    None
}

fn walk_for_times(v: &serde_json::Value, map: &mut HashMap<String, DateTime<Utc>>, depth: u32) {
    if depth > 30 {
        return;
    }
    if let serde_json::Value::Object(obj) = v {
        let race_id = obj
            .get("raceId")
            .or_else(|| obj.get("racecard"))
            .and_then(|v| match v {
                serde_json::Value::Number(n) => Some(n.to_string()),
                serde_json::Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()));
        let time_str = obj
            .get("raceTime")
            .or_else(|| obj.get("offTime"))
            .or_else(|| obj.get("startTime"))
            .and_then(|v| v.as_str());
        if let (Some(id), Some(ts)) = (race_id, time_str) {
            if let Some(dt) = parse_race_time(ts) {
                map.entry(id).or_insert(dt);
            }
        }
        for child in obj.values() {
            walk_for_times(child, map, depth + 1);
        }
    } else if let serde_json::Value::Array(arr) = v {
        for item in arr {
            walk_for_times(item, map, depth + 1);
        }
    }
}

fn build_race_time_map(html: &str) -> HashMap<String, DateTime<Utc>> {
    let mut map = HashMap::new();
    let marker = r#"<script id="__NEXT_DATA__" type="application/json">"#;
    let start = match html.find(marker) {
        Some(i) => i + marker.len(),
        None => return map,
    };
    let end = match html[start..].find("</script>") {
        Some(i) => start + i,
        None => return map,
    };
    match serde_json::from_str::<serde_json::Value>(&html[start..end]) {
        Ok(json) => walk_for_times(&json, &mut map, 0),
        Err(e) => eprintln!("racecards: failed to parse __NEXT_DATA__: {}", e),
    }
    map
}

fn extract_racecard_urls(html: &str) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut out = Vec::<String>::new();
    let mut start = 0usize;

    while let Some(rel) = html[start..].find("/racecards/") {
        let idx = start + rel;
        let end = html[idx..]
            .find(|c: char| c == '"' || c == '\'' || c.is_whitespace() || c == '<' || c == '>')
            .map(|r| idx + r)
            .unwrap_or_else(|| html.len());

        let raw = &html[idx..end];
        if let Some(url) = normalize_racecards_url(raw).and_then(filter_racecard_detail_url) {
            if seen.insert(url.clone()) {
                out.push(url);
            }
        }

        start = end;
    }

    out.sort();
    out
}

fn normalize_racecards_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let no_q = trimmed
        .split('#')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("");

    let no_q = no_q.trim_end_matches('/');

    if no_q.is_empty() {
        return None;
    }

    if no_q.starts_with("http://") || no_q.starts_with("https://") {
        Some(no_q.to_string())
    } else {
        Some(format!("https://www.racingpost.com{}", no_q))
    }
}

fn filter_racecard_detail_url(url: String) -> Option<String> {
    let path = url
        .strip_prefix("https://www.racingpost.com")
        .unwrap_or(&url);

    if is_racecard_detail_path(path) {
        Some(url)
    } else {
        None
    }
}

fn is_racecard_detail_path(path: &str) -> bool {
    // Expected: /racecards/<course_no>/<course_slug>/<yyyy-mm-dd>/<race_id>
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() != 5 {
        return false;
    }
    if parts[0] != "racecards" {
        return false;
    }
    if !parts[1].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if parts[2].is_empty() {
        return false;
    }
    let date = parts[3];
    if date.len() != 10 {
        return false;
    }
    let bytes = date.as_bytes();
    let is_date = bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[8..10].iter().all(|b| b.is_ascii_digit());
    if !is_date {
        return false;
    }
    if !parts[4].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    true
}
