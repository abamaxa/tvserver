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
}

const TO_MP4: Conversion = Conversion {
    name: "Convert to MP4",
    description: "Copies all video and audio streams to a new file in the MP4 file format",
    exec: "ffmpeg",
    args: "-i '{source}' -map 0 -c:v copy -c:a copy -y '{destination}'",
};

const TO_X265: Conversion = Conversion {
    name: "Encode Video with x265 codec",
    description:
        "Encodes the video stream with the x265 codec, all audio streams are copied unchanged",
    exec: "ffmpeg",
    args: "-i '{source}' -map 0 -c:v libx265 -vtag hvc1 -vprofile main -c:a copy -pix_fmt yuv420p -y '{destination}'",
};

const INCREASE_VOLUME: Conversion = Conversion {
    name: "Increase Volume by 10dB",
    description:
        "Increases the volume of all audio streams by 10dB, video streams are copied unchanged",
    exec: "ffmpeg",
    args: "-i '{source}' -filter:a volume=volume=10dB -y '{destination}'",
};

pub const AVAILABLE_CONVERSIONS: [Conversion; 3] = [TO_MP4, TO_X265, INCREASE_VOLUME];

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
        if let Some(destination) = get_next_version_name(source) {
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
