/// Fused clamp→multiply→quantise (linear)→log→gate kernel.
/// Inputs `old_value_log`/outputs remain in log domain, while [`normalize_bounds`],
/// [`sanitize_quantum`] and [`sanitize_eps`] keep the linear step numerically safe
/// (no denormals, NaNs, or long-run bias thanks to ties-to-even rounding).
pub fn log_mul_eps(
    old_value: f64,
    a: f64,
    b: f64,
    eps: f64,
    min_r: f64,
    max_r: f64,
    quantum: f64,
) -> f64 {
    let (lo, hi) = normalize_bounds(min_r, max_r);
    let eps = sanitize_eps(eps);
    let quantum = sanitize_quantum(quantum, lo);
    let inv_quantum = quantum.recip();

    let ac = clamp_operand(a, lo, hi);
    let bc = clamp_operand(b, lo, hi);

    // Multiply while keeping the result within the sanitised range.
    let product = (ac * bc).clamp(lo, hi);

    // Quantise in linear space using ties-to-even to avoid long-run bias.
    let quantised_linear = quantize_ties_even_linear(product, inv_quantum, quantum).clamp(lo, hi);

    // Convert back to log space with a path that preserves precision near one.
    let new_log = ln_near_one(quantised_linear);

    if !old_value.is_finite() {
        return new_log;
    }

    if eps > 0.0 && (new_log - old_value).abs() < eps {
        old_value
    } else {
        new_log
    }
}

/// Sanitise the caller-provided clamp range before entering the hot loop.
#[inline(always)]
fn normalize_bounds(min_r: f64, max_r: f64) -> (f64, f64) {
    let mut lo = min_r.min(max_r);
    let mut hi = min_r.max(max_r);

    // Keep bounds ≥ MIN_POSITIVE so subsequent math avoids denormals.
    if !lo.is_finite() || lo <= 0.0 {
        lo = f64::MIN_POSITIVE;
    }
    if !hi.is_finite() || hi < lo {
        hi = lo;
    }
    (lo, hi)
}

/// Keep the epsilon gate positive and finite.
#[inline(always)]
fn sanitize_eps(eps: f64) -> f64 {
    if eps.is_finite() {
        eps.abs()
    } else {
        0.0
    }
}

/// Ensure the linear quantisation step never drops below a meaningful minimum.
#[inline(always)]
fn sanitize_quantum(quantum_lin: f64, lo: f64) -> f64 {
    const ABS_MIN_Q: f64 = 1e-12;
    // Floor quantisation to ≥ max(1e-12, ~1 ULP at the lower bound) so steps stay meaningful.
    let min_step = (f64::EPSILON * lo).max(ABS_MIN_Q);
    if quantum_lin.is_finite() && quantum_lin > 0.0 {
        quantum_lin.max(min_step)
    } else {
        min_step
    }
}

/// Defensive clamp used for both operands; treats NaN/Inf as bound hits.
#[inline(always)]
fn clamp_operand(value: f64, lo: f64, hi: f64) -> f64 {
    let sanitized = if value.is_nan() {
        lo
    } else if !value.is_finite() {
        if value.is_sign_negative() {
            lo
        } else {
            hi
        }
    } else {
        value
    };
    sanitized.max(lo).min(hi)
}

/// Scale→round→rescale using ties-to-even to avoid long-run bias.
#[inline(always)]
fn quantize_ties_even_linear(value: f64, inv_quantum: f64, quantum: f64) -> f64 {
    let scaled = value * inv_quantum;
    round_ties_even(scaled) * quantum
}

/// IEEE-754 round-to-nearest, ties-to-even with an ULP-scaled tie slack.
#[inline(always)]
fn round_ties_even(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }

    // IEEE-754 round-to-nearest, ties-to-even with ULP-scaled slack for half-way detection.
    let t = x.trunc();
    let frac = (x - t).abs();
    let slack = ulp(x);

    if frac < 0.5 - slack {
        return t;
    }
    if frac > 0.5 + slack {
        return t + x.signum();
    }

    // `t` is integral up to 2^53; beyond that every representable value is already even.
    if t.rem_euclid(2.0) == 0.0 {
        t
    } else {
        t + x.signum()
    }
}

/// Compute the unit in the last place around `x` (handles zero and infinities).
#[inline(always)]
fn ulp(x: f64) -> f64 {
    if !x.is_finite() {
        return 0.0;
    }
    if x == 0.0 {
        return f64::MIN_POSITIVE;
    }
    let bits = x.to_bits();
    if x > 0.0 {
        (f64::from_bits(bits + 1) - x).abs()
    } else {
        (x - f64::from_bits(bits - 1)).abs()
    }
}

