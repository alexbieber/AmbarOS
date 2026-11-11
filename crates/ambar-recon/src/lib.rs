//! Ambar's own reconnaissance scanner.
//!
//! Every command checks its targets against an engagement scope ([`scope`]) before
//! sending a single packet. Without a scope file, only this computer can be scanned.

pub mod http;
pub mod ports;
pub mod scope;
pub mod subdomains;
pub mod tls;
