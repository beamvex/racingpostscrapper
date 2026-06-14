pub fn full_result_url_before_idx(html: &str, idx: usize) -> Option<String> {
    let around = 1200usize;
    let start = idx.saturating_sub(around);
    let end = (idx + around).min(html.len());
    let slice = &html[start..end];
    let href = find_href_near_idx(slice, idx - start)?;
    Some(normalize_racingpost_url(href))
}

fn find_href_near_idx(slice: &str, local_idx: usize) -> Option<&str> {
    let before = &slice[..local_idx.min(slice.len())];
    if let Some(href) = find_href_in_text(before) {
        return Some(href);
    }
    let after = &slice[local_idx.min(slice.len())..];
    find_href_in_text(after)
}

fn find_href_in_text(text: &str) -> Option<&str> {
    let needle = "href=\"";
    let href_pos_rel = text.rfind(needle).or_else(|| text.find(needle))?;
    let href_start = href_pos_rel + needle.len();
    let href_end_rel = text[href_start..].find('"')?;
    let href = &text[href_start..href_start + href_end_rel];
    if href.is_empty() { None } else { Some(href) }
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
