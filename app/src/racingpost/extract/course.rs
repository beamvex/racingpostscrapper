pub fn course_name_before_idx(html: &str, idx: usize) -> String {
    let window = &html[idx.saturating_sub(20000)..idx];
    let Some(anchor_pos) = window.rfind("data-test-selector=\"text-raceNameTimeView\"") else {
        return "unknown".to_string();
    };
    let after_anchor = &window[anchor_pos..];
    let Some(gt_rel) = after_anchor.find('>') else {
        return "unknown".to_string();
    };
    let inner_start = anchor_pos + gt_rel + 1;
    let Some(a_end_rel) = window[inner_start..].find("</a>") else {
        return "unknown".to_string();
    };
    let inner = &window[inner_start..inner_start + a_end_rel];
    let cleaned = crate::utils::remove_svg_blocks(inner);
    let name = crate::utils::strip_tags_and_collapse_ws(&cleaned);
    if name.is_empty() {
        "unknown".to_string()
    } else {
        name
    }
}
