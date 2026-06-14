use chromiumoxide::browser::Browser;

pub fn slug_or_unknown(course: &str) -> String {
    let s = crate::utils::sanitize_filename_component(course);
    if s.is_empty() {
        "unknown".to_string()
    } else {
        s
    }
}

pub async fn download_one(
    browser: &mut Browser,
    url: &str,
    dir: &str,
    course_part: &str,
    idx: usize,
    seq: &mut usize,
) -> anyhow::Result<bool> {
    for attempt in 1..=3 {
        *seq += 1;
        eprintln!(
            "scraper: seq={seq} fetching full result html (attempt {attempt}/3) {url}",
            seq = *seq
        );
        if let Some(html) =
            crate::scrape::full_results_fetch::fetch_detail_html(browser, url, attempt, seq).await
        {
            return Ok(crate::scrape::full_results_write::write_html(
                dir,
                course_part,
                idx,
                &html,
                url,
                seq,
            )
            .is_ok());
        }
    }
    Ok(false)
}
