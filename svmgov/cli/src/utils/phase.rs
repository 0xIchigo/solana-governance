//! Proposal lifecycle phases, derived from on-chain state.
//!
//! The program fixes a proposal's schedule at the moment support crosses the
//! threshold: `activate_voting` sets `voting`, `start_epoch` and `end_epoch`
//! from the epoch the crossing happened in, which can be any epoch inside the
//! support window. A schedule recomputed from `creation_epoch` and the config
//! durations therefore only matches reality for a proposal that crosses on the
//! very last epoch of its window — for anything that crosses early it reports
//! the proposal as still gathering support long after it has advanced.
//!
//! So: once `voting` is set, the on-chain epochs are authoritative and the
//! config model is not consulted. It is used only to project a schedule for a
//! proposal that has not yet activated.

/// Fields needed to place a proposal in its lifecycle. Plain values rather than
/// the generated account types, so the logic is unit-testable.
#[derive(Debug, Clone, Copy)]
pub struct PhaseInputs {
    pub creation_epoch: u64,
    pub start_epoch: u64,
    pub end_epoch: u64,
    pub voting: bool,
    pub finalized: bool,
    pub max_support_epochs: u64,
    pub discussion_epochs: u64,
    pub snapshot_epoch_extension: u64,
    pub voting_epochs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalPhase {
    Support,
    Discussion,
    Snapshot,
    Voting,
    Ended,
    Finalized,
    Failed,
}

impl ProposalPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Support => "Support",
            Self::Discussion => "Discussion",
            Self::Snapshot => "Snapshot",
            Self::Voting => "Voting",
            Self::Ended => "Ended (awaiting finalization)",
            Self::Finalized => "Finalized",
            Self::Failed => "Failed (support threshold not reached)",
        }
    }

    /// Lowercase identifier used by `--status` filters and JSON output.
    pub fn id(self) -> &'static str {
        match self {
            Self::Support => "support",
            Self::Discussion => "discussion",
            Self::Snapshot => "snapshot",
            Self::Voting => "voting",
            Self::Ended => "ended",
            Self::Finalized => "finalized",
            Self::Failed => "failed",
        }
    }
}

pub fn proposal_phase(p: &PhaseInputs, current_epoch: u64) -> ProposalPhase {
    if p.finalized {
        return ProposalPhase::Finalized;
    }

    if p.voting {
        // Support succeeded and the program pinned the schedule. Nothing here
        // may fall back to the config model.
        if current_epoch >= p.end_epoch {
            return ProposalPhase::Ended;
        }
        if current_epoch >= p.start_epoch {
            return ProposalPhase::Voting;
        }
        // The snapshot is taken in the epoch immediately before voting opens.
        // Written as an addition so a start_epoch of 0 cannot underflow.
        if current_epoch + 1 >= p.start_epoch {
            return ProposalPhase::Snapshot;
        }
        return ProposalPhase::Discussion;
    }

    // Not activated. The program only accepts support inside
    // [creation_epoch, creation_epoch + max_support_epochs]; past that with
    // `voting` still unset, the proposal can never advance.
    if current_epoch <= p.creation_epoch.saturating_add(p.max_support_epochs) {
        ProposalPhase::Support
    } else {
        ProposalPhase::Failed
    }
}

/// Inclusive epoch ranges for each phase. `projected` marks a schedule inferred
/// from the config because the proposal has not activated yet — those epochs
/// will shift if it crosses the threshold earlier than the window's last epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseTimeline {
    pub support: (u64, u64),
    pub discussion: (u64, u64),
    pub snapshot: (u64, u64),
    pub voting: (u64, u64),
    pub projected: bool,
}

