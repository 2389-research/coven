// ABOUTME: gRPC service implementations for the local gateway
// ABOUTME: CovenControl (agents), ClientService (TUI), PackService (packs)

pub mod client;
pub mod control;
pub mod pack;

pub use client::ClientServiceImpl;
pub use control::CovenControlService;
pub use pack::PackServiceImpl;
