// Default RPC endpoints
pub const DEFAULT_RPC_URL: &str = "https://api.mainnet-beta.solana.com";
pub const DEFAULT_WSS_URL: &str = "wss://api.mainnet-beta.solana.com";

// Network-specific default RPC URLs
pub const DEFAULT_MAINNET_RPC_URL: &str = "https://api.mainnet-beta.solana.com";
pub const DEFAULT_TESTNET_RPC_URL: &str = "https://api.testnet.solana.com";
pub const DEFAULT_OPERATOR_API_URL: &str = "https://ncn-governance.solana.com";

// Voting constants
pub const BASIS_POINTS_TOTAL: u64 = 10_000;

/// Compute-unit limit requested for `support_proposal` and `retally_support`,
/// which re-tally the whole supporter list on every call (~20k CU + ~132 per
/// supporter, so ~285k at the 2000 cap) and outgrow the 200k default at 1347
/// supporters. Headroom only: mainnet's ~800 validators cannot reach that.
///
/// 600k is >2x the worst case at the cap and well under the 1.4M ceiling.
/// Priority fees price the requested limit rather than the amount consumed, so
/// right-size this off simulation if a compute-unit price is ever added.
///
/// Measured by `tests/support_compute_budget.rs`; keep in sync with the copy in
/// `frontend/src/chain/instructions/types.ts`.
pub const SUPPORT_COMPUTE_UNIT_LIMIT: u32 = 600_000;

// UI constants
pub const SPINNER_TICK_DURATION_MS: u64 = 100;

// Environment variable names
pub const SVMGOV_KEY_ENV: &str = "SVMGOV_KEY";
pub const SVMGOV_RPC_ENV: &str = "SVMGOV_RPC";
