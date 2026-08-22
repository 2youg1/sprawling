// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The digest pipeline: what a long document looks like from outside.
//!
//! Three rules hold this module together, and each of them exists because
//! a summary is lossy and the loss happens before anyone can check it.
//!
//! - **A structure tree is mechanical.** Headings, their levels and their
//!   byte spans are read from the document itself, so this half of a
//!   digest is reproducible and never disagrees with the source.
//! - **Anything a model wrote is `suspect`.** Where a digest and the
//!   source conflict, the source wins: at the moment a conflict is
//!   noticed the only certain fact is that one of them is wrong, and the
//!   source is the one nobody rewrote.
//! - **A content hash is digested once in its life.** The cache is keyed
//!   by the hash of the bytes, so the same document costs one digest no
//!   matter how many runs meet it.
//!
//! The model call itself is the caller's: this module decides what to ask
//! for and what to trust, and `bin::assembly` owns the provider.

use kernel::{AxCode, AxError, B3Hash, ByteLen, Locator};
use serde::{Deserialize, Serialize};

/// One heading and the bytes beneath it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructureNode {
    /// Heading depth, 1 for `#`. Depth is the document's, not ours.
    pub level: u8,
    pub title: String,
    /// Byte offset of the heading line within the document.
    pub offset: ByteLen,
    /// Bytes from this heading up to the next heading of the same or a
    /// shallower level.
    pub span: ByteLen,
}

/// A digest of one document.
///
/// `prose` is present only when a model wrote one, and carries `suspect`
/// with it for as long as it exists. There is no method that clears the
/// flag: a summary does not stop being a summary by being read twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Digest {
    source: B3Hash,
    origin: Option<Locator>,
    structure: Vec<StructureNode>,
    prose: Option<String>,
}

impl Digest {
    /// The mechanical digest: structure only, and nothing to distrust.
    #[must_use]
    pub fn structural(source: B3Hash, origin: Option<Locator>, text: &str) -> Digest {
        Digest {
            source,
            origin,
            structure: structure_of(text),
            prose: None,
        }
    }

    /// Adds the prose a model wrote. It is suspect from here on, which is
    /// why this consumes and returns rather than mutating in place: the
    /// value that carries prose is a different value.
    #[must_use]
    pub fn with_prose(mut self, prose: String) -> Digest {
        self.prose = Some(prose);
        self
    }

    #[must_use]
    pub fn source(&self) -> B3Hash {
        self.source
    }

    #[must_use]
    pub fn origin(&self) -> Option<&Locator> {
        self.origin.as_ref()
    }

    #[must_use]
    pub fn structure(&self) -> &[StructureNode] {
        &self.structure
    }

    #[must_use]
    pub fn prose(&self) -> Option<&str> {
        self.prose.as_deref()
    }

    /// True while any part of this digest was written rather than read.
    #[must_use]
    pub fn is_suspect(&self) -> bool {
        self.prose.is_some()
    }

    /// How a digest introduces itself in a window. A suspect digest says
    /// so and says where the source is, so the reader can go and check
    /// rather than believing a paraphrase.
    #[must_use]
    pub fn window_header(&self) -> String {
        let origin = self
            .origin
            .as_ref()
            .map_or_else(|| self.source.to_string(), Locator::to_string);
        if self.is_suspect() {
            format!("digest of {origin} (suspect: written, not read; the source decides)")
        } else {
            format!("structure of {origin} (read from the source)")
        }
    }
}

/// Reads the heading structure of a markdown document.
///
/// Pure and total: a document with no headings has no structure, which is
/// a fact about the document rather than a failure. Fenced code blocks are
/// skipped, because a `#` comment inside a shell example is not a heading
/// and a structure tree that says otherwise would send a reader to a line
/// that does not exist.
#[must_use]
pub fn structure_of(text: &str) -> Vec<StructureNode> {
    let mut nodes: Vec<StructureNode> = Vec::new();
    let mut starts: Vec<(usize, usize)> = Vec::new(); // (node index, offset)
    let mut offset: usize = 0;
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            fenced = !fenced;
        } else if !fenced && trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            let title = trimmed
                .trim_start_matches('#')
                .trim_matches(|c: char| c == ' ' || c == '\n' || c == '\r')
                .to_owned();
            if (1..=6).contains(&level) && !title.is_empty() {
                let level = u8::try_from(level).unwrap_or(6);
                close_deeper(&mut nodes, &mut starts, level, offset);
                starts.push((nodes.len(), offset));
                nodes.push(StructureNode {
                    level,
                    title,
                    offset: ByteLen::new(u64::try_from(offset).unwrap_or(u64::MAX)),
                    span: ByteLen::new(0),
                });
            }
        }
        offset = offset.saturating_add(line.len());
    }
    close_deeper(&mut nodes, &mut starts, 0, offset);
    nodes
}

