//! The terminal of the home page's code window (browser side of the `showcase` island).

use active::State;

use crate::data::samples::{TERMINAL, TermLine};
use crate::web::{Timer, every};

/// Types the terminal script when it first scrolls into view. A terminal already seen when
/// the page starts keeps the output the server rendered.
pub fn watch_terminal(el: &web_sys::Element, progress: State<(usize, usize)>, tab: State<usize>, timer: &Timer) {
    if super::reduced_motion() || super::seen(el) {
        return;
    }
    progress.set((0, 0));
    let timer = timer.clone();
    super::when_visible(el, 0.3, move || {
        // Already typing: a tab was clicked meanwhile.
        if timer.borrow().is_none() {
            type_script(progress, TERMINAL[tab.get()], &timer);
        }
    });
}

/// Types `script` from the start: commands character by character, output line by line.
pub fn type_script(progress: State<(usize, usize)>, script: &'static [TermLine], timer: &Timer) {
    let (mut line, mut ch, mut wait) = (0usize, 0usize, 6u32);
    let interval = every(32, move || {
        if wait > 0 {
            wait -= 1;
            return;
        }
        let Some(current) = script.get(line) else { return };
        match current {
            TermLine::Cmd(cmd) => {
                if ch < cmd.chars().count() {
                    ch += 1;
                } else {
                    line += 1;
                    ch = 0;
                    wait = 10;
                }
            }
            TermLine::Out(_) | TermLine::Ok(_) => {
                line += 1;
                wait = 3;
            }
        }
        progress.set((line, ch));
    });
    // Replacing the previous interval stops it.
    *timer.borrow_mut() = Some(interval);
}
