fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&#x27;")
}

pub fn generate_video_html(video_url: &str, name: &str) -> String {
    let escaped_name = html_escape(name);
    let escaped_url = html_escape(video_url);
    format!(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{}</title>
    <style>
        body, html {{
            margin: 0;
            padding: 0;
            height: 100%;
            width: 100%;
            background-color: black;
            overflow: hidden;
        }}
        #videoContainer {{
            position: absolute;
            top: 0;
            left: 0;
            width: 100%;
            height: 100%;
            display: flex;
            justify-content: center;
            align-items: center;
        }}
        video {{
            max-width: 100%;
            max-height: 100%;
            width: auto;
            height: auto;
        }}
    </style>
</head>
<body>
    <div id="videoContainer">
        <video controls autoplay>
            <source src="{}" type="video/mp4">
            Your browser does not support the video tag.
        </video>
    </div>
    <script>
        var video = document.querySelector('video');
        video.addEventListener('loadedmetadata', function() {{
            if (video.videoHeight > video.videoWidth) {{
                video.style.height = '100%';
                video.style.width = 'auto';
            }} else {{
                video.style.width = '100%';
                video.style.height = 'auto';
            }}
        }});
    </script>
</body>
</html>
"#, escaped_name, escaped_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("hello"), "hello");
        assert_eq!(html_escape("<script>alert('xss')</script>"), "&lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;");
        assert_eq!(html_escape("a&b"), "a&amp;b");
        assert_eq!(html_escape(r#"a"b"#), "a&quot;b");
    }

    #[test]
    fn test_generate_video_html_escapes_xss() {
        let html = generate_video_html("/video.mp4", "<script>alert(1)</script>");
        assert!(!html.contains("<script>alert(1)</script>"));
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    }

    #[test]
    fn test_generate_video_html_escapes_url() {
        let html = generate_video_html(r#"" onload="alert(1)"#, "test");
        assert!(!html.contains(r#"" onload="alert(1)"#));
        assert!(html.contains("&quot; onload=&quot;alert(1)"));
    }
}
