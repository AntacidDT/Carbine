//! The generator substrate: a seed, a stream of numbers, and a noise field.
//!
//! Everything under `carbine.procedural` is allowed to be surprising about its
//! shape but not about its answers: the same seed has to give the same texture,
//! the same town, the same storm, on this machine and on the four others in a
//! session. So the maths lives here once - one integer mixer, one lattice hash,
//! one value-noise and its summed octaves - and a generator asks it for numbers
//! instead of carrying a private hash that drifts from the others.
//!
//! Nothing in here draws or spawns anything. It is the dice, not the game.

use glam::Vec3;

/// A number a world is generated from. Written by a script as an integer or as a
/// word, which is hashed, so `Seed::of("pine")` is a biome a friend can type in
/// and stand in the same forest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seed(pub u64);

impl Seed {
    /// The seed every generator falls back to when a script names none.
    pub const DEFAULT: Seed = Seed(0x5EED_1234_ABCD_0001);

    /// A seed from a word. The hash is FNV-1a and is never changed: a saved
    /// level's name must keep meaning the level it meant.
    pub fn of(text: &str) -> Self {
        let mut value: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in text.as_bytes() {
            value ^= *byte as u64;
            value = value.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self(value)
    }

    /// A seed from a number the script can read back out and type to somebody
    /// else, which is the whole point of reproducibility.
    pub fn of_number(value: i64) -> Self {
        Self(value as u64)
    }
}

impl Default for Seed {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The one integer mixer everything below is built from: splitmix64's round,
/// chosen because it is four operations long, has no look-up table to disagree
/// about, and is well past good enough for scattering drops.
#[inline]
fn mix(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Turns a hashed integer into a fraction of one, using the 24 bits a float keeps
/// exactly. Every number in this module that a script can read is 0..1 this way,
/// so 0.3 of something means the same thing wherever it was asked.
#[inline]
fn fraction(value: u64) -> f32 {
    ((value >> (64 - 24)) as f32) * (1.0 / ((1_u64 << 24) as f32))
}

/// A stream of numbers from a seed. A stream is only reproducible if it is walked
/// in the same order, which is why generators take their places from [`scatter`]
/// rather than threading one of these through their own code.
#[derive(Clone, Copy, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: Seed) -> Self {
        Self { state: seed.0 }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        mix(self.state)
    }

    /// The next number, 0..1.
    pub fn next_f32(&mut self) -> f32 {
        fraction(self.next_u64())
    }

    /// The next number between two bounds. Either end may be the larger one, so a
    /// script does not have to know which way round it typed them.
    pub fn between(&mut self, low: f32, high: f32) -> f32 {
        let (low, high) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        low + (high - low) * self.next_f32()
    }

    /// A point inside a box of these dimensions centred on the origin, which is
    /// the shape of every spawn area a generator asks for.
    pub fn inside(&mut self, area: Vec3) -> Vec3 {
        Vec3::new(
            self.between(-area.x, area.x) * 0.5,
            self.between(-area.y, area.y) * 0.5,
            self.between(-area.z, area.z) * 0.5,
        )
    }

    /// An index into a list of this many choices, or `None` when there is nothing
    /// to choose between.
    pub fn pick(&mut self, choices: usize) -> Option<usize> {
        if choices == 0 {
            return None;
        }
        Some((self.next_u64() % choices as u64) as usize)
    }

    /// A whole number in a range, which is how a building generator rolls floors.
    pub fn count(&mut self, low: i32, high: i32) -> i32 {
        if high <= low {
            return low;
        }
        low + (self.next_u64() % (high - low + 1) as u64) as i32
    }

