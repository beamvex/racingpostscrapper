use anyhow::Context;

pub fn write_html(
    dir: &str,
    course_part: &str,
    idx: usize,
    html: &str,
    url: &str,
    seq: &mut usize,
) -> anyhow::Result<()> {
    let path = format!("{dir}/{course_part}-{idx}.html");
    if let Err(e) = std::fs::write(&path, html).with_context(|| format!("write {path}")) {
        *seq += 1;
        eprintln!(
            "scraper: seq={seq} write html failed url={url} path={path} err={e}",
            seq = *seq
        );
        return Err(e);
    }
    Ok(())
}
