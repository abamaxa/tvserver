use crate::domain::config::get_movie_dir;
use futures::StreamExt;
use std::io;
use std::io::SeekFrom;
use std::path::PathBuf;
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio_util::codec::{BytesCodec, FramedRead};

pub async fn stream_video(
    stream: &mut tokio::net::TcpStream,
    video_name: &str,
    start: u64,
    mut end: u64,
) {
    let file_path = PathBuf::from(get_movie_dir()).join(video_name);
    if !file_path.exists() {
        let response = b"HTTP/1.1 404 Not Found\r\n\r\n";
        let _ = stream.write_all(response).await;
        return;
    }

    let mut file = match TokioFile::open(file_path).await {
        Ok(file) => file,
        Err(_) => {
            let response = b"HTTP/1.1 500 Internal Server Error\r\n\r\n";
            let _ = stream.write_all(response).await;
            return;
        }
    };

    let file_size = match file.metadata().await {
        Ok(metadata) => metadata.len(),
        Err(_) => {
            let response = b"HTTP/1.1 500 Internal Server Error\r\n\r\n";
            let _ = stream.write_all(response).await;
            return;
        }
    };

    if end == 0 {
        end = file_size - 1;
    }

    file.seek(SeekFrom::Start(start)).await.unwrap();

    let response = format!(
        "HTTP/1.1 206 Partial Content\r\nAccept-Ranges: bytes\r\nContent-Range: bytes {}-{}/{}\r\nContent-Length: {}\r\nContent-Type: video/mp4\r\n\r\n",
        start, end, file_size, 1 + end - start
    );
    let _ = stream.write_all(response.as_bytes()).await;

    if let Err(e) = copy_file_to_stream_in_chunks(file, stream, 0x200000).await {
        println!("Streaming error: {}", e.to_string());
    }
}

pub fn get_range(req: &httparse::Request) -> (u64, u64) {
    let range = req
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("range"));

    let (start, end) = match range {
        Some(range) => {
            let range_value = String::from_utf8(range.value.to_vec()).unwrap();
            let range_str = range_value
                .trim_start_matches("bytes=")
                .splitn(2, '-')
                .collect::<Vec<&str>>();
            let start: u64 = range_str[0].parse().unwrap();
            let end: u64 = range_str.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            (start, end)
        }
        None => (0, 0),
    };

    (start, end)
}

async fn copy_file_to_stream_in_chunks(
    file: TokioFile,
    stream: &mut tokio::net::TcpStream,
    buffer_size: usize,
) -> io::Result<()> {
    let framed_read = FramedRead::with_capacity(file, BytesCodec::new(), buffer_size);

    let mut chunks = framed_read.map(Result::unwrap);

    while let Some(chunk) = chunks.next().await {
        if let Err(e) = stream.write_all(chunk.as_ref()).await {
            return Err(e);
        }
    }

    Ok(())
}

