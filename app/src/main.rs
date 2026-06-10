use anyhow::Context;
use chromiumoxide::browser::Browser;
use futures::StreamExt;
use tokio::time::{timeout, Duration};

fn extract_full_result_urls(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    let needle = "<span>Full result</span>";

    let mut start = 0;
    while let Some(rel) = html[start..].find(needle) {
        let idx = start + rel;

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

                        if seen.insert(url.clone()) {
                            out.push(url);
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

    eprintln!("scraper: extracting full result urls");
    let urls = extract_full_result_urls(&html);
    let urls_out_path = format!(
        "/data/racingpost-results-{date}-full-result-urls.txt",
        date = results_date
    );
    eprintln!("scraper: writing {} urls to {}", urls.len(), urls_out_path);
    std::fs::write(&urls_out_path, urls.join("\n"))
        .with_context(|| format!("write {urls_out_path}"))?;
    eprintln!("scraper: urls saved");

    eprintln!("scraper: closing browser");
    browser.close().await.ok();
    handler_task.abort();

    eprintln!("scraper: done");

    Ok(())
}
