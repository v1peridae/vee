use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

const TICK_CHARS: &str = "▁▂▃▄▅▆▇█";

pub fn spinner(msg: &str) -> ProgressBar {
    let progress_bar = ProgressBar::new_spinner();
    progress_bar.set_style(ProgressStyle::with_template("{spinner:.magenta} {msg}").unwrap().tick_chars(TICK_CHARS));
    progress_bar.set_message(msg.to_string());
    progress_bar.enable_steady_tick(Duration::from_millis(80));
    progress_bar
}

pub fn warn(msg: &str) {eprintln!("{} {}", style("⚠").yellow(), msg);}
pub fn success(msg: &str) {println!("{} {}", style("✦").green().bold(), msg);}
pub fn error(msg: &str) {eprintln!("{} {}", style("✕").red().bold(), msg);}
pub fn info(msg: &str) {println!("{} {}", style("꒰").magenta(), msg);}



pub fn progress(total: u64, msg: &str) -> ProgressBar {
    let progress_bar = ProgressBar::new(total);
    progress_bar.set_style(ProgressStyle::with_template("{spinner:.magenta} {bar:25.magenta/dim} {pos}/{len} {msg}").unwrap().tick_chars(TICK_CHARS).progress_chars("█▓░"));
    progress_bar.set_message(msg.to_string());
    progress_bar.enable_steady_tick(Duration::from_millis(80));
    progress_bar
}
