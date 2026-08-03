//! Compute cost of the full supporter re-tally in `support_proposal` /
//! `retally_support`, which scales with `num_supporters` (capped at
//! `MAX_SUPPORTERS`) and outgrows the 200k CU default a single-instruction
//! transaction gets. Pins the limit the CLI and frontend request.

mod common;

use {
    common::*, solana_address::Address, solana_compute_budget_interface::ComputeBudgetInstruction,
    solana_instruction::Instruction, solana_instruction_error::InstructionError,
    solana_signer::Signer, solana_transaction_error::TransactionError,
};

/// Keep in sync with `SUPPORT_COMPUTE_UNIT_LIMIT` in svmgov/cli/src/constants.rs
/// and frontend/src/chain/instructions/types.ts.
const SUPPORT_COMPUTE_UNIT_LIMIT: u32 = 600_000;
/// Minimum multiple of the measured worst case the request must cover, so cost
/// drift trips CI well before it trips a caller.
const REQUIRED_HEADROOM_FACTOR: u64 = 2;
const DEFAULT_COMPUTE_UNITS_PER_IX: u32 = 200_000;

/// Tracks the supporter cap, so raising `MAX_SUPPORTERS_LIMIT` re-measures the
/// new worst case instead of silently testing the old one. Equal to the
/// supporter count, so the threshold lands at 100% and the list can be filled
/// without activating voting until the very last support.
const VALIDATOR_COUNT: usize = MAX_SUPPORTERS as usize;

fn support_ix_for(h: &Harness, proposal: Address, idx: usize, ballot_box: Address) -> Instruction {
    let vote = h.validators[idx].vote.pubkey();
    let (support, _) = Address::find_program_address(
        &[b"support", proposal.as_ref(), vote.as_ref()],
        &SVMGOV_PROGRAM_ID,
    );
    support_proposal_ix(
        &h.validators[idx].identity.pubkey(),
        proposal,
        support,
        vote,
        ballot_box,
        h.program_config,
        h.global_config,
    )
}

/// The default budget really does run out, so the client-side request is
/// load-bearing. If this ever fails to find a cliff, the re-tally got cheap
/// enough that the request is unnecessary — confirm that deliberately.
#[test_log::test]
fn support_without_compute_budget_request_exhausts_default_limit() {
    const CREATION_EPOCH: u64 = 10;
    let mut h = setup_harness(CREATION_EPOCH, VALIDATOR_COUNT, VALIDATOR_COUNT);
    let ballot_box = seed_ballot_box(&mut h.svm, expected_snapshot_slot(CREATION_EPOCH));
    let proposal = create_proposal(&mut h, 1, "default compute budget");

    let mut first_failing_count: Option<usize> = None;
    for i in 0..VALIDATOR_COUNT {
        let signer = h.validators[i].identity.insecure_clone();
        let ix = support_ix_for(&h, proposal, i, ballot_box);
        match try_send_ix(&mut h.svm, &signer, &[ix]) {
            Ok(_) => continue,
            Err(e) => {
                // Exhausting the meter inside the program surfaces as
                // ProgramFailedToComplete, not ComputationalBudgetExceeded.
                assert!(
                    matches!(
                        e.err,
                        TransactionError::InstructionError(
                            0,
                            InstructionError::ProgramFailedToComplete
                                | InstructionError::ComputationalBudgetExceeded
                        )
                    ) && e.meta.logs.iter().any(|l| {
                        l.contains(&format!(
                            "consumed {DEFAULT_COMPUTE_UNITS_PER_IX} of \
                             {DEFAULT_COMPUTE_UNITS_PER_IX} compute units"
                        ))
                    }),
                    "expected compute-budget exhaustion, got {:?}\nlogs: {:#?}",
                    e.err,
                    e.meta.logs
                );
                first_failing_count = Some(i);
                break;
            }
        }
    }

    let failed_at = first_failing_count
        .expect("default budget never ran out — the raised client limit may be unnecessary");
    println!(
        "default {DEFAULT_COMPUTE_UNITS_PER_IX} CU budget exhausted at {failed_at} supporters"
    );

    // Same call, same state, with the limit clients request.
    let signer = h.validators[failed_at].identity.insecure_clone();
    let ix = support_ix_for(&h, proposal, failed_at, ballot_box);
    send_ix(
        &mut h.svm,
        &signer,
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(SUPPORT_COMPUTE_UNIT_LIMIT),
            ix,
        ],
    );
    assert_eq!(
        fetch_proposal(&h.svm, &proposal).num_supporters as usize,
        failed_at + 1
    );
}

