mod course;
mod url;

pub fn extract_time_order_course_and_full_result_urls(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    let needle = "data-test-selector=\"button-fullResult\"";

    let mut start = 0;
    while let Some(rel) = html[start..].find(needle) {
        let idx = start + rel;
        let course = course::course_name_before_idx(html, idx);
        if let Some(url) = url::full_result_url_before_idx(html, idx) {
            if seen.insert(format!("{course}::{url}")) {
                out.push((course, url));
            }
        }
        start = idx + needle.len();
    }

    out
}
