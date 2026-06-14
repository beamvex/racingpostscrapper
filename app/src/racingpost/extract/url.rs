pub fn full_result_url_before_idx(html: &str, idx: usize) -> Option<String> {
    let a_pos = html[..idx].rfind("<a")?;
    let tag_end = html[a_pos..].find('>').map(|r| a_pos + r).unwrap_or(idx);
    let tag = &html[a_pos..tag_end.min(idx)];
    let href = extract_href(tag)?;
    Some(normalize_racingpost_url(href))
}

fn extract_href(tag: &str) -> Option<&str> {
    let href_pos_rel = tag.find("href=\"")?;
    let href_start = href_pos_rel + "href=\"".len();
    let href_end_rel = tag[href_start..].find('"')?;
    let href = &tag[href_start..href_start + href_end_rel];
    if href.is_empty() {
        None
    } else {
        Some(href)
    }
}

fn normalize_racingpost_url(href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if href.starts_with('/') {
        return format!("https://www.racingpost.com{href}");
    }
    href.to_string()
}
