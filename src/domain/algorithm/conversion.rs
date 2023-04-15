use crate::domain::algorithm::get_next_version_name;
use crate::domain::traits::{ProcessSpawner, Task};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversion {
    name: &'static str,
    description: &'static str,
    exec: &'static str,
    args: &'static str,
    extension: Option<&'static str>,
}

const TO_MP4: Conversion = Conversion {
    name: "Convert to MP4",
    description: "Copies all video and audio streams to a new file in the MP4 file format",
    exec: "ffmpeg",
    args: "-i '{source}' -map 0 -c:v copy -c:a copy -y '{destination}'",
    extension: Some("mp4"),
};

const TO_X265: Conversion = Conversion {
    name: "Encode Video with x265 codec",
    description:
        "Encodes the video stream with the x265 codec, all audio streams are copied unchanged",
    exec: "ffmpeg",
    args: "-i '{source}' -map 0 -c:v libx265 -vtag hvc1 -vprofile main -c:a copy -pix_fmt yuv420p -y '{destination}'",
    extension: None,
};

const INCREASE_VOLUME: Conversion = Conversion {
    name: "Increase Volume by 10dB",
    description:
        "Increases the volume of all audio streams by 10dB, video streams are copied unchanged",
    exec: "ffmpeg",
    args: "-i '{source}' -filter:a volume=volume=10dB -y '{destination}'",
    extension: None,
};

const TO_H264_AAC_MP4: Conversion = Conversion {
    name: "Convert to H.264, AAC + MP4",
    description: "Encodes video with H.264 codec, audio with AAC codec and saves as MP4 file",
    exec: "ffmpeg",
    args: "-i '{source}' -c:v libx264 -profile:v high -level 4.0 -preset slow -crf 22 -c:a aac -b:a 128k -movflags +faststart -y '{destination}'",
    extension: Some("mp4"),
};

const TO_H265_AAC_MP4: Conversion = Conversion {
    name: "Convert to H.265, AAC + MP4",
    description: "Encodes video with H.265 codec, audio with AAC codec and saves as MP4 file",
    exec: "ffmpeg",
    args: "-i '{source}' ffmpeg -i input_video.ext -c:v libx265 -profile:v main -crf 28 -c:a aac -b:a 128k -movflags +faststart -y '{destination}'",
    extension: Some("mp4"),
};

const TO_MPEG4_AAC_MP4: Conversion = Conversion {
    name: "Convert to MPEG-4, AAC + MP4",
    description:
        "Encodes video with MPEG-4 Part 2 codec, audio with AAC codec and saves as MP4 file",
    exec: "ffmpeg",
    args:
        "-i '{source}' -c:v mpeg4 -q:v 5 -c:a aac -b:a 128k -movflags +faststart -y '{destination}'",
    extension: Some("mp4"),
};

const TO_VP9_AAC_MKV: Conversion = Conversion {
    name: "Convert to VP9, AAC + MKV",
    description: "Encodes video with VP9 codec, audio with AAC codec and saves as MKV file",
    exec: "ffmpeg",
    args:
        "-i '{source}' -c:v libvpx-vp9 -crf 30 -b:v 0 -c:a libopus -b:a 128k -movflags +faststart -y '{destination}'",
    extension: Some("mkv"),
};

pub const AVAILABLE_CONVERSIONS: [Conversion; 7] = [
    TO_MP4,
    TO_X265,
    INCREASE_VOLUME,
    TO_H264_AAC_MP4,
    TO_H265_AAC_MP4,
    TO_MPEG4_AAC_MP4,
    TO_VP9_AAC_MKV,
];

impl Conversion {
    pub fn find(name: &str) -> Option<&Conversion> {
        AVAILABLE_CONVERSIONS
            .iter()
            .filter(|c| c.name == name)
            .collect::<Vec<&Conversion>>()
            .first()
            .copied()
    }

    pub async fn execute(&self, spawner: Arc<impl ProcessSpawner>, source: &str) -> Option<Task> {
        if let Some(destination) = get_next_version_name(source, self.extension) {
            let args = self.make_args(source, &destination);
            Some(spawner.execute(self.name, self.exec, args).await)
        } else {
            None
        }
    }

    fn make_args<'a>(&'a self, source: &'a str, destination: &'a str) -> Vec<&str> {
        self.args
            .split(' ')
            .map(|arg| match arg {
                "'{source}'" => source,
                "'{destination}'" => destination,
                _ => arg,
            })
            .collect()
    }
}
