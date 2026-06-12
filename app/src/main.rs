use anyhow::Context;
use chromiumoxide::browser::Browser;
use futures::StreamExt;
use tokio::time::{timeout, Duration};

fn sanitize_filename_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

fn strip_tags_and_collapse_ws(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut prev_ws = false;

    for ch in s.chars() {
        match ch {
            '<' => {
                in_tag = true;
            }
            '>' => {
                in_tag = false;
            }
            _ if in_tag => {}
            _ if ch.is_whitespace() => {
                if !prev_ws {
                    out.push(' ');
                    prev_ws = true;
                }
            }
            _ => {
                out.push(ch);
                prev_ws = false;
            }
        }
    }

    out.trim().to_string()
}

fn remove_svg_blocks(s: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while let Some(rel) = s[i..].find("<svg") {
        let svg_start = i + rel;
        out.push_str(&s[i..svg_start]);

        if let Some(end_rel) = s[svg_start..].find("</svg>") {
            i = svg_start + end_rel + "</svg>".len();
        } else {
            // no closing tag; drop remainder
            return out;
        }
    }
    out.push_str(&s[i..]);
    out
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn find_between<'a>(haystack: &'a str, start_pat: &str, end_pat: &str) -> Option<&'a str> {
    let start = haystack.find(start_pat)? + start_pat.len();
    let end_rel = haystack[start..].find(end_pat)?;
    Some(&haystack[start..start + end_rel])
}

fn extract_text_after(haystack: &str, marker: &str) -> Option<String> {
    let pos = haystack.find(marker)?;
    let after = &haystack[pos + marker.len()..];
    let gt = after.find('>')?;
    let rest = &after[gt + 1..];
    let lt = rest.find('<')?;
    Some(rest[..lt].trim().to_string())
}

fn parse_full_result_page(html: &str, url: &str, course_from_list: &str) -> String {
    let title = find_between(html, "<title>", "</title>")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "".to_string());

    let race_id = if let Some(id) = find_between(html, "data-race-id=\"", "\"") {
        id.to_string()
    } else {
        "".to_string()
    };

    // Each runner row is marked with data-test-selector="table-row".
    let mut runners_json = Vec::new();
    let mut search = 0;
    while let Some(rel) = html[search..].find("data-test-selector=\"table-row\"") {
        let idx = search + rel;
        let row_start = html[..idx].rfind("<tr").unwrap_or(idx);
        let row_end = html[idx..]
            .find("</tr>")
            .map(|r| idx + r + "</tr>".len())
            .unwrap_or(html.len());
        let row = &html[row_start..row_end];

        let position = extract_text_after(row, "data-test-selector=\"text-horsePosition\"")
            .unwrap_or_else(|| "".to_string());
        let horse = extract_text_after(row, "data-test-selector=\"link-horseName\"")
            .unwrap_or_else(|| "".to_string());
        let jockey = extract_text_after(row, "data-test-selector=\"link-jockeyName\"")
            .unwrap_or_else(|| "".to_string());
        let trainer = extract_text_after(row, "data-test-selector=\"link-trainerName\"")
            .unwrap_or_else(|| "".to_string());
        let age = extract_text_after(row, "data-test-selector=\"horse-age\"")
            .unwrap_or_else(|| "".to_string());
        let w_st = extract_text_after(row, "data-test-selector=\"horse-weight-st\"")
            .unwrap_or_else(|| "".to_string());
        let w_lb = extract_text_after(row, "data-test-selector=\"horse-weight-lb\"")
            .unwrap_or_else(|| "".to_string());
        let or_rating = row
            .split("data-ending=\"OR\"")
            .nth(1)
            .and_then(|s| extract_text_after(s, ">"))
            .unwrap_or_else(|| "".to_string());
        let ts = extract_text_after(row, "data-test-selector=\"full-result-topspeed\"")
            .unwrap_or_else(|| "".to_string());
        let rpr = extract_text_after(row, "data-test-selector=\"full-result-rpr\"")
            .unwrap_or_else(|| "".to_string());

        runners_json.push(format!(
            "{{\"position\":\"{pos}\",\"horse\":\"{horse}\",\"jockey\":\"{jockey}\",\"trainer\":\"{trainer}\",\"age\":\"{age}\",\"weight_st\":\"{wst}\",\"weight_lb\":\"{wlb}\",\"or\":\"{or_rating}\",\"ts\":\"{ts}\",\"rpr\":\"{rpr}\"}}",
            pos = json_escape(&position),
            horse = json_escape(&horse),
            jockey = json_escape(&jockey),
            trainer = json_escape(&trainer),
            age = json_escape(&age),
            wst = json_escape(&w_st),
            wlb = json_escape(&w_lb),
            or_rating = json_escape(&or_rating),
            ts = json_escape(&ts),
            rpr = json_escape(&rpr),
        ));

        search = row_end;
    }

    format!(
        "{{\"course\":\"{course}\",\"url\":\"{url}\",\"race_id\":\"{race_id}\",\"title\":\"{title}\",\"runners\":[{runners}]}}",
        course = json_escape(course_from_list),
        url = json_escape(url),
        race_id = json_escape(&race_id),
        title = json_escape(&title),
        runners = runners_json.join(","),
    )
}

