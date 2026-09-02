// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The client the browser downloads, and which bytes answer which path.
//!
//! Two sources, one contract: the release binary carries the bundle
//! inside itself, and a development loop points at the directory the
//! wasm build writes, read per request so an edit shows on refresh.
//! Which source is in use changes no policy, which is what makes this
//! an adapter rather than a decision.
//!
//! The whole of the path policy is here and none of it is in a handler:
//! `""` and `"/"` mean the index page, a path that steps outside the
//! bundle is a miss rather than an error, and the content type is the
//! one a browser needs to run a WebAssembly client. `channels::server`
//! adds headers to what this answers and decides nothing.

use kernel::{AxCode, AxError};

/// One file of the embedded client: the path a browser requests and the
/// gzip bytes the build wrote into the binary.
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedFile {
    /// Request path relative to the root, forward slashes, no leading
    /// slash: `index.html`, `web.js`, `snippets/<crate>/inline0.js`.
    pub path: &'static str,
    /// The file, gzip-compressed at build time with a zeroed timestamp,
    /// so the same source bytes embed identically on every build.
    pub gz: &'static [u8],
}

/// The client the browser downloads, from whichever source this process
/// was given. Two sources, one contract: the release binary carries the
/// bundle inside itself; a development loop points at the directory the
/// wasm build writes, read per request so an edit shows on refresh.
#[derive(Debug)]
pub enum ClientAssets {
    /// The bundle inside the binary, one gzip per file.
    Embedded(&'static [EmbeddedFile]),
    /// A directory on disk, read per request. Loopback development only;
    /// the release path never constructs this arm.
    Disk(std::path::PathBuf),
}

/// What one asset request gets back.
#[derive(Debug)]
pub enum AssetReply {
    Found {
        bytes: Vec<u8>,
        content_type: &'static str,
        /// Whether `bytes` are gzip and need `Content-Encoding: gzip`.
        gzipped: bool,
    },
    Miss(AxError),
}

impl ClientAssets {
    /// Answers one request path. Pure over the embedded arm; the disk arm
    /// reads exactly the file the sanitised path names.
    ///
    /// `""` and `"/"` mean `index.html`. A path that steps outside the
    /// bundle (`..`, empty segments, drive letters, leading dots) is a
    /// miss, not an error: the bundle is the whole world this route knows.
    #[must_use]
    pub fn lookup(&self, request_path: &str) -> AssetReply {
        let Some(rel) = sanitize_asset_path(request_path) else {
            return AssetReply::Miss(
                AxError::failure(
                    AxCode::InvalidArgs,
                    "serve a client asset",
                    format!("the path {request_path} steps outside the client bundle"),
                )
                .with_recovery("request a bundle-relative path such as /web.js"),
            );
        };
        match self {
            ClientAssets::Embedded(files) => {
                for file in *files {
                    if file.path == rel {
                        return AssetReply::Found {
                            bytes: file.gz.to_vec(),
                            content_type: content_type_of(&rel),
                            gzipped: true,
                        };
                    }
                }
                AssetReply::Miss(missing_asset(&rel, "this binary"))
            }
            ClientAssets::Disk(root) => {
                let full = root.join(&rel);
                match std::fs::read(&full) {
                    Ok(bytes) => AssetReply::Found {
                        bytes,
                        content_type: content_type_of(&rel),
                        gzipped: false,
                    },
                    Err(_) => AssetReply::Miss(missing_asset(&rel, "the --web-dir directory")),
                }
            }
        }
    }
}

fn missing_asset(rel: &str, source: &str) -> AxError {
    AxError::failure(
        AxCode::InvalidArgs,
        "serve a client asset",
        format!("{source} does not carry {rel}"),
    )
    .with_recovery("rebuild the client (`just build-web`), then rebuild or restart the server")
}

/// Normalises a request path to a bundle-relative one, or refuses.
/// Rejects `..`, empty segments, backslashes, drive colons and segments
/// that start with a dot; empty input means the index page.
fn sanitize_asset_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some("index.html".to_owned());
    }
    if trimmed.contains('\\') || trimmed.contains(':') {
        return None;
    }
    let mut segments = Vec::new();
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment.starts_with('.') {
            return None;
        }
        segments.push(segment);
    }
    Some(segments.join("/"))
}

