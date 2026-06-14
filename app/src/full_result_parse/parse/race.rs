use crate::full_result_parse::{find_between, json_escape};

pub fn extract_title(html: &str) -> String {
    find_between(html, "<title>", "</title>")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "".to_string())
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
