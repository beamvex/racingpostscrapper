use anyhow::Context;

pub fn out_base_dir_for_date(results_date: &str) -> anyhow::Result<String> {
    let (year, month, day) = split_ymd(results_date);
    let out_base_dir = format!("/data/{}/{}/{}/", year, month, day);
    std::fs::create_dir_all(&out_base_dir).with_context(|| format!("create {out_base_dir}"))?;
    Ok(out_base_dir)
}

fn split_ymd(date: &str) -> (&str, &str, &str) {
    let mut it = date.split('-');
    (
        it.next().unwrap_or("unknown"),
        it.next().unwrap_or("unknown"),
        it.next().unwrap_or("unknown"),
    )
}
