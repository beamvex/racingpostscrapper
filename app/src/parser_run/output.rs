use anyhow::Context;

pub fn output_json_path(input_path: &str, out_dir: &str) -> String {
    let out_filename = std::path::Path::new(input_path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|name| {
            name.replace(
                "-time-order-full-result-urls.tsv",
                "-time-order-full-results.json",
            )
        })
        .unwrap_or_else(|| "racingpost-time-order-full-results.json".to_string());
    format!("{}/{}", out_dir.trim_end_matches('/'), out_filename)
}

pub fn write_json(json_out_path: &str, json: &[String]) -> anyhow::Result<()> {
    std::fs::write(json_out_path, format!("[{}]", json.join(",")))
        .with_context(|| format!("write {json_out_path}"))
}