/// Closes every open heading at or deeper than `level`, giving each the
/// span from its own offset to `end`.
fn close_deeper(
    nodes: &mut [StructureNode],
    starts: &mut Vec<(usize, usize)>,
    level: u8,
    end: usize,
) {
    while let Some((index, start)) = starts.last().copied() {
        let Some(node) = nodes.get_mut(index) else {
            starts.pop();
            continue;
        };
        if level != 0 && node.level < level {
            break;
        }
        node.span = ByteLen::new(u64::try_from(end.saturating_sub(start)).unwrap_or(u64::MAX));
        starts.pop();
    }
}

/// The circuit breaker around a digester that can fail.
///
/// Counted, not timed: a wall clock inside a decision would break replay,
/// and "three failures in a row" is a fact the caller can reproduce.
/// Opening the breaker is not an error — it means the pipeline stops
/// asking and hands back the source, which is the answer that was always
/// correct and merely more expensive to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breaker {
    limit: u32,
    consecutive: u32,
}

/// What the pipeline should do next. Exhaustive on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakerVerdict {
    /// Ask the digester.
    Attempt,
    /// Stop asking and use the source as it is.
    Open { after: u32 },
}

impl Breaker {
    #[must_use]
    pub fn new(limit: u32) -> Breaker {
        Breaker {
            limit,
            consecutive: 0,
        }
    }

    #[must_use]
    pub fn verdict(&self) -> BreakerVerdict {
        if self.limit > 0 && self.consecutive >= self.limit {
            BreakerVerdict::Open {
                after: self.consecutive,
            }
        } else {
            BreakerVerdict::Attempt
        }
    }

    /// One digest came back. The count resets, because the failures this
    /// breaker cares about are consecutive ones: an intermittent provider
    /// is a different condition from a broken one.
    pub fn succeeded(&mut self) {
        self.consecutive = 0;
    }

    pub fn failed(&mut self) {
        self.consecutive = self.consecutive.saturating_add(1);
    }

    #[must_use]
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive
    }
}

/// What one pass over a document produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DigestOutcome {
    /// Read out of the cache; nothing was asked of anyone.
    Cached(Digest),
    /// Digested now, and worth writing to the cache under `source`.
    Fresh(Digest),
    /// The prose digester failed or was not attempted; the structural
    /// digest still stands, because reading headings cannot fail.
    Structural { digest: Digest, reason: AxError },
}

impl DigestOutcome {
    #[must_use]
    pub fn digest(&self) -> &Digest {
        match self {
            DigestOutcome::Cached(digest)
            | DigestOutcome::Fresh(digest)
            | DigestOutcome::Structural { digest, .. } => digest,
        }
    }
}

