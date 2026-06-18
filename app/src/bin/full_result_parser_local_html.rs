use anyhow::Context;
use racingpost_scraper::full_result_parse;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (input_path, out_dir) = parse_args();

    std::fs::create_dir_all(&out_dir).with_context(|| format!("create {out_dir}"))?;

    eprintln!("parser(local-html): input={input_path}");
    eprintln!("parser(local-html): out_dir={out_dir}");

    let pairs = full_result_parse::read_tsv_lines(&input_path)?;
    eprintln!("parser(local-html): {} urls", pairs.len());

    let (html_dir, results_date) = infer_html_dir_and_date(&input_path);
    eprintln!("parser(local-html): inferred results_date={results_date}");
    eprintln!("parser(local-html): html_dir={html_dir}");

    let mut json = Vec::new();
    let mut failed = 0usize;

    for (i, (course, url)) in pairs.iter().enumerate() {
        let idx = i + 1;
        let course_part = slug_or_unknown(course);
        let html_path = format!("{}/{course_part}-{idx}.html", html_dir.trim_end_matches('/'));

        let html = match std::fs::read_to_string(&html_path) {
            Ok(s) => s,
            Err(e) => {
                failed += 1;
                eprintln!(
                    "parser(local-html): missing html idx={idx} course={course} url={url} path={html_path} err={e}"
                );
                continue;
            }
        };

        let race_json = full_result_parse::parse_full_result_page(&html, url, course);
        json.push(race_json);
    }

    let json_out_path = output_json_path(&input_path, &out_dir);
    eprintln!(
        "parser(local-html): writing {} races to {} (failed {})",
        json.len(),
        json_out_path,
        failed
    );
    write_json(&json_out_path, &json)?;

    eprintln!("parser(local-html): done");
    Ok(())
}

fn parse_args() -> (String, String) {
    let mut input_path: Option<String> = None;
    let mut out_dir: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" | "-i" => input_path = args.next(),
            "--out-dir" | "-o" => out_dir = args.next(),
            _ => {}
        }
    }

    (
        input_path.unwrap_or_else(|| {
            "/data/racingpost-results-time-order-full-result-urls.tsv".to_string()
        }),
        out_dir.unwrap_or_else(|| "/data".to_string()),
    )
}

fn infer_html_dir_and_date(input_path: &str) -> (String, String) {
    let filename = std::path::Path::new(input_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let results_date = filename
        .strip_prefix("racingpost-results-")
        .and_then(|s| s.split("-time-order-full-result-urls.tsv").next())
        .unwrap_or("unknown")
        .to_string();

    let parent = std::path::Path::new(input_path)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or(".")
        .trim_end_matches('/');

    let html_dir = format!(
        "{}/racingpost-results-{}-time-order-full-results-html",
        parent, results_date
    );

    (html_dir, results_date)
}

fn slug_or_unknown(course: &str) -> String {
    let s = sanitize_filename_component(course);
    if s.is_empty() {
        "unknown".to_string()
    } else {
        s
    }
}

fn sanitize_filename_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

fn output_json_path(input_path: &str, out_dir: &str) -> String {
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

fn write_json(json_out_path: &str, json: &[String]) -> anyhow::Result<()> {
    std::fs::write(json_out_path, json.join("\n"))
        .with_context(|| format!("write {json_out_path}"))
}
