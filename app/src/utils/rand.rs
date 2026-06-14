use tokio::time::Duration;

pub fn pseudo_random_in_range(min_ms: u64, max_ms: u64) -> u64 {
    if max_ms <= min_ms {
        return min_ms;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let mixed = now.as_nanos() as u64 ^ now.as_secs();
    let span = max_ms - min_ms + 1;
    min_ms + (mixed % span)
}