/// Digests one document, once in its life.
///
/// `cached` answers what the store already holds for this content hash;
/// `write_prose` is the model call, which the caller owns. The order is
/// the whole policy: hash, ask the cache, read the structure, and only
/// then spend a model call — and skip that call entirely once the breaker
/// is open.
///
/// # Errors
/// Propagates a cache read that fails. A prose failure is not an error
/// here: it becomes [`DigestOutcome::Structural`], because a document
/// whose summary failed is still a document with headings.
pub fn digest_once(
    text: &str,
    origin: Option<Locator>,
    breaker: &mut Breaker,
    cached: &mut dyn FnMut(&B3Hash) -> Result<Option<Digest>, AxError>,
    write_prose: &mut dyn FnMut(&str) -> Result<String, AxError>,
) -> Result<DigestOutcome, AxError> {
    let source = B3Hash::digest(text.as_bytes());
    if let Some(hit) = cached(&source)? {
        return Ok(DigestOutcome::Cached(hit));
    }
    let structural = Digest::structural(source, origin, text);
    if let BreakerVerdict::Open { after } = breaker.verdict() {
        return Ok(DigestOutcome::Structural {
            digest: structural,
            reason: AxError::failure(
                AxCode::DigestSuspect,
                "summarise a document",
                format!("the digester failed {after} times in a row"),
            )
            .with_recovery("read the structure and the source; prose resumes after one success"),
        });
    }
    match write_prose(text) {
        Ok(prose) => {
            breaker.succeeded();
            Ok(DigestOutcome::Fresh(structural.with_prose(prose)))
        }
        Err(reason) => {
            breaker.failed();
            Ok(DigestOutcome::Structural {
                digest: structural,
                reason,
            })
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]
mod tests {
    use super::*;

    const DOC: &str = "\
# Title

intro line

## First

body of first

```sh
# not a heading
```

## Second

body of second
";

    #[test]
    fn headings_become_a_tree_and_code_fences_do_not() {
        let nodes = structure_of(DOC);
        let titles: Vec<&str> = nodes.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles, ["Title", "First", "Second"]);
        assert_eq!(nodes[0].level, 1);
        assert_eq!(nodes[1].level, 2);
        // The top heading spans the whole document; a section spans up to
        // the next heading of its own depth.
        assert_eq!(usize::try_from(nodes[0].span.get()).unwrap(), DOC.len());
        assert!(nodes[1].span.get() < nodes[0].span.get());
    }

    #[test]
    fn a_document_without_headings_has_no_structure_rather_than_a_failure() {
        assert!(structure_of("just prose, no headings\n").is_empty());
    }

    #[test]
    fn prose_is_suspect_and_says_so_where_a_reader_will_see_it() {
        let digest = Digest::structural(B3Hash::digest(b"x"), None, DOC);
        assert!(!digest.is_suspect());
        assert!(digest.window_header().contains("read from the source"));

        let written = digest.with_prose("it is about three things".to_owned());
        assert!(written.is_suspect());
        assert!(written.window_header().contains("suspect"));
        assert!(written.window_header().contains("the source decides"));
    }

    #[test]
    fn a_content_hash_is_digested_once_in_its_life() {
        use std::cell::RefCell;

        let mut breaker = Breaker::new(3);
        let calls = RefCell::new(0u32);
        let store: RefCell<Option<Digest>> = RefCell::new(None);
        let first = {
            let mut cached = |hash: &B3Hash| -> Result<Option<Digest>, AxError> {
                Ok(store
                    .borrow()
                    .as_ref()
                    .filter(|d| &d.source() == hash)
                    .cloned())
            };
            let mut write = |_: &str| -> Result<String, AxError> {
                *calls.borrow_mut() += 1;
                Ok("a summary".to_owned())
            };
            digest_once(DOC, None, &mut breaker, &mut cached, &mut write).unwrap()
        };
        let DigestOutcome::Fresh(fresh) = first else {
            panic!("the first pass digests");
        };
        assert_eq!(*calls.borrow(), 1);

        *store.borrow_mut() = Some(fresh);
        let again = {
            let mut cached = |hash: &B3Hash| -> Result<Option<Digest>, AxError> {
                Ok(store
                    .borrow()
                    .as_ref()
                    .filter(|d| &d.source() == hash)
                    .cloned())
            };
            let mut write = |_: &str| -> Result<String, AxError> {
                *calls.borrow_mut() += 1;
                Ok("a summary".to_owned())
            };
            digest_once(DOC, None, &mut breaker, &mut cached, &mut write).unwrap()
        };
        assert!(matches!(again, DigestOutcome::Cached(_)));
        assert_eq!(*calls.borrow(), 1, "the same bytes cost one digest, ever");
    }

    #[test]
    fn the_breaker_opens_after_repeated_failure_and_the_structure_still_stands() {
        let mut breaker = Breaker::new(2);
        let mut attempts = 0u32;
        let mut cached = |_: &B3Hash| -> Result<Option<Digest>, AxError> { Ok(None) };
        let mut write = |_: &str| -> Result<String, AxError> {
            attempts += 1;
            Err(AxError::failure(
                AxCode::DigestSuspect,
                "summarise a document",
                "the provider refused",
            ))
        };

        for _ in 0..4 {
            let outcome = digest_once(DOC, None, &mut breaker, &mut cached, &mut write).unwrap();
            let DigestOutcome::Structural { digest, .. } = outcome else {
                panic!("a failing digester still leaves the headings");
            };
            assert!(
                !digest.is_suspect(),
                "nothing was written, nothing to doubt"
            );
            assert_eq!(digest.structure().len(), 3);
        }
        assert_eq!(attempts, 2, "the breaker stops asking after its limit");
        assert!(matches!(breaker.verdict(), BreakerVerdict::Open { .. }));

        // One success closes it again: intermittent is not broken.
        breaker.succeeded();
        assert_eq!(breaker.verdict(), BreakerVerdict::Attempt);
    }
}
