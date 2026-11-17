# tests/e2e_phase5

These tier tests (`tier1_feature_coverage.rs` to `tier4_scenarios.rs`) check behaviour against the
models in `contract_models.rs`. Those models are written inside the test tree. For example,
`RoomAnnouncement` formats the room messages itself. So a test that calls a contract model
validates the written contract, not the code in `crates/`.

Some tests do call product code directly (`syncverse_chat`, `syncverse_sync::wire`,
`syncverse_ui::snapshot`, `syncverse_ui::gui::updater`). Treat those as real checks and the
contract-model ones as specification checks.

Real behaviour is covered by the unit and integration tests inside each crate
(`cargo test --workspace`).
