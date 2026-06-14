use anyhow::Context;
use chromiumoxide::browser::Browser;

pub async fn download_full_results_html(
    browser: &mut Browser,
    grouped: &std::collections::BTreeMap<String, Vec<String>>,
    dir: &str,
) -> anyhow::Result<(usize, usize)> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {dir}"))?;
    eprintln!("scraper: downloading full result pages html into {}", dir);
    let mut seq = 0usize;
    let mut downloaded = 0usize;
    let mut failed = 0usize;
    for (course, urls) in grouped {
        let course_part = crate::scrape::full_results_loop::slug_or_unknown(course);
        for (i, url) in urls.iter().enumerate() {
            if crate::scrape::full_results_loop::download_one(
                browser,
                url,
                dir,
                &course_part,
                i + 1,
                &mut seq,
            )
            .await?
            {
                downloaded += 1;
            } else {
                failed += 1;
            }
        }
    }
    Ok((downloaded, failed))
}