    /// The next `how_many` numbers of the stream, for the tests and for a script
    /// that wants to hand an answer back.
    pub fn stream(&mut self, how_many: usize) -> Vec<f32> {
        (0..how_many).map(|_| self.next_f32()).collect()
    }
}

impl Default for Rng {
    fn default() -> Self {
        Self::new(Seed::default())
    }
}

#[inline]
fn lattice3(x: i64, y: i64, z: i64, seed: Seed) -> f32 {
    let mixed = mix(seed.0
        ^ (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as u64)
            .rotate_left(21)
            .wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
        ^ (z as u64)
            .rotate_left(42)
            .wrapping_mul(0x1656_67b1_9e37_79f9));
    fraction(mixed)
}

/// The curve that makes a noise field continuous between its lattice points: flat
/// at both ends, so the field has no crease where two cells meet.
#[inline]
fn smooth(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}

/// Walk from one number to another by a fraction of the way between them.
#[inline]
fn blend(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

/// A dice roll keyed to a place, 0..1, with no field around it: what a scatter or
/// a per-pixel pattern asks when it needs a number and not a gradient.
pub fn hash(point: Vec3, seed: Seed) -> f32 {
    lattice3(
        point.x.to_bits() as i64,
        point.y.to_bits() as i64,
        point.z.to_bits() as i64,
        seed,
    )
}

/// Value noise: a random value at every whole number and the eight around a point
/// blended by how close it is. This is the field a biome is a mask of and a cloud
/// is a threshold of.
pub fn noise3(point: Vec3, seed: Seed) -> f32 {
    let base = Vec3::new(point.x.floor(), point.y.floor(), point.z.floor());
    let part = point - base;
    let (ix, iy, iz) = (base.x as i64, base.y as i64, base.z as i64);
    let (fx, fy, fz) = (smooth(part.x), smooth(part.y), smooth(part.z));

    let corner = |dx: i64, dy: i64, dz: i64| lattice3(ix + dx, iy + dy, iz + dz, seed);

    let low = blend(corner(0, 0, 0), corner(1, 0, 0), fx);
    let high = blend(corner(0, 1, 0), corner(1, 1, 0), fx);
    let near = blend(low, high, fy);

    let low = blend(corner(0, 0, 1), corner(1, 0, 1), fx);
    let high = blend(corner(0, 1, 1), corner(1, 1, 1), fx);
    let far = blend(low, high, fy);

    blend(near, far, fz)
}

/// The same field on the ground plane, at a height of zero.
pub fn noise2(x: f32, y: f32, seed: Seed) -> f32 {
    noise3(Vec3::new(x, y, 0.0), seed)
}

/// Noise again and again at doubling size and halving weight, which is what makes
/// a field look like weather instead of a checkerboard. `octaves` is held to eight
/// because past that the detail is smaller than a pixel and only costs time.
pub fn fbm3(point: Vec3, seed: Seed, octaves: u32) -> f32 {
    let octaves = octaves.clamp(1, 8);
    let mut total = 0.0f32;
    let mut weight = 0.0f32;
    let mut scale = 1.0f32;

    for octave in 0..octaves {
        let amplitude = 0.5f32.powi(octave as i32);
        total += noise3(point * scale, seed) * amplitude;
        weight += amplitude;
        scale *= 2.0;
    }

    total / weight
}

/// The plane's version of the same sum.
pub fn fbm2(x: f32, y: f32, seed: Seed, octaves: u32) -> f32 {
    fbm3(Vec3::new(x, y, 0.0), seed, octaves)
}

/// The dice behind the `index`-th thing a generator lays out from `seed`.
///
/// Rolling each position from its own index, rather than from one shared stream,
/// is what lets a generator grow: a new thing added at the end cannot move the
/// ones already placed. Anything else a generator wants to roll for that thing -
/// a size, an age, a choice of brick - draws from the same dice, after the place.
pub fn salt(seed: Seed, index: usize) -> Seed {
    Seed(mix(
        seed.0 ^ (index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
    ))
}

/// `count` places inside a box, in the same order every time, from a seed alone.
///
/// Each place is rolled from its own index, so a generator that has been given
/// more drops keeps the places of the ones it already had: a storm going from 900
/// to 1,000 adds a hundred and does not shuffle the rest.
pub fn scatter(count: usize, area: Vec3, seed: Seed) -> Vec<Vec3> {
    (0..count)
        .map(|index| Rng::new(salt(seed, index)).inside(area))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_written_as_a_word_means_the_same_thing_everywhere() {
        assert_eq!(Seed::of("pine"), Seed::of("pine"));
        assert_ne!(Seed::of("pine"), Seed::of("pineh"));
        assert_eq!(Seed::of_number(7), Seed(7));
    }

    #[test]
    fn a_stream_of_numbers_replays_itself() {
        let first = Rng::new(Seed::of("yard")).stream(64);
        let again = Rng::new(Seed::of("yard")).stream(64);
        let other = Rng::new(Seed::of("barn")).stream(64);

        assert_eq!(first, again);
        assert_ne!(first, other);
    }

    #[test]
    fn the_numbers_land_where_they_said_they_would() {
        let mut rng = Rng::new(Seed::of("numbers"));
        for _ in 0..2_000 {
            let value = rng.next_f32();
            assert!((0.0..1.0).contains(&value), "{value} is not a fraction");

            let thrown = rng.between(-3.5, 2.0);
            assert!((-3.5..=2.0).contains(&thrown), "{thrown} fell outside");

            // Written backwards, still the same range.
            let flipped = rng.between(2.0, -3.5);
            assert!((-3.5..=2.0).contains(&flipped), "{flipped} fell outside");
        }
    }

    #[test]
    fn a_stream_spreads_over_its_choices() {
        let mut rng = Rng::new(Seed::of("choices"));
        assert_eq!(rng.pick(0), None);

        let mut seen = [0u32; 4];
        for _ in 0..400 {
            let index = rng.pick(4).unwrap();
            seen[index] += 1;
        }
        for count in seen {
            assert!(count > 40, "a choice only came up {count} times in 400");
        }

        assert_eq!(rng.count(3, 3), 3);
        for _ in 0..200 {
            let rolled = rng.count(2, 6);
            assert!((2..=6).contains(&rolled), "{rolled} is outside the roll");
        }
    }

    #[test]
    fn a_point_inside_a_box_stays_inside_it() {
        let area = Vec3::new(8.0, 6.0, 2.0);
        let mut rng = Rng::new(Seed::of("box"));
        for _ in 0..1_000 {
            let point = rng.inside(area);
            assert!(
                point.x.abs() <= 4.0 && point.y.abs() <= 3.0 && point.z.abs() <= 1.0,
                "{point} is outside the box it was rolled in"
            );
        }
    }

    #[test]
    fn a_noise_field_is_bounded_and_continuous() {
        let seed = Seed::of("clouds");
        let mut previous = noise3(Vec3::new(0.0, 0.5, 0.25), seed);

        for step in 1..1_000 {
            let x = step as f32 * 0.01;
            let value = noise3(Vec3::new(x, 0.5, 0.25), seed);
            assert!((0.0..=1.0).contains(&value), "{value} escaped the field");
            assert!(
                (value - previous).abs() < 0.05,
                "the field jumped from {previous} to {value} across a hundredth of a meter"
            );
            previous = value;
        }

        assert!((0.0..=1.0).contains(&noise2(3.7, -1.2, seed)));
    }

    #[test]
    fn the_field_answers_the_same_question_the_same_way() {
        let seed = Seed::of("lattice");
        let at = Vec3::new(12.0, -3.0, 7.0);

        assert_eq!(noise3(at, seed), noise3(at, seed));
        assert_ne!(noise3(at, seed), noise3(at, Seed::of("latticf")));
        // A step of one whole meter is a step to the next lattice point, so the
        // field does repeat its corners - which is exactly why a generator wants
        // octaves rather than one raw pass.
        assert!(noise3(at, seed) != noise3(at + Vec3::new(0.37, 0.0, 0.0), seed));
    }

    #[test]
    fn octaves_add_detail_without_leaving_the_field() {
        let seed = Seed::of("terrain");
        let at = Vec3::new(2.0, 5.0, 0.0);

        let one = fbm3(at, seed, 1);
        let many = fbm3(at, seed, 6);

        assert!((0.0..=1.0).contains(&one), "{one}");
        assert!((0.0..=1.0).contains(&many), "{many}");
        assert_ne!(one, many, "an octave that changed nothing is not an octave");
        assert!((0.0..=1.0).contains(&fbm2(1.5, 9.0, seed, 4)));

        // No octaves, or a hundred, both mean a sane middle ground.
        assert_eq!(fbm3(at, seed, 0), one);
        assert_eq!(fbm3(at, seed, 9), fbm3(at, seed, 8));
    }

    #[test]
    fn a_scatter_is_a_list_of_places_that_does_not_move() {
        let area = Vec3::new(10.0, 10.0, 1.0);
        let seed = Seed::of("rain");

        let first = scatter(500, area, seed);
        assert_eq!(
            first,
            scatter(500, area, seed),
            "the same storm dropped its drops elsewhere"
        );
        assert_ne!(first, scatter(500, area, Seed::of("rein")));

        for point in &first {
            assert!(point.x.abs() <= 5.0 && point.y.abs() <= 5.0 && point.z.abs() <= 0.5);
        }

        // And not so many of them in the same spot that a veil of them draws one
        // thick line instead of weather.
        let distinct: std::collections::HashSet<[i32; 3]> = first
            .iter()
            .map(|point| {
                [
                    (point.x * 100.0) as i32,
                    (point.y * 100.0) as i32,
                    (point.z * 100.0) as i32,
                ]
            })
            .collect();
        assert!(
            distinct.len() > 480,
            "only {} distinct places",
            distinct.len()
        );
    }

    #[test]
    fn a_scatter_keeps_the_places_it_had_when_it_grows() {
        let area = Vec3::new(20.0, 20.0, 4.0);
        let seed = Seed::of("storm");

        let thin = scatter(200, area, seed);
        let thick = scatter(320, area, seed);
        assert_eq!(&thick[..200], &thin[..]);
    }

    #[test]
    fn a_hash_of_a_place_answers_the_same_way_twice() {
        let at = Vec3::new(1.5, -2.25, 0.75);
        let seed = Seed::of("grain");

        assert_eq!(hash(at, seed), hash(at, seed));
        assert_ne!(hash(at, seed), hash(at + Vec3::new(0.0, 0.0, 0.001), seed));
        assert!((0.0..1.0).contains(&hash(at, seed)));
    }
}
