//! Server-rendered library and native HTML5 player for older TV browsers.

use std::{cmp::Ordering, collections::BTreeMap, ops::Range};

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use serde::Deserialize;

use crate::domain::{models::VideoDetails, traits::Repository};

const PAGE_SIZE: usize = 20;

pub(super) fn router(repository: Repository) -> Router {
    Router::new()
        .route("/tv", get(shows))
        .route("/tv/", get(shows))
        .route("/tv/show", get(show))
        .route("/tv/watch/{checksum}", get(watch))
        .route("/tv/styles.css", get(styles))
        .with_state(repository)
}

#[derive(Default, Deserialize)]
struct ShowsQuery {
    #[serde(default)]
    q: String,
    page: Option<usize>,
}

#[derive(Deserialize)]
struct ShowQuery {
    title: String,
    season: Option<String>,
    page: Option<usize>,
}

async fn shows(State(repository): State<Repository>, Query(query): Query<ShowsQuery>) -> Response {
    let mut shows = match repository.list_all_series().await {
        Ok(shows) => shows,
        Err(error) => return database_error(error),
    };
    let search = query.q.trim();
    let needle = search.to_lowercase();
    shows.retain(|show| show_name(&show.collection).to_lowercase().contains(&needle));
    shows.sort_by_key(|show| show_name(&show.collection).to_lowercase());

    let mut body = format!(
        r#"<form action="/tv" method="get" class="search">
<label for="search">Find a show</label>
<input id="search" name="q" type="text" value="{}">
<button type="submit">Search</button>
</form><p class="meta">{} shows</p>"#,
        escape(search),
        shows.len(),
    );
    let (page, range) = page_range(query.page, shows.len());
    if shows.is_empty() {
        body.push_str(if search.is_empty() {
            "<p>No videos yet. Add videos to your library to start watching.</p>"
        } else {
            "<p>No shows match your search.</p><p><a href=\"/tv\">Show all shows</a></p>"
        });
    } else {
        body.push_str("<ul class=\"listing\">");
        for show in &shows[range] {
            body.push_str(&entry(
                &show_url(&show.collection, None, 1),
                show_name(&show.collection),
                "Browse episodes",
                &show.thumbnail,
            ));
        }
        body.push_str("</ul>");
        body.push_str(&pagination(page, shows.len(), |page| {
            format!("/tv?q={}&page={page}", urlencoding::encode(search))
        }));
    }
    page_response(StatusCode::OK, "Shows", &body)
}

async fn show(State(repository): State<Repository>, Query(query): Query<ShowQuery>) -> Response {
    // Query the title separately from the season: titles may contain slashes or be numeric.
    let videos = match repository.list_series_details(&query.title, None).await {
        Ok(videos) => videos,
        Err(error) => return database_error(error),
    };
    if videos.is_empty() {
        return error_page(StatusCode::NOT_FOUND, "Show not found");
    }

    let mut seasons: BTreeMap<String, Vec<VideoDetails>> = BTreeMap::new();
    for video in videos {
        seasons
            .entry(video.series.season.clone())
            .or_default()
            .push(video);
    }
    let mut body = String::from("<nav class=\"breadcrumbs\" aria-label=\"Library\">");
    body.push_str(&link("/tv", "All shows"));
    if query.season.is_some() {
        body.push_str(" / ");
        body.push_str(&link(&show_url(&query.title, None, 1), show_name(&query.title)));
    }
    body.push_str("</nav>");

    // Match the existing library: a show with no season goes straight to its videos.
    let season = query
        .season
        .as_deref()
        .or_else(|| (seasons.len() == 1 && seasons.contains_key("")).then_some(""));
    if let Some(season) = season {
        let Some(mut episodes) = seasons.remove(season) else {
            return error_page(StatusCode::NOT_FOUND, "Season not found");
        };
        sort_episodes(&mut episodes);
        if !season.is_empty() || query.season.is_some() {
            body.push_str(&format!("<h2>{}</h2>", escape(&season_name(season))));
        }
        body.push_str(&format!("<p class=\"meta\">{} episodes</p>", episodes.len()));
        let (page, range) = page_range(query.page, episodes.len());
        body.push_str("<ul class=\"listing\">");
        for episode in &episodes[range] {
            body.push_str(&entry(
                &watch_url(episode),
                &episode_name(episode),
                &duration(episode.metadata.duration),
                &episode.thumbnail,
            ));
        }
        body.push_str("</ul>");
        body.push_str(&pagination(page, episodes.len(), |page| {
            show_url(&query.title, query.season.as_deref(), page)
        }));
    } else {
        body.push_str("<h2>Seasons</h2><ul class=\"listing\">");
        let mut seasons: Vec<_> = seasons.into_iter().collect();
        seasons.sort_by(|(left, _), (right, _)| number_order(left, right));
        for (season, mut episodes) in seasons {
            sort_episodes(&mut episodes);
            body.push_str(&entry(
                &show_url(&query.title, Some(&season), 1),
                &season_name(&season),
                &format!("{} episodes", episodes.len()),
                &episodes[0].thumbnail,
            ));
        }
        body.push_str("</ul>");
    }
    page_response(StatusCode::OK, show_name(&query.title), &body)
}