/*use axum::http::header::HeaderName;
use axum::{
    body::StreamBody,
    http::{header, StatusCode},
    response::{AppendHeaders, IntoResponse},
};
use axum_extra::body::AsyncReadBody;
use std::io::{self, SeekFrom};
use tokio::io::{AsyncRead, AsyncSeekExt};
use tokio_util::io::ReaderStream;

pub async fn stream_video(video_file: &str, headers: header::HeaderMap) -> impl IntoResponse {
    //const BUFFER_SIZE: usize = 0x100000; // 1 megabyte
    const BUFFER_SIZE: usize = 0x200000;

    let file_parts: Vec<&str> = video_file.rsplitn(2, '/').collect();
    let file_name = String::from(file_parts[0]);
    let (found_range, stream_from, mut stream_to) = get_range(headers);

    let mut file = match tokio::fs::File::open(video_file).await {
        Ok(file) => file,
        Err(err) => return Err((StatusCode::NOT_FOUND, format!("File not found: {}", err))),
    };

    let file_size = match get_file_size(&mut file).await {
        Ok(file_size) => file_size,
        Err(err) => {
            return Err((
                StatusCode::NOT_FOUND,
                format!("Could not determine file size: {}", err),
            ))
        }
    };

    if file_size == 0 {
        return Err((StatusCode::BAD_REQUEST, "corrupt file".to_string()));
    }

    if stream_from >= file_size {
        return Err((
            StatusCode::RANGE_NOT_SATISFIABLE,
            format!(
                "requested read from {} but file is {} bytes long",
                stream_from, file_size
            ),
        ));
    }

    if stream_from > 0 {
        match file.seek(SeekFrom::Start(stream_from)).await {
            Ok(o) => o,
            Err(err) => return Err((StatusCode::NOT_FOUND, format!("Cannot seek: {}", err))),
        };
    }

    if stream_to == 0 {
        stream_to = file_size - 1;
    }

    // convert the `AsyncRead` into a `Stream`
    let stream = ReaderStream::with_capacity(file, BUFFER_SIZE);
    // convert the `Stream` into an `axum::body::HttpBody`
    let body = StreamBody::new(stream);
    //let body = AsyncReadBody::new(file);

    // Sadly we can't use the builtin in header names as they are all lower case, which is the
    // standard for HTTP2. However, this HTTP/1.1 server has a Samsung TV as a client with a built
    // in web browser that expects the headers to be capitalized, as below. Trying to use lower case
    // headers breaks the video control, which entirely defeats the purpose. Regrettably, there is
    // no way to force axum/http not to convert the headers to lowercase, so we currently need to
    // compile using a hacked version of the http lib, which is hosted on my github.
    let content_type = HeaderName::from_static_preserve_case("Content-Type");
    let content_length = HeaderName::from_static_preserve_case("Content-Length");
    let content_disposition = HeaderName::from_static_preserve_case("Content-Disposition");
    let content_range = HeaderName::from_static_preserve_case("Content-Range");
    let accept_ranges = HeaderName::from_static_preserve_case("Accept-Ranges");

    let headers = AppendHeaders([
        (accept_ranges, "bytes".to_string()),
        (content_type, "video/mp4".to_string()),
        (content_length, format!("{}", 1 + stream_to - stream_from)),
        (
            content_range,
            format!("bytes {}-{}/{}", stream_from, stream_to, file_size),
        ),
        // (
        //     content_disposition,
        //     format!("attachment; filename=\"{}\"", file_name),
        // ),
    ]);

    let code = match found_range {
        true => StatusCode::PARTIAL_CONTENT,
        _ => StatusCode::OK,
    };

    Ok((code, headers, body))
}

fn get_range(headers: header::HeaderMap) -> (bool, u64, u64) {
    let mut stream_from = 0;
    let mut stream_to = 0;
    let mut found_range = false;

    for (k, v) in headers.iter() {
        println!("{} - {}", k, v.to_str().unwrap_or("???"));

        if k != "range" {
            continue;
        }

        if let Ok(value) = v.to_str() {
            (stream_from, stream_to) = get_offsets(value);
            found_range = true;
        }
    }

    (found_range, stream_from, stream_to)
}

fn get_offsets(offsets: &str) -> (u64, u64) {
    // TODO: add support for multiple ranges and end of file syntax
    let mut parts = offsets.splitn(2, '=');
    let mut range = parts.nth(1).unwrap().splitn(2, '-');

    let start = match range.next() {
        Some(start) => start.parse::<u64>().unwrap_or(0),
        None => 0,
    };

    let end = match range.next() {
        Some(end) => end.parse::<u64>().unwrap_or(0),
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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use axum::http::HeaderValue;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_file_sizes() -> Result<()> {
        let mut file = tokio::fs::File::open("tests/fixtures/media_dir/test.mp4").await?;

        let mut size = get_file_size(&mut file).await?;

        assert_eq!(size, 256);

        file = tokio::fs::File::open("tests/fixtures/media_dir/empty.mp4").await?;

        size = get_file_size(&mut file).await?;

        assert_eq!(size, 0);

        Ok(())
    }

    #[test]
    fn test_get_offsets() {
        let test_cases = [
            ("bytes=0-127", (0, 127)),
            ("bytes=0", (0, 0)),
            ("bytes=1000-2000", (1000, 2000)),
            ("bytes=5000", (5000, 0)),
        ];

        for (offsets, expected) in test_cases {
            let result = get_offsets(offsets);
            assert_eq!(result, expected);
        }
    }

    #[tokio::test]
    async fn test_headers_preserve_case() -> Result<()> {
        let result =
            stream_video("tests/fixtures/media_dir/test.mp4", header::HeaderMap::new()).await;

        let response = result.into_response();

        assert_eq!(response.status(), StatusCode::OK);

        let headers = response.headers();

        assert_eq!(headers.len(), 4);

        // HeaderMaps get() and contains_key() methods don't work with mixed case names.
        let header_map: HashMap<String, String> = headers
            .iter()
            .map(|h| (h.0.to_string(), h.1.to_str().unwrap().to_string()))
            .collect();

        assert_eq!(header_map.get("Content-Type").unwrap(), "video/mp4");
        assert!(!header_map.contains_key("content-type"));

        assert_eq!(header_map.get("Content-Length").unwrap(), "256");
        assert!(!header_map.contains_key("content-length"));

        assert_eq!(
            header_map.get("Content-Disposition").unwrap(),
            "attachment; filename=\"test.mp4\""
        );
        assert!(!header_map.contains_key("content-disposition"));

        assert_eq!(header_map.get("Content-Range").unwrap(), "bytes 0-255/256");
        assert!(!header_map.contains_key("content-range"));

        assert!(!header_map.contains_key("Accept-Ranges"));
        assert!(!header_map.contains_key("accept-ranges"));

        Ok(())
    }

    #[tokio::test]
    async fn test_headers_preserve_case_accept_range() -> Result<()> {
        let mut request_headers = header::HeaderMap::new();
        request_headers.insert("range", HeaderValue::from_static("bytes=0-127"));
        let result = stream_video("tests/fixtures/media_dir/test.mp4", request_headers).await;

        let response = result.into_response();

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);

        let headers = response.headers();

        assert_eq!(headers.len(), 4);

        // HeaderMaps get() and contains_key() methods don't work with mixed case names.
        let header_map: HashMap<String, String> = headers
            .iter()
            .map(|h| (h.0.to_string(), h.1.to_str().unwrap().to_string()))
            .collect();

        assert_eq!(header_map.get("Content-Type").unwrap(), "video/mp4");
        assert!(!header_map.contains_key("content-type"));

        assert!(!header_map.contains_key("Content-Length"));
        assert!(!header_map.contains_key("content-length"));

        assert_eq!(
            header_map.get("Content-Disposition").unwrap(),
            "attachment; filename=\"test.mp4\""
        );
        assert!(!header_map.contains_key("content-disposition"));

        assert_eq!(header_map.get("Content-Range").unwrap(), "bytes 0-127/256");
        assert!(!header_map.contains_key("content-range"));

        assert_eq!(header_map.get("Accept-Ranges").unwrap(), "bytes");
        assert!(!header_map.contains_key("accept-ranges"));

        Ok(())
    }
}*/
