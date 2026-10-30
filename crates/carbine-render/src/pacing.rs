//! Frame pacing: how fast the window is allowed to draw.
//!
//! Two limits meet here. The cap is the number the game asked for; the monitor is
//! a limit nobody gets past once it is in charge of the swap. The lower one wins,
//! which is what lets one setting travel with a player from a 144 Hz desktop to a
//! 60 Hz laptop: the desktop holds at the cap and the laptop holds at the screen.
//!
//! Neither limit is there unless the game puts it there. Left alone, nothing holds
//! the frames and the compositor runs the swap its own way, which is what a machine
//! that was bought to draw expects; a cap and the screen's rate are both choices.

use std::time::{Duration, Instant};

/// The cap, the sync wish, and the refresh rate of whatever screen the window was
/// last standing on. Every one of them starts as nothing, because a frame the game
/// never spoke about is a frame nobody should be waiting for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pacing {
    fps_cap: Option<u32>,
    monitor_sync: bool,
    monitor_hz: u32,
    next_frame: Option<Instant>,
}

impl Pacing {
    /// `None`, or a cap of zero frames a second, both mean the same thing: no cap.
    pub fn set_fps_cap(&mut self, fps: Option<u32>) {
        self.fps_cap = fps.filter(|fps| *fps > 0);
    }

    pub fn fps_cap(&self) -> Option<u32> {
        self.fps_cap
    }

    pub fn set_monitor_sync(&mut self, sync: bool) {
        self.monitor_sync = sync;
    }

    pub fn monitor_sync(&self) -> bool {
        self.monitor_sync
    }

    /// What the screen asks for, zero when there is none to ask: an offscreen
    /// target, or a compositor that will not say.
    pub fn set_monitor_hz(&mut self, hz: u32) {
        self.monitor_hz = hz;
    }

    /// The rate the frame settles at, whichever of the two limits is lower, or
    /// `None` when nothing holds the frame at all.
    pub fn limit_fps(&self) -> Option<u32> {
        let screen = (self.monitor_sync && self.monitor_hz > 0).then_some(self.monitor_hz);
        match (self.fps_cap, screen) {
            (None, None) => None,
            (Some(fps), None) | (None, Some(fps)) => Some(fps),
            (Some(cap), Some(screen)) => Some(cap.min(screen)),
        }
    }

    /// The gap to leave between the starts of two frames, or none when someone
    /// else already keeps the pace: a synced screen drawing at its own rate blocks
    /// in `get_current_texture`, and an uncapped frame waits for nobody.
    pub fn frame_time(&self) -> Option<Duration> {
        let limit = self.limit_fps()?;
        if self.monitor_sync && limit == self.monitor_hz {
            return None;
        }
        Some(Duration::from_secs_f64(1.0 / f64::from(limit)))
    }

    /// Sleeps out the rest of the frame so the window cannot draw past the limit,
    /// and books the slot the next frame is aimed at. Called once the frame is on
    /// its way to the screen, so the wait never costs the frame that came before.
    ///
    /// The slots march at exactly one frame gap each, so a slow frame is paid for
    /// once instead of borrowing speed from the frames behind it.
    pub fn hold(&mut self) {
        let Some(frame_time) = self.frame_time() else {
            self.next_frame = None;
            return;
        };
        let now = Instant::now();
        let due = self.next_frame.unwrap_or(now + frame_time).max(now);
        std::thread::sleep(due.saturating_duration_since(now));
        self.next_frame = Some(due + frame_time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kevin's desktop: a 144 Hz screen told to draw 90.
    #[test]
    fn the_cap_holds_when_it_is_the_lower_limit() {
        let mut pacing = Pacing::default();
        pacing.set_monitor_sync(true);
        pacing.set_monitor_hz(144);
        pacing.set_fps_cap(Some(90));

        assert_eq!(pacing.limit_fps(), Some(90));
        assert_eq!(
            pacing.frame_time(),
            Some(Duration::from_secs_f64(1.0 / 90.0))
        );
    }

    /// The same game, on the laptop at the café.
    #[test]
    fn the_screen_holds_when_it_is_the_lower_limit() {
        let mut pacing = Pacing::default();
        pacing.set_monitor_sync(true);
        pacing.set_fps_cap(Some(90));
        pacing.set_monitor_hz(60);

        assert_eq!(pacing.limit_fps(), Some(60));
        // Nothing to sleep for: the compositor is the limit, and it blocks for us.
        assert_eq!(pacing.frame_time(), None);
    }

    #[test]
    fn an_uncapped_synced_window_runs_at_the_screen() {
        let mut pacing = Pacing::default();
        pacing.set_monitor_sync(true);
        pacing.set_monitor_hz(144);

        assert_eq!(pacing.limit_fps(), Some(144));
        assert_eq!(pacing.frame_time(), None);
    }

    /// The screen the window sits on may be a compositor that will not name its
    /// rate, or no screen at all, so the cap has to be enforced by waiting.
    #[test]
    fn a_cap_is_held_even_with_no_screen_rate_to_trust() {
        let mut pacing = Pacing::default();
        pacing.set_fps_cap(Some(45));

        assert_eq!(pacing.limit_fps(), Some(45));
        assert_eq!(
            pacing.frame_time(),
            Some(Duration::from_secs_f64(1.0 / 45.0))
        );
    }

    #[test]
    fn unlocking_the_screen_lets_the_cap_be_the_only_limit() {
        let mut pacing = Pacing::default();
        pacing.set_fps_cap(Some(90));
        pacing.set_monitor_hz(60);

        assert_eq!(pacing.limit_fps(), Some(90));
        assert_eq!(
            pacing.frame_time(),
            Some(Duration::from_secs_f64(1.0 / 90.0))
        );
    }

    /// Left alone the engine holds nothing: no cap, no sync, so a frame starts the
    /// moment the one before it is gone.
    #[test]
    fn nothing_asked_for_means_nothing_to_wait_for() {
        let pacing = Pacing::default();

        assert_eq!(pacing.fps_cap(), None);
        assert!(!pacing.monitor_sync());
        assert_eq!(pacing.limit_fps(), None);
        assert_eq!(pacing.frame_time(), None);
    }

    /// A screen rate is only a limit once sync is asked for, so a machine that was
    /// never told anything still runs free even after the panel has been read.
    #[test]
    fn a_panel_says_nothing_until_sync_is_asked_for() {
        let mut pacing = Pacing::default();
        pacing.set_monitor_hz(60);

        assert_eq!(pacing.limit_fps(), None);

        pacing.set_monitor_sync(true);
        assert_eq!(pacing.limit_fps(), Some(60));
    }

    /// Zero is how Python spells "no cap", and it must not become a divide by zero.
    #[test]
    fn a_cap_of_zero_is_no_cap() {
        let mut pacing = Pacing::default();
        pacing.set_fps_cap(Some(90));
        pacing.set_fps_cap(Some(0));

        assert_eq!(pacing.fps_cap(), None);
        assert_eq!(pacing.limit_fps(), None);
        assert_eq!(pacing.frame_time(), None);
    }

    /// A frame that sleeps out its gap and then aims the next one one gap further
    /// holds the rate; one that aims from where it fell asleep runs at twice it.
    #[test]
    fn the_slots_stay_one_frame_apart() {
        let mut pacing = Pacing::default();
        pacing.set_fps_cap(Some(25));

        let start = Instant::now();
        for _ in 0..5 {
            pacing.hold();
        }

        assert!(
            start.elapsed() >= Duration::from_millis(185),
            "five frames at 25 should take 200 ms, took {:?}",
            start.elapsed()
        );
    }
}
