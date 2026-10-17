//! Stable element ids shared by views and UI tests (SPEC §9.8).
//!
//! Every interactive or asserted element takes its id from here and calls `.test_support()`.

/// Label shown by the M0.6 hello window.
pub const HELLO_LABEL: &str = "hello-label";
