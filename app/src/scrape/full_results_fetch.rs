use chromiumoxide::browser::Browser;
use tokio::time::{timeout, Duration};

pub async fn fetch_detail_html(
    browser: &mut Browser,
    url: &str,
    attempt: usize,
    seq: &mut usize,
) -> Option<String> {
    let page = match timeout(Duration::from_secs(30), browser.new_page(url)).await {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            *seq += 1;
            eprintln!("scraper: seq={seq} open full result page failed (attempt {attempt}/3) url={url} err={e}", seq = *seq);
            return None;
        }
        Err(_) => {
            *seq += 1;
            eprintln!("scraper: seq={seq} timeout opening full result page (attempt {attempt}/3) url={url}", seq = *seq);
            return None;
        }
    };

    let wait_ms = crate::utils::pseudo_random_in_range(1500, 3500);
    tokio::time::sleep(Duration::from_millis(wait_ms)).await;

    match timeout(Duration::from_secs(30), page.content()).await {
        Ok(Ok(h)) => Some(h),
        Ok(Err(e)) => {
            *seq += 1;
            eprintln!("scraper: seq={seq} fetch full result html failed (attempt {attempt}/3) url={url} err={e}", seq = *seq);
            None
        }
        Err(_) => {
            *seq += 1;
            eprintln!("scraper: seq={seq} timeout fetching full result html (attempt {attempt}/3) url={url}", seq = *seq);
            None
        }
    }
}
