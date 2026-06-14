use crate::full_result_parse::{extract_text_after, json_escape};

pub fn extract_runners_json(html: &str) -> Vec<String> {
    let mut runners_json = Vec::new();
    let mut search = 0;
    while let Some(rel) = html[search..].find("data-test-selector=\"table-row\"") {
        let idx = search + rel;
        let row_start = html[..idx].rfind("<tr").unwrap_or(idx);
        let row_end = html[idx..]
            .find("</tr>")
            .map(|r| idx + r + "</tr>".len())
            .unwrap_or(html.len());
        runners_json.push(parse_runner_row(&html[row_start..row_end]));
        search = row_end;
    }
    runners_json
}

fn parse_runner_row(row: &str) -> String {
    let position = val(row, "data-test-selector=\"text-horsePosition\"");
    let horse = val(row, "data-test-selector=\"link-horseName\"");
    let jockey = val(row, "data-test-selector=\"link-jockeyName\"");
    let trainer = val(row, "data-test-selector=\"link-trainerName\"");
    let age = val(row, "data-test-selector=\"horse-age\"");
    let w_st = val(row, "data-test-selector=\"horse-weight-st\"");
    let w_lb = val(row, "data-test-selector=\"horse-weight-lb\"");
    let ts = val(row, "data-test-selector=\"full-result-topspeed\"");
    let rpr = val(row, "data-test-selector=\"full-result-rpr\"");
    let or_rating = row
        .split("data-ending=\"OR\"")
        .nth(1)
        .and_then(|s| extract_text_after(s, ">"))
        .unwrap_or_else(|| "".to_string());

    format!(
        "{{\"position\":\"{pos}\",\"horse\":\"{horse}\",\"jockey\":\"{jockey}\",\"trainer\":\"{trainer}\",\"age\":\"{age}\",\"weight_st\":\"{wst}\",\"weight_lb\":\"{wlb}\",\"or\":\"{or_rating}\",\"ts\":\"{ts}\",\"rpr\":\"{rpr}\"}}",
        pos = json_escape(&position),
        horse = json_escape(&horse),
        jockey = json_escape(&jockey),
        trainer = json_escape(&trainer),
        age = json_escape(&age),
        wst = json_escape(&w_st),
        wlb = json_escape(&w_lb),
        or_rating = json_escape(&or_rating),
        ts = json_escape(&ts),
        rpr = json_escape(&rpr)
    )
}

fn val(row: &str, marker: &str) -> String {
    extract_text_after(row, marker).unwrap_or_else(|| "".to_string())
}
