pub fn group_and_filter_course_urls(
    course_urls: Vec<(String, String)>,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut grouped = std::collections::BTreeMap::<String, Vec<String>>::new();
    for (course, url) in course_urls {
        if is_foreign_country_course(&course) {
            continue;
        }
        grouped.entry(course).or_default().push(url);
    }
    grouped
}

fn is_foreign_country_course(course: &str) -> bool {
    let has_country = course.contains('(') && course.contains(')');
    let is_ire = course.contains("(IRE)");
    has_country && !is_ire
}
