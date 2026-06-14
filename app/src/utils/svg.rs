pub fn remove_svg_blocks(s: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while let Some(rel) = s[i..].find("<svg") {
        let svg_start = i + rel;
        out.push_str(&s[i..svg_start]);
        match svg_block_end(s, svg_start) {
            Some(next_i) => i = next_i,
            None => return out,
        }
    }
    out.push_str(&s[i..]);
    out
}

fn svg_block_end(s: &str, svg_start: usize) -> Option<usize> {
    let end_rel = s[svg_start..].find("</svg>")?;
    Some(svg_start + end_rel + "</svg>".len())
}
