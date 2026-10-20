//! Executes a validated `Action`. For now every action is a stub that reports what it would do;
//! file search, scanning, printing and conversion replace these one at a time.

use marmalade_core::Action;

pub const HELP: &str = "Try: find my cv  \u{2022}  print invoice.pdf  \u{2022}  convert photo.heic to jpg  \u{2022}  is this file safe?";

/// Run an action and return a short, user-facing status line.
pub fn execute(action: &Action) -> String {
    match action {
        Action::Help => HELP.to_string(),
        other => format!("{other} \u{2014} coming soon"),
    }
}
