use anyhow::Context;
use chromiumoxide::browser::Browser;
use futures::StreamExt;
use std::time::Duration;
use tokio::time::timeout;

use racingpost_scraper::full_result_parse;

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
    for (course, url) in pairs {
        eprintln!("parser: fetching {url}");

        let detail_page = timeout(Duration::from_secs(30), browser.new_page(&url))
            .await
            .context("timeout opening full result page")?
            .context("open full result page")?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        let detail_html = timeout(Duration::from_secs(30), detail_page.content())
            .await
            .context("timeout fetching full result html")?
            .context("fetch full result html")?;

        full_results_json.push(full_result_parse::parse_full_result_page(
            &detail_html,
            &url,
            &course,
        ));
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
        "parser: writing {} races to {}",
        full_results_json.len(),
        json_out_path
    );

    std::fs::write(&json_out_path, format!("[{}]", full_results_json.join(",")))
        .with_context(|| format!("write {json_out_path}"))?;

    eprintln!("parser: closing browser");
    browser.close().await.ok();
    handler_task.abort();

    eprintln!("parser: done");
    Ok(())
}
