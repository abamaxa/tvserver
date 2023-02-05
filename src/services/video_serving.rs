use std::io::{self, SeekFrom};
use crate::services::web::Context;

use axum::{
    body::StreamBody,
    http::{header, StatusCode},
    response::{AppendHeaders, IntoResponse},
    extract::{State}
};
use axum::http::header::HeaderName;
use tokio::io::AsyncSeekExt;
use tokio_util::io::ReaderStream;


pub async fn video(State(_): State<Context>, headers: header::HeaderMap) -> impl IntoResponse {
    // `File` implements `AsyncRead`
    const BUFFER_SIZE: usize = 0x100000; // 1 megabyte
    const VIDEO_FILE: &str = "/Users/chris2/Movies/Only Fools and Horses/Series 6/S06E01 - Yuppy Love (Uncut).mp4";

    let mut stream_from = 0;
    let mut stream_to = 0;
    let mut found_range = false;

    //println!("dumping headers");
    for (k, v) in headers.iter() {
        if k == "range" {
            (stream_from, stream_to) = get_offsets(v.to_str().unwrap());
            found_range = true;
        }
        //println!("{} -> {:?}", k, v);
    }

    let mut file = match tokio::fs::File::open(VIDEO_FILE).await {
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
        stream_to = file_size - 1;
    }

    //let chunk_size = stream_to - stream_from + 1;

    // convert the `AsyncRead` into a `Stream`
    let stream = ReaderStream::with_capacity(file, BUFFER_SIZE);
    // convert the `Stream` into an `axum::body::HttpBody`
    let body = StreamBody::new(stream);

    let content_type = HeaderName::from_static("Content-Type");
    let content_length = HeaderName::from_static("Content-Length");
    //let content_disposition = HeaderName::from_static("Content-Disposition");
    let content_range = HeaderName::from_static("Content-Range");
    let accept_ranges = HeaderName::from_static("Accept-Ranges");

    if !found_range || stream_from == 0 {
        let headers = AppendHeaders([
            (content_type, "video/mp4".to_string()),
            (content_length, file_size.to_string()),
            //(content_disposition,"attachment; filename=\"OnlyFools.mp4\"".to_string()),
            (content_range, format!("bytes {}-{}/{}", stream_from, stream_to, file_size)),
        ]);

        return Ok((StatusCode::OK, headers, body));
    }

    let headers = AppendHeaders([
        (accept_ranges, "bytes".to_string()),
        (content_type, "video/mp4".to_string()),
        (content_range, format!("bytes {}-{}/{}", stream_from, stream_to, file_size)),
        //(content_length, chunk_size.to_string()),
        //(content_disposition,"attachment; filename=\"OnlyFools.mp4\"".to_string()),
    ]);

    Ok((StatusCode::PARTIAL_CONTENT, headers, body))
}

/*
    if !found_range {
        let headers = AppendHeaders([
            (header::CONTENT_TYPE, "video/mp4".to_string()),
            (header::CONTENT_LENGTH, file_size.to_string()),
            (header::CONTENT_DISPOSITION,"attachment; filename=\"OnlyFools.mp4\"".to_string()),
        ]);

        return Ok((StatusCode::OK, headers, body));
    }

    let headers = AppendHeaders([
        (header::ACCEPT_RANGES, "bytes".to_string()),
        (header::CONTENT_TYPE, "video/mp4".to_string()),
        (header::CONTENT_RANGE, format!("bytes {}-{}/{}", stream_from, stream_to, file_size)),
        //(header::CONTENT_DISPOSITION,"attachment; filename=\"OnlyFools.mp4\"".to_string()),
    ]);

    Ok((StatusCode::PARTIAL_CONTENT, headers, body))
 */


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