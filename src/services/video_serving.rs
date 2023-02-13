use std::io::{self, SeekFrom};
use axum::{
    body::StreamBody,
    http::{header, StatusCode},
    response::{AppendHeaders, IntoResponse}
};
use axum::http::header::HeaderName;
use tokio::io::AsyncSeekExt;
use tokio_util::io::ReaderStream;


pub async fn stream_video(video_file: String, headers: header::HeaderMap) -> impl IntoResponse {
    const BUFFER_SIZE: usize = 0x100000; // 1 megabyte

    let file_parts: Vec<&str> = video_file.rsplitn(1, "/").collect();
    let file_name = String::from(file_parts[0]);
    let mut stream_from = 0;
    let mut stream_to = 0;
    let mut found_range = false;

    for (k, v) in headers.iter() {
        if k == "range" {
            (stream_from, stream_to) = get_offsets(v.to_str().unwrap());
            found_range = true;
        }
    }

    let mut file = match tokio::fs::File::open(video_file).await {
        Ok(file) => file,
        Err(err) => return Err((StatusCode::NOT_FOUND, format!("File not found: {}", err))),
    };

    let file_size = match get_file_size(&mut file).await {
        Ok(file_size) => file_size,
        Err(err) => return Err((StatusCode::NOT_FOUND, format!("Could not determine file size: {}", err))),
    };

    if stream_from > 0 {
        match file.seek(SeekFrom::Start(stream_from)).await {
            Ok(o) => o,
            Err(err) => return Err((StatusCode::NOT_FOUND, format!("Cannot seek: {}", err))),
        };
    }

    if stream_to == 0 {
        // stream_to = file_size - 1;
        let buf_size = BUFFER_SIZE as u64;
        stream_to = if stream_from + buf_size < file_size {stream_from + buf_size} else {file_size - 1};
    }

    // convert the `AsyncRead` into a `Stream`
    let stream = ReaderStream::with_capacity(file, BUFFER_SIZE);
    // convert the `Stream` into an `axum::body::HttpBody`
    let body = StreamBody::new(stream);

    let content_type = HeaderName::from_static("Content-Type");
    let content_length = HeaderName::from_static("Content-Length");
    let content_disposition = HeaderName::from_static("Content-Disposition");
    let content_range = HeaderName::from_static("Content-Range");
    let accept_ranges = HeaderName::from_static("Accept-Ranges");

    if !found_range || (stream_to - stream_from) >= (file_size - 1) {
        let headers = AppendHeaders([
            (content_type, "video/mp4".to_string()),
            (content_length, file_size.to_string()),
            (content_disposition,format!("attachment; filename=\"{}\"", file_name)),
            (content_range, format!("bytes {}-{}/{}", stream_from, stream_to, file_size)),
        ]);

        return Ok((StatusCode::OK, headers, body));
    }

    let headers = AppendHeaders([
        (accept_ranges, "bytes".to_string()),
        (content_type, "video/mp4".to_string()),
        (content_range, format!("bytes {}-{}/{}", stream_from, stream_to, file_size)),
        (content_disposition,format!("attachment; filename=\"{}\"", file_name)),
    ]);

    Ok((StatusCode::PARTIAL_CONTENT, headers, body))
}

fn get_offsets(offsets: &str) -> (u64, u64) {
    let mut parts = offsets.splitn(2, "=");
    let mut range = parts.nth(1).unwrap().splitn(2, "-");

    let start = match range.nth(0) {
        Some(start) => start.parse::<u64>().unwrap(),
        None => 0,
    };

    let end = match range.nth(1) {
        Some(end) => end.parse::<u64>().unwrap(),
        None => 0,
    };
    (start, end)
}

async fn get_file_size(file: &mut tokio::fs::File) -> io::Result<u64> {
    file.seek(SeekFrom::End(0)).await?;
    let position = file.stream_position().await?;
    file.seek(SeekFrom::Start(0)).await?;
    Ok(position)
}