/// The content type a browser needs to run the client. `application/wasm`
/// is load-bearing: `WebAssembly.instantiateStreaming` refuses anything
/// else and the loader falls back to a slower path with a console warning.
fn content_type_of(rel: &str) -> &'static str {
    let suffix = rel.rsplit('.').next().unwrap_or_default();
    match suffix {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::*;

    const BUNDLE: &[EmbeddedFile] = &[
        EmbeddedFile {
            path: "index.html",
            gz: b"gzipped-html",
        },
        EmbeddedFile {
            path: "web.js",
            gz: b"gzipped-js",
        },
        EmbeddedFile {
            path: "web_bg.wasm",
            gz: b"gzipped-wasm",
        },
        EmbeddedFile {
            path: "snippets/dioxus-abc/inline0.js",
            gz: b"gzipped-snippet",
        },
    ];

    #[test]
    fn the_embedded_bundle_answers_the_paths_the_page_asks_for() {
        let assets = ClientAssets::Embedded(BUNDLE);
        for (asked, want_type, want_bytes) in [
            ("", "text/html; charset=utf-8", b"gzipped-html".as_slice()),
            ("/", "text/html; charset=utf-8", b"gzipped-html".as_slice()),
            (
                "web.js",
                "text/javascript; charset=utf-8",
                b"gzipped-js".as_slice(),
            ),
            (
                "/web_bg.wasm",
                "application/wasm",
                b"gzipped-wasm".as_slice(),
            ),
            (
                "snippets/dioxus-abc/inline0.js",
                "text/javascript; charset=utf-8",
                b"gzipped-snippet".as_slice(),
            ),
        ] {
            let AssetReply::Found {
                bytes,
                content_type,
                gzipped,
            } = assets.lookup(asked)
            else {
                panic!("{asked} must be found");
            };
            assert_eq!(bytes, want_bytes, "{asked}");
            assert_eq!(content_type, want_type, "{asked}");
            assert!(gzipped, "embedded files travel gzipped");
        }
    }

    #[test]
    fn a_path_outside_the_bundle_is_a_miss_that_names_the_path() {
        let assets = ClientAssets::Embedded(BUNDLE);
        for hostile in [
            "../Cargo.toml",
            "a/../../secret",
            "a//b.js",
            "C:/windows/system32",
            "snippets\\x\\y.js",
            ".git/config",
        ] {
            let AssetReply::Miss(err) = assets.lookup(hostile) else {
                panic!("{hostile} must miss");
            };
            assert!(!err.recovery().is_empty(), "a miss says what to do next");
        }
    }

    #[test]
    fn an_unknown_file_misses_and_the_recovery_names_the_rebuild() {
        let assets = ClientAssets::Embedded(BUNDLE);
        let AssetReply::Miss(err) = assets.lookup("missing.js") else {
            panic!("an absent file is a miss");
        };
        assert!(err.recovery().contains("just build-web"));
    }

    #[test]
    fn the_disk_arm_reads_per_request_and_never_claims_gzip() {
        let dir = std::env::temp_dir().join(format!("sprawl-assets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("web.js"), b"fresh from disk").unwrap();
        let assets = ClientAssets::Disk(dir.clone());
        let AssetReply::Found {
            bytes,
            content_type,
            gzipped,
        } = assets.lookup("/web.js")
        else {
            panic!("the file exists");
        };
        assert_eq!(bytes, b"fresh from disk");
        assert_eq!(content_type, "text/javascript; charset=utf-8");
        assert!(!gzipped, "disk bytes are identity-encoded");
        std::fs::write(dir.join("web.js"), b"edited").unwrap();
        let AssetReply::Found { bytes, .. } = assets.lookup("web.js") else {
            panic!("still there");
        };
        assert_eq!(
            bytes, b"edited",
            "a refresh sees the edit without a restart"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
