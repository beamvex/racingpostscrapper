pub async fn run_for_date(
    results_date: &str,
    target_url: &str,
    out_base_dir: &str,
) -> anyhow::Result<()> {
    let (mut browser, handler_task) = crate::scrape::connect_browser_and_spawn_handler().await?;
    let out_path = format!("{out_base_dir}racingpost-results-{results_date}.html");
    let html =
        crate::scrape::fetch_and_save_time_order_html(&mut browser, target_url, &out_path).await?;

    let marker = "data-test-selector=\"button-fullResult\"";
    let marker_count = html.matches(marker).count();
    eprintln!("scraper: time-order marker count={marker_count}");

    eprintln!("scraper: extracting full result urls (time-order)");
    let course_urls = crate::racingpost::extract_time_order_course_and_full_result_urls(&html);
    let grouped = crate::racingpost::group_and_filter_course_urls(course_urls);
    let _lines = crate::scrape::write_url_files(out_base_dir, results_date, &grouped)?;

    let dir =
        format!("{out_base_dir}racingpost-results-{results_date}-time-order-full-results-html");
    let _ = crate::scrape::download_full_results_html(&mut browser, &grouped, &dir).await?;

    close_browser(browser, handler_task).await;
    eprintln!("scraper: done");
    Ok(())
}

async fn close_browser(
    mut browser: chromiumoxide::browser::Browser,
    handler_task: tokio::task::JoinHandle<()>,
) {
    eprintln!("scraper: closing browser");
    browser.close().await.ok();
    handler_task.abort();
}
