//! Language-neutral wire definitions shared by Athenaeum services and clients.
//! This crate owns no vault, database, transport, authorization, or model behavior.

pub mod bridge;
pub mod library;
pub mod sync;

pub const BRIDGE_PROTOCOL_VERSION: u32 = 1;
pub const API_VERSION: u32 = 1;
pub const SYNC_PROTOCOL_VERSION: u32 = 1;
pub const PORTABLE_FORMAT_VERSION: u32 = 1;
