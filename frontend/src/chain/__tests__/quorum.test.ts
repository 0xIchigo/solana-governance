import {
  computeQuorum,
  QUORUM_DENOMINATOR,
  QUORUM_NUMERATOR,
  resolveQuorumDenominator,
} from "../quorum";

/** Roughly mainnet scale, so the arithmetic is exercised where doubles get coarse. */
const NETWORK_STAKE = 400_000_000 * 1_000_000_000;
const ONE_THIRD = NETWORK_STAKE / 3;

describe("the quorum threshold", () => {
  it("is one third, as SGP-0001 Art. IV.3 requires", () => {
    // Guards the value the whole display is built on. It was 60% before, taken
    // from a `// TODO ?` placeholder rather than the constitution.
    expect(QUORUM_NUMERATOR / QUORUM_DENOMINATOR).toBeCloseTo(1 / 3, 12);
  });
});

describe("computeQuorum", () => {
  it("reports unknown when the snapshot never recorded a total", () => {
    // The state that matters most: a snapshot uploaded before the verifier
    // recorded the total cannot be back-filled.
    expect(
      computeQuorum({
        forLamports: 1,
        againstLamports: 2,
        abstainLamports: 3,
        totalActiveStake: undefined,
      }),
    ).toEqual({ known: false });
  });

  it("reports unknown rather than dividing by a zero total", () => {
    expect(
      computeQuorum({
        forLamports: 1,
        againstLamports: 0,
        abstainLamports: 0,
        totalActiveStake: 0,
      }),
    ).toEqual({ known: false });
  });

  it("distinguishes no votes from an unknown denominator", () => {
    // Zero participation against a known total is a real answer, and must not
    // look like missing data — nor the reverse. Collapsing the two would report
    // "0% participation, quorum not met" for a vote that may have met it.
    const result = computeQuorum({
      forLamports: 0,
      againstLamports: 0,
      abstainLamports: 0,
      totalActiveStake: NETWORK_STAKE,
    });

    expect(result.known).toBe(true);
    if (!result.known) throw new Error("unreachable");
    expect(result.participationPercent).toBe(0);
    expect(result.isMet).toBe(false);
    expect(result.remainingLamports).toBe(ONE_THIRD);
  });

  it("counts abstain toward participation", () => {
    // Both readings of the disputed Art. IV.4 agree on this: abstain is
    // participation for quorum purposes. Only the supermajority denominator is
    // contested, and that is deliberately not computed here.
    const abstainOnly = computeQuorum({
      forLamports: 0,
      againstLamports: 0,
      abstainLamports: ONE_THIRD,
      totalActiveStake: NETWORK_STAKE,
    });

    expect(abstainOnly.known && abstainOnly.isMet).toBe(true);
  });

  it("is met exactly at one third, and not just below it", () => {
    // The margin is one SOL rather than one lamport: totals at this scale are
    // past Number.MAX_SAFE_INTEGER, where a single lamport is not representable.
    // See the precision note in computeQuorum.
    const ONE_SOL = 1_000_000_000;
    const at = computeQuorum({
      forLamports: ONE_THIRD,
      againstLamports: 0,
      abstainLamports: 0,
      totalActiveStake: NETWORK_STAKE,
    });
    const below = computeQuorum({
      forLamports: ONE_THIRD - ONE_SOL,
      againstLamports: 0,
      abstainLamports: 0,
      totalActiveStake: NETWORK_STAKE,
    });

    expect(at.known && at.isMet).toBe(true);
    expect(below.known && below.isMet).toBe(false);
    expect(below.known && below.remainingLamports).toBe(ONE_SOL);
  });

  it("does not let display rounding decide a borderline vote", () => {
    // A hair under one third still rounds to "33.33%". The verdict comes from
    // the lamport comparison, so the two disagree here by design.
    const result = computeQuorum({
      forLamports: ONE_THIRD - 1_000,
      againstLamports: 0,
      abstainLamports: 0,
      totalActiveStake: NETWORK_STAKE,
    });

    expect(result.known).toBe(true);
    if (!result.known) throw new Error("unreachable");
    expect(result.participationPercent.toFixed(2)).toBe("33.33");
    expect(result.isMet).toBe(false);
  });

  it("sums the three buckets and never reports negative remaining", () => {
    const result = computeQuorum({
      forLamports: NETWORK_STAKE / 2,
      againstLamports: NETWORK_STAKE / 4,
      abstainLamports: NETWORK_STAKE / 8,
      totalActiveStake: NETWORK_STAKE,
    });

    expect(result.known).toBe(true);
    if (!result.known) throw new Error("unreachable");
    expect(result.participatingLamports).toBe(NETWORK_STAKE * 0.875);
    expect(result.participationPercent).toBeCloseTo(87.5, 9);
    expect(result.isMet).toBe(true);
    expect(result.remainingLamports).toBe(0);
  });
});

describe("resolveQuorumDenominator", () => {
  const meta = { slot: 500, total_active_stake: NETWORK_STAKE };

  it("uses the total when the served snapshot is the proposal's", () => {
    expect(resolveQuorumDenominator(meta, 500)).toBe(NETWORK_STAKE);
  });

  it("refuses a total from a different snapshot", () => {
    // `/meta` serves the newest snapshot, which moves past a proposal still
    // being voted on. Its total describes a different stake distribution, so
    // using it would misreport participation.
    expect(resolveQuorumDenominator(meta, 499)).toBeUndefined();
    expect(resolveQuorumDenominator(meta, 501)).toBeUndefined();
  });

  it("handles a snapshot that predates the field, and a missing one", () => {
    expect(
      resolveQuorumDenominator({ slot: 500, total_active_stake: null }, 500),
    ).toBeUndefined();
    expect(resolveQuorumDenominator({ slot: 500 }, 500)).toBeUndefined();
    expect(resolveQuorumDenominator(undefined, 500)).toBeUndefined();
  });

  it("treats an unactivated proposal's zero slot as unknown", () => {
    // `snapshot_slot` is 0 until activate_voting sets it; 0 must never be
    // matched against a real slot.
    expect(resolveQuorumDenominator({ slot: 0 }, 0)).toBeUndefined();
  });
});
