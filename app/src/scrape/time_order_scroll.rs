use anyhow::Context;
use chromiumoxide::page::Page;

pub async fn scroll_until_stable(page: &Page) -> anyhow::Result<()> {
    let mut prev = 0i64;
    for _ in 0..12 {
        let h = scroll_step_and_get_height(page).await?;
        if h == prev {
            return Ok(());
        }
        prev = h;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    Ok(())
}

async fn scroll_step_and_get_height(page: &Page) -> anyhow::Result<i64> {
    let js = r#"(() => {
        const pick = () => {
            const els = Array.from(document.querySelectorAll('*'));
            const scrollables = els.filter(e => {
                const s = getComputedStyle(e);
                const ok = (s.overflowY === 'auto' || s.overflowY === 'scroll');
                return ok && e.scrollHeight > e.clientHeight + 50;
            });
            scrollables.sort((a, b) => b.scrollHeight - a.scrollHeight);
            return scrollables[0] || document.scrollingElement || document.documentElement || document.body;
        };
        const el = pick();
        el.scrollTop = el.scrollHeight;
        return el.scrollHeight || 0;
    })()"#;

    let v = page.evaluate(js).await.context("scroll evaluate")?;
    Ok(v.value().unwrap_or_default().as_i64().unwrap_or(0))
}
