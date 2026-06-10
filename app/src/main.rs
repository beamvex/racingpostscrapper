use anyhow::Context;
use chromiumoxide::browser::Browser;
use futures::StreamExt;
use tokio::time::{timeout, Duration};

fn extract_attr_value(tag: &str, attr: &str) -> Option<String> {
    let pat = format!("{attr}=\"", attr = attr);
    let start = tag.find(&pat)? + pat.len();
    let end = tag[start..].find('"')?;
    Some(tag[start..start + end].to_string())
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

fn extract_course_and_full_result_urls(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    let needle = "<span>Full result</span>";

    let mut start = 0;
    while let Some(rel) = html[start..].find(needle) {
        let idx = start + rel;

        let course_name = if let Some(section_pos) = html[..idx].rfind("<section") {
            if let Some(tag_end_rel) = html[section_pos..].find('>') {
                let tag = &html[section_pos..section_pos + tag_end_rel + 1];
                extract_attr_value(tag, "data-diffusion-coursename")
                    .unwrap_or_else(|| "unknown".to_string())
            } else {
                "unknown".to_string()
            }
        } else {
            // fallback: find any data-diffusion-coursename occurrence nearby
            let window_start = idx.saturating_sub(5000);
            let window = &html[window_start..idx];
            if let Some(attr_pos) = window.rfind("data-diffusion-coursename=\"") {
                let value_start = attr_pos + "data-diffusion-coursename=\"".len();
                if let Some(value_end) = window[value_start..].find('"') {
                    window[value_start..value_start + value_end].to_string()
                } else {
                    "unknown".to_string()
                }
            } else {
                "unknown".to_string()
            }
        };

        if let Some(a_pos) = html[..idx].rfind("<a") {
            if let Some(href_pos_rel) = html[a_pos..idx].find("href=\"") {
                let href_start = a_pos + href_pos_rel + "href=\"".len();
                if let Some(href_end_rel) = html[href_start..].find('"') {
                    let href = &html[href_start..href_start + href_end_rel];
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
        "https://www.racingpost.com/results/{date}",
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

    eprintln!("scraper: extracting full result urls (grouped by course)");
    let course_urls = extract_course_and_full_result_urls(&html);
    let mut grouped = std::collections::BTreeMap::<String, Vec<String>>::new();
    for (course, url) in course_urls {
        grouped.entry(course).or_default().push(url);
    }

    eprintln!("scraper: writing {} course files", grouped.len());
    for (course, urls) in grouped {
        let course_slug = sanitize_filename_component(&course);
        let urls_out_path = format!(
            "/data/racingpost-results-{date}-full-result-urls-{course}.txt",
            date = results_date,
            course = if course_slug.is_empty() { "unknown" } else { &course_slug }
        );
        eprintln!(
            "scraper: writing {} urls for course={} to {}",
            urls.len(),
            course,
            urls_out_path
        );
        std::fs::write(&urls_out_path, urls.join("\n"))
            .with_context(|| format!("write {urls_out_path}"))?;
    }
    eprintln!("scraper: urls saved");

    eprintln!("scraper: closing browser");
    browser.close().await.ok();
    handler_task.abort();

    eprintln!("scraper: done");

    Ok(())
}
