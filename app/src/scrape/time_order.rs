use anyhow::Context;
use chromiumoxide::browser::Browser;
use chromiumoxide::page::Page;
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
    scroll_until_stable(&page).await?;

    let html = timeout(Duration::from_secs(30), page.content())
        .await
        .context("timeout fetching html")?
        .context("fetch html")?;

    eprintln!("scraper: writing html to {out_path}");
    std::fs::write(out_path, &html).with_context(|| format!("write {out_path}"))?;
    eprintln!("scraper: html saved");

    Ok(html)
}

async fn scroll_until_stable(page: &Page) -> anyhow::Result<()> {
    let mut prev = 0i64;
    for _ in 0..8 {
        let h = page_height(page).await?;
        if h == prev {
            return Ok(());
        }
        prev = h;
        page.evaluate("window.scrollTo(0, document.body.scrollHeight);")
            .await
            .context("scroll")?;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Ok(())
}

async fn page_height(page: &Page) -> anyhow::Result<i64> {
    let v = page
        .evaluate("document.body && document.body.scrollHeight ? document.body.scrollHeight : 0")
        .await
        .context("evaluate height")?;
    Ok(v.value().unwrap_or_default().as_i64().unwrap_or(0))
}