/// Accurate natural log for values close to one; falls back to `ln` otherwise.
#[inline(always)]
fn ln_near_one(x: f64) -> f64 {
    debug_assert!(x > 0.0 && x.is_finite());
    let delta = x - 1.0;
    // Switch to log1p when |x-1| ≤ 1e-6; empirically this threshold balances accuracy and cost.
    if delta.abs() <= 1e-6 {
        delta.ln_1p()
    } else {
        x.ln()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_operand, ln_near_one, log_mul_eps, normalize_bounds, quantize_ties_even_linear,
        round_ties_even, sanitize_eps, sanitize_quantum,
    };
    use rand::{rngs::StdRng, Rng, SeedableRng};

    #[test]
    fn near_one_values_quantise_cleanly() {
        let old = 0.0;
        let a = 1.0005;
        let b = 1.0004;
        let quantum = 1e-6;
        let min_r = 0.5;
        let max_r = 2.0;

        let result = log_mul_eps(old, a, b, 1e-12, min_r, max_r, quantum);
        let (lo, hi) = normalize_bounds(min_r, max_r);
        let quantum = sanitize_quantum(quantum, lo);
        let inv_q = quantum.recip();

        let ac = clamp_operand(a, lo, hi);
        let bc = clamp_operand(b, lo, hi);
        let product = ac * bc;
        let quantised_linear = quantize_ties_even_linear(product, inv_q, quantum).clamp(lo, hi);
        let expected_log = ln_near_one(quantised_linear);
        let actual_linear = result.exp();
        let tol = quantum * 0.5 + f64::EPSILON;

        assert!(
            (actual_linear - quantised_linear).abs() <= tol,
            "linear diff too large: actual={actual_linear}, target={quantised_linear}"
        );
        assert!(
            (result - expected_log).abs() <= tol.max(1e-12),
            "log diff too large: result={result}, expected={expected_log}"
        );
    }

    #[test]
    fn clamps_inputs_to_bounds() {
        let old = 0.0;
        let a = 10.0;
        let b = 0.0001;
        let result = log_mul_eps(old, a, b, 1e-12, 0.1, 2.0, 1e-3);
        let linear = result.exp();

        assert!(linear <= 2.0 + 1e-12);
        assert!(linear >= 0.1 - 1e-12);
    }

    #[test]
    fn epsilon_gate_prevents_small_changes() {
        let old = 0.0;
        let a = 1.0 + 2.0e-6;
        let b = 1.0;
        let quantum = 1e-6;

        let raw = log_mul_eps(old, a, b, 0.0, 0.5, 2.0, quantum);
        let diff = (raw - old).abs();
        assert!(diff > 0.0);

        let eps = 5e-6;
        assert!(diff < eps);

        let gated = log_mul_eps(old, a, b, eps, 0.5, 2.0, quantum);
        assert_eq!(gated, old);
    }

    #[test]
    fn idempotent_when_reapplied() {
        let old = 0.0;
        let a = 1.01;
        let b = 0.99;
        let first = log_mul_eps(old, a, b, 1e-12, 0.5, 2.0, 1e-4);
        let second = log_mul_eps(first, a, b, 1e-12, 0.5, 2.0, 1e-4);
        assert_eq!(first, second);
    }

    #[test]
    fn nan_inputs_default_to_bounds() {
        let old = 0.0;
        let result = log_mul_eps(old, f64::NAN, f64::INFINITY, 1e-12, 0.1, 10.0, 1e-3);
        let linear = result.exp();
        assert!(linear.is_finite());
        assert!(linear >= 0.1 - 1e-12);
        assert!(linear <= 10.0 + 1e-12);
    }

    #[test]
    fn random_walk_matches_reference_within_band() {
        let mut rng = StdRng::seed_from_u64(0x1234_5678_90AB_CDEF);
        let min_r = 0.5;
        let max_r = 2.0;
        let quantum = 1e-5;
        let eps = 5e-6;
        let (lo, hi) = normalize_bounds(min_r, max_r);
        let q = sanitize_quantum(quantum, lo);
        let inv_q = q.recip();

        let mut state = 0.0;
        for _ in 0..4096 {
            let a = 1.0 + rng.random_range(-5e-4..=5e-4);
            let b = 1.0 + rng.random_range(-5e-4..=5e-4);

            let next = log_mul_eps(state, a, b, eps, min_r, max_r, quantum);

            let ac = clamp_operand(a, lo, hi);
            let bc = clamp_operand(b, lo, hi);
            let product = ac * bc;
            let quantised = quantize_ties_even_linear(product, inv_q, q).clamp(lo, hi);
            let expected_log = ln_near_one(quantised);
            let gated_expected = if eps > 0.0 && (expected_log - state).abs() < eps {
                state
            } else {
                expected_log
            };

            assert!(
                (next - gated_expected).abs() <= f64::EPSILON,
                "drift exceeded: next={next}, expected={gated_expected}"
            );

            state = next;
        }
    }

    #[test]
    fn ties_even_rounding_is_unbiased() {
        // Check positive and negative halfway cases.
        assert_eq!(round_ties_even(1.5), 2.0);
        assert_eq!(round_ties_even(2.5), 2.0);
        assert_eq!(round_ties_even(-1.5), -2.0);
        assert_eq!(round_ties_even(-2.5), -2.0);
        assert_eq!(round_ties_even(3.49), 3.0);
        assert_eq!(round_ties_even(-3.49), -3.0);
    }

    #[test]
    fn quantum_sanitizer_falls_back_to_floor() {
        let lo = 0.5;
        let q = sanitize_quantum(0.0, lo);
        assert!(q >= f64::EPSILON * lo);
        let q_nan = sanitize_quantum(f64::NAN, lo);
        assert!(q_nan >= f64::EPSILON * lo);
    }

    #[test]
    fn sanitize_eps_clamps_non_finite_and_flips_sign() {
        assert_eq!(sanitize_eps(-1e-4), 1e-4);
        assert_eq!(sanitize_eps(f64::INFINITY), 0.0);
        assert_eq!(sanitize_eps(f64::NEG_INFINITY), 0.0);
        assert_eq!(sanitize_eps(f64::NAN), 0.0);
    }

    #[test]
    fn normalize_bounds_swaps_and_enforces_min_positive() {
        let (lo, hi) = normalize_bounds(2.0, 0.5);
        assert_eq!(lo, 0.5);
        assert_eq!(hi, 2.0);

        let (lo_neg, hi_neg) = normalize_bounds(-3.0, 5.0);
        assert_eq!(lo_neg, f64::MIN_POSITIVE);
        assert_eq!(hi_neg, 5.0);

        let (lo_inf, hi_inf) = normalize_bounds(f64::INFINITY, f64::NEG_INFINITY);
        assert_eq!(lo_inf, f64::MIN_POSITIVE);
        assert_eq!(hi_inf, f64::MIN_POSITIVE);
    }

    #[test]
    fn clamp_operand_handles_non_finite_inputs() {
        let (lo, hi) = normalize_bounds(0.5, 2.0);
        assert_eq!(clamp_operand(f64::NAN, lo, hi), lo);
        assert_eq!(clamp_operand(f64::INFINITY, lo, hi), hi);
        assert_eq!(clamp_operand(f64::NEG_INFINITY, lo, hi), lo);
    }

    #[test]
    fn ties_even_reduces_bias_relative_to_ties_away() {
        let (lo, _) = normalize_bounds(0.5, 2.0);
        let quantum = sanitize_quantum(1e-4, lo);
        let inv_q = quantum.recip();

        let mut bias_even = 0.0;
        let mut bias_away = 0.0;
        let samples = 50_000usize;
        let base = (1.0 / quantum).round() as i64;
        let tie_hi = (base as f64 + 0.5) * quantum;
        let tie_lo = (base as f64 - 0.5) * quantum;

        for i in 0..samples {
            let value = if i % 2 == 0 { tie_hi } else { tie_lo };
            let even = quantize_ties_even_linear(value, inv_q, quantum);
            let away = quantize_ties_away_linear(value, inv_q, quantum);
            bias_even += even - value;
            bias_away += away - value;
        }

        let bias_even_avg = bias_even / samples as f64;
        let bias_away_avg = bias_away / samples as f64;

        assert!(
            bias_even_avg.abs() < 1e-7,
            "ties-even bias should hover near zero, got avg {bias_even_avg}"
        );
        assert!(
            bias_away_avg.abs() > bias_even_avg.abs() * 10.0,
            "ties-away should introduce more bias: away_avg={bias_away_avg}, even_avg={bias_even_avg}"
        );
    }

    fn round_ties_away(x: f64) -> f64 {
        if !x.is_finite() {
            return x;
        }
        let floor = x.floor();
        let ceil = floor + 1.0;
        let frac = x - floor;
        if frac < 0.5 {
            floor
        } else if frac > 0.5 {
            ceil
        } else if x >= 0.0 {
            ceil
        } else {
            floor
        }
    }

    fn quantize_ties_away_linear(value: f64, inv_quantum: f64, quantum: f64) -> f64 {
        round_ties_away(value * inv_quantum) * quantum
    }
}
