//! Slice reel — spring-driven focus for the theme slider.
//!
//! Ported from skwd-wall (MIT, © liixini) `frontend/animation/animation.rs`
//! and translated to the HVE slice carousel. The slider no longer uses a
//! fixed-duration bezier tween (which restarts its curve on every chained
//! step and feels mechanical): a critically damped oscillator is integrated
//! at fixed 240 Hz sub-steps, so retargeting mid-flight keeps velocity and
//! chained steps glide naturally. Rust drives `focus-pos` per frame; Slint
//! only renders (the `animate focus-pos` block was removed for this).

/// Critically damped spring — port of skwd-wall `Spring` (MIT).
/// `k = ω², c = 2ω` with `ω = 6.64 / seconds` maps a desired settle duration
/// to spring constants with no overshoot (ζ = 1).
#[derive(Debug, Clone, Copy)]
pub struct ReelSpring {
    pub x: f32,
    pub v: f32,
    pub target: f32,
    k: f32,
    c: f32,
}

impl ReelSpring {
    pub fn new(x: f32, k: f32, c: f32) -> Self {
        Self { x, v: 0.0, target: x, k, c }
    }

    /// Map a desired settle duration to spring constants. ω = 6.64/s with
    /// k = ω², c = 2ω is critically damped (ζ = 1): fastest settle with zero
    /// overshoot — the "suave y natural" skwd-wall feel.
    pub fn for_duration_ms(x: f32, ms: f32) -> Self {
        let omega = 6.64 / (ms.max(1.0) / 1000.0);
        Self::new(x, omega * omega, 2.0 * omega)
    }

    /// Retarget WITHOUT touching x or v — the glide continues from the live
    /// position with its current momentum (the whole point of the spring).
    pub fn retarget(&mut self, target: f32) {
        self.target = target;
    }

    /// Jump instantly (kept for future programmatic snaps; the driver-level
    /// `snap` disarms the reel instead).
    #[allow(dead_code)]
    pub fn snap(&mut self, value: f32) {
        self.x = value;
        self.v = 0.0;
        self.target = value;
    }

    /// Integrate toward `target`. Large dt is split into fixed 1/240 s hops
    /// so a slow frame can never explode the state (skwd-wall's stability
    /// contract).
    pub fn tick(&mut self, dt: f32) {
        let mut remaining = dt.min(0.05);
        let hop = 1.0 / 240.0;
        while remaining > 0.0 {
            let step = remaining.min(hop);
            self.v += (-self.k * (self.x - self.target) - self.c * self.v) * step;
            self.x += self.v * step;
            remaining -= step;
        }
        if self.settled() {
            self.x = self.target;
            self.v = 0.0;
        }
    }

    /// The oscillator considers itself done within skwd-wall's tolerance.
    pub fn settled(&self) -> bool {
        (self.x - self.target).abs() < 0.05 && self.v.abs() < 0.5
    }

    /// Display value: exact target once settled, live x otherwise.
    pub fn value(&self) -> f32 {
        if self.settled() { self.target } else { self.x }
    }
}

// ── Driver state (pure — main.rs pumps it from its display timer) ───────

use std::sync::Mutex;

static REEL: Mutex<Option<ReelSpring>> = Mutex::new(None);

/// Arm the reel toward `target`. When idle the spring starts at `current`
/// (the displayed position); while gliding, the target is retargeted and
/// momentum is preserved (chained steps glide instead of restarting).
/// A non-positive duration snaps instantly.
pub fn glide_to(current: f32, target: f32, duration_ms: f32) {
    let mut reel = REEL.lock().unwrap();
    if duration_ms <= 0.0 {
        *reel = None;
        return;
    }
    match reel.as_mut() {
        Some(spring) => spring.retarget(target),
        None => {
            let mut spring = ReelSpring::for_duration_ms(current, duration_ms);
            spring.retarget(target);
            *reel = Some(spring);
        }
    }
}

/// Advance the reel by `dt` seconds. `Some(new_value)` while it moves —
/// including the final exact target write — then `None` when idle.
pub fn step(dt: f32) -> Option<f32> {
    let mut reel = REEL.lock().unwrap();
    let Some(spring) = reel.as_mut() else {
        return None;
    };
    spring.tick(dt);
    let value = spring.value();
    if spring.settled() {
        *reel = None;
    }
    Some(value)
}

