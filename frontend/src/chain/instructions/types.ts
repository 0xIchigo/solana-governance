import { AnchorWallet } from "@solana/wallet-adapter-react";
import { PublicKey } from "@solana/web3.js";
import { BN } from "@coral-xyz/anchor";

import svmgovProgramIdl from "@/chain/idl/svmgov_program.json";
import govV1idl from "@/chain/idl/gov-v1.json";
import { RPCEndpoint } from "@/types";

// Common types
export interface TransactionResult {
  signature: string;
  success: boolean;
  error?: string;
}

export interface BlockchainParams {
  network: RPCEndpoint;
  endpoint: string;
  ncnApiUrl?: string;
}

// Instruction parameter types
export interface CreateProposalParams {
  title: string;
  description: string;
  seed?: number;
  wallet: AnchorWallet | undefined;
  voteAccount?: PublicKey;
}

export interface CastVoteParams {
  proposalId: string;
  forVotesBp: number;
  againstVotesBp: number;
  abstainVotesBp: number;
  wallet: AnchorWallet | undefined;
  voteAccount?: PublicKey;
  consensusResult: PublicKey;
}

export interface ModifyVoteParams {
  proposalId: string;
  forVotesBp: number;
  againstVotesBp: number;
  abstainVotesBp: number;
  wallet: AnchorWallet | undefined;
  voteAccount?: PublicKey;
  consensusResult: PublicKey;
}

export interface CastVoteOverrideParams {
  proposalId: string;
  forVotesBp: number;
  againstVotesBp: number;
  abstainVotesBp: number;
  stakeAccount: string;
  wallet: AnchorWallet | undefined;
  consensusResult: PublicKey;
}

export interface ModifyVoteOverrideParams {
  proposalId: string;
  forVotesBp: number;
  againstVotesBp: number;
  abstainVotesBp: number;
  stakeAccount: string;
  wallet: AnchorWallet | undefined;
  consensusResult: PublicKey;
}

export interface SupportProposalParams {
  proposalId: string;
  wallet: AnchorWallet | undefined;
  voteAccount?: PublicKey;
}

/** GlobalConfig fields needed to derive snapshot ballot PDAs when supporting a proposal (from governance config / hook). */
export interface SupportProposalGlobalConfigInput {
  discussionEpochs: number;
  snapshotEpochExtension: number;
  snapshotSlotOffset: number;
}

export interface AddMerkleRootParams {
  proposalId: string;
  merkleRootHash: string;
  wallet: AnchorWallet | undefined;
}

export interface FinalizeProposalParams {
  proposalId: string;
  wallet: AnchorWallet | undefined;
}

// API response types (based on solgov.online API)
export interface VoteAccountProofResponse {
  meta_merkle_leaf: {
    active_stake: number;
    stake_merkle_root: string;
    vote_account: string;
    voting_wallet: string;
  };
  meta_merkle_proof: string[];
  network: string;
  snapshot_slot: number;
}

export interface StakeMerkleLeafRaw {
  active_stake: number;
  stake_account: string;
  voting_wallet: string;
}

export interface StakeMerkleLeafConverted {
  activeStake: BN;
  stakeAccount: PublicKey;
  votingWallet: PublicKey;
}

export interface StakeAccountProofResponse {
  stake_merkle_leaf: StakeMerkleLeafRaw;
  stake_merkle_proof: string[];
  network: string;
  snapshot_slot: number;
  /**
   * The validator vote account this stake was delegated to AT SNAPSHOT TIME. This is the
   * authoritative vote account for an override vote: pairing the stake proof with the live
   * on-chain delegation instead breaks for redelegated stake. Sourced from the meta leaf at
   * snapshot upload time by the verifier service.
   */
  vote_account: string;
}

export interface ChainVoteAccountData {
  activeStake: number;
  voteAccount: string;
  nodePubkey: string;
}

export interface VoterSummaryResponse {
  network: string;
  snapshot_slot: number;
  voting_wallet: string;
  stake_accounts: {
    active_stake: number;
    stake_account: string;
    vote_account: string;
  }[];
  vote_accounts: {
    activeStake: number;
    voteAccount: string;
  }[];
}

export interface NetworkMetaResponse {
  network: string;
  slot: number;
  merkle_root: string;
  snapshot_hash: string;
  created_at: string;
}

// Constants
export const BASIS_POINTS_TOTAL = 10000;
export const SVMGOV_PROGRAM_ID = new PublicKey(svmgovProgramIdl.address);
export const SNAPSHOT_PROGRAM_ID = new PublicKey(govV1idl.address);

/**
 * Compute-unit limit requested for `support_proposal`, which re-tallies the
 * whole supporter list on every call (~20k CU + ~132 per supporter, so ~285k at
 * the 2000 cap) and outgrows the 200k default at 1347 supporters. Headroom
 * only: mainnet's ~800 validators cannot reach that.
 *
 * 600k is >2x the worst case at the cap and well under the 1.4M ceiling.
 * Priority fees price the requested limit rather than the amount consumed, so
 * right-size this off simulation if a compute-unit price is ever added.
 *
 * Measured by the program's `tests/support_compute_budget.rs`; keep in sync
 * with the copy in `svmgov/cli/src/constants.rs`.
 */
export const SUPPORT_COMPUTE_UNIT_LIMIT = 600_000;
