use std::path::Path;

use anyhow::{Context, Result};

use crate::SidebarMotionStep;

// Preserve the legacy spring's physical constants so this batch changes only
// the time integration and frame source. The closed-form coefficient method is
// adapted from Charm's mature Harmonica solver (zlib license), which in turn
// follows Ryan Juckett's damped harmonic oscillator derivation:
// https://github.com/charmbracelet/harmonica/blob/master/spring.go
const ANGULAR_FREQUENCY: f64 = 17.320_508_075_688_77; // sqrt(300)
const DAMPING_RATIO: f64 = 0.894_892_917_243_919; // 31 / (2 * sqrt(300))
const MAX_FRAME_DELTA_SECONDS: f64 = 1.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SidebarMotionMode {
    DisplayLinked,
    FixedTimer,
}

impl SidebarMotionMode {
    pub(crate) fn from_environment() -> Self {
        match std::env::var("USHER_SIDEBAR_MOTION").as_deref() {
            Ok("timer") => Self::FixedTimer,
            Ok("display-link") | Err(_) => Self::DisplayLinked,
            Ok(other) => {
                log::warn!(
                    "[SidebarMotion] Unknown USHER_SIDEBAR_MOTION={other:?}; using display-link"
                );
                Self::DisplayLinked
            }
        }
    }

    pub(crate) fn is_display_linked(self) -> bool {
        matches!(self, Self::DisplayLinked)
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::DisplayLinked => "display-link",
            Self::FixedTimer => "timer",
        }
    }
}

pub(crate) fn advance(
    progress: f32,
    velocity: f32,
    target: f32,
    delta_seconds: f32,
) -> SidebarMotionStep {
    let delta_seconds = f64::from(delta_seconds).clamp(0.0, MAX_FRAME_DELTA_SECONDS);
    let progress = f64::from(progress);
    let velocity = f64::from(velocity);
    let target = f64::from(target);

    if delta_seconds <= f64::EPSILON {
        return SidebarMotionStep {
            progress: progress as f32,
            velocity: velocity as f32,
        };
    }

    let omega_zeta = ANGULAR_FREQUENCY * DAMPING_RATIO;
    let alpha = ANGULAR_FREQUENCY * (1.0 - DAMPING_RATIO * DAMPING_RATIO).sqrt();
    let exponential = (-omega_zeta * delta_seconds).exp();
    let angle = alpha * delta_seconds;
    let exponential_sine = exponential * angle.sin();
    let exponential_cosine = exponential * angle.cos();
    let omega_zeta_sine_over_alpha = omega_zeta * exponential_sine / alpha;

    let position_from_position = exponential_cosine + omega_zeta_sine_over_alpha;
    let position_from_velocity = exponential_sine / alpha;
    let velocity_from_position =
        -alpha * exponential_sine - omega_zeta * omega_zeta_sine_over_alpha;
    let velocity_from_velocity = exponential_cosine - omega_zeta_sine_over_alpha;

    let displacement = progress - target;
    SidebarMotionStep {
        progress: (displacement * position_from_position
            + velocity * position_from_velocity
            + target) as f32,
        velocity: (displacement * velocity_from_position + velocity * velocity_from_velocity)
            as f32,
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TraceSample {
    pub(crate) elapsed_microseconds: u128,
    pub(crate) progress: f32,
    pub(crate) target_expanded: bool,
    pub(crate) active: bool,
}

pub(crate) fn write_trace(
    path: &Path,
    mode: SidebarMotionMode,
    samples: &[TraceSample],
) -> Result<()> {
    let mut output = String::from("elapsed_us\tprogress\ttarget_expanded\tactive\tmode\n");
    for sample in samples {
        use std::fmt::Write as _;
        writeln!(
            output,
            "{}\t{:.7}\t{}\t{}\t{}",
            sample.elapsed_microseconds,
            sample.progress,
            sample.target_expanded,
            sample.active,
            mode.label(),
        )
        .expect("writing to a String cannot fail");
    }
    std::fs::write(path, output)
        .with_context(|| format!("Failed to write sidebar motion trace: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_near(actual: f32, expected: f32, tolerance: f32) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "actual={actual}, expected={expected}, tolerance={tolerance}"
        );
    }

    #[test]
    fn closed_form_is_independent_of_frame_partitioning() {
        let whole = advance(0.12, 2.4, 1.0, 0.016);
        let first_half = advance(0.12, 2.4, 1.0, 0.008);
        let second_half = advance(first_half.progress, first_half.velocity, 1.0, 0.008);

        assert_near(whole.progress, second_half.progress, 1.0e-6);
        assert_near(whole.velocity, second_half.velocity, 1.0e-5);
    }

    #[test]
    fn retargeting_preserves_the_incoming_state() {
        let progress = 0.62;
        let velocity = 1.75;
        let unchanged = advance(progress, velocity, 0.0, 0.0);

        assert_eq!(unchanged.progress, progress);
        assert_eq!(unchanged.velocity, velocity);
        let reversed = advance(progress, velocity, 0.0, 0.001);
        assert!(reversed.progress > progress);
        assert!(reversed.velocity < velocity);
    }

    #[test]
    fn variable_frames_converge_without_non_finite_values() {
        let mut state = SidebarMotionStep {
            progress: 0.0,
            velocity: 0.0,
        };
        for delta in [0.008, 0.011, 0.024, 0.006, 0.052, 0.016, 0.033] {
            state = advance(state.progress, state.velocity, 1.0, delta);
            assert!(state.progress.is_finite());
            assert!(state.velocity.is_finite());
        }
        for _ in 0..80 {
            state = advance(state.progress, state.velocity, 1.0, 0.016);
        }

        assert_near(state.progress, 1.0, 1.0e-5);
        assert_near(state.velocity, 0.0, 1.0e-4);
    }
}
