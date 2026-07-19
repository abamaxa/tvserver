use serde::{Deserialize, Serialize, Serializer};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SaveBookProgressRequest {
    pub locator: RawBookLocator,
    pub progression: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RawBookLocator {
    #[serde(rename = "type")]
    pub locator_type: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookLocatorType {
    EpubCfi,
    PdfPage,
}

impl Serialize for BookLocatorType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            Self::EpubCfi => "epub-cfi",
            Self::PdfPage => "pdf-page",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BookLocator {
    #[serde(rename = "type")]
    pub locator_type: BookLocatorType,
    pub value: String,
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookProgress {
    #[serde(serialize_with = "serialize_checksum")]
    pub checksum: i64,
    pub locator: BookLocator,
    pub progression: Option<f64>,
    pub updated_on: String,
}

fn serialize_checksum<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.collect_str(value)
}

#[cfg(test)]
mod tests {
    use super::{
        BookLocator, BookLocatorType, BookProgress, RawBookLocator, SaveBookProgressRequest,
    };

    #[test]
    fn progress_serializes_full_width_checksum_and_utc_timestamp() {
        let progress = BookProgress {
            checksum: i64::MAX,
            locator: BookLocator {
                locator_type: BookLocatorType::EpubCfi,
                value: "epubcfi(/6/4!/4/2/8)".into(),
            },
            progression: Some(0.42),
            updated_on: "2026-07-19T12:00:00.000Z".into(),
        };

        assert_eq!(
            serde_json::to_value(progress).unwrap(),
            serde_json::json!({
                "checksum": "9223372036854775807",
                "locator": { "type": "epub-cfi", "value": "epubcfi(/6/4!/4/2/8)" },
                "progression": 0.42,
                "updatedOn": "2026-07-19T12:00:00.000Z"
            })
        );
    }

    #[test]
    fn locator_types_serialize_to_their_wire_values() {
        for (locator_type, wire_value) in
            [(BookLocatorType::EpubCfi, "epub-cfi"), (BookLocatorType::PdfPage, "pdf-page")]
        {
            let locator = BookLocator {
                locator_type,
                value: "location".into(),
            };

            assert_eq!(
                serde_json::to_value(locator).unwrap(),
                serde_json::json!({ "type": wire_value, "value": "location" })
            );
        }
    }

    #[test]
    fn progress_omits_absent_progression_and_request_keeps_raw_locator() {
        let progress = BookProgress {
            checksum: 42,
            locator: BookLocator {
                locator_type: BookLocatorType::PdfPage,
                value: "7".into(),
            },
            progression: None,
            updated_on: "2026-07-19T12:00:00.000Z".into(),
        };

        assert_eq!(
            serde_json::to_value(progress).unwrap(),
            serde_json::json!({
                "checksum": "42",
                "locator": { "type": "pdf-page", "value": "7" },
                "updatedOn": "2026-07-19T12:00:00.000Z"
            })
        );

        assert_eq!(
            serde_json::from_value::<SaveBookProgressRequest>(serde_json::json!({
                "locator": { "type": "future-locator", "value": "raw-value" }
            }))
            .unwrap(),
            SaveBookProgressRequest {
                locator: RawBookLocator {
                    locator_type: "future-locator".into(),
                    value: "raw-value".into(),
                },
                progression: None,
            }
        );
    }
}