/// Disarm the reel (rebase path, programmatic snaps — the caller writes the
/// new position to the property itself).
pub fn snap() {
    *REEL.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-6;

    /// Integrate until settled; returns (settle_ms, max_overshoot).
    fn run_until_settled(s: &mut ReelSpring) -> (f32, f32) {
        let start_delta = (s.target - s.x).abs();
        let mut t = 0.0f32;
        let mut overshoot = 0.0f32;
        for _ in 0..(240 * 5) {
            s.tick(1.0 / 240.0);
            t += 1000.0 / 240.0;
            let delta = (s.target - s.x).abs();
            if delta > start_delta + EPS {
                overshoot = overshoot.max(delta - start_delta);
            }
            if s.settled() {
                return (t, overshoot);
            }
        }
        (t, overshoot)
    }

    #[test]
    fn spring_settles_around_requested_duration_without_overshoot() {
        let mut s = ReelSpring::for_duration_ms(0.0, 350.0);
        s.retarget(1.0);
        let (ms, overshoot) = run_until_settled(&mut s);
        assert!(
            (250.0..=500.0).contains(&ms),
            "350ms spring must settle near its duration, took {:.0}ms",
            ms
        );
        assert_eq!(overshoot, 0.0, "critical damping must never overshoot");
        assert_eq!(s.value(), 1.0, "value reaches target exactly");
    }

    #[test]
    fn spring_retarget_keeps_velocity_and_converges() {
        let mut s = ReelSpring::for_duration_ms(0.0, 300.0);
        s.retarget(10.0);
        s.tick(1.0 / 240.0);
        s.tick(1.0 / 240.0);
        let v_mid = s.v;
        assert!(v_mid > 0.0, "spring must build velocity toward the target");
        // Chained step: retarget mid-flight — velocity must NOT reset.
        s.retarget(11.0);
        assert_eq!(s.v, v_mid, "retarget must keep momentum (glide continuity)");
        for _ in 0..(240 * 5) {
            s.tick(1.0 / 240.0);
            if s.settled() {
                break;
            }
        }
        assert!(s.settled(), "retargeted spring must converge");
        assert_eq!(s.value(), 11.0);
    }

    #[test]
    fn spring_survives_a_huge_frame_step() {
        let mut s = ReelSpring::for_duration_ms(0.0, 300.0);
        s.retarget(5.0);
        s.tick(0.05); // 50ms frame — worst case
        s.tick(0.05);
        assert!(s.x.is_finite() && s.v.is_finite(), "sub-stepping must stay stable");
    }

    #[test]
    fn snap_jumps_and_zeroes_velocity() {
        let mut s = ReelSpring::for_duration_ms(0.0, 300.0);
        s.retarget(10.0);
        s.tick(1.0 / 240.0);
        s.snap(3.0);
        assert_eq!(s.x, 3.0);
        assert_eq!(s.v, 0.0);
        assert!(s.settled(), "snap lands exactly on the value");
    }

    #[test]
    fn glide_and_step_converge_then_report_idle() {
        let _serial = test_serial();
        glide_to(0.0, 1.0, 300.0);
        let mut moved = false;
        let mut last = 0.0f32;
        for _ in 0..(240 * 5) {
            if let Some(v) = step(1.0 / 240.0) {
                moved = true;
                last = v;
            } else {
                break;
            }
        }
        assert!(moved, "step must report movement while gliding");
        assert_eq!(last, 1.0, "the final reported value is the target");
        assert_eq!(step(1.0 / 240.0), None, "idle after settling");
    }

    #[test]
    fn snap_disarms_the_reel() {
        let _serial = test_serial();
        glide_to(0.0, 5.0, 300.0);
        assert!(step(1.0 / 240.0).is_some(), "armed reel moves");
        snap();
        assert_eq!(step(1.0 / 240.0), None, "snap disarms — no further movement");
    }

    #[test]
    fn glide_to_zero_duration_snaps_instantly() {
        let _serial = test_serial();
        glide_to(0.0, 3.0, 0.0);
        assert_eq!(step(1.0 / 240.0), None, "zero duration = already settled");
        assert_eq!(step(1.0 / 240.0), None);
    }

    /// The driver owns one process-global reel — the driver tests must not
    /// interleave (cargo test runs in parallel by default).
    fn test_serial() -> std::sync::MutexGuard<'static, ()> {
        static DRIVER_LOCK: Mutex<()> = Mutex::new(());
        DRIVER_LOCK.lock().unwrap()
    }
}
