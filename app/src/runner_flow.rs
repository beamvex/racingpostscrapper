use anyhow::Context;

pub async fn run() -> anyhow::Result<()> {
    eprintln!("scraper: starting");
    std::fs::create_dir_all("/data").context("create /data")?;
    eprintln!("scraper: ensured /data exists");

    let results_date = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("RESULTS_DATE").ok())
        .unwrap_or_else(crate::utils::current_utc_date_yyyy_mm_dd);

    let target_url = format!("https://www.racingpost.com/results/{results_date}/time-order");
    eprintln!("scraper: target_url={target_url}");

    let out_base_dir = crate::scrape::out_base_dir_for_date(&results_date)?;
    eprintln!("scraper: out_base_dir={out_base_dir}");

    crate::runner_ops::run_for_date(&results_date, &target_url, &out_base_dir).await
}
