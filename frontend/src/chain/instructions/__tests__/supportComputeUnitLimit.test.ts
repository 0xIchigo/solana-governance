import { supportComputeUnitLimit } from "../types";

/** Peak measured in the program's tests/support_compute_budget.rs at the cap. */
const MEASURED_AT_CAP = 284_953;
/** Most expensive activating call measured in the program's tests/ncn_flow.rs. */
const MEASURED_ACTIVATION = 49_473;

describe("supportComputeUnitLimit", () => {
  it("covers the measured cost with headroom", () => {
    // An empty list can still be the call that activates voting and creates the
    // ballot box via CPI, which is the expensive path at that size.
    expect(supportComputeUnitLimit(0)).toBeGreaterThan(MEASURED_ACTIVATION);
    expect(supportComputeUnitLimit(2_000)).toBeGreaterThan(MEASURED_AT_CAP);
  });

  it("scales with the supporter count", () => {
    // The point of modelling: a small list must request far less than the flat
    // 600k it used to.
    expect(supportComputeUnitLimit(0)).toBeLessThan(100_000);
    expect(supportComputeUnitLimit(50)).toBeLessThan(
      supportComputeUnitLimit(500),
    );
    expect(supportComputeUnitLimit(500)).toBeLessThan(
      supportComputeUnitLimit(2_000),
    );
  });

  it("never exceeds the per-transaction maximum", () => {
    expect(supportComputeUnitLimit(1_000_000)).toBe(1_400_000);
  });

  it("matches the Rust mirror in svmgov/cli/src/constants.rs", () => {
    // Both clients must request the same budget for the same state; these are
    // the values support_compute_unit_limit() produces.
    expect(supportComputeUnitLimit(0)).toBe(58_075);
    expect(supportComputeUnitLimit(2_000)).toBe(361_675);
  });
});
