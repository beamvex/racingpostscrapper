use anyhow::Context;

pub fn build_url_tsv_lines(
    grouped: &std::collections::BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let mut lines = Vec::new();
    for (course, urls) in grouped {
        for url in urls {
            lines.push(format!("{course}\t{url}"));
        }
    }
    lines
}

pub fn write_urls_tsv(
    out_base_dir: &str,
    results_date: &str,
    grouped: &std::collections::BTreeMap<String, Vec<String>>,
    lines: &[String],
) -> anyhow::Result<()> {
    let path =
        format!("{out_base_dir}racingpost-results-{results_date}-time-order-full-result-urls.tsv");
    eprintln!(
        "scraper: writing {} links ({} courses) to {}",
        lines.len(),
        grouped.len(),
        path
    );
    std::fs::write(&path, lines.join("\n")).with_context(|| format!("write {path}"))
}
