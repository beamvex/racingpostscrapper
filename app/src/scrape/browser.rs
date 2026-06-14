use anyhow::Context;
use chromiumoxide::browser::Browser;
use futures::StreamExt;
use tokio::time::{timeout, Duration};

pub async fn connect_browser_and_spawn_handler(
) -> anyhow::Result<(Browser, tokio::task::JoinHandle<()>)> {
    eprintln!("scraper: connecting to chromium at http://127.0.0.1:9222");
    let (browser, mut handler) = timeout(
        Duration::from_secs(15),
        Browser::connect("http://127.0.0.1:9222"),
    )
    .await
    .context("timeout connecting to chromium")?
    .context("connect to chromium")?;
    eprintln!("scraper: connected");

    let handler_task =
        tokio::spawn(async move { while let Some(_event) = handler.next().await {} });

    Ok((browser, handler_task))
}
