use {
    borsh::{BorshDeserialize, BorshSerialize},
    litesvm::LiteSVM,
    sha2::{Digest, Sha256},
    solana_account::Account,
    solana_address::Address,
    solana_clock::Clock,
    solana_compute_budget_interface::ComputeBudgetInstruction,
    solana_instruction::{AccountMeta, Instruction},
    solana_instruction_error::InstructionError,
    solana_keypair::Keypair,
    solana_message::Message,
    solana_native_token::LAMPORTS_PER_SOL,
    solana_sdk_ids::{native_loader, system_program, vote},
    solana_signer::Signer,
    solana_transaction::Transaction,
    solana_transaction_error::TransactionError,
    solana_vote_interface_host::state::{VoteInit, VoteStateV3, VoteStateVersions},
    std::{collections::HashMap, path::PathBuf},
    svmgov_program::GovernanceError,
};

const SVMGOV_PROGRAM_ID: Address =
    Address::from_str_const("govYkyQ3ePtGULAtY6V75qjWE8UH4vCUVQ1W4HdCAZU");
const NCN_SNAPSHOT_PROGRAM_ID: Address =
    Address::from_str_const("ncnwF8AgynRcdEnGLcprSQNaKvgSMTgk3yPRc8cf9Zf");

const VALIDATOR_COUNT: usize = 1_000;
const SUPPORTER_COUNT: usize = 150;
const STAKE_PER_VALIDATOR: u64 = LAMPORTS_PER_SOL;
const CLUSTER_SUPPORT_PCT_MIN_BPS: u64 = 1_500; // 15%
const SLOTS_PER_EPOCH: u64 = 432_000;
const DISCUSSION_EPOCHS: u64 = 1;
const MAX_SUPPORT_EPOCHS: u64 = 10;
/// Anchor `#[error_code]` offset (`anchor_lang::error::ERROR_CODE_OFFSET`).
const ANCHOR_ERROR_CODE_OFFSET: u32 = 6000;

fn anchor_custom_error(err: GovernanceError) -> InstructionError {
    InstructionError::Custom(ANCHOR_ERROR_CODE_OFFSET + err as u32)
}

fn pk_bytes(a: &Address) -> [u8; 32] {
    a.to_bytes()
}

fn anchor_discriminator(namespace: &str, name: &str) -> [u8; 8] {
    let preimage = format!("{namespace}:{name}");
    let hash = Sha256::digest(preimage.as_bytes());
    let mut out = [0u8; 8];
    out.copy_from_slice(&hash[..8]);
    out
}

fn read_program() -> Vec<u8> {
    // CARGO_MANIFEST_DIR = programs/svmgov_program
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../target/deploy/svmgov_program.so");
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read {}: {e}. Build with: cargo-build-sbf -p svmgov_program",
            path.display()
        )
    })
}

#[derive(BorshSerialize)]
struct GlobalConfigAccount {
    admin: [u8; 32],
    pending_admin: Option<[u8; 32]>,
    max_title_length: u16,
    max_description_length: u16,
    max_support_epochs: u64,
    min_proposal_stake_lamports: u64,
    cluster_support_pct_min_bps: u64,
    discussion_epochs: u64,
    voting_epochs: u64,
    snapshot_epoch_extension: u64,
    snapshot_slot_offset: i64,
    bump: u8,
    max_supporters: u32,
}

#[derive(BorshSerialize)]
struct ProposalIndexAccount {
    current_index: u32,
    bump: u8,
}

#[derive(Debug, BorshDeserialize)]
#[allow(dead_code)]
struct ProposalAccount {
    author: [u8; 32],
    title: String,
    description: String,
    creation_epoch: u64,
    start_epoch: u64,
    end_epoch: u64,
    proposer_stake_weight_bp: u64,
    cluster_support_lamports: u64,
    for_votes_lamports: u64,
    against_votes_lamports: u64,
    abstain_votes_lamports: u64,
    voting: bool,
    finalized: bool,
    proposal_bump: u8,
    creation_timestamp: i64,
    vote_count: u32,
    index: u32,
    consensus_result: Option<[u8; 32]>,
    snapshot_slot: u64,
    proposal_seed: u64,
    vote_account_pubkey: [u8; 32],
    supporters: Vec<[u8; 32]>,
}

struct Validator {
    identity: Keypair,
    vote: Keypair,
}

