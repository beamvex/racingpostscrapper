pub fn strip_tags_and_collapse_ws(s: &str) -> String {
    inner(s).trim().to_string()
}

fn inner(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch == '<' {
            in_tag = true;
            continue;
        }
        if ch == '>' {
            in_tag = false;
            continue;
        }
        if in_tag {
            continue;
        }
        if ch.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
            continue;
        }
        out.push(ch);
        prev_ws = false;
    }
    out
}
