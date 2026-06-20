use crate::full_result_parse::{find_between, json_escape};

pub fn extract_title(html: &str) -> String {
    find_between(html, "<title>", "</title>")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "".to_string())
}

pub fn extract_going(html: &str) -> String {
    // Prefer Next.js JSON if present.
    if let Some(next) = find_between(
        html,
        "<script id=\"__NEXT_DATA__\" type=\"application/json\">",
        "</script>",
    ) {
        if let Some(g) = find_json_string(next, "\"goingDescription\":\"")
            .or_else(|| find_json_string(next, "\"going\":\""))
        {
            let g = g.trim();
            if !g.is_empty() {
                return g.to_string();
            }
        }
    }

    // Fallback: try common HTML label patterns.
    if let Some(g) = find_between(html, "Going</span>", "</") {
        let g = g.trim();
        if !g.is_empty() {
            return g.to_string();
        }
    }

    "".to_string()
}

fn find_json_string<'a>(haystack: &'a str, key_pat: &str) -> Option<&'a str> {
    let start = haystack.find(key_pat)? + key_pat.len();
    let end_rel = haystack[start..].find('"')?;
    Some(&haystack[start..start + end_rel])
}

pub fn extract_race_id(html: &str) -> String {
    find_between(html, "data-race-id=\"", "\"")
        .map(|s| s.to_string())
        .unwrap_or_else(|| "".to_string())
}

pub fn build_race_json(
    title: &str,
    race_id: &str,
    runners_json: &[String],
    url: &str,
    course_from_list: &str,
) -> String {
    format!(
        "{{\"url\":\"{url}\",\"course\":\"{course}\",\"title\":\"{title}\",\"race_id\":\"{race_id}\",\"runners\":[{runners}]}}",
        url = json_escape(url),
        course = json_escape(course_from_list),
        title = json_escape(title),
        race_id = json_escape(race_id),
        runners = runners_json.join(",")
    )
}
