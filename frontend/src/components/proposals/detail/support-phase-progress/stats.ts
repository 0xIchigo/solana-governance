export interface SupportStatsInput {
  currentSupportLamports: number;
  totalStakedLamports: number;
  /** Already resolved via `supportThresholdPercentFromConfig`. */
  thresholdPercent: number;
  validatorCount: number;
  numOfValidators: number;
}

export interface SupportStats {
  currentSupportLamports: number;
  totalStakedLamports: number;
  requiredThresholdLamports: number;
  thresholdPercent: number;
  progressPercent: number;
  supportPercentOfTotal: number;
  remainingLamports: number;
  isThresholdMet: boolean;
  validatorCount: number;
  participationPercent: number;
  avgStakePerValidator: number;
}

/**
 * Support-phase figures for the proposal detail page. Pure so the zero states
 * are testable: validator stake and the supporter list arrive from separate
 * queries, and either being absent must not read as progress.
 */
export function computeSupportStats({
  currentSupportLamports,
  totalStakedLamports,
  thresholdPercent,
  validatorCount,
  numOfValidators,
}: SupportStatsInput): SupportStats {
  const requiredThresholdLamports =
    totalStakedLamports * (thresholdPercent / 100);

  const progressPercent =
    requiredThresholdLamports > 0
      ? (currentSupportLamports / requiredThresholdLamports) * 100
      : 0;

  const supportPercentOfTotal =
    totalStakedLamports > 0
      ? (currentSupportLamports / totalStakedLamports) * 100
      : 0;

  const remainingLamports = Math.max(
    0,
    requiredThresholdLamports - currentSupportLamports,
  );

  // A required threshold of 0 means validator stake has not loaded yet, not
  // that the bar has been cleared. Without the guard any support satisfies
  // `>= 0`, so the panel claims success on first paint.
  const isThresholdMet =
    requiredThresholdLamports > 0 &&
    currentSupportLamports >= requiredThresholdLamports;

  const participationPercent =
    numOfValidators > 0 ? (validatorCount / numOfValidators) * 100 : 0;
  const avgStakePerValidator =
    validatorCount > 0 ? currentSupportLamports / validatorCount : 0;

  return {
    currentSupportLamports,
    totalStakedLamports,
    requiredThresholdLamports,
    thresholdPercent,
    progressPercent,
    supportPercentOfTotal,
    remainingLamports,
    isThresholdMet,
    validatorCount,
    participationPercent,
    avgStakePerValidator,
  };
}