async fn watch(State(repository): State<Repository>, Path(checksum): Path<i64>) -> Response {
    let video = match repository.retrieve_video(checksum).await {
        Ok(video) => video,
        Err(sqlx::Error::RowNotFound) => {
            return error_page(StatusCode::NOT_FOUND, "Video not found")
        }
        Err(error) => return database_error(error),
    };
    let mut episodes = match repository
        .list_series_details(&video.series.series_title, Some(&video.series.season))
        .await
    {
        Ok(episodes) => episodes,
        Err(error) => return database_error(error),
    };
    sort_episodes(&mut episodes);
    let position = episodes
        .iter()
        .position(|episode| episode.checksum == checksum);
    let back_url = show_url(
        &video.series.series_title,
        Some(&video.series.season),
        position.unwrap_or(0) / PAGE_SIZE + 1,
    );
    let stream_url = format!("/api/stream/{}", encode_path(&video.get_download_path()));
    let mut body = format!(
        r#"<nav class="breadcrumbs" aria-label="Library">{} / {} / {}</nav>
<div id="videoContainer"><video id="videoPlayer" controls autoplay preload="metadata" src="{}">
Your browser does not support the video tag. {}
</video></div>
<nav class="playback-nav" aria-label="Episodes">{}"#,
        link("/tv", "All shows"),
        link(
            &show_url(&video.series.series_title, None, 1),
            show_name(&video.series.series_title)
        ),
        link(&back_url, &season_name(&video.series.season)),
        escape(&stream_url),
        link(&stream_url, "Open video directly"),
        link(&back_url, "Back to episodes"),
    );
    if let Some(position) = position {
        if position > 0 {
            body.push_str(&link(&watch_url(&episodes[position - 1]), "Previous episode"));
        }
        if let Some(next) = episodes.get(position + 1) {
            body.push_str(&link(&watch_url(next), "Next episode"));
        }
    }
    body.push_str("</nav>");
    body.push_str(&format!("<p>{}</p>", link(&stream_url, "Open video directly")));
    if !video.description.is_empty() {
        body.push_str(&format!(
            "<p class=\"description\">{}</p>",
            escape(&video.description)
        ));
    }
    page_response(StatusCode::OK, &episode_name(&video), &body)
}

async fn styles() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("tv/styles.css"),
    )
}

fn page_response(status: StatusCode, title: &str, body: &str) -> Response {
    let html = format!(include_str!("tv/page.html"), title = escape(title), body = body);
    (status, [(header::CACHE_CONTROL, "no-store")], Html(html)).into_response()
}

fn error_page(status: StatusCode, title: &str) -> Response {
    page_response(status, title, "<p><a href=\"/tv\">Back to all shows</a></p>")
}

fn database_error(error: sqlx::Error) -> Response {
    tracing::error!("Failed to load TV library: {error}");
    error_page(
        StatusCode::INTERNAL_SERVER_ERROR,
        "The library is temporarily unavailable",
    )
}

