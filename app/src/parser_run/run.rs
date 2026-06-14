use anyhow::Context;

pub async fn run(input_path: &str, out_dir: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(out_dir).with_context(|| format!("create {out_dir}"))?;
    eprintln!("parser: input={input_path}");
    eprintln!("parser: out_dir={out_dir}");

    let pairs = crate::full_result_parse::read_tsv_lines(input_path)?;
    eprintln!("parser: {} urls to fetch", pairs.len());

    let (mut browser, handler_task) = crate::parser_run::browser::connect().await?;
    let (json, failed) = crate::parser_run::fetch::fetch_all(&mut browser, &pairs).await;

    let json_out_path = crate::parser_run::output::output_json_path(input_path, out_dir);
    eprintln!(
        "parser: writing {} races to {} (failed {})",
        json.len(),
        json_out_path,
        failed
    );
    crate::parser_run::output::write_json(&json_out_path, &json)?;

    eprintln!("parser: closing browser");
    browser.close().await.ok();
    handler_task.abort();
    eprintln!("parser: done");
    Ok(())
}
