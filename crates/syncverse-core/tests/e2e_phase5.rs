//! End-to-End (E2E) Test Suite for Syncplay-Alt Phase 5 (Watch-Party Social & Auto-Update Polish).
//!
//! Executes 4 tiers of opaque-box integration tests:
//! - Tier 1: Feature Coverage (90 tests covering all 18 inventoried features)
//! - Tier 2: Boundary & Corner Cases (25 tests covering 5 boundary domains)
//! - Tier 3: Cross-Feature Combinations (5 pairwise integration tests)
//! - Tier 4: Real-World Application Scenarios (5 full end-to-end user workflows)

#![deny(warnings)]

#[path = "../../../tests/e2e_phase5/mod.rs"]
mod e2e_phase5;
