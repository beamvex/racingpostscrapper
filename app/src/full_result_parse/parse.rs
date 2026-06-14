mod race;
mod runner;

pub fn parse_full_result_page(html: &str, url: &str, course_from_list: &str) -> String {
    let title = race::extract_title(html);
    let race_id = race::extract_race_id(html);
    let runners = runner::extract_runners_json(html);
    race::build_race_json(&title, &race_id, &runners, url, course_from_list)
}
