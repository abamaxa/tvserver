use crate::adaptors::youtube_runtime;

pub(super) const MAX_RESULTS: usize = 25;

/// Use the same managed yt-dlp runtime for each site's keyless search.
pub(super) async fn search(query: &str, prefix: &str, source: &str) -> anyhow::Result<Vec<u8>> {
    let mut command = youtube_runtime::command().await?;
    let query = format!(
        "{prefix}{MAX_RESULTS}:{}",
        query.trim().chars().take(200).collect::<String>()
    );
    command.args.extend(
        [
            "--flat-playlist",
            "--dump-single-json",
            "--skip-download",
            "--quiet",
            "--no-warnings",
            "--socket-timeout",
            "15",
            "--extractor-retries",
            "1",
            "--",
            &query,
        ]
        .into_iter()
        .map(String::from),
    );
    run(command, source).await
}

pub(super) async fn run(
    command: youtube_runtime::YtdlpCommand,
    source: &str,
) -> anyhow::Result<Vec<u8>> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(45),
        tokio::process::Command::new(command.program)
            .args(command.args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("{source} search timed out; please try again"))??;
    anyhow::ensure!(
        output.status.success(),
        "{source} search failed: {}",
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(600)
            .collect::<String>()
    );
    Ok(output.stdout)
}
