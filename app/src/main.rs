use anyhow::Context;
use chromiumoxide::browser::Browser;
use futures::StreamExt;
use tokio::time::{timeout, Duration};

fn current_utc_date_yyyy_mm_dd() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe.div_euclid(1_460) + doe.div_euclid(36_524) - doe.div_euclid(146_096))
        .div_euclid(365);
    let y = (yoe + era * 400) as i32;
    let doy = doe - (365 * yoe + yoe.div_euclid(4) - yoe.div_euclid(100));
    let mp = (5 * doy + 2).div_euclid(153);
    let d = (doy - (153 * mp + 2).div_euclid(5) + 1) as u32;
    let m = (mp + if mp < 10 { 3 } else { -9 }) as u32;
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m, d)
}

fn pseudo_random_in_range(min_ms: u64, max_ms: u64) -> u64 {
    if max_ms <= min_ms {
        return min_ms;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let mixed = now.as_nanos() as u64 ^ now.as_secs();
    let span = max_ms - min_ms + 1;
    min_ms + (mixed % span)
}

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
        .unwrap_or_else(current_utc_date_yyyy_mm_dd);
    let target_url = format!(
        "https://www.racingpost.com/results/{date}/time-order",
        date = results_date
    );
    eprintln!("scraper: target_url={target_url}");

    let (year, month, day) = {
        let mut it = results_date.split('-');
        (
            it.next().unwrap_or("unknown"),
            it.next().unwrap_or("unknown"),
            it.next().unwrap_or("unknown"),
        )
    };
    let out_base_dir = format!("/data/{}/{}/{}/", year, month, day);
    std::fs::create_dir_all(&out_base_dir)
        .with_context(|| format!("create {out_base_dir}"))?;
    eprintln!("scraper: out_base_dir={out_base_dir}");

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

    let out_path = format!(
        "{base}racingpost-results-{date}.html",
        base = out_base_dir,
        date = results_date
    );
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
        "{base}racingpost-results-{date}-time-order-full-result-urls.tsv",
        base = out_base_dir,
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
    for (course, urls) in &grouped {
        let course_slug = sanitize_filename_component(&course);
        let per_course_out = format!(
            "{base}racingpost-results-{date}-time-order-full-result-urls-{course}.txt",
            base = out_base_dir,
            date = results_date,
            course = if course_slug.is_empty() { "unknown" } else { &course_slug }
        );
        std::fs::write(&per_course_out, urls.join("\n"))
            .with_context(|| format!("write {per_course_out}"))?;
    }

    eprintln!("scraper: urls saved");

    let full_results_html_dir = format!(
        "{base}racingpost-results-{date}-time-order-full-results-html",
        base = out_base_dir,
        date = results_date
    );
    std::fs::create_dir_all(&full_results_html_dir)
        .with_context(|| format!("create {full_results_html_dir}"))?;

    eprintln!(
        "scraper: downloading full result pages html into {}",
        full_results_html_dir
    );
    let mut downloaded = 0usize;
    let mut failed = 0usize;
    for (course, urls) in &grouped {
        let course_slug = sanitize_filename_component(course);
        let course_part = if course_slug.is_empty() { "unknown" } else { &course_slug };
        for (i, url) in urls.iter().enumerate() {
            let mut ok = false;
            for attempt in 1..=3 {
                eprintln!("scraper: fetching full result html (attempt {attempt}/3) {url}");

                let detail_page = match timeout(Duration::from_secs(30), browser.new_page(url)).await {
                    Ok(Ok(p)) => p,
                    Ok(Err(e)) => {
                        eprintln!("scraper: open full result page failed (attempt {attempt}/3) url={url} err={e}");
                        continue;
                    }
                    Err(_) => {
                        eprintln!("scraper: timeout opening full result page (attempt {attempt}/3) url={url}");
                        continue;
                    }
                };

                let wait_ms = pseudo_random_in_range(1500, 3500);
                tokio::time::sleep(Duration::from_millis(wait_ms)).await;

                let detail_html = match timeout(Duration::from_secs(30), detail_page.content()).await {
                    Ok(Ok(h)) => h,
                    Ok(Err(e)) => {
                        eprintln!("scraper: fetch full result html failed (attempt {attempt}/3) url={url} err={e}");
                        continue;
                    }
                    Err(_) => {
                        eprintln!("scraper: timeout fetching full result html (attempt {attempt}/3) url={url}");
                        continue;
                    }
                };

                let html_out_path = format!(
                    "{dir}/{course}-{idx}.html",
                    dir = full_results_html_dir,
                    course = course_part,
                    idx = i + 1
                );
                if let Err(e) = std::fs::write(&html_out_path, detail_html)
                    .with_context(|| format!("write {html_out_path}"))
                {
                    eprintln!("scraper: write html failed url={url} path={html_out_path} err={e}");
                    break;
                }

                downloaded += 1;
                ok = true;
                break;
            }

            if !ok {
                failed += 1;
            }
        }
    }
    eprintln!(
        "scraper: downloaded {} full result html pages (failed {})",
        downloaded, failed
    );

    eprintln!("scraper: closing browser");
    browser.close().await.ok();
    handler_task.abort();

    eprintln!("scraper: done");

    Ok(())
}
