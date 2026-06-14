pub fn write_url_files(
    out_base_dir: &str,
    results_date: &str,
    grouped: &std::collections::BTreeMap<String, Vec<String>>,
) -> anyhow::Result<Vec<String>> {
    let lines = crate::scrape::urls_tsv::build_url_tsv_lines(grouped);
    crate::scrape::urls_tsv::write_urls_tsv(out_base_dir, results_date, grouped, &lines)?;
    crate::scrape::urls_course::write_urls_per_course(out_base_dir, results_date, grouped)?;
    eprintln!("scraper: urls saved");
    Ok(lines)
}
