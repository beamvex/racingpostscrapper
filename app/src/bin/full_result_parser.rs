use anyhow::Context;
use chromiumoxide::browser::Browser;
use futures::StreamExt;
use std::time::Duration;
use tokio::time::timeout;

use racingpost_scraper::full_result_parse;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut input_path: Option<String> = None;
    let mut out_dir: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" | "-i" => {
                input_path = args.next();
            }
            "--out-dir" | "-o" => {
                out_dir = args.next();
            }
            _ => {}
        }
    }

    let input_path = input_path.unwrap_or_else(|| "/data/racingpost-results-time-order-full-result-urls.tsv".to_string());
    let out_dir = out_dir.unwrap_or_else(|| "/data".to_string());

    std::fs::create_dir_all(&out_dir).with_context(|| format!("create {out_dir}"))?;

    eprintln!("parser: input={input_path}");
    eprintln!("parser: out_dir={out_dir}");

    let pairs = full_result_parse::read_tsv_lines(&input_path)?;
    eprintln!("parser: {} urls to fetch", pairs.len());

    eprintln!("parser: connecting to chromium at http://127.0.0.1:9222");
    let (mut browser, mut handler) = timeout(Duration::from_secs(15), Browser::connect("http://127.0.0.1:9222"))
        .await
        .context("timeout connecting to chromium")?
        .context("connect to chromium")?;

    let handler_task = tokio::spawn(async move {
        while let Some(_event) = handler.next().await {
            // drain events
        }
    });

    let mut full_results_json = Vec::new();
    let mut failed = 0usize;
    for (course, url) in pairs {
        let mut ok = false;
        for attempt in 1..=3 {
            eprintln!("parser: fetching (attempt {attempt}/3) {url}");

            let detail_page = match timeout(Duration::from_secs(30), browser.new_page(&url)).await {
                Ok(Ok(p)) => p,
                Ok(Err(e)) => {
                    eprintln!("parser: open page failed (attempt {attempt}/3) url={url} err={e}");
                    continue;
                }
                Err(_) => {
                    eprintln!("parser: timeout opening page (attempt {attempt}/3) url={url}");
                    continue;
                }
            };

            let wait_ms = pseudo_random_in_range(1500, 3500);
            tokio::time::sleep(Duration::from_millis(wait_ms)).await;

            let detail_html = match timeout(Duration::from_secs(30), detail_page.content()).await {
                Ok(Ok(h)) => h,
                Ok(Err(e)) => {
                    eprintln!("parser: fetch html failed (attempt {attempt}/3) url={url} err={e}");
                    continue;
                }
                Err(_) => {
                    eprintln!("parser: timeout fetching html (attempt {attempt}/3) url={url}");
                    continue;
                }
            };

            full_results_json.push(full_result_parse::parse_full_result_page(
                &detail_html,
                &url,
                &course,
            ));
            ok = true;
            break;
        }

        if !ok {
            failed += 1;
        }
    }

    let out_filename = std::path::Path::new(&input_path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|name| {
            name.replace(
                "-time-order-full-result-urls.tsv",
                "-time-order-full-results.json",
            )
        })
        .unwrap_or_else(|| "racingpost-time-order-full-results.json".to_string());
    let json_out_path = format!("{}/{}", out_dir.trim_end_matches('/'), out_filename);
    eprintln!(
        "parser: writing {} races to {} (failed {})",
        full_results_json.len(),
        json_out_path,
        failed
    );

    std::fs::write(&json_out_path, format!("[{}]", full_results_json.join(",")))
        .with_context(|| format!("write {json_out_path}"))?;

    eprintln!("parser: closing browser");
    browser.close().await.ok();
    handler_task.abort();

    eprintln!("parser: done");
    Ok(())
}
