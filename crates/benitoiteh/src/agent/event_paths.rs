//! ハードリンクの通知の生のパスを JSON に残す（設計書 01-05「ハードリンクの表示」）。

use super::text;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum EventPath {
    Utf8(String),
    Hex { path_hex: String },
}

pub(super) fn serialize<S: Serializer>(
    paths: &[PathBuf],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let encoded = paths
        .iter()
        .map(|path| match path.to_str() {
            Some(path) => EventPath::Utf8(path.to_owned()),
            None => EventPath::Hex {
                path_hex: path
                    .as_os_str()
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            },
        })
        .collect::<Vec<_>>();
    encoded.serialize(serializer)
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<PathBuf>, D::Error> {
    Vec::<EventPath>::deserialize(deserializer)?
        .into_iter()
        .map(|path| match path {
            EventPath::Utf8(path) => Ok(path.into()),
            EventPath::Hex { path_hex } => {
                let (pairs, remainder) = path_hex.as_bytes().as_chunks::<2>();
                if !remainder.is_empty() {
                    return Err(D::Error::custom(text::INVALID_EVENT_PATH));
                }
                let bytes = pairs
                    .iter()
                    .map(|pair| {
                        let text = std::str::from_utf8(pair)
                            .map_err(|_| D::Error::custom(text::INVALID_EVENT_PATH))?;
                        u8::from_str_radix(text, 16)
                            .map_err(|_| D::Error::custom(super::text::INVALID_EVENT_PATH))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(PathBuf::from(OsString::from_vec(bytes)))
            }
        })
        .collect()
}
