// ═══════════════════════════════════════════════════════════════════════════════
// views/ — Screen and Panel Modules
//
// Each file renders one "screen" or "panel" of the application.
// All views take state references and return action structs.
// They never own state — they only read and mutate through passed references.
// ═══════════════════════════════════════════════════════════════════════════════

pub mod boot;
pub mod setup;
pub mod dashboard;
pub mod search;
pub mod timeline;
