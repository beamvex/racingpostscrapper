mod racingpost;
mod runner;
mod runner_flow;
mod runner_ops;
mod scrape;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    runner::run().await
}
