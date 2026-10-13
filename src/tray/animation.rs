//! Event-loop-owned playback of the embedded orca jump frames.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::{Timer, TimerMode};

const FRAME_INTERVAL: Duration = Duration::from_millis(30);

pub(super) const FRAME_PNGS: &[&[u8]] = &[
    include_bytes!("../../ui/assets/tray-jump/jump-00.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-01.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-02.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-03.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-04.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-05.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-06.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-07.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-08.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-09.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-10.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-11.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-12.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-13.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-14.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-15.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-16.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-17.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-18.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-19.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-20.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-21.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-22.png"),
    include_bytes!("../../ui/assets/tray-jump/jump-23.png"),
];

/// Keeps native images and the playback timer on the thread that owns the tray.
/// Call `stop` before removing the native tray; dropping the player also stops
/// its timer without retaining itself through the callback.
pub(super) struct Player<I> {
    timer: Timer,
    still: I,
    frames: Vec<I>,
    apply: Box<dyn Fn(&I)>,
    timeline: RefCell<Timeline>,
}

impl<I: 'static> Player<I> {
    pub(super) fn new(still: I, frames: Vec<I>, apply: impl Fn(&I) + 'static) -> Rc<Self> {
        let frame_count = frames.len();
        Rc::new(Self {
            timer: Timer::default(),
            still,
            frames,
            apply: Box::new(apply),
            timeline: RefCell::new(Timeline::new(frame_count)),
        })
    }

    pub(super) fn play(self: &Rc<Self>) {
        let first_frame = self.timeline.borrow_mut().start(Instant::now());
        let Some(first_frame) = first_frame else {
            return;
        };
        self.apply_frame(first_frame);

        let weak = Rc::downgrade(self);
        self.timer
            .start(TimerMode::Repeated, FRAME_INTERVAL, move || {
                if let Some(player) = weak.upgrade() {
                    player.tick(Instant::now());
                }
            });
    }

    pub(super) fn stop(&self) {
        self.timer.stop();
        self.timeline.borrow_mut().stop();
        (self.apply)(&self.still);
    }

    fn tick(&self, now: Instant) {
        let frame = self.timeline.borrow_mut().advance(now);
        if let Some(frame) = frame {
            if frame == Frame::Still {
                self.timer.stop();
            }
            self.apply_frame(frame);
        }
    }

    fn apply_frame(&self, frame: Frame) {
        let image = match frame {
            Frame::Animation(index) => &self.frames[index],
            Frame::Still => &self.still,
        };
        (self.apply)(image);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Frame {
    Animation(usize),
    Still,
}

/// Pure playback policy: repeated clicks keep the first deadline, late ticks
/// skip expired frames, and neither idle nor cancelled ticks write an image.
struct Timeline {
    frame_count: usize,
    started_at: Option<Instant>,
    last_frame: usize,
}

impl Timeline {
    fn new(frame_count: usize) -> Self {
        Self {
            frame_count,
            started_at: None,
            last_frame: 0,
        }
    }

    fn start(&mut self, now: Instant) -> Option<Frame> {
        if self.started_at.is_some() || self.frame_count == 0 {
            return None;
        }
        self.started_at = Some(now);
        self.last_frame = 0;
        Some(Frame::Animation(0))
    }

    fn advance(&mut self, now: Instant) -> Option<Frame> {
        let started_at = self.started_at?;
        let elapsed = now.saturating_duration_since(started_at);
        let index = elapsed.as_nanos() / FRAME_INTERVAL.as_nanos();
        if index >= self.frame_count as u128 {
            self.stop();
            return Some(Frame::Still);
        }
        let index = index as usize;
        if index == self.last_frame {
            return None;
        }
        self.last_frame = index;
        Some(Frame::Animation(index))
    }

    fn stop(&mut self) {
        self.started_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_completion_restores_the_real_still_and_stops_its_timer() {
        i_slint_backend_testing::init_no_event_loop();
        let writes = Rc::new(RefCell::new(Vec::new()));
        let recorder = writes.clone();
        let player = Player::new(-99, (0..24).collect(), move |image| {
            recorder.borrow_mut().push(*image);
        });

        player.play();
        assert_eq!(writes.borrow().as_slice(), &[0]);
        assert!(player.timer.running());
        let started_at = player.timeline.borrow().started_at.unwrap();
        player.play();
        assert_eq!(writes.borrow().as_slice(), &[0]);
        assert_eq!(player.timeline.borrow().started_at, Some(started_at));

        player.tick(started_at + Duration::from_millis(690));
        player.tick(started_at + Duration::from_millis(720));
        assert_eq!(writes.borrow().as_slice(), &[0, 23, -99]);
        assert!(!player.timer.running());
        player.tick(started_at + Duration::from_secs(1));
        i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(1));
        assert_eq!(writes.borrow().as_slice(), &[0, 23, -99]);
    }

    #[test]
    fn player_stop_and_drop_prevent_late_callbacks_without_an_rc_cycle() {
        i_slint_backend_testing::init_no_event_loop();
        let writes = Rc::new(RefCell::new(Vec::new()));
        let recorder = writes.clone();
        let player = Player::new(-99, (0..24).collect(), move |image| {
            recorder.borrow_mut().push(*image);
        });

        player.play();
        let started_at = player.timeline.borrow().started_at.unwrap();
        player.stop();
        assert_eq!(writes.borrow().as_slice(), &[0, -99]);
        assert!(!player.timer.running());
        player.tick(started_at + Duration::from_millis(60));
        i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(1));
        assert_eq!(writes.borrow().as_slice(), &[0, -99]);

        player.play();
        assert!(player.timer.running());
        let weak = Rc::downgrade(&player);
        drop(player);
        assert!(weak.upgrade().is_none());
        i_slint_backend_testing::mock_elapsed_time(Duration::from_secs(1));
        assert_eq!(writes.borrow().as_slice(), &[0, -99, 0]);
    }

    #[test]
    fn repeated_clicks_do_not_extend_the_original_deadline() {
        let start = Instant::now();
        let mut timeline = Timeline::new(24);
        assert_eq!(timeline.start(start), Some(Frame::Animation(0)));
        assert_eq!(timeline.start(start + Duration::from_millis(400)), None);
        assert_eq!(
            timeline.advance(start + Duration::from_millis(719)),
            Some(Frame::Animation(23))
        );
        assert_eq!(
            timeline.advance(start + Duration::from_millis(720)),
            Some(Frame::Still)
        );
    }

    #[test]
    fn delayed_ticks_skip_expired_frames_and_restore_after_a_long_pause() {
        let start = Instant::now();
        let mut timeline = Timeline::new(24);
        timeline.start(start);
        assert_eq!(
            timeline.advance(start + Duration::from_millis(210)),
            Some(Frame::Animation(7))
        );
        assert_eq!(
            timeline.advance(start + Duration::from_secs(2)),
            Some(Frame::Still)
        );
        assert_eq!(timeline.advance(start + Duration::from_secs(3)), None);
    }

    #[test]
    fn frame_boundaries_do_not_rewrite_the_same_image() {
        let start = Instant::now();
        let mut timeline = Timeline::new(24);
        timeline.start(start);
        assert_eq!(timeline.advance(start + Duration::from_millis(29)), None);
        assert_eq!(
            timeline.advance(start + Duration::from_millis(30)),
            Some(Frame::Animation(1))
        );
        assert_eq!(timeline.advance(start + Duration::from_millis(59)), None);
        assert_eq!(
            timeline.advance(start + Duration::from_millis(60)),
            Some(Frame::Animation(2))
        );
    }

    #[test]
    fn cancellation_prevents_late_writes_and_allows_a_fresh_playback() {
        let start = Instant::now();
        let mut timeline = Timeline::new(24);
        timeline.start(start);
        timeline.advance(start + Duration::from_millis(90));
        timeline.stop();
        assert_eq!(timeline.advance(start + Duration::from_millis(120)), None);
        assert_eq!(timeline.advance(start + Duration::from_secs(1)), None);
        let restart = start + Duration::from_secs(2);
        assert_eq!(timeline.start(restart), Some(Frame::Animation(0)));
        assert_eq!(
            timeline.advance(restart + Duration::from_millis(30)),
            Some(Frame::Animation(1))
        );
    }

    #[test]
    fn empty_animation_remains_idle() {
        let start = Instant::now();
        let mut timeline = Timeline::new(0);
        assert_eq!(timeline.start(start), None);
        assert_eq!(timeline.advance(start + Duration::from_secs(1)), None);
    }

    #[test]
    fn embedded_frames_are_24_valid_transparent_pngs_of_one_size() {
        assert_eq!(FRAME_PNGS.len(), 24);
        let mut expected_size = None;
        for png in FRAME_PNGS {
            let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
                .expect("embedded tray animation must decode");
            let rgba = image.to_rgba8();
            let size = rgba.dimensions();
            assert!(size.0 > 0 && size.1 > 0);
            assert_eq!(*expected_size.get_or_insert(size), size);
            assert!(rgba.pixels().any(|pixel| pixel[3] == 0));
            assert!(rgba.pixels().any(|pixel| pixel[3] > 0));
        }
    }
}
