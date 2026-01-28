// ABOUTME: Library exports for coven-link device linking tool
// ABOUTME: Allows coven-cli to integrate link functionality directly

pub mod config;
pub mod link;

pub use link::run;
