use anyhow::Context;

pub fn write_urls_per_course(
    out_base_dir: &str,
    results_date: &str,
    grouped: &std::collections::BTreeMap<String, Vec<String>>,
) -> anyhow::Result<()> {
    eprintln!("scraper: also writing one file per course");
    for (course, urls) in grouped {
        let path = per_course_urls_path(out_base_dir, results_date, course);
        std::fs::write(&path, urls.join("\n")).with_context(|| format!("write {path}"))?;
    }
    Ok(())
}

fn per_course_urls_path(out_base_dir: &str, results_date: &str, course: &str) -> String {
    let slug = crate::utils::sanitize_filename_component(course);
    let course_part = if slug.is_empty() { "unknown" } else { &slug };
    format!("{out_base_dir}racingpost-results-{results_date}-time-order-full-result-urls-{course_part}.txt")
}
