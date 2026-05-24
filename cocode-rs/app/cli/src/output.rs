//! Output formatting for CLI responses.

/// Print an error message.
pub fn print_error(error: &str) {
    eprintln!("Error: {error}");
}

/// Print a separator line.
pub fn print_separator() {
    println!("─────────────────────────────────────────");
}

/// Print session start information.
pub fn print_session_start(session_id: &str, model: &str, provider: &str) {
    println!("Session: {session_id}");
    println!("Model:   {provider}/{model}");
    print_separator();
    println!("Type your message. Press Ctrl+D to exit.");
    println!();
}

/// Print turn completion summary.
pub fn print_turn_summary(input_tokens: i64, output_tokens: i64) {
    println!();
    println!("[Tokens: {input_tokens} in / {output_tokens} out]");
}
