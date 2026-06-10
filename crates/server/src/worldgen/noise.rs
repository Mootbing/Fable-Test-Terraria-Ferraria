//! 1D layered value noise for the surface heightmap and underworld
//! ceiling/floor lines (DESIGN §1.2 passes 1 and 4). Lattice values are drawn
//! from the shared deterministic [`Pcg32`], so equal seeds give equal
//! terrain on every platform.

use ferraria_shared::rng::Pcg32;

/// One octave of value noise: random lattice values every `wavelength`
/// tiles, smoothstep-interpolated between them. Output is in `[-1, 1]`.
pub struct Noise1d {
    values: Vec<f32>,
    wavelength: f32,
}

impl Noise1d {
    /// Covers `0..length` tiles with lattice points `wavelength` apart.
    pub fn new(rng: &mut Pcg32, length: u32, wavelength: f32) -> Noise1d {
        let wavelength = wavelength.max(1.0);
        let points = (length as f32 / wavelength).ceil() as usize + 2;
        Noise1d {
            values: (0..points).map(|_| rng.gen_range_f32(-1.0, 1.0)).collect(),
            wavelength,
        }
    }

    pub fn sample(&self, x: f32) -> f32 {
        let t = (x / self.wavelength).max(0.0);
        let i = (t as usize).min(self.values.len() - 2);
        let f = (t - i as f32).clamp(0.0, 1.0);
        let s = f * f * (3.0 - 2.0 * f); // smoothstep
        self.values[i] + (self.values[i + 1] - self.values[i]) * s
    }
}

/// Layered octaves: `sample(x) = Σ amplitude_i × octave_i(x)`.
pub struct LayeredNoise {
    octaves: Vec<(Noise1d, f32)>,
}

impl LayeredNoise {
    /// `octaves` are `(wavelength, amplitude)` pairs (§1.2 pass 1 uses
    /// 120/40, 40/15, 10/5).
    pub fn new(rng: &mut Pcg32, length: u32, octaves: &[(f32, f32)]) -> LayeredNoise {
        LayeredNoise {
            octaves: octaves
                .iter()
                .map(|&(wl, amp)| (Noise1d::new(rng, length, wl), amp))
                .collect(),
        }
    }

    pub fn sample(&self, x: f32) -> f32 {
        self.octaves.iter().map(|(n, amp)| n.sample(x) * amp).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_bounded_and_smooth() {
        let mut rng = Pcg32::new(7);
        let n = Noise1d::new(&mut rng, 1000, 25.0);
        let mut prev = n.sample(0.0);
        for i in 1..1000 {
            let v = n.sample(i as f32);
            assert!((-1.0..=1.0).contains(&v), "out of range at {i}: {v}");
            // One tile of a 25-tile wavelength can't jump far.
            assert!((v - prev).abs() < 0.25, "kink at {i}");
            prev = v;
        }
    }

    #[test]
    fn layered_noise_is_deterministic() {
        let a: Vec<f32> = {
            let mut rng = Pcg32::new(42);
            let n = LayeredNoise::new(&mut rng, 500, &[(120.0, 40.0), (40.0, 15.0), (10.0, 5.0)]);
            (0..500).map(|x| n.sample(x as f32)).collect()
        };
        let mut rng = Pcg32::new(42);
        let n = LayeredNoise::new(&mut rng, 500, &[(120.0, 40.0), (40.0, 15.0), (10.0, 5.0)]);
        for (x, &v) in a.iter().enumerate() {
            assert_eq!(v, n.sample(x as f32));
        }
        // Amplitudes bound the sum.
        assert!(a.iter().all(|v| v.abs() <= 60.0));
    }
}
