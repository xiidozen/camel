//! # camel-util
//!
//! Utility helpers shared by all Camel crates. This crate is the Rust port of the Java
//! `core/camel-util` module (`org.apache.camel.util`).

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod ant_path_matcher;
pub mod camel_uri_parser;
pub mod key_value_holder;
pub mod lang;
pub mod net;
pub mod ognl_helper;
pub mod pair;
pub mod reference_count;
pub mod sensitive_utils;
pub mod stop_watch;
pub mod string_helper;
pub mod string_quote_helper;
pub mod time_utils;
pub mod unit_utils;
pub mod unsafe_uri_characters_encoder;
pub mod uri_support;