fn extract_time_order_course_and_full_result_urls(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    let needle = "data-test-selector=\"button-fullResult\"";

    let mut start = 0;
    while let Some(rel) = html[start..].find(needle) {
        let idx = start + rel;

        // Time-order page has a course name link block like:
        // <a ... data-test-selector="text-raceNameTimeView">Southwell ...</a>
        let course_name = {
            let window_start = idx.saturating_sub(20000);
            let window = &html[window_start..idx];
            if let Some(anchor_pos) = window.rfind("data-test-selector=\"text-raceNameTimeView\"") {
                let after_anchor = &window[anchor_pos..];
                if let Some(gt_rel) = after_anchor.find('>') {
                    let inner_start = anchor_pos + gt_rel + 1;
                    if let Some(a_end_rel) = window[inner_start..].find("</a>") {
                        let inner = &window[inner_start..inner_start + a_end_rel];
                        let inner_without_svg = remove_svg_blocks(inner);
                        let name = strip_tags_and_collapse_ws(&inner_without_svg);
                        if name.is_empty() {
                            "unknown".to_string()
                        } else {
                            name
                        }
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    "unknown".to_string()
                }
            } else {
                "unknown".to_string()
            }
        };

        // Look back from the button marker to the opening <a ... href="..."> in the same tag.
        if let Some(a_pos) = html[..idx].rfind("<a") {
            let tag_end = html[a_pos..].find('>').map(|r| a_pos + r).unwrap_or(idx);
            let tag = &html[a_pos..tag_end.min(idx)];
            if let Some(href_pos_rel) = tag.find("href=\"") {
                let href_start = href_pos_rel + "href=\"".len();
                if let Some(href_end_rel) = tag[href_start..].find('"') {
                    let href = &tag[href_start..href_start + href_end_rel];
                    if !href.is_empty() {
                        let url = if href.starts_with("http://") || href.starts_with("https://") {
                            href.to_string()
                        } else if href.starts_with('/') {
                            format!("https://www.racingpost.com{href}")
                        } else {
                            href.to_string()
                        };

                        let key = format!("{course}::{url}", course = course_name, url = url);
                        if seen.insert(key) {
                            out.push((course_name.clone(), url));
                        }
                    }
                }
            }
        }

        start = idx + needle.len();
    }

    out
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("scraper: starting");
    std::fs::create_dir_all("/data").context("create /data")?;
    eprintln!("scraper: ensured /data exists");

    let results_date = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("RESULTS_DATE").ok())
        .unwrap_or_else(|| "2026-06-07".to_string());
    let target_url = format!(
        "https://www.racingpost.com/results/{date}/time-order",
        date = results_date
    );
    eprintln!("scraper: target_url={target_url}");

    eprintln!("scraper: connecting to chromium at http://127.0.0.1:9222");
    let (mut browser, mut handler) = timeout(
        Duration::from_secs(15),
        Browser::connect("http://127.0.0.1:9222"),
    )
    .await
    .context("timeout connecting to chromium")?
    .context("connect to chromium")?;
    eprintln!("scraper: connected");

    let handler_task = tokio::spawn(async move {
        while let Some(_event) = handler.next().await {
            // drain events
        }
    });

    eprintln!("scraper: opening page {target_url}");
    let page = timeout(
        Duration::from_secs(30),
        browser.new_page(&target_url),
    )
    .await
    .context("timeout opening page")?
    .context("open page")?;

    eprintln!("scraper: page opened, waiting briefly before screenshot");
    tokio::time::sleep(Duration::from_secs(5)).await;

    let out_path = format!("/data/racingpost-results-{date}.html", date = results_date);
    eprintln!("scraper: fetching html");
    let html = timeout(Duration::from_secs(30), page.content())
        .await
        .context("timeout fetching html")?
        .context("fetch html")?;

    eprintln!("scraper: writing html to {out_path}");
    std::fs::write(&out_path, &html).with_context(|| format!("write {out_path}"))?;
    eprintln!("scraper: html saved");

    eprintln!("scraper: extracting full result urls (time-order)");
    let course_urls = extract_time_order_course_and_full_result_urls(&html);
    let mut grouped = std::collections::BTreeMap::<String, Vec<String>>::new();
    for (course, url) in course_urls {
        let has_country = course.contains('(') && course.contains(')');
        let is_ire = course.contains("(IRE)");
        if has_country && !is_ire {
            continue;
        }

        grouped.entry(course).or_default().push(url);
    }

    let mut lines = Vec::new();
    for (course, urls) in &grouped {
        for url in urls {
            lines.push(format!("{course}\t{url}"));
        }
    }

    let urls_out_path = format!(
        "/data/racingpost-results-{date}-time-order-full-result-urls.tsv",
        date = results_date
    );
    eprintln!(
        "scraper: writing {} links ({} courses) to {}",
        lines.len(),
        grouped.len(),
        urls_out_path
    );
    std::fs::write(&urls_out_path, lines.join("\n"))
        .with_context(|| format!("write {urls_out_path}"))?;

    eprintln!("scraper: also writing one file per course");
    for (course, urls) in grouped {
        let course_slug = sanitize_filename_component(&course);
        let per_course_out = format!(
            "/data/racingpost-results-{date}-time-order-full-result-urls-{course}.txt",
            date = results_date,
            course = if course_slug.is_empty() { "unknown" } else { &course_slug }
        );
        std::fs::write(&per_course_out, urls.join("\n"))
            .with_context(|| format!("write {per_course_out}"))?;
    }

    eprintln!("scraper: urls saved");

    eprintln!("scraper: visiting full result pages and building json");
    let mut full_results_json = Vec::new();
    for line in lines {
        let mut parts = line.split('\t');
        let course = parts.next().unwrap_or("");
        let url = parts.next().unwrap_or("");
        if url.is_empty() {
            continue;
        }

        eprintln!("scraper: fetching full result {url}");
        let detail_page = timeout(Duration::from_secs(30), browser.new_page(url))
            .await
            .context("timeout opening full result page")?
            .context("open full result page")?;
        tokio::time::sleep(Duration::from_secs(2)).await;

        let detail_html = timeout(Duration::from_secs(30), detail_page.content())
            .await
            .context("timeout fetching full result html")?
            .context("fetch full result html")?;

        full_results_json.push(parse_full_result_page(&detail_html, url, course));
    }

    let json_out_path = format!(
        "/data/racingpost-results-{date}-time-order-full-results.json",
        date = results_date
    );
    eprintln!(
        "scraper: writing {} races to {}",
        full_results_json.len(),
        json_out_path
    );
    std::fs::write(&json_out_path, format!("[{}]", full_results_json.join(",")))
        .with_context(|| format!("write {json_out_path}"))?;

    eprintln!("scraper: closing browser");
    browser.close().await.ok();
    handler_task.abort();

    eprintln!("scraper: done");

    Ok(())
}
