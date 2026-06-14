use chromiumoxide::browser::Browser;
use std::time::Duration;
use tokio::time::timeout;

pub async fn fetch_all(browser: &mut Browser, pairs: &[(String, String)]) -> (Vec<String>, usize) {
    let mut json = Vec::new();
    let mut failed = 0usize;
    for (course, url) in pairs {
        match fetch_one(browser, course, url).await {
            Some(j) => json.push(j),
            None => failed += 1,
        }
    }
    (json, failed)
}

async fn fetch_one(browser: &mut Browser, course: &str, url: &str) -> Option<String> {
    for attempt in 1..=3 {
        eprintln!("parser: fetching (attempt {attempt}/3) {url}");
        let page = match timeout(Duration::from_secs(30), browser.new_page(url)).await {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => {
                eprintln!("parser: open page failed (attempt {attempt}/3) url={url} err={e}");
                continue;
            }
            Err(_) => {
                eprintln!("parser: timeout opening page (attempt {attempt}/3) url={url}");
                continue;
            }
        };

        let wait_ms = crate::parser_run::jitter::pseudo_random_in_range(1500, 3500);
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;

        let html = match timeout(Duration::from_secs(30), page.content()).await {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => {
                eprintln!("parser: fetch html failed (attempt {attempt}/3) url={url} err={e}");
                continue;
            }
            Err(_) => {
                eprintln!("parser: timeout fetching html (attempt {attempt}/3) url={url}");
                continue;
            }
        };

        return Some(crate::full_result_parse::parse_full_result_page(
            &html, url, course,
        ));
    }
    None
}
