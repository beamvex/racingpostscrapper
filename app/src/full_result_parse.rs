use anyhow::Context;

pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn find_between<'a>(haystack: &'a str, start_pat: &str, end_pat: &str) -> Option<&'a str> {
    let start = haystack.find(start_pat)? + start_pat.len();
    let end_rel = haystack[start..].find(end_pat)?;
    Some(&haystack[start..start + end_rel])
}

pub fn extract_text_after(haystack: &str, marker: &str) -> Option<String> {
    let pos = haystack.find(marker)?;
    let after = &haystack[pos + marker.len()..];
    let gt = after.find('>')?;
    let rest = &after[gt + 1..];
    let lt = rest.find('<')?;
    Some(rest[..lt].trim().to_string())
}

pub fn parse_full_result_page(html: &str, url: &str, course_from_list: &str) -> String {
    let title = find_between(html, "<title>", "</title>")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "".to_string());

    let race_id = if let Some(id) = find_between(html, "data-race-id=\"", "\"") {
        id.to_string()
    } else {
        "".to_string()
    };

    let mut runners_json = Vec::new();
    let mut search = 0;
    while let Some(rel) = html[search..].find("data-test-selector=\"table-row\"") {
        let idx = search + rel;
        let row_start = html[..idx].rfind("<tr").unwrap_or(idx);
        let row_end = html[idx..]
            .find("</tr>")
            .map(|r| idx + r + "</tr>".len())
            .unwrap_or(html.len());
        let row = &html[row_start..row_end];

        let position = extract_text_after(row, "data-test-selector=\"text-horsePosition\"")
            .unwrap_or_else(|| "".to_string());
        let horse = extract_text_after(row, "data-test-selector=\"link-horseName\"")
            .unwrap_or_else(|| "".to_string());
        let jockey = extract_text_after(row, "data-test-selector=\"link-jockeyName\"")
            .unwrap_or_else(|| "".to_string());
        let trainer = extract_text_after(row, "data-test-selector=\"link-trainerName\"")
            .unwrap_or_else(|| "".to_string());
        let age = extract_text_after(row, "data-test-selector=\"horse-age\"")
            .unwrap_or_else(|| "".to_string());
        let w_st = extract_text_after(row, "data-test-selector=\"horse-weight-st\"")
            .unwrap_or_else(|| "".to_string());
        let w_lb = extract_text_after(row, "data-test-selector=\"horse-weight-lb\"")
            .unwrap_or_else(|| "".to_string());
        let or_rating = row
            .split("data-ending=\"OR\"")
            .nth(1)
            .and_then(|s| extract_text_after(s, ">"))
            .unwrap_or_else(|| "".to_string());
        let ts = extract_text_after(row, "data-test-selector=\"full-result-topspeed\"")
            .unwrap_or_else(|| "".to_string());
        let rpr = extract_text_after(row, "data-test-selector=\"full-result-rpr\"")
            .unwrap_or_else(|| "".to_string());

        runners_json.push(format!(
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
            rpr = json_escape(&rpr),
        ));

        search = row_end;
    }

    format!(
        "{{\"course\":\"{course}\",\"url\":\"{url}\",\"race_id\":\"{race_id}\",\"title\":\"{title}\",\"runners\":[{runners}]}}",
        course = json_escape(course_from_list),
        url = json_escape(url),
        race_id = json_escape(&race_id),
        title = json_escape(&title),
        runners = runners_json.join(","),
    )
}

pub fn read_tsv_lines(input_path: &str) -> anyhow::Result<Vec<(String, String)>> {
    let raw = std::fs::read_to_string(input_path).with_context(|| format!("read {input_path}"))?;
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let course = parts.next().unwrap_or("").trim().to_string();
        let url = parts.next().unwrap_or("").trim().to_string();
        if url.is_empty() {
            continue;
        }
        out.push((course, url));
    }
    Ok(out)
}