struct Harness {
    svm: LiteSVM,
    validators: Vec<Validator>,
    global_config: Address,
    proposal_index: Address,
    program_config: Address,
}

fn make_vote_account_data(node: &Address) -> Vec<u8> {
    let vote_init = VoteInit {
        node_pubkey: *node,
        authorized_voter: *node,
        authorized_withdrawer: *node,
        commission: 0,
    };
    let state = VoteStateV3::new(&vote_init, &Clock::default());
    let versioned = VoteStateVersions::V3(Box::new(state));
    let mut data = vec![0u8; VoteStateV3::size_of()];
    VoteStateV3::serialize(&versioned, &mut data).expect("serialize VoteStateV3");
    data
}

fn write_anchor_account<T: BorshSerialize>(
    svm: &mut LiteSVM,
    address: Address,
    owner: Address,
    discriminator: [u8; 8],
    data: &T,
) {
    let mut bytes = discriminator.to_vec();
    bytes.extend(borsh::to_vec(data).expect("borsh serialize"));
    let lamports = svm.minimum_balance_for_rent_exemption(bytes.len());
    svm.set_account(
        address,
        Account {
            lamports,
            data: bytes,
            owner,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

fn try_send_ix(
    svm: &mut LiteSVM,
    payer: &Keypair,
    ixs: &[Instruction],
) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata> {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new(ixs, Some(&payer.pubkey()));
    let tx = Transaction::new(&[payer], msg, blockhash);
    svm.send_transaction(tx)
}

fn send_ix(
    svm: &mut LiteSVM,
    payer: &Keypair,
    ixs: &[Instruction],
) -> litesvm::types::TransactionMetadata {
    try_send_ix(svm, payer, ixs).unwrap_or_else(|e| {
        panic!("tx failed: {:#?}\nlogs: {:#?}", e.err, e.meta.logs);
    })
}

fn create_proposal_ix(
    author: &Address,
    proposal: Address,
    proposal_index: Address,
    vote_account: Address,
    global_config: Address,
    seed: u64,
    title: &str,
    description: &str,
) -> Instruction {
    let mut data = anchor_discriminator("global", "create_proposal").to_vec();
    data.extend(seed.to_le_bytes());
    data.extend((title.len() as u32).to_le_bytes());
    data.extend(title.as_bytes());
    data.extend((description.len() as u32).to_le_bytes());
    data.extend(description.as_bytes());

    Instruction {
        program_id: SVMGOV_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*author, true),
            AccountMeta::new(proposal, false),
            AccountMeta::new(proposal_index, false),
            AccountMeta::new_readonly(vote_account, false),
            AccountMeta::new_readonly(global_config, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    }
}

fn support_proposal_ix(
    supporter: &Address,
    proposal: Address,
    support: Address,
    vote_account: Address,
    ballot_box: Address,
    program_config: Address,
    global_config: Address,
) -> Instruction {
    Instruction {
        program_id: SVMGOV_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*supporter, true),
            AccountMeta::new(proposal, false),
            AccountMeta::new(support, false),
            AccountMeta::new_readonly(vote_account, false),
            AccountMeta::new(ballot_box, false),
            AccountMeta::new_readonly(NCN_SNAPSHOT_PROGRAM_ID, false),
            AccountMeta::new_readonly(program_config, false),
            AccountMeta::new_readonly(global_config, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: anchor_discriminator("global", "support_proposal").to_vec(),
    }
}

fn retally_support_ix(
    caller: &Address,
    proposal: Address,
    ballot_box: Address,
    program_config: Address,
    global_config: Address,
) -> Instruction {
    Instruction {
        program_id: SVMGOV_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*caller, true),
            AccountMeta::new(proposal, false),
            AccountMeta::new(ballot_box, false),
            AccountMeta::new_readonly(NCN_SNAPSHOT_PROGRAM_ID, false),
            AccountMeta::new_readonly(program_config, false),
            AccountMeta::new_readonly(global_config, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: anchor_discriminator("global", "retally_support").to_vec(),
    }
}

fn fetch_proposal(svm: &LiteSVM, proposal: &Address) -> ProposalAccount {
    let account = svm.get_account(proposal).expect("proposal account");
    assert!(account.data.len() > 8, "proposal too small");
    // Realloc leaves trailing capacity; ignore unread bytes after the Borsh payload.
    let mut data: &[u8] = &account.data[8..];
    ProposalAccount::deserialize(&mut data).expect("deserialize proposal")
}

fn expected_snapshot_slot(crossing_epoch: u64) -> u64 {
    // discussion_epochs=1, snapshot_epoch_extension=0, snapshot_slot_offset=0
    (crossing_epoch + DISCUSSION_EPOCHS) * SLOTS_PER_EPOCH
}

fn set_clock(svm: &mut LiteSVM, epoch: u64) {
    let mut clock = svm.get_sysvar::<Clock>();
    clock.epoch = epoch;
    // Stay inside the epoch so snapshot_slot (= next epoch start) remains in the future.
    clock.slot = epoch * SLOTS_PER_EPOCH + 1;
    svm.set_sysvar(&clock);
}

fn seed_ballot_box(svm: &mut LiteSVM, snapshot_slot: u64) -> Address {
    let (ballot_box, _) = Address::find_program_address(
        &[b"BallotBox", &snapshot_slot.to_le_bytes()],
        &NCN_SNAPSHOT_PROGRAM_ID,
    );
    if svm.get_account(&ballot_box).is_none() {
        svm.set_account(
            ballot_box,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(1),
                data: vec![1],
                owner: NCN_SNAPSHOT_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    }
    ballot_box
}

fn setup_harness(creation_epoch: u64) -> Harness {
    let mut svm = LiteSVM::new();
    svm.add_program(SVMGOV_PROGRAM_ID, &read_program()).unwrap();
    set_clock(&mut svm, creation_epoch);

    let (global_config, global_bump) =
        Address::find_program_address(&[b"global_config"], &SVMGOV_PROGRAM_ID);
    let (proposal_index, index_bump) =
        Address::find_program_address(&[b"index"], &SVMGOV_PROGRAM_ID);
    let (program_config, _) =
        Address::find_program_address(&[b"ProgramConfig"], &NCN_SNAPSHOT_PROGRAM_ID);

    write_anchor_account(
        &mut svm,
        global_config,
        SVMGOV_PROGRAM_ID,
        anchor_discriminator("account", "GlobalConfig"),
        &GlobalConfigAccount {
            admin: pk_bytes(&Address::new_unique()),
            pending_admin: None,
            max_title_length: 200,
            max_description_length: 500,
            max_support_epochs: MAX_SUPPORT_EPOCHS,
            min_proposal_stake_lamports: 0,
            cluster_support_pct_min_bps: CLUSTER_SUPPORT_PCT_MIN_BPS,
            discussion_epochs: DISCUSSION_EPOCHS,
            voting_epochs: 3,
            snapshot_epoch_extension: 0,
            snapshot_slot_offset: 0,
            bump: global_bump,
            max_supporters: 2000,
        },
    );
    write_anchor_account(
        &mut svm,
        proposal_index,
        SVMGOV_PROGRAM_ID,
        anchor_discriminator("account", "ProposalIndex"),
        &ProposalIndexAccount {
            current_index: 0,
            bump: index_bump,
        },
    );

    // Stand-ins so constraints pass; non-empty ballot box skips ncn_snapshot CPI.
    svm.set_account(
        NCN_SNAPSHOT_PROGRAM_ID,
        Account {
            lamports: 1,
            data: vec![0],
            owner: native_loader::ID,
            executable: true,
            rent_epoch: 0,
        },
    )
    .unwrap();
    svm.set_account(
        program_config,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(1),
            data: vec![0],
            owner: NCN_SNAPSHOT_PROGRAM_ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let validators: Vec<Validator> = (0..VALIDATOR_COUNT)
        .map(|_| Validator {
            identity: Keypair::new(),
            vote: Keypair::new(),
        })
        .collect();

    let mut stakes = HashMap::with_capacity(VALIDATOR_COUNT);
    for v in &validators {
        stakes.insert(v.vote.pubkey(), STAKE_PER_VALIDATOR);
    }
    svm.set_epoch_stakes(stakes).unwrap();

    for v in validators.iter().take(SUPPORTER_COUNT) {
        let data = make_vote_account_data(&v.identity.pubkey());
        let lamports = svm.minimum_balance_for_rent_exemption(data.len());
        svm.set_account(
            v.vote.pubkey(),
            Account {
                lamports,
                data,
                owner: vote::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        // Enough lamports for many proposals / reallocs across sub-tests.
        svm.airdrop(&v.identity.pubkey(), 100 * LAMPORTS_PER_SOL)
            .unwrap();
    }

    Harness {
        svm,
        validators,
        global_config,
        proposal_index,
        program_config,
    }
}

fn create_proposal(h: &mut Harness, seed: u64, title: &str) -> Address {
    let author = h.validators[0].identity.insecure_clone();
    let author_vote = h.validators[0].vote.pubkey();
    let (proposal, _) = Address::find_program_address(
        &[b"proposal", &seed.to_le_bytes(), author_vote.as_ref()],
        &SVMGOV_PROGRAM_ID,
    );
    send_ix(
        &mut h.svm,
        &author,
        &[create_proposal_ix(
            &author.pubkey(),
            proposal,
            h.proposal_index,
            author_vote,
            h.global_config,
            seed,
            title,
            "https://github.com/solana-foundation/solana-governance",
        )],
    );
    proposal
}

fn try_support_one(
    h: &mut Harness,
    proposal: Address,
    validator_idx: usize,
    ballot_box: Address,
) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata> {
    let identity = h.validators[validator_idx].identity.insecure_clone();
    let vote = h.validators[validator_idx].vote.pubkey();
    let (support, _) = Address::find_program_address(
        &[b"support", proposal.as_ref(), vote.as_ref()],
        &SVMGOV_PROGRAM_ID,
    );
    try_send_ix(
        &mut h.svm,
        &identity,
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            support_proposal_ix(
                &identity.pubkey(),
                proposal,
                support,
                vote,
                ballot_box,
                h.program_config,
                h.global_config,
            ),
        ],
    )
}

fn support_one(h: &mut Harness, proposal: Address, validator_idx: usize, ballot_box: Address) {
    try_support_one(h, proposal, validator_idx, ballot_box).unwrap_or_else(|e| {
        panic!("support failed: {:#?}\nlogs: {:#?}", e.err, e.meta.logs);
    });
}

fn try_retally_one(
    h: &mut Harness,
    proposal: Address,
    caller_idx: usize,
    ballot_box: Address,
) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata> {
    let caller = h.validators[caller_idx].identity.insecure_clone();
    try_send_ix(
        &mut h.svm,
        &caller,
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            retally_support_ix(
                &caller.pubkey(),
                proposal,
                ballot_box,
                h.program_config,
                h.global_config,
            ),
        ],
    )
}

fn retally_one(h: &mut Harness, proposal: Address, caller_idx: usize, ballot_box: Address) {
    try_retally_one(h, proposal, caller_idx, ballot_box).unwrap_or_else(|e| {
        panic!("retally failed: {:#?}\nlogs: {:#?}", e.err, e.meta.logs);
    });
}

fn assert_threshold_reached(proposal: &ProposalAccount, crossing_epoch: u64) {
    let expected_support = STAKE_PER_VALIDATOR * SUPPORTER_COUNT as u64;
    let cluster = STAKE_PER_VALIDATOR * VALIDATOR_COUNT as u64;
    assert_eq!(proposal.cluster_support_lamports, expected_support);
    assert_eq!(proposal.supporters.len(), SUPPORTER_COUNT);
    assert!(proposal.voting, "expected voting activated at 15% support");
    assert_eq!(expected_support * 100 / cluster, 15);
    assert_eq!(
        proposal.snapshot_slot,
        expected_snapshot_slot(crossing_epoch)
    );
}

/// Stake drift alone (no new supporter) can activate voting via `retally_support`.
///
/// 149 supporters leave the proposal under the 15% threshold; after one of those
/// supporters' epoch stake grows, a permissionless retally re-measures and flips
/// `voting` without another `support_proposal`.
#[test_log::test]
fn retally_support_activates_voting_after_stake_drift() {
    const CREATION_EPOCH: u64 = 10;
    const UNDER_THRESHOLD: usize = SUPPORTER_COUNT - 1; // 149 / 1000 = 14.9%
    let mut h = setup_harness(CREATION_EPOCH);

    // Placeholder ballot box for the under-threshold supports (activation skipped).
    let early_ballot = seed_ballot_box(&mut h.svm, expected_snapshot_slot(CREATION_EPOCH));
    let proposal = create_proposal(&mut h, 99, "retally after stake drift");

    for i in 0..UNDER_THRESHOLD {
        support_one(&mut h, proposal, i, early_ballot);
    }

    let before = fetch_proposal(&h.svm, &proposal);
    assert_eq!(before.supporters.len(), UNDER_THRESHOLD);
    assert_eq!(
        before.cluster_support_lamports,
        STAKE_PER_VALIDATOR * UNDER_THRESHOLD as u64
    );
    assert!(!before.voting);

    // Drift: +1 SOL on an existing supporter, −1 SOL on a non-supporter so the
    // cluster total stays 1000 SOL (threshold remains exactly 150 SOL). Bumping
    // without a matching decrease would lift the lamport threshold above 150 SOL
    // (1001e9 * 1500 / 10_000 = 150.15 SOL) and leave 150 SOL still short.
    let boosted = h.validators[0].vote.pubkey();
    let non_supporter = h.validators[VALIDATOR_COUNT - 1].vote.pubkey();
    h.svm
        .set_epoch_stake(boosted, 2 * STAKE_PER_VALIDATOR)
        .unwrap();
    h.svm.set_epoch_stake(non_supporter, 0).unwrap();
    assert_eq!(
        h.svm.epoch_total_stake(),
        STAKE_PER_VALIDATOR * VALIDATOR_COUNT as u64
    );

    // Retally in a later epoch inside the window (re-measure at current stake).
    let crossing_epoch = CREATION_EPOCH + 1;
    set_clock(&mut h.svm, crossing_epoch);
    let ballot_box = seed_ballot_box(&mut h.svm, expected_snapshot_slot(crossing_epoch));
    // Permissionless: any signer may call (use a non-supporter identity).
    retally_one(&mut h, proposal, UNDER_THRESHOLD, ballot_box);

    let after = fetch_proposal(&h.svm, &proposal);
    assert_eq!(after.supporters.len(), UNDER_THRESHOLD);
    assert_eq!(
        after.cluster_support_lamports,
        STAKE_PER_VALIDATOR * UNDER_THRESHOLD as u64 + STAKE_PER_VALIDATOR
    );
    assert!(
        after.voting,
        "retally should activate voting after stake drift"
    );
    assert_eq!(after.snapshot_slot, expected_snapshot_slot(crossing_epoch));
    assert_eq!(after.start_epoch, crossing_epoch + DISCUSSION_EPOCHS + 1);
}

/// `support_proposal` re-measures prior supporters at the current epoch before
/// adding the newcomer — so stake drift on an earlier supporter can be what
/// pushes the tally over the threshold (without `retally_support`).
#[test_log::test]
fn support_proposal_remeasures_prior_stake_across_epochs() {
    const CREATION_EPOCH: u64 = 10;
    // 148 @ 1 SOL; after +1 SOL drift on one prior, a 149th support remeasures
    // to 149 + 1 = 150. Stale (support-time) weights would only reach 149.
    const PRIOR: usize = SUPPORTER_COUNT - 2;
    let mut h = setup_harness(CREATION_EPOCH);
    let early_ballot = seed_ballot_box(&mut h.svm, expected_snapshot_slot(CREATION_EPOCH));
    let proposal = create_proposal(&mut h, 98, "remeasure on support");

    for i in 0..PRIOR {
        support_one(&mut h, proposal, i, early_ballot);
    }
    assert!(!fetch_proposal(&h.svm, &proposal).voting);

    h.svm
        .set_epoch_stake(h.validators[0].vote.pubkey(), 2 * STAKE_PER_VALIDATOR)
        .unwrap();
    h.svm
        .set_epoch_stake(h.validators[VALIDATOR_COUNT - 1].vote.pubkey(), 0)
        .unwrap();

    let crossing_epoch = CREATION_EPOCH + 1;
    set_clock(&mut h.svm, crossing_epoch);
    let ballot_box = seed_ballot_box(&mut h.svm, expected_snapshot_slot(crossing_epoch));
    support_one(&mut h, proposal, PRIOR, ballot_box);

    let after = fetch_proposal(&h.svm, &proposal);
    assert_eq!(after.supporters.len(), PRIOR + 1);
    assert_eq!(
        after.cluster_support_lamports,
        STAKE_PER_VALIDATOR * (PRIOR as u64 + 2) // 148 + boost + newcomer
    );
    assert!(after.voting);
    assert_eq!(after.snapshot_slot, expected_snapshot_slot(crossing_epoch));
}

/// Once voting is active, both `support_proposal` and `retally_support` return
/// `ProposalClosed`.
#[test_log::test]
fn support_and_retally_reject_after_voting_activated() {
    const CREATION_EPOCH: u64 = 10;
    const UNDER_THRESHOLD: usize = SUPPORTER_COUNT - 1;
    let mut h = setup_harness(CREATION_EPOCH);
    let ballot_box = seed_ballot_box(&mut h.svm, expected_snapshot_slot(CREATION_EPOCH));
    let proposal = create_proposal(&mut h, 97, "closed after voting");

    for i in 0..UNDER_THRESHOLD {
        support_one(&mut h, proposal, i, ballot_box);
    }
    // Cross threshold via stake drift + retally (leaves validator UNDER_THRESHOLD free).
    h.svm
        .set_epoch_stake(h.validators[0].vote.pubkey(), 2 * STAKE_PER_VALIDATOR)
        .unwrap();
    h.svm
        .set_epoch_stake(h.validators[VALIDATOR_COUNT - 1].vote.pubkey(), 0)
        .unwrap();
    retally_one(&mut h, proposal, UNDER_THRESHOLD, ballot_box);
    assert!(fetch_proposal(&h.svm, &proposal).voting);

    let closed = TransactionError::InstructionError(
        1,
        anchor_custom_error(GovernanceError::ProposalClosed),
    );
    let support_err = try_support_one(&mut h, proposal, UNDER_THRESHOLD, ballot_box)
        .expect_err("support after voting should fail");
    assert_eq!(support_err.err, closed);

    // Different caller than the activating retally (avoid LiteSVM AlreadyProcessed).
    let retally_err = try_retally_one(&mut h, proposal, 1, ballot_box)
        .expect_err("retally after voting should fail");
    assert_eq!(retally_err.err, closed);
}

/// Retally is rejected once the clock moves past the support window
/// (same inclusive bound as `support_proposal`).
#[test_log::test]
fn retally_support_rejects_after_support_window() {
    const CREATION_EPOCH: u64 = 10;
    let mut h = setup_harness(CREATION_EPOCH);
    let ballot_box = seed_ballot_box(&mut h.svm, expected_snapshot_slot(CREATION_EPOCH));
    let proposal = create_proposal(&mut h, 8, "expired retally window");

    // Need ≥1 supporter so the failure is the window check, not NoSupporters.
    support_one(&mut h, proposal, 0, ballot_box);

    // Last inclusive epoch still allows retally.
    set_clock(&mut h.svm, CREATION_EPOCH + MAX_SUPPORT_EPOCHS);
    retally_one(&mut h, proposal, 1, ballot_box);
    assert!(!fetch_proposal(&h.svm, &proposal).voting);

    // One epoch past the window end → SupportPeriodExpired.
    // Different caller so LiteSVM does not treat this as a duplicate tx.
    set_clock(&mut h.svm, CREATION_EPOCH + MAX_SUPPORT_EPOCHS + 1);
    let err = try_retally_one(&mut h, proposal, 2, ballot_box)
        .expect_err("retally after window end should fail");
    assert_eq!(
        err.err,
        TransactionError::InstructionError(
            1,
            anchor_custom_error(GovernanceError::SupportPeriodExpired),
        )
    );
    assert!(!fetch_proposal(&h.svm, &proposal).voting);
}

/// Support is rejected once the clock moves past
/// `creation_epoch + max_support_epochs` (inclusive window end).
#[test_log::test]
fn support_proposal_rejects_after_support_window() {
    const CREATION_EPOCH: u64 = 10;
    let mut h = setup_harness(CREATION_EPOCH);
    // Ballot box is unused on the failing path (activation never runs), but the
    // account meta is still required by the instruction.
    let ballot_box = seed_ballot_box(&mut h.svm, expected_snapshot_slot(CREATION_EPOCH));
    let proposal = create_proposal(&mut h, 7, "expired support window");

    // Last inclusive epoch of the window must still accept support.
    set_clock(&mut h.svm, CREATION_EPOCH + MAX_SUPPORT_EPOCHS);
    support_one(&mut h, proposal, 0, ballot_box);
    let mid = fetch_proposal(&h.svm, &proposal);
    assert_eq!(mid.supporters.len(), 1);
    assert!(!mid.voting);

    // One epoch past the window end → SupportPeriodExpired.
    // Ix index 1: compute-budget CU limit is instruction 0.
    set_clock(&mut h.svm, CREATION_EPOCH + MAX_SUPPORT_EPOCHS + 1);
    let err = try_support_one(&mut h, proposal, 1, ballot_box)
        .expect_err("support after window end should fail");
    assert_eq!(
        err.err,
        TransactionError::InstructionError(
            1,
            anchor_custom_error(GovernanceError::SupportPeriodExpired),
        )
    );

    let state = fetch_proposal(&h.svm, &proposal);
    assert_eq!(state.supporters.len(), 1, "failed support must not append");
    assert!(!state.voting);
}

/// This test has 15% show support in a single epoch
#[test_log::test]
fn support_proposal_reaches_threshold_at_150_of_1000() {
    const CREATION_EPOCH: u64 = 10;
    let mut h = setup_harness(CREATION_EPOCH);

    // Same-epoch activation: ballot box for crossing_epoch == creation_epoch.
    let ballot_box = seed_ballot_box(&mut h.svm, expected_snapshot_slot(CREATION_EPOCH));
    let proposal = create_proposal(&mut h, 42, "same-epoch support");

    for i in 0..SUPPORTER_COUNT {
        support_one(&mut h, proposal, i, ballot_box);
        if (i + 1) % 50 == 0 {
            println!("supported {}/{} (same epoch)", i + 1, SUPPORTER_COUNT);
        }
    }

    let state = fetch_proposal(&h.svm, &proposal);
    assert_eq!(state.creation_epoch, CREATION_EPOCH);
    assert_threshold_reached(&state, CREATION_EPOCH);
    println!(
        "ok same-epoch: {}/{} supported; voting={}",
        state.supporters.len(),
        VALIDATOR_COUNT,
        state.voting
    );
}

/// For each support span of 2..=10 epochs, create a fresh proposal and spread
/// 150 supports across that span so the threshold is crossed only on the last
/// epoch (re-tallying prior supporters each time under the current clock).
#[test_log::test]
fn support_proposal_reaches_threshold_across_2_to_10_epochs() {
    const CREATION_EPOCH: u64 = 100;

    for span in 2u64..=10 {
        let mut h = setup_harness(CREATION_EPOCH);
        let crossing_epoch = CREATION_EPOCH + span - 1;
        let ballot_box = seed_ballot_box(&mut h.svm, expected_snapshot_slot(crossing_epoch));
        let proposal = create_proposal(&mut h, 42, &format!("{span}-epoch support"));

        let mut next = 0usize;
        for offset in 0..span {
            let epoch = CREATION_EPOCH + offset;
            set_clock(&mut h.svm, epoch);

            let remaining_epochs = (span - offset) as usize;
            let remaining_supporters = SUPPORTER_COUNT - next;
            // Evenly drain the remaining quota; last epoch takes the rest so
            // the 150th (threshold) support lands on crossing_epoch.
            let batch = if offset + 1 == span {
                remaining_supporters
            } else {
                remaining_supporters / remaining_epochs
            };

            for idx in next..next + batch {
                support_one(&mut h, proposal, idx, ballot_box);
            }
            next += batch;

            let mid = fetch_proposal(&h.svm, &proposal);
            if offset + 1 < span {
                assert!(
                    !mid.voting,
                    "span={span}: voting must not activate before final epoch (epoch {epoch}, supporters={})",
                    mid.supporters.len()
                );
            }
        }
        assert_eq!(next, SUPPORTER_COUNT);

        let state = fetch_proposal(&h.svm, &proposal);
        assert_eq!(state.creation_epoch, CREATION_EPOCH);
        assert_threshold_reached(&state, crossing_epoch);
        // Schedule anchors on the crossing epoch (discussion_epochs=1).
        assert_eq!(state.start_epoch, crossing_epoch + DISCUSSION_EPOCHS + 1);
        assert_eq!(state.end_epoch, state.start_epoch + 3);

        println!(
            "ok span={span}: crossed at epoch {crossing_epoch}; supporters={}; snapshot_slot={}",
            state.supporters.len(),
            state.snapshot_slot
        );
    }
}
