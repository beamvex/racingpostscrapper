mod race;
mod runner;

pub fn parse_full_result_page(html: &str, url: &str, course_from_list: &str) -> String {
    let title = race::extract_title(html);
    let race_id = race::extract_race_id(html);
    let runners = runner::extract_runners_json(html);
    race::build_race_json(&title, &race_id, &runners, url, course_from_list)
}

pub fn extract_title(html: &str) -> String {
    race::extract_title(html)
}

pub fn extract_race_id(html: &str) -> String {
    race::extract_race_id(html)
}

pub fn extract_runners_json(html: &str) -> Vec<String> {
    runner::extract_runners_json(html)
}
