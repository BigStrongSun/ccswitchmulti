use super::{ProbeReadiness, TransportKind};
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

    fn suitability_score(self) -> Option<(bool, bool, bool, bool)> {
        if self.baseline != ProbeStageStatus::Passed {
            return None;
        }
        Some((
            self.continuation == ProbeStageStatus::Passed,
            self.forced_tool == ProbeStageStatus::Passed,
            self.streaming == ProbeStageStatus::Passed,
            self.transport == TransportKind::OpenAiResponses,
        ))
    }
}

#[cfg(test)]
pub fn select_preferred_transport(
    assessments: &[TransportProbeAssessment],
) -> Option<TransportKind> {
    select_transport_outcome(assessments).map(|selection| selection.transport)
}

pub fn select_transport_outcome(
    assessments: &[TransportProbeAssessment],
) -> Option<TransportSelection> {
    assessments
        .iter()
        .filter_map(|assessment| {
            assessment
                .suitability_score()
                .map(|score| (score, assessment.transport))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, transport)| TransportSelection {
            transport,
            readiness: assessments
                .iter()
                .find(|assessment| assessment.transport == transport)
                .filter(|assessment| assessment.is_complete())
                .map_or(ProbeReadiness::Partial, |_| ProbeReadiness::Verified),
        })
}
