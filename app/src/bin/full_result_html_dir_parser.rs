use anyhow::Context;
use arrow::array::{ArrayRef, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use parquet::file::properties::WriterVersion;
use racingpost_scraper::full_result_parse;
use serde_json::Value;
use std::sync::Arc;

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

    let mut rows: Vec<Row> = Vec::new();
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
        let going = full_result_parse::extract_going(&html);
        let runners = full_result_parse::extract_runners_json(&html);

        for runner_json in runners {
            if let Ok(v) = serde_json::from_str::<Value>(&runner_json) {
                rows.push(Row::from_json(
                    url,
                    &course,
                    &title,
                    &race_id,
                    &going,
                    &v,
                ));
            }
        }
    }

    let parquet_out_path = output_parquet_path(&html_dir, &out_dir);
    eprintln!(
        "parser(html-dir): writing {} runners to {} (failed {})",
        rows.len(),
        parquet_out_path,
        failed
    );

    write_parquet(&parquet_out_path, &rows)?;

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

fn infer_ymd_from_path(path: &str) -> Option<(String, String, String)> {
    let p = path.replace('\\', "/");
    let parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
    for i in 0..parts.len().saturating_sub(2) {
        let y = parts[i];
        let m = parts[i + 1];
        let d = parts[i + 2];
        if y.len() == 4
            && m.len() == 2
            && d.len() == 2
            && y.chars().all(|c| c.is_ascii_digit())
            && m.chars().all(|c| c.is_ascii_digit())
            && d.chars().all(|c| c.is_ascii_digit())
        {
            return Some((y.to_string(), m.to_string(), d.to_string()));
        }
    }
    None
}

fn output_parquet_path(html_dir: &str, out_dir: &str) -> String {
    let out_base = out_dir.trim_end_matches('/');
    if let Some((y, m, d)) = infer_ymd_from_path(html_dir) {
        format!("{}/year={}/month={}/day={}/full-results-runners.parquet", out_base, y, m, d)
    } else {
        format!("{}/full-results-runners.parquet", out_base)
    }
}

#[derive(Clone, Default)]
struct Row {
    url: String,
    course: String,
    title: String,
    race_id: String,
    going: String,
    position: String,
    horse: String,
    jockey: String,
    trainer: String,
    age: String,
    weight_st: String,
    weight_lb: String,
    or_rating: String,
    ts: String,
    rpr: String,
}

impl Row {
    fn from_json(url: &str, course: &str, title: &str, race_id: &str, going: &str, v: &Value) -> Row {
        Row {
            url: url.to_string(),
            course: course.to_string(),
            title: title.to_string(),
            race_id: race_id.to_string(),
            going: going.to_string(),
            position: json_field(v, "position"),
            horse: json_field(v, "horse"),
            jockey: json_field(v, "jockey"),
            trainer: json_field(v, "trainer"),
            age: json_field(v, "age"),
            weight_st: json_field(v, "weight_st"),
            weight_lb: json_field(v, "weight_lb"),
            or_rating: json_field(v, "or"),
            ts: json_field(v, "ts"),
            rpr: json_field(v, "rpr"),
        }
    }
}

fn json_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|vv| vv.as_str())
        .unwrap_or("")
        .to_string()
}

fn opt(s: &str) -> Option<&str> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t) }
}

fn write_parquet(out_path: &str, rows: &[Row]) -> anyhow::Result<()> {
    if let Some(parent) = std::path::Path::new(out_path).parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))?;
    }

    let schema = Arc::new(Schema::new(vec![
        Field::new("url", DataType::Utf8, true),
        Field::new("course", DataType::Utf8, true),
        Field::new("title", DataType::Utf8, true),
        Field::new("race_id", DataType::Utf8, true),
        Field::new("going", DataType::Utf8, true),
        Field::new("position", DataType::Utf8, true),
        Field::new("horse", DataType::Utf8, true),
        Field::new("jockey", DataType::Utf8, true),
        Field::new("trainer", DataType::Utf8, true),
        Field::new("age", DataType::Utf8, true),
        Field::new("weight_st", DataType::Utf8, true),
        Field::new("weight_lb", DataType::Utf8, true),
        Field::new("or", DataType::Utf8, true),
        Field::new("ts", DataType::Utf8, true),
        Field::new("rpr", DataType::Utf8, true),
    ]));

    let make = |f: fn(&Row) -> &str| -> ArrayRef {
        Arc::new(StringArray::from(
            rows.iter().map(|r| opt(f(r))).collect::<Vec<_>>(),
        ))
    };

    let columns: Vec<ArrayRef> = vec![
        make(|r| &r.url),
        make(|r| &r.course),
        make(|r| &r.title),
        make(|r| &r.race_id),
        make(|r| &r.going),
        make(|r| &r.position),
        make(|r| &r.horse),
        make(|r| &r.jockey),
        make(|r| &r.trainer),
        make(|r| &r.age),
        make(|r| &r.weight_st),
        make(|r| &r.weight_lb),
        make(|r| &r.or_rating),
        make(|r| &r.ts),
        make(|r| &r.rpr),
    ];

    let batch = RecordBatch::try_new(schema.clone(), columns)
        .with_context(|| "build record batch")?;

    let file = std::fs::File::create(out_path).with_context(|| format!("create {out_path}"))?;
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .set_dictionary_enabled(false)
        .set_writer_version(WriterVersion::PARQUET_1_0)
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))
        .with_context(|| "create parquet writer")?;
    writer.write(&batch).with_context(|| "write parquet")?;
    writer.close().with_context(|| "close parquet")?;
    Ok(())
}
