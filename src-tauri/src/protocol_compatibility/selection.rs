use super::{ProbeReadiness, ReasoningSemantic, TransportKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStageStatus {
    Passed,
    Unsupported,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportProbeAssessment {
    pub transport: TransportKind,
    pub baseline: ProbeStageStatus,
    pub streaming: ProbeStageStatus,
    pub forced_tool: ProbeStageStatus,
    pub continuation: ProbeStageStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportSelection {
    pub transport: TransportKind,
    pub readiness: ProbeReadiness,
}

impl TransportProbeAssessment {
    pub fn is_complete(self) -> bool {
        matches!(
            (
                self.baseline,
                self.streaming,
                self.forced_tool,
                self.continuation,
            ),
            (
                ProbeStageStatus::Passed,
                ProbeStageStatus::Passed,
                ProbeStageStatus::Passed,
                ProbeStageStatus::Passed,
            )
        )
    }

    fn capability_score(self) -> Option<(bool, bool, bool)> {
        if self.baseline != ProbeStageStatus::Passed {
            return None;
        }
        Some((
            self.continuation == ProbeStageStatus::Passed,
            self.forced_tool == ProbeStageStatus::Passed,
            self.streaming == ProbeStageStatus::Passed,
        ))
    }
}

#[cfg(test)]
pub fn select_preferred_transport(
    assessments: &[TransportProbeAssessment],
) -> Option<TransportKind> {
    select_transport_outcome(assessments).map(|selection| selection.transport)
}

#[cfg(test)]
pub fn select_transport_outcome(
    assessments: &[TransportProbeAssessment],
) -> Option<TransportSelection> {
    let candidates = assessments
        .iter()
        .map(|assessment| (*assessment, ReasoningSemantic::None))
        .collect::<Vec<_>>();
    select_transport_outcome_with_reasoning(&candidates)
}

pub fn select_transport_outcome_with_reasoning(
    candidates: &[(TransportProbeAssessment, ReasoningSemantic)],
) -> Option<TransportSelection> {
    candidates
        .iter()
        .filter_map(|(assessment, semantic)| {
            assessment.capability_score().map(|capability| {
                (
                    (
                        capability.0,
                        capability.1,
                        capability.2,
                        codex_presentation_rank(*semantic),
                        assessment.transport == TransportKind::OpenAiResponses,
                    ),
                    *assessment,
                )
            })
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, assessment)| TransportSelection {
            transport: assessment.transport,
            readiness: if assessment.is_complete() {
                ProbeReadiness::Verified
            } else {
                ProbeReadiness::Partial
            },
        })
}

fn codex_presentation_rank(semantic: ReasoningSemantic) -> u8 {
    match semantic {
        ReasoningSemantic::Summary => 2,
        ReasoningSemantic::Readable => 1,
        ReasoningSemantic::Opaque | ReasoningSemantic::None => 0,
    }
}
