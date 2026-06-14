use racingpost_scraper::parser_run;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (input_path, out_dir) = parse_args();
    parser_run::run(&input_path, &out_dir).await
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
