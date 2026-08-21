/**
 * Quorum as SGP-0001 Article IV.3 defines it: a vote is a valid signal only
 * once one-third of network stake has participated, participation being the
 * sum of `For`, `Against` and `Abstain`.
 *
 * Article IV.4's supermajority test is deliberately not implemented here. Its
 * denominator is under discussion on the SGP-0001 pull request — whether
 * `Abstain` belongs in it at all — so rendering a pass/fail verdict would mean
 * picking a side in an unresolved argument and changing it mid-vote. Quorum is
 * unaffected by that discussion: both readings count `Abstain` toward it.
 */
export const QUORUM_NUMERATOR = 1;
export const QUORUM_DENOMINATOR = 3;

/** Required participation as a fraction of network stake (1/3). */
export const QUORUM_FRACTION = QUORUM_NUMERATOR / QUORUM_DENOMINATOR;

export interface QuorumInput {
  forLamports: number;
  againstLamports: number;
  abstainLamports: number;
  /**
   * Total active stake in the snapshot the proposal votes against, from the
   * verifier's `/meta`.
   *
   * `undefined` when the snapshot predates the verifier recording it, and it
   * cannot be back-filled without the original upload. Article IV.2 fixes the
   * stake distribution at the snapshot for the whole voting period, so a live
   * cluster total is not a substitute: it drifts as stake moves and would make
   * the percentage change for reasons unrelated to voting.
   */
  totalActiveStake: number | undefined;
}

/**
 * Quorum with the denominator's absence modelled explicitly.
 *
 * `known: false` is not the same as zero participation. Collapsing the two
 * would report "0% participation, quorum not met" for a vote that may well
 * have met it, which reads as a real outcome rather than missing data.
 */
export type QuorumStatus =
  | { known: false }
  | {
      known: true;
      /** `For + Against + Abstain`, in lamports. */
      participatingLamports: number;
      totalActiveStake: number;
      /** Participation as a percentage of network stake, 0–100. */
      participationPercent: number;
      /** Participation needed to reach one-third, in lamports. */
      requiredLamports: number;
      /** Still needed to reach quorum, in lamports; `0` once met. */
      remainingLamports: number;
      isMet: boolean;
    };

export function computeQuorum({
  forLamports,
  againstLamports,
  abstainLamports,
  totalActiveStake,
}: QuorumInput): QuorumStatus {
  // A zero total is unusable as a denominator and means the same thing as a
  // missing one: nothing can be said about participation.
  if (totalActiveStake === undefined || totalActiveStake <= 0) {
    return { known: false };
  }

  const participatingLamports = forLamports + againstLamports + abstainLamports;
  const requiredLamports =
    (totalActiveStake * QUORUM_NUMERATOR) / QUORUM_DENOMINATOR;

  return {
    known: true,
    participatingLamports,
    totalActiveStake,
    participationPercent: (participatingLamports / totalActiveStake) * 100,
    requiredLamports,
    remainingLamports: Math.max(0, requiredLamports - participatingLamports),
    // Compared in lamports rather than on the rounded percentage, so a vote
    // near the threshold is not decided by how many digits are displayed.
    //
    // Lamport totals at mainnet scale (~1.6e17) exceed Number.MAX_SAFE_INTEGER,
    // so this cannot resolve differences below ~64 lamports — 6.4e-8 SOL. That
    // is the precision the rest of the app already carries (every *Lamports
    // field is a number), and it is far finer than any margin a vote turns on.
    isMet: participatingLamports >= requiredLamports,
  };
}

/** The `/meta` fields quorum needs; a subset so tests need not build the rest. */
export interface SnapshotTotalSource {
  slot: number;
  total_active_stake?: number | null;
}

/**
 * The snapshot total to measure a proposal's quorum against, or `undefined`
 * when it cannot be established.
 *
 * `/meta` serves only the newest snapshot, while a proposal votes against the
 * one frozen at activation (`Proposal.snapshot_slot`). Once operators resume
 * uploading, the newest snapshot moves past any proposal still being voted on,
 * and its total is a different number for a different stake distribution.
 * Using it anyway would misreport participation — the same mistake as sourcing
 * proofs from the latest slot rather than the proposal's.
 *
 * So the total is used only when the served snapshot *is* the proposal's.
 * Reporting an unknown denominator is correct; guessing at one is not. A
 * `/meta?slot=` lookup would let this resolve for any proposal.
 */
export function resolveQuorumDenominator(
  meta: SnapshotTotalSource | undefined,
  proposalSnapshotSlot: number | undefined,
): number | undefined {
  if (!meta || !proposalSnapshotSlot) return undefined;
  if (meta.slot !== proposalSnapshotSlot) return undefined;
  return meta.total_active_stake ?? undefined;
}
