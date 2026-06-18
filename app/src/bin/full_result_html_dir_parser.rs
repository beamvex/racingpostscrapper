use anyhow::Context;
use racingpost_scraper::full_result_parse;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (html_dir, out_dir) = parse_args();

    std::fs::create_dir_all(&out_dir).with_context(|| format!("create {out_dir}"))?;

    eprintln!("parser(html-dir): html_dir={html_dir}");
    eprintln!("parser(html-dir): out_dir={out_dir}");

    let mut html_paths: Vec<std::path::PathBuf> = std::fs::read_dir(&html_dir)
        .with_context(|| format!("read_dir {html_dir}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("html"))
        .collect();

    html_paths.sort();

    let mut json = Vec::new();
    let mut failed = 0usize;

    for path in html_paths {
        let path_str = path.display().to_string();
        let html = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                failed += 1;
                eprintln!("parser(html-dir): read failed path={path_str} err={e}");
                continue;
            }
        };

        let course = infer_course_from_filename(&path).unwrap_or_else(|| "".to_string());
        let url = "";

        let title = full_result_parse::extract_title(&html);
        let race_id = full_result_parse::extract_race_id(&html);
        let runners = full_result_parse::extract_runners_json(&html);

        for runner_json in runners {
            json.push(format!(
                "{{\"url\":\"{}\",\"course\":\"{}\",\"title\":\"{}\",\"race_id\":\"{}\",\"runner\":{}}}",
                full_result_parse::json_escape(url),
                full_result_parse::json_escape(&course),
                full_result_parse::json_escape(&title),
                full_result_parse::json_escape(&race_id),
                runner_json
            ));
        }
    }

    let json_out_path = output_json_path(&html_dir, &out_dir);
    eprintln!(
        "parser(html-dir): writing {} runners to {} (failed {})",
        json.len(),
        json_out_path,
        failed
    );

    write_json(&json_out_path, &json)?;

    eprintln!("parser(html-dir): done");
    Ok(())
}

fn parse_args() -> (String, String) {
    let mut html_dir: Option<String> = None;
    let mut out_dir: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--html-dir" | "-d" => html_dir = args.next(),
            "--out-dir" | "-o" => out_dir = args.next(),
            _ => {}
        }
    }

    (
        html_dir.unwrap_or_else(|| "/data/racingpost-results-unknown-time-order-full-results-html".to_string()),
        out_dir.unwrap_or_else(|| "/data".to_string()),
    )
}

fn infer_course_from_filename(path: &std::path::Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let mut it = stem.rsplitn(2, '-');
    let _idx = it.next()?;
    let course = it.next()?;
    Some(course.to_string())
}

fn output_json_path(html_dir: &str, out_dir: &str) -> String {
    let dir_name = std::path::Path::new(html_dir)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("full-results-html");

    let out_filename = dir_name.replace("-time-order-full-results-html", "-time-order-full-results.json");

    format!("{}/{}", out_dir.trim_end_matches('/'), out_filename)
}

fn write_json(json_out_path: &str, json: &[String]) -> anyhow::Result<()> {
    std::fs::write(json_out_path, json.join("\n"))
        .with_context(|| format!("write {json_out_path}"))
}
