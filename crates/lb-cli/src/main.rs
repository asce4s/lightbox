//! Headless `lightbox-cli` entry point (SPEC §8).

mod commands;

use clap::Parser;
use std::process::ExitCode;

/// Exit status: success.
const OK: u8 = 0;
/// Exit status: the user asked for something invalid.
const USER_ERROR: u8 = 1;
/// Exit status: something failed while running.
const RUNTIME_ERROR: u8 = 2;

fn main() -> ExitCode {
    let cli = match commands::Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let code = if e.use_stderr() { USER_ERROR } else { OK };
            let _ = e.print();
            return ExitCode::from(code);
        }
    };
    let json = cli.json;
    match commands::run(cli) {
        Ok(out) => {
            print_output(&out, json);
            ExitCode::from(OK)
        }
        Err(e) => {
            if json {
                eprintln!("{}", serde_json::json!({ "error": e.message() }));
            } else {
                eprintln!("error: {}", e.message());
            }
            ExitCode::from(if e.is_user() {
                USER_ERROR
            } else {
                RUNTIME_ERROR
            })
        }
    }
}

#[allow(clippy::print_stdout)]
fn print_output(out: &commands::Output, json: bool) {
    if json {
        println!("{}", out.json);
    } else if !out.text.is_empty() {
        println!("{}", out.text);
    }
}
