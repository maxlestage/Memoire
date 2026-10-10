//! The hero's rotating tagline (browser side of the `typing` island).

use std::cell::Cell;

use active::State;

#[derive(Clone, Copy)]
enum Phase {
    Typing,
    Holding(u32),
    Deleting,
}

/// Types, pauses, deletes and moves to the next line, forever. The first line is already
/// shown (rendered by the server), so the loop starts by holding it.
pub fn start(text: State<String>, lines: &'static [&'static str]) {
    if lines.is_empty() || super::reduced_motion() {
        return;
    }
    let line = Cell::new(0usize);
    let chars = Cell::new(lines[0].chars().count());
    let phase = Cell::new(Phase::Holding(38));
    let interval = crate::web::every(48, move || {
        let current: Vec<char> = lines[line.get() % lines.len()].chars().collect();
        match phase.get() {
            Phase::Typing => {
                if chars.get() < current.len() {
                    chars.set(chars.get() + 1);
                } else {
                    phase.set(Phase::Holding(38));
                }
            }
            Phase::Holding(0) => phase.set(Phase::Deleting),
            Phase::Holding(n) => phase.set(Phase::Holding(n - 1)),
            Phase::Deleting => {
                if chars.get() > 0 {
                    chars.set(chars.get().saturating_sub(2));
                } else {
                    line.set(line.get() + 1);
                    phase.set(Phase::Typing);
                }
            }
        }
        let next: String = current.iter().take(chars.get()).collect();
        if text.with(|t| *t != next) {
            text.set(next);
        }
    });
    // The tagline lives as long as the page.
    std::mem::forget(interval);
}
