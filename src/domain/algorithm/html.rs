pub fn generate_video_html(video_url: &str, name: &str) -> String {
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
"#, name, video_url)
}
