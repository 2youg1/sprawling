// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! A versioned probe: the same questions, asked twice, to see whether
//! something survived a boundary.
//!
//! The first object is the Handoff, and the shape comes from what that
//! has to prove: ask before the freeze, ask after the resume, compare.
//! If the answers differ, the handoff lost something, and the probe says
//! which question lost it rather than reporting a score.
//!
//! **Probes are versioned and results never mix across versions.** A
//! probe whose questions changed is a different instrument, and
//! comparing its readings with the old one measures the instrument. The
//! comparison refuses it rather than averaging it away.

use kernel::{AxCode, AxError};

/// A probe's identity and edition. Both are compared; neither is a
/// timestamp, because two probes built on the same day are still two
/// probes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeId {
    pub name: String,
    pub version: u32,
}

/// The questions, in the order they are asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    id: ProbeId,
    questions: Vec<String>,
}

impl Probe {
    /// # Errors
    /// Refuses a probe with no questions, and one with a blank question:
    /// a blank cannot be answered, so it would read as a loss on every
    /// comparison for as long as it stayed in the list.
    pub fn new(id: ProbeId, questions: Vec<String>) -> Result<Probe, AxError> {
        if questions.is_empty() {
            return Err(AxError::failure(
                AxCode::ConfigInvalid,
                "build a probe",
                format!("{} v{} asks nothing", id.name, id.version),
            )
            .with_recovery("ask at least one question, or do not build the probe"));
        }
        if questions.iter().any(|question| question.trim().is_empty()) {
            return Err(AxError::failure(
                AxCode::ConfigInvalid,
                "build a probe",
                format!("{} v{} has a blank question", id.name, id.version),
            )
            .with_recovery(
                "remove the blank; a question nobody can answer always reads as a loss",
            ));
        }
        Ok(Probe { id, questions })
    }

    #[must_use]
    pub fn id(&self) -> &ProbeId {
        &self.id
    }

    #[must_use]
    pub fn questions(&self) -> &[String] {
        &self.questions
    }

    /// Binds answers to this probe. The probe does not gather them: what
    /// asks the questions is a Run, and this crate is never in the loop
    /// that produces the material it measures.
    ///
    /// # Errors
    /// Refuses a set of answers that is not the same length as the
    /// questions — a shorter one would silently compare question three
    /// against question four.
    pub fn answered(&self, answers: Vec<String>) -> Result<Answers, AxError> {
        if answers.len() != self.questions.len() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "record probe answers",
                format!(
                    "{} answers for {} questions",
                    answers.len(),
                    self.questions.len()
                ),
            )
            .with_recovery(
                "answer every question, in order; an unanswered one is an empty string",
            ));
        }
        Ok(Answers {
            id: self.id.clone(),
            answers,
        })
    }
}

/// One reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answers {
    id: ProbeId,
    answers: Vec<String>,
}

impl Answers {
    #[must_use]
    pub fn id(&self) -> &ProbeId {
        &self.id
    }
}

/// What survived and what did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    pub kept: u32,
    /// The indices whose answers differ, in order. Indices rather than
    /// text: the probe says where the loss is, and the person reads the
    /// two answers themselves rather than trusting a summary of them.
    pub lost: Vec<u32>,
}

impl Comparison {
    #[must_use]
    pub fn intact(&self) -> bool {
        self.lost.is_empty()
    }
}

/// Compares two readings of the same probe.
///
/// # Errors
/// Refuses two different probes and two editions of one probe. Mixing
/// them measures the instrument rather than the thing.
pub fn compare(before: &Answers, after: &Answers) -> Result<Comparison, AxError> {
    if before.id != after.id {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "compare probe readings",
            format!(
                "{} v{} against {} v{}",
                before.id.name, before.id.version, after.id.name, after.id.version
            ),
        )
        .with_recovery(
            "compare readings of one edition; a probe whose questions changed is a different \
             instrument, and mixing the two measures the instrument",
        ));
    }
    let mut kept: u32 = 0;
    let mut lost = Vec::new();
    for (index, (was, now)) in before.answers.iter().zip(after.answers.iter()).enumerate() {
        let position = u32::try_from(index).unwrap_or(u32::MAX);
        if was.trim() == now.trim() {
            kept = kept.saturating_add(1);
        } else {
            lost.push(position);
        }
    }
    Ok(Comparison { kept, lost })
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

    fn handoff_probe(version: u32) -> Probe {
        Probe::new(
            ProbeId {
                name: "handoff".to_owned(),
                version,
            },
            vec![
                "what is the next step".to_owned(),
                "which card is open".to_owned(),
                "what did the last red run find".to_owned(),
            ],
        )
        .unwrap()
    }

    #[test]
    fn a_handoff_that_carried_everything_reads_as_intact() {
        let probe = handoff_probe(1);
        let answers = vec![
            "wire the pr tool".to_owned(),
            "P3.02".to_owned(),
            "the trunk moved under a waiting request".to_owned(),
        ];
        let before = probe.answered(answers.clone()).unwrap();
        let after = probe.answered(answers).unwrap();
        let comparison = compare(&before, &after).unwrap();
        assert!(comparison.intact());
        assert_eq!(comparison.kept, 3);
    }

    #[test]
    fn a_loss_is_reported_by_position_rather_than_as_a_score() {
        let probe = handoff_probe(1);
        let before = probe
            .answered(vec![
                "wire the pr tool".to_owned(),
                "P3.02".to_owned(),
                "the trunk moved".to_owned(),
            ])
            .unwrap();
        let after = probe
            .answered(vec![
                "wire the pr tool".to_owned(),
                "P3.02".to_owned(),
                String::new(),
            ])
            .unwrap();
        let comparison = compare(&before, &after).unwrap();
        assert!(!comparison.intact());
        assert_eq!(comparison.lost, vec![2]);
        assert_eq!(comparison.kept, 2);
    }

    #[test]
    fn two_editions_are_two_instruments() {
        let first = handoff_probe(1);
        let second = handoff_probe(2);
        let before = first
            .answered(vec!["a".to_owned(), "b".to_owned(), "c".to_owned()])
            .unwrap();
        let after = second
            .answered(vec!["a".to_owned(), "b".to_owned(), "c".to_owned()])
            .unwrap();
        let refused = compare(&before, &after).unwrap_err();
        assert_eq!(refused.code(), &AxCode::InvalidArgs);
        assert!(refused.recovery().contains("instrument"));
    }

    #[test]
    fn answers_must_line_up_with_questions() {
        let probe = handoff_probe(1);
        assert!(probe.answered(vec!["a".to_owned()]).is_err());
        assert!(
            Probe::new(
                ProbeId {
                    name: "empty".to_owned(),
                    version: 1
                },
                Vec::new()
            )
            .is_err()
        );
    }
}
