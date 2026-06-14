pub fn full_result_url_before_idx(html: &str, idx: usize) -> Option<String> {
    let around = 1200usize;
    let start = idx.saturating_sub(around);
    let end = (idx + around).min(html.len());
    let slice = &html[start..end];
    let href = find_best_href_near_idx(slice, idx - start)?;
    Some(normalize_racingpost_url(href))
}

fn find_best_href_near_idx(slice: &str, local_idx: usize) -> Option<&str> {
    let mut i = 0usize;
    let mut best: Option<(usize, &str)> = None;
    while let Some(rel) = slice[i..].find("href=\"") {
        let pos = i + rel;
        let start = pos + "href=\"".len();
        let end_rel = slice[start..].find('"')?;
        let href = &slice[start..start + end_rel];
        if is_full_result_href(href) {
            let dist = if pos > local_idx { pos - local_idx } else { local_idx - pos };
            if best.map(|(b, _)| dist < b).unwrap_or(true) {
                best = Some((dist, href));
            }
        }
        i = start;
    }
    best.map(|(_, h)| h)
}

fn is_full_result_href(href: &str) -> bool {
    let h = href.trim();
    if !(h.contains("/results/") && h.chars().last().is_some_and(|c| c.is_ascii_digit())) {
        return false;
    }
    h.matches('/').count() >= 5
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
