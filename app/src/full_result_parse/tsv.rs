use anyhow::Context;

pub fn read_tsv_lines(path: &str) -> anyhow::Result<Vec<(String, String)>> {
    let contents = std::fs::read_to_string(path).with_context(|| format!("read {path}"))?;
    let mut out = Vec::new();
    for line in contents.lines() {
        let mut parts = line.split('\t');
        let course = parts.next().unwrap_or("").to_string();
        let url = parts.next().unwrap_or("").to_string();
        if !url.is_empty() {
            out.push((course, url));
        }
    }
    Ok(out)
}