/// The requested limit covers the worst case: the last support at the cap,
/// which re-tallies every prior supporter and activates voting.
#[test_log::test]
fn support_at_max_supporters_fits_within_requested_limit() {
    const CREATION_EPOCH: u64 = 10;
    let mut h = setup_harness(CREATION_EPOCH, VALIDATOR_COUNT, VALIDATOR_COUNT);
    let ballot_box = seed_ballot_box(&mut h.svm, expected_snapshot_slot(CREATION_EPOCH));
    let proposal = create_proposal(&mut h, 2, "max supporters compute budget");

    let mut peak_cu = 0u64;
    for i in 0..VALIDATOR_COUNT {
        let meta = try_support_one(&mut h, proposal, i, ballot_box).unwrap_or_else(|e| {
            panic!("support #{i} failed: {:?}\nlogs: {:#?}", e.err, e.meta.logs)
        });
        peak_cu = peak_cu.max(meta.compute_units_consumed);
        if (i + 1) % 500 == 0 {
            println!(
                "supported {}/{VALIDATOR_COUNT} — {} CU",
                i + 1,
                meta.compute_units_consumed
            );
        }
    }

    let state = fetch_proposal(&h.svm, &proposal);
    assert_eq!(state.num_supporters as usize, VALIDATOR_COUNT);
    assert!(state.voting, "threshold should have activated voting");

    println!("peak at {VALIDATOR_COUNT} supporters: {peak_cu} CU of {SUPPORT_COMPUTE_UNIT_LIMIT}");
    assert!(
        peak_cu * REQUIRED_HEADROOM_FACTOR < SUPPORT_COMPUTE_UNIT_LIMIT as u64,
        "support at the cap consumed {peak_cu} CU, under {REQUIRED_HEADROOM_FACTOR}x headroom of \
         the {SUPPORT_COMPUTE_UNIT_LIMIT} clients request. Raise SUPPORT_COMPUTE_UNIT_LIMIT in \
         svmgov/cli/src/constants.rs AND frontend/src/chain/instructions/types.ts (ceiling is \
         1_400_000), or make the re-tally cheaper."
    );
}

/// `retally_support` walks the same list, so it carries the same requirement.
#[test_log::test]
fn retally_at_max_supporters_fits_within_requested_limit() {
    const CREATION_EPOCH: u64 = 10;
    let mut h = setup_harness(CREATION_EPOCH, VALIDATOR_COUNT, VALIDATOR_COUNT);
    let ballot_box = seed_ballot_box(&mut h.svm, expected_snapshot_slot(CREATION_EPOCH));
    let proposal = create_proposal(&mut h, 3, "max supporters retally budget");

    // One short of the cap, so the retally measures a full list without
    // activating voting.
    for i in 0..VALIDATOR_COUNT - 1 {
        support_one(&mut h, proposal, i, ballot_box);
    }

    let meta = try_retally_one(&mut h, proposal, 0, ballot_box)
        .unwrap_or_else(|e| panic!("retally failed: {:?}\nlogs: {:#?}", e.err, e.meta.logs));
    println!(
        "retally over {} supporters: {} CU",
        VALIDATOR_COUNT - 1,
        meta.compute_units_consumed
    );
    assert!(
        meta.compute_units_consumed * REQUIRED_HEADROOM_FACTOR < SUPPORT_COMPUTE_UNIT_LIMIT as u64,
        "retally consumed {} CU, under {REQUIRED_HEADROOM_FACTOR}x headroom of the \
         {SUPPORT_COMPUTE_UNIT_LIMIT} clients request",
        meta.compute_units_consumed
    );
}
