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
