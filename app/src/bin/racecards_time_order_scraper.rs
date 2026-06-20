use anyhow::Context;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("racecards: starting");
    std::fs::create_dir_all("/data").context("create /data")?;

    let results_date = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("RESULTS_DATE").ok())
        .unwrap_or_else(racingpost_scraper::utils::current_utc_date_yyyy_mm_dd);

    let target_url = "https://www.racingpost.com/racecards/time-order/";
    eprintln!("racecards: target_url={target_url}");

    let out_base_dir = racingpost_scraper::scrape::out_base_dir_for_date(&results_date)?;
    eprintln!("racecards: out_base_dir={out_base_dir}");

    let (mut browser, handler_task) = racingpost_scraper::scrape::connect_browser_and_spawn_handler().await?;

    let out_path = format!("{out_base_dir}racingpost-racecards-{results_date}.html");
    let _html = racingpost_scraper::scrape::fetch_and_save_time_order_html(&mut browser, target_url, &out_path).await?;

    eprintln!("racecards: closing browser");
    browser.close().await.ok();
    handler_task.abort();

    eprintln!("racecards: done");
    Ok(())
}