pub fn phase_timeline(p: &PhaseInputs) -> PhaseTimeline {
    // The epoch support crossed the threshold. `activate_voting` computes
    // start_epoch = crossing + discussion_epochs + snapshot_epoch_extension + 1,
    // so inverting it recovers the crossing epoch from on-chain state alone.
    let crossing = if p.voting {
        p.start_epoch
            .saturating_sub(1)
            .saturating_sub(p.discussion_epochs)
            .saturating_sub(p.snapshot_epoch_extension)
    } else {
        // Not activated: project the latest it could still cross.
        p.creation_epoch.saturating_add(p.max_support_epochs)
    };

    // Discussion begins in the crossing epoch itself: support closed partway
    // through it, so that epoch is the tail of support and the head of
    // discussion. `proposal_phase` reports Discussion there, and the timeline
    // has to agree. Running discussion to the epoch before the snapshot keeps
    // the two definitions in step for every epoch in between.
    let snapshot_epoch = crossing
        .saturating_add(p.discussion_epochs)
        .saturating_add(p.snapshot_epoch_extension);
    let discussion_start = crossing;
    let discussion_end = snapshot_epoch.saturating_sub(1);

    let (voting_start, voting_end) = if p.voting {
        (p.start_epoch, p.end_epoch)
    } else {
        let start = snapshot_epoch.saturating_add(1);
        (start, start.saturating_add(p.voting_epochs))
    };

    PhaseTimeline {
        support: (p.creation_epoch, crossing),
        discussion: (discussion_start, discussion_end),
        snapshot: (snapshot_epoch, snapshot_epoch),
        voting: (voting_start, voting_end),
        projected: !p.voting,
    }
}

