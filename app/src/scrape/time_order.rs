use anyhow::Context;
use chromiumoxide::browser::Browser;
use tokio::time::{timeout, Duration};

pub async fn fetch_and_save_time_order_html(
    browser: &mut Browser,
    target_url: &str,
    out_path: &str,
) -> anyhow::Result<String> {
    let page = timeout(Duration::from_secs(30), browser.new_page(target_url))
        .await
        .context("timeout opening page")?
        .context("open page")?;

    tokio::time::sleep(Duration::from_secs(5)).await;
    crate::scrape::time_order_scroll::scroll_until_stable(&page).await?;

    let html = timeout(Duration::from_secs(30), page.content())
        .await
        .context("timeout fetching html")?
        .context("fetch html")?;

    eprintln!("scraper: writing html to {out_path}");
    std::fs::write(out_path, &html).with_context(|| format!("write {out_path}"))?;
    eprintln!("scraper: html saved");

    Ok(html)
}