fn escape(value: &str) -> String {
    html_escape::encode_safe(value).into_owned()
}

fn link(url: &str, label: &str) -> String {
    format!("<a href=\"{}\">{}</a>", escape(url), escape(label))
}

fn entry(url: &str, title: &str, detail: &str, thumbnails: &[String]) -> String {
    let image = thumbnails
        .first()
        .map(|thumbnail| {
            let name = thumbnail
                .strip_prefix("/api/thumbnails/")
                .unwrap_or(thumbnail);
            format!(
                "<img class=\"thumbnail\" src=\"{}\" width=\"160\" height=\"90\" alt=\"\">",
                escape(&format!("/api/thumbnails/{}", encode_path(name))),
            )
        })
        .unwrap_or_default();
    format!(
        "<li><a class=\"item\" href=\"{}\">{image}<span class=\"item-title\">{}</span><span class=\"meta\">{}</span></a></li>",
        escape(url), escape(title), escape(detail),
    )
}

fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|part| urlencoding::encode(part).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn show_url(title: &str, season: Option<&str>, page: usize) -> String {
    let mut url = format!("/tv/show?title={}", urlencoding::encode(title));
    if let Some(season) = season {
        url.push_str(&format!("&season={}", urlencoding::encode(season)));
    }
    if page > 1 {
        url.push_str(&format!("&page={page}"));
    }
    url
}

fn watch_url(video: &VideoDetails) -> String {
    format!("/tv/watch/{}", video.checksum)
}

fn show_name(title: &str) -> &str {
    if title.is_empty() {
        "Other videos"
    } else {
        title
    }
}

fn season_name(season: &str) -> String {
    if season.is_empty() {
        "Other episodes".to_string()
    } else if season.parse::<u64>().is_ok() {
        format!("Season {season}")
    } else {
        season.to_string()
    }
}

fn episode_name(video: &VideoDetails) -> String {
    let title = &video.series.episode_title;
    let episode = &video.series.episode;
    match (episode.is_empty(), title.is_empty()) {
        (false, false) => format!("Episode {episode}: {title}"),
        (false, true) => format!("Episode {episode}"),
        (true, false) => title.clone(),
        (true, true) => video.video.clone(),
    }
}

fn duration(seconds: f64) -> String {
    if seconds.is_finite() && seconds > 0.0 {
        format!("{} min", (seconds / 60.0).ceil() as u64)
    } else {
        String::new()
    }
}

fn number_order(left: &str, right: &str) -> Ordering {
    match (left.parse::<u64>(), right.parse::<u64>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => left.to_lowercase().cmp(&right.to_lowercase()),
    }
    .then_with(|| left.cmp(right))
}

fn sort_episodes(episodes: &mut [VideoDetails]) {
    episodes.sort_by(|left, right| {
        number_order(&left.series.episode, &right.series.episode)
            .then_with(|| left.series.episode_title.cmp(&right.series.episode_title))
            .then_with(|| left.video.cmp(&right.video))
            .then_with(|| left.checksum.cmp(&right.checksum))
    });
}

fn page_range(requested: Option<usize>, total: usize) -> (usize, Range<usize>) {
    let last_page = total.saturating_sub(1) / PAGE_SIZE + 1;
    let page = requested.unwrap_or(1).clamp(1, last_page);
    let start = (page - 1) * PAGE_SIZE;
    (page, start..(start + PAGE_SIZE).min(total))
}

fn pagination(page: usize, total: usize, url: impl Fn(usize) -> String) -> String {
    let pages = total.saturating_sub(1) / PAGE_SIZE + 1;
    if pages <= 1 {
        return String::new();
    }
    let mut body = String::from("<nav class=\"pagination\" aria-label=\"Pages\">");
    if page > 1 {
        body.push_str(&link(&url(page - 1), "Previous page"));
    }
    body.push_str(&format!("<span>Page {page} of {pages}</span>"));
    if page < pages {
        body.push_str(&link(&url(page + 1), "Next page"));
    }
    body.push_str("</nav>");
    body
}