/// How many epochs until the current phase gives way to the next.
pub fn epochs_remaining(p: &PhaseInputs, current_epoch: u64) -> Option<(u64, &'static str)> {
    let timeline = phase_timeline(p);
    match proposal_phase(p, current_epoch) {
        ProposalPhase::Support => Some((
            timeline.support.1.saturating_sub(current_epoch),
            "until the support window closes",
        )),
        ProposalPhase::Discussion => Some((
            timeline.snapshot.0.saturating_sub(current_epoch),
            "until snapshot",
        )),
        ProposalPhase::Snapshot => Some((
            timeline.voting.0.saturating_sub(current_epoch),
            "until voting opens",
        )),
        ProposalPhase::Voting => Some((
            p.end_epoch.saturating_sub(current_epoch),
            "until voting ends",
        )),
        ProposalPhase::Ended | ProposalPhase::Finalized | ProposalPhase::Failed => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Double Disinflation" as it stood on-chain in epoch 1012: created in
    /// 1011, crossed the threshold in 1012 — early in a seven-epoch support
    /// window — so the program scheduled voting for 1021-1024.
    fn double_disinflation() -> PhaseInputs {
        PhaseInputs {
            creation_epoch: 1011,
            start_epoch: 1021,
            end_epoch: 1024,
            voting: true,
            finalized: false,
            max_support_epochs: 7,
            discussion_epochs: 7,
            snapshot_epoch_extension: 1,
            voting_epochs: 3,
        }
    }

    #[test]
    fn an_early_crossing_is_not_reported_as_still_gathering_support() {
        // The reported bug: the CLI said "Support" with "6 epoch(s) until
        // discussion" for a proposal that had already advanced, because
        // current_epoch was still inside the config-derived support window.
        let p = double_disinflation();
        assert_eq!(proposal_phase(&p, 1012), ProposalPhase::Discussion);
        assert_eq!(
            epochs_remaining(&p, 1012).map(|(n, _)| n),
            Some(8) // 1020 - 1012
        );
    }

    #[test]
    fn recovers_the_crossing_epoch_from_on_chain_state() {
        // start_epoch inverts back to the epoch support actually crossed, so
        // the printed timeline matches reality instead of the config model.
        let t = phase_timeline(&double_disinflation());
        assert!(!t.projected);
        assert_eq!(t.support, (1011, 1012));
        // 1012 is shared: support closed partway through it and discussion
        // began. Anything else would contradict proposal_phase(1012).
        assert_eq!(t.discussion, (1012, 1019));
        assert_eq!(t.snapshot, (1020, 1020)); // snapshot_slot 440_641_000 / 432_000
        assert_eq!(t.voting, (1021, 1024));
    }

    #[test]
    fn the_timeline_agrees_with_the_phase_at_every_epoch() {
        // The bug this whole module exists to fix was a timeline that disagreed
        // with the reported status, so pin that they cannot drift apart.
        let p = double_disinflation();
        let t = phase_timeline(&p);
        for epoch in t.discussion.0..=t.discussion.1 {
            assert_eq!(
                proposal_phase(&p, epoch),
                ProposalPhase::Discussion,
                "epoch {epoch} is inside the discussion window"
            );
        }
        for epoch in t.snapshot.0..=t.snapshot.1 {
            assert_eq!(proposal_phase(&p, epoch), ProposalPhase::Snapshot);
        }
        for epoch in t.voting.0..t.voting.1 {
            assert_eq!(proposal_phase(&p, epoch), ProposalPhase::Voting);
        }
    }

    #[test]
    fn the_timeline_is_internally_ordered() {
        // The old output had voting (1021-1024) starting before the snapshot
        // (1026) and before discussion ended (1025).
        let t = phase_timeline(&double_disinflation());
        // Support and discussion share the crossing epoch; everything else is
        // strictly ordered.
        assert!(t.support.1 <= t.discussion.0);
        assert!(t.discussion.0 <= t.discussion.1);
        assert!(t.discussion.1 < t.snapshot.0);
        assert!(t.snapshot.1 < t.voting.0);
        assert!(t.voting.0 < t.voting.1);
    }

    #[test]
    fn walks_the_whole_lifecycle() {
        let p = double_disinflation();
        assert_eq!(proposal_phase(&p, 1011), ProposalPhase::Discussion);
        assert_eq!(proposal_phase(&p, 1019), ProposalPhase::Discussion);
        assert_eq!(proposal_phase(&p, 1020), ProposalPhase::Snapshot);
        assert_eq!(proposal_phase(&p, 1021), ProposalPhase::Voting);
        assert_eq!(proposal_phase(&p, 1023), ProposalPhase::Voting);
        assert_eq!(proposal_phase(&p, 1024), ProposalPhase::Ended);
    }

    #[test]
    fn an_unactivated_proposal_stays_in_support_until_its_window_closes() {
        let p = PhaseInputs {
            voting: false,
            start_epoch: 0,
            end_epoch: 0,
            ..double_disinflation()
        };
        assert_eq!(proposal_phase(&p, 1011), ProposalPhase::Support);
        assert_eq!(proposal_phase(&p, 1018), ProposalPhase::Support);
    }

    #[test]
    fn an_unactivated_proposal_past_its_window_has_failed() {
        // The old code reported "support" forever here. Support and retally
        // both reject past creation_epoch + max_support_epochs, so the proposal
        // can never advance.
        let p = PhaseInputs {
            voting: false,
            start_epoch: 0,
            end_epoch: 0,
            ..double_disinflation()
        };
        assert_eq!(proposal_phase(&p, 1019), ProposalPhase::Failed);
        assert_eq!(epochs_remaining(&p, 1019), None);
    }

    #[test]
    fn an_unactivated_timeline_is_marked_projected() {
        let p = PhaseInputs {
            voting: false,
            start_epoch: 0,
            end_epoch: 0,
            ..double_disinflation()
        };
        let t = phase_timeline(&p);
        assert!(t.projected);
        // Latest possible crossing is the last epoch of the support window.
        assert_eq!(t.support, (1011, 1018));
        assert_eq!(t.discussion, (1018, 1025));
        assert_eq!(t.snapshot, (1026, 1026));
        assert_eq!(t.voting, (1027, 1030));
    }

    #[test]
    fn finalized_wins_over_everything() {
        let p = PhaseInputs {
            finalized: true,
            ..double_disinflation()
        };
        assert_eq!(proposal_phase(&p, 1012), ProposalPhase::Finalized);
        assert_eq!(epochs_remaining(&p, 1012), None);
    }

    #[test]
    fn a_zero_start_epoch_does_not_underflow() {
        let p = PhaseInputs {
            voting: true,
            start_epoch: 0,
            end_epoch: 0,
            creation_epoch: 0,
            ..double_disinflation()
        };
        // end_epoch 0 means current >= end, so this is Ended rather than a panic.
        assert_eq!(proposal_phase(&p, 0), ProposalPhase::Ended);
        let _ = phase_timeline(&p);
    }
}
