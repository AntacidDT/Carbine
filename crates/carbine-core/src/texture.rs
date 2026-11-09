//! The texture generator: a picture made from a seed instead of from a file somebody
//! had to find, check the licence on and pack.
//!
//! A script names what the surface is made of, what has gone wrong with it and how
//! finely it wants to see it, and the engine paints that tile and writes it as the
//! `.ktx2` the renderer already reads. Nothing here looks for a picture on disk and
//! nothing here needs a decoder: the pixels come out of the same dice every generator
//! rolls, and they go straight through Carbine's own BC7 writer.
//!
//! Two rules hold the whole module together. A field is asked for in position *across
//! the tile*, 0..1, and only ever from the wrapping noise in [`crate::procgen`] - a
//! tile that does not meet its own edges is a wall with a seam down it. And the pattern
//! is sized in counts, not in texels: six courses of brick are six courses at 256 and
//! at 1024, which is what leaves quality a knob about detail rather than about scale.

use std::fmt;
use std::path::{Path, PathBuf};

use glam::{IVec2, Vec2, Vec3};

use crate::procgen::{Seed, fbm_tile, salt, tile_hash};

/// How far a texel of height is worth in the picture: the tile is treated as one unit of
/// ground standing this tall, so a bump is leaned over by the light in proportion to how
/// fast the ground moves sideways under it.
const RELIEF: f32 = 0.22;

/// The direction the tile is lit from while it is painted - a sun over the shoulder,
/// which is how a hand-made texture gets its hollows. Unit length to a thousandth.
const SUN: [f32; 3] = [0.42, 0.26, 0.87];

/// How far the light reaches into a hollow and how much it leaves in shadow. The lit
/// side comes out about a sixth brighter than the flat colour and the dark side keeps a
/// little bounce, so no recipe has to be painted twice to be seen.
const AMBIENT: f32 = 0.72;
const KEY: f32 = 0.46;

/// The rolls a recipe asks its fields for, so no two of them answer the same question
/// and a moss patch never lines up with the dune under it.
const GROUND: usize = 1;
const PATCH: usize = 2;
const BLADE: usize = 3;
const GRAIN: usize = 4;
const CREASE: usize = 5;
const FINE: usize = 6;
const PEBBLE_AT: usize = 7;
const PEBBLE_WIDE: usize = 8;
const PEBBLE_KEEP: usize = 9;
const FACE: usize = 10;
const TILT: usize = 11;
const WARP: usize = 12;
const MOSS: usize = 13;
const DRY: usize = 14;
const DUST: usize = 15;
const SPECKLE: usize = 16;

/// Fields the height pass raises and the colour pass reads back, instead of rolling the
/// same noise twice. Three slots is the most any surface shares: rock keeps its ground
/// and its two creases, sand its ground and its pebbles, grass its blades. A surface
/// fills only the slots it uses, in the order its height recipe names them.
const SHARED: usize = 3;

/// What a generated picture is a surface of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextureType {
    /// A lawn: mounds of it, blades lying one way, and soil where it is thin.
    Grass,
    /// A beach or a desert: dunes, ripples running along them and loose grain.
    Sand,
    /// Broken stone: ridged ground, crevices where it turns back on itself, and pitting.
    Rock,
    /// A wall: courses of brick over a bed of mortar, every other one shifted.
    Brick,
}

impl TextureType {
    /// The kinds a script can name, in the order they are listed back to it when it
    /// names one that is not there.
    pub const NAMES: &'static [&'static str] = &["brick", "grass", "rock", "sand"];

    pub fn from_name(name: &str) -> Option<TextureType> {
        Some(match name.trim().to_lowercase().as_str() {
            "grass" | "lawn" | "turf" | "meadow" => Self::Grass,
            "sand" | "beach" | "desert" => Self::Sand,
            "rock" | "stone" | "granite" | "cliff" => Self::Rock,
            "brick" | "block" | "masonry" => Self::Brick,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Grass => "grass",
            Self::Sand => "sand",
            Self::Rock => "rock",
            Self::Brick => "brick",
        }
    }

    /// What this one can be gone wrong with. A name on this list is what a script
    /// writes for `variation`; anything else is refused rather than walked past.
    pub fn variations(self) -> &'static [&'static str] {
        match self {
            Self::Grass => &["dry", "moss"],
            Self::Sand => &["dust", "pebble"],
            Self::Rock => &["cracked", "mossy"],
            Self::Brick => &["dusty", "mossy"],
        }
    }
}

/// How finely the tile is painted: how far the detail is summed, and how many texels
/// are there to put it in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextureQuality {
    /// For a backdrop, a far-off roof, or a game that ships small.
    Low,
    /// The one a script gets when it names none.
    Medium,
    /// For the wall the player is stood in front of.
    High,
}

impl TextureQuality {
    /// Alphabetical, which is the order a script is offered them in.
    pub const NAMES: &'static [&'static str] = &["high", "low", "medium"];

    pub fn from_name(name: &str) -> Option<TextureQuality> {
        Some(match name.trim().to_lowercase().as_str() {
            "low" | "small" | "potato" => Self::Low,
            "medium" | "med" | "mid" | "standard" => Self::Medium,
            "high" | "hi" | "max" | "ultra" => Self::High,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Texels across the tile, which is square.
    pub fn size(self) -> u32 {
        match self {
            Self::Low => 256,
            Self::Medium => 512,
            Self::High => 1024,
        }
    }

    /// How many doublings of detail a field is summed over. Past seven the grain is
    /// finer than the texels that have to hold it.
    pub fn octaves(self) -> u32 {
        match self {
            Self::Low => 3,
            Self::Medium => 5,
            Self::High => 7,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TextureError {
    /// A kind the engine does not paint.
    Type { asked: String },
    /// A variation that kind does not have.
    Variation {
        kind: &'static str,
        asked: String,
        offered: &'static [&'static str],
    },
    /// A quality that is not one of the three.
    Quality { asked: String },
    /// A finished picture the BC7 encoder refused.
    Encoder { why: String },
    /// The bytes were ready and had nowhere to go.
    Write { path: String, why: String },
}

impl fmt::Display for TextureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Type { asked } => write!(
                f,
                "\"{asked}\" is not a surface Carbine can paint. It paints: {}.",
                TextureType::NAMES.join(", ")
            ),
            Self::Variation {
                kind,
                asked,
                offered,
            } => write!(
                f,
                "\"{asked}\" is not a way to ruin {kind}. {kind} takes: {}.",
                offered.join(", ")
            ),
            Self::Quality { asked } => write!(
                f,
                "\"{asked}\" is not a quality. Carbine paints at {}.",
                TextureQuality::NAMES.join(", ")
            ),
            Self::Encoder { why } => {
                write!(f, "the painted tile would not compress to BC7: {why}")
            }
            Self::Write { path, why } => write!(
                f,
                "the generated texture could not be written to \"{path}\": {why}"
            ),
        }
    }
}

impl std::error::Error for TextureError {}

/// A colour as a painter mixes it: display-encoded, 0..1 a channel, because the file
/// it lands in is read as a picture and not as light.
fn tint(r: f32, g: f32, b: f32) -> Vec3 {
    Vec3::new(r, g, b)
}

/// The walk from one colour to another, held to the two ends.
fn wash(from: Vec3, to: Vec3, at: f32) -> Vec3 {
    from.lerp(to, at.clamp(0.0, 1.0))
}

/// How far past a threshold a value has gone, eased across the band that leads to it so
/// nothing in a generated picture has a hard edge of a single field's making. Either end
/// of the band may be the higher one, so a script of a recipe can fade a mask in or out
/// with the same call.
fn shade(from: f32, to: f32, at: f32) -> f32 {
    let across = (at - from) / (to - from);
    let eased = across.clamp(0.0, 1.0);
    eased * eased * (3.0 - 2.0 * eased)
}

/// One point of the tile being painted, in position across it rather than in texels.
#[derive(Clone, Copy)]
struct Place {
    u: f32,
    v: f32,
    variation: Option<&'static str>,
}

impl Place {
    /// Whether the recipe is being painted with this thing gone wrong on it.
    fn ruined(&self, name: &str) -> bool {
        self.variation.is_some_and(|have| have == name)
    }
}

/// One texel of the tile being painted: where it is across the tile, which texel it is,
/// and what the script asked to have happened to the surface.
struct Spot {
    at: Place,
    /// The texel being filled, for the per-texel rolls a grain of sand or a fleck of
    /// soil is cut from.
    col: u32,
    row: u32,
}

/// What the light pass reads back from the height pass: the whole tile's heights and
/// the fields its recipes kept beside them.
#[derive(Clone, Copy)]
struct Raised<'a> {
    heights: &'a [f32],
    fields: &'a [[f32; SHARED]],
}

/// The tile area one paint worker is worth, which is where handing a row to another core
/// stops costing more than the row does. Below it a tile is painted by the caller alone.
const PAINT_PER_WORKER: usize = 4096;

/// How many tile rows go to one worker: as many bands as there are cores, and never finer
/// than one row apiece.
fn rows_per_worker(count: usize) -> usize {
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let lanes = (count * count / PAINT_PER_WORKER).min(cores).max(1);
    count.div_ceil(lanes)
}

/// One tile being painted: the dice it comes from and how far its detail goes.
struct Canvas {
    size: u32,
    octaves: u32,
    seed: Seed,
}

impl Canvas {
    fn new(quality: TextureQuality, seed: Seed) -> Canvas {
        Canvas {
            size: quality.size(),
            octaves: quality.octaves(),
            seed,
        }
    }

    /// The dice this recipe rolls for one of its fields.
    fn dice(&self, index: usize) -> Seed {
        salt(self.seed, index)
    }

    /// A wrapping field of `cells` whole lattice cells across the tile, summed to the
    /// quality's detail.
    fn field(&self, u: f32, v: f32, cells: i64, index: usize) -> f32 {
        fbm_tile(u, v, cells, cells, self.octaves, self.dice(index))
    }

    /// The same field stretched along one axis, which is what makes blotches lie one
    /// way: blades of grass, ripples in sand.
    fn stretched(&self, u: f32, v: f32, cells_x: i64, cells_y: i64, index: usize) -> f32 {
        fbm_tile(u, v, cells_x, cells_y, self.octaves, self.dice(index))
    }

    /// Relief is cut from a shallower sum than colour: a bump is felt at the scale of
    /// a handful of cells, and past that it is a mip the camera never rests on.
    fn bump(&self, u: f32, v: f32, cells: i64, index: usize) -> f32 {
        fbm_tile(u, v, cells, cells, self.octaves.min(4), self.dice(index))
    }

    /// A roll for one cell of a wrapping grid - a texel of grain, a pebble, a brick.
    fn roll(&self, x: i64, y: i64, cells_x: i64, cells_y: i64, index: usize) -> f32 {
        tile_hash(x, y, cells_x, cells_y, self.dice(index))
    }

    /// The roll of the texel being filled, which is the same roll on either side of the
    /// tile's edge because the grid it comes from has a cell per texel across it.
    fn grain(&self, spot: &Spot, index: usize) -> f32 {
        let cells = self.size as i64;
        self.roll(spot.col as i64, spot.row as i64, cells, cells, index)
    }

    /// How much of a point is on a crease: the field folded about its middle, so the
    /// line is where the ground turns back on itself and the tile closes on either side.
    fn crease(&self, u: f32, v: f32, cells: i64, index: usize) -> f32 {
        let at = self.field(u, v, cells, index);
        1.0 - shade(0.0, 0.055, (at - 0.5).abs())
    }

    /// The whole tile as tightly packed RGBA8, lit as it is painted so the file needs
    /// no second pass and no normal map to read as relief.
    fn paint(&self, kind: TextureType, variation: Option<&'static str>) -> Vec<u8> {
        self.paint_in(kind, variation, rows_per_worker(self.size as usize))
    }

    /// The tile painted in row bands this tall, which is the whole work split over as
    /// many workers as it is worth.
    ///
    /// Both passes are rows of texels that answer only for themselves, so a row never
    /// reads a row another worker is still writing: the light pass borrows the whole
    /// height map, which is finished before it starts. Nothing is decided in the order
    /// the rows are asked for, so handing them out differently cannot move a byte.
    fn paint_in(&self, kind: TextureType, variation: Option<&'static str>, rows: usize) -> Vec<u8> {
        let across = 1.0 / self.size as f32;
        let count = self.size as usize;
        let rows = rows.max(1);

        // One height per texel, and the slope read off its neighbours instead of asked
        // for again. A tile that meets its own edges lets the last column borrow the
        // first one's height, which is what keeps the light from drawing a seam even
        // where the colour alone would not have shown one.
        //
        // The fields both passes want are raised once and kept beside the height, so the
        // colour reads them rather than rolling the same noise a second time. They are
        // deterministic, so handing them over moves no byte.
        let mut heights = vec![0.0f32; count * count];
        let mut shared = vec![[0.0f32; SHARED]; count * count];
        std::thread::scope(|work| {
            let bands = heights
                .chunks_mut(rows * count)
                .zip(shared.chunks_mut(rows * count));
            for (band, (into, fields)) in bands.enumerate() {
                let first = band * rows;
                work.spawn(move || self.raise(kind, variation, across, first, into, fields));
            }
        });

        let mut pixels = vec![255u8; count * count * 4];
        let raised = Raised {
            heights: heights.as_slice(),
            fields: shared.as_slice(),
        };
        std::thread::scope(|work| {
            for (band, into) in pixels.chunks_mut(rows * count * 4).enumerate() {
                let first = band * rows;
                work.spawn(move || self.light(raised, kind, variation, across, first, into));
            }
        });

        pixels
    }

    /// The heights of the rows this band covers, `first` being the tile row it starts on.
    /// The fields the colour will want again are kept beside each height as it is raised.
    fn raise(
        &self,
        kind: TextureType,
        variation: Option<&'static str>,
        across: f32,
        first: usize,
        band: &mut [f32],
        shared: &mut [[f32; SHARED]],
    ) {
        let count = self.size as usize;
        for (row, into) in band.chunks_mut(count).enumerate() {
            let fields = &mut shared[row * count..(row + 1) * count];
            for (col, slot) in into.iter_mut().enumerate() {
                *slot = self.height(
                    kind,
                    Place {
                        // The middle of the texel, so the tile is symmetric about its own
                        // edges and no pattern sits over the seam it has to close.
                        u: (col as f32 + 0.5) * across,
                        v: ((first + row) as f32 + 0.5) * across,
                        variation,
                    },
                    &mut fields[col],
                );
            }
        }
    }

    /// The colours of the rows this band covers, lit off the map the tile has raised.
    fn light(
        &self,
        raised: Raised<'_>,
        kind: TextureType,
        variation: Option<&'static str>,
        across: f32,
        first: usize,
        band: &mut [u8],
    ) {
        let count = self.size as usize;
        let heights = raised.heights;
        let sun = Vec3::from_array(SUN);
        for (row, into) in band.chunks_mut(count * 4).enumerate() {
            let row = first + row;
            for (col, pixel) in into.chunks_mut(4).enumerate() {
                let here = heights[row * count + col];
                let east = heights[row * count + (col + 1) % count] - here;
                let north = heights[((row + 1) % count) * count + col] - here;

                // Divided by the texel and multiplied by the height the tile is worth,
                // which is the same thing whichever way the slope runs: a finer tile
                // resolves the same wall, it does not soften it. The divide is folded
                // into the third term, so this normal is the one the ratio leans to.
                let tilt = Vec3::new(-east, -north, across / RELIEF).normalize();
                let lit = AMBIENT + KEY * tilt.dot(sun).max(0.0);

                let spot = Spot {
                    at: Place {
                        u: (col as f32 + 0.5) * across,
                        v: (row as f32 + 0.5) * across,
                        variation,
                    },
                    col: col as u32,
                    row: row as u32,
                };
                let color = (self.color(&spot, kind, here, &raised.fields[row * count + col])
                    * lit)
                    .clamp(Vec3::ZERO, Vec3::ONE);

                for (channel, value) in color.to_array().iter().enumerate() {
                    pixel[channel] = (value * 255.0 + 0.5) as u8;
                }
            }
        }
    }

    /// How high the surface stands at one point of the tile. Asked a texel apart by the
    /// light above, so it is kept cheap and free of anything the colour only knows. The
    /// fields the colour shares are written into `shared` as they are rolled.
    fn height(&self, kind: TextureType, at: Place, shared: &mut [f32; SHARED]) -> f32 {
        match kind {
            TextureType::Grass => self.grass_height(at, shared),
            TextureType::Sand => self.sand_height(at, shared),
            TextureType::Rock => self.rock_height(at, shared),
            TextureType::Brick => self.brick_height(at, shared),
        }
    }

    /// The colour of one texel of the tile, given how high it stands there and the fields
    /// the height pass already rolled for it.
    fn color(&self, spot: &Spot, kind: TextureType, height: f32, shared: &[f32; SHARED]) -> Vec3 {
        match kind {
            TextureType::Grass => self.grass_color(spot, height, shared),
            TextureType::Sand => self.sand_color(spot, height, shared),
            TextureType::Rock => self.rock_color(spot, height, shared),
            TextureType::Brick => self.brick_color(spot, height, shared),
        }
    }
}

/// The grass: low mounds of it, blades lying one way across the tile, and soil showing
/// wherever the crowd is thin.
impl Canvas {
    fn grass_height(&self, at: Place, shared: &mut [f32; SHARED]) -> f32 {
        let mounds = self.field(at.u, at.v, 3, GROUND);
        let blades = self.stretched(at.u, at.v, 44, 6, BLADE);
        shared[0] = blades;
        (0.55 * mounds + 0.45 * blades).clamp(0.0, 1.0)
    }

    fn grass_color(&self, spot: &Spot, height: f32, shared: &[f32; SHARED]) -> Vec3 {
        let u = spot.at.u;
        let v = spot.at.v;

        let patch = self.field(u, v, 7, PATCH);
        let blades = shared[0];
        let mut color = wash(tint(0.13, 0.26, 0.10), tint(0.30, 0.46, 0.17), patch);
        // The tops of the blades catch the light and the gaps between them stay dark.
        color = wash(
            color,
            tint(0.44, 0.58, 0.25),
            shade(0.56, 0.82, blades) * 0.6,
        );
        if spot.at.ruined("moss") {
            // Moss is a carpet of its own: a deeper, purer green than the lawn under it,
            // and plush enough to lie over the blades it grows on.
            let mask = shade(0.44, 0.64, self.field(u, v, 4, MOSS));
            color = wash(color, tint(0.10, 0.32, 0.10), mask);
        }
        if spot.at.ruined("dry") {
            // Dead leaves come off the tops of the mounds first and spread down from there.
            let mask = shade(0.44, 0.68, self.field(u, v, 5, DRY)) * (0.45 + 0.55 * height);
            color = wash(color, tint(0.58, 0.52, 0.26), mask);
        }
        if self.grain(spot, SPECKLE) > 0.965 {
            // Bare soil wherever a blade is missing.
            color = wash(color, tint(0.20, 0.17, 0.11), 0.55);
        }

        color * (0.93 + 0.14 * self.grain(spot, GRAIN))
    }
}

/// The sand: dunes, ripples running along them and bent by them, and loose grain over
/// the whole thing.
impl Canvas {
    fn sand_height(&self, at: Place, shared: &mut [f32; SHARED]) -> f32 {
        let dunes = self.field(at.u, at.v, 3, GROUND);
        let warp = self.field(at.u, at.v, 6, WARP);
        // Ripples run across the tile and lean wherever the dune field bends them. The
        // count is a whole number of wavelengths so the wave closes at the edge too.
        let ripples = ((at.v * 44.0 + 2.0 * warp) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let pebbles = self.pebble(at.u, at.v, at.ruined("pebble"));
        shared[0] = dunes;
        shared[1] = pebbles;

        (0.48 * dunes + 0.14 * ripples + pebbles).clamp(0.0, 1.0)
    }

    fn sand_color(&self, spot: &Spot, height: f32, shared: &[f32; SHARED]) -> Vec3 {
        let u = spot.at.u;
        let v = spot.at.v;

        let dunes = shared[0];
        let mut color = wash(tint(0.70, 0.58, 0.40), tint(0.87, 0.78, 0.58), dunes);
        let pebbles = shared[1];
        if pebbles > 0.0 {
            color = wash(color, tint(0.64, 0.60, 0.54), pebbles);
        }
        if spot.at.ruined("dust") {
            // Powder settles in the hollows and takes the contrast off everything else.
            let mask =
                shade(0.30, 0.72, self.field(u, v, 5, DUST)) * (0.35 + 0.65 * (1.0 - height));
            color = wash(color, tint(0.84, 0.80, 0.70), mask);
            color = wash(color, tint(0.78, 0.72, 0.58), 0.15);
        }

        color * (0.90 + 0.20 * self.grain(spot, GRAIN))
    }

    /// The pebbles: a lattice of cells, each rolled for whether it holds one, where in
    /// the cell it sits and how wide. A point inside one stands on its dome, and the
    /// answer is the height the domes are worth, not the dome alone.
    fn pebble(&self, u: f32, v: f32, crowded: bool) -> f32 {
        let cells = if crowded { 34 } else { 24 };
        let keep = if crowded { 0.34 } else { 0.52 };
        let at = Vec2::new(u * cells as f32, v * cells as f32);
        let base = at.floor();
        let mut dome = 0.0f32;

        for dy in -1..=1 {
            for dx in -1..=1 {
                let cell = base + Vec2::new(dx as f32, dy as f32);
                let (ix, iy) = (cell.x as i64, cell.y as i64);
                if self.roll(ix, iy, cells, cells, PEBBLE_KEEP) < keep {
                    continue;
                }
                let centre = cell
                    + Vec2::new(
                        self.roll(ix, iy, cells, cells, PEBBLE_AT),
                        self.roll(ix, iy, cells, cells, PEBBLE_AT + 1),
                    );
                let radius = 0.20 + 0.26 * self.roll(ix, iy, cells, cells, PEBBLE_WIDE);
                let far = at.distance(centre) / radius;
                dome = dome.max((1.0 - far * far).max(0.0));
            }
        }

        dome * if crowded { 0.46 } else { 0.30 }
    }
}

/// The rock: ground folded back on itself so the crevices are where it turns, pitted
/// by weather, and darker wherever it is hollow.
impl Canvas {
    fn rock_height(&self, at: Place, shared: &mut [f32; SHARED]) -> f32 {
        let ground = self.field(at.u, at.v, 5, GROUND);
        let crag = 1.0 - (2.0 * ground - 1.0).abs();
        let pitting = self.field(at.u, at.v, 28, FINE);
        let crease9 = self.crease(at.u, at.v, 9, CREASE);
        let crease13 = self.crease(at.u, at.v, 13, CREASE + 1);
        let creases = crease9.max(crease13);
        shared[0] = ground;
        shared[1] = crease9;
        shared[2] = crease13;

        let cut = if at.ruined("cracked") { 0.46 } else { 0.30 };
        (0.42 * crag + 0.34 * pitting + 0.24 - cut * creases).clamp(0.0, 1.0)
    }

    fn rock_color(&self, spot: &Spot, height: f32, shared: &[f32; SHARED]) -> Vec3 {
        let u = spot.at.u;
        let v = spot.at.v;

        let ground = shared[0];
        let crag = 1.0 - (2.0 * ground - 1.0).abs();
        let mut color = wash(
            tint(0.29, 0.28, 0.27),
            tint(0.57, 0.55, 0.51),
            crag * 0.6 + ground * 0.4,
        );
        // A warm band through the middle of it, so ten metres of it is not one grey.
        color = wash(
            color,
            tint(0.49, 0.44, 0.37),
            self.field(u, v, 3, PATCH) * 0.4,
        );
        let creases = shared[1].max(shared[2]);
        if spot.at.ruined("cracked") {
            color = wash(color, tint(0.13, 0.13, 0.12), creases);
        } else {
            color = wash(color, tint(0.17, 0.16, 0.15), creases * 0.55);
        }
        if spot.at.ruined("mossy") {
            // Growth keeps to the hollows, where the rain it needs is held.
            let mask = shade(0.46, 0.70, self.field(u, v, 4, MOSS)) * (1.15 - height);
            color = wash(color, tint(0.18, 0.31, 0.13), mask);
        }

        color * (0.92 + 0.16 * self.grain(spot, GRAIN))
    }
}

/// The brick: courses of it over a bed of mortar, every other course shifted half a
/// brick so the joints never line up.
impl Canvas {
    /// Courses laid across one tile. Even, because the shift alternates and the tile
    /// has to close on itself.
    const COURSES: i64 = 6;
    /// Bricks to a course.
    const PER_COURSE: i64 = 4;
    /// How wide a joint is, as a fraction of the course it runs along.
    const JOINT_HIGH: f32 = 0.075;
    const JOINT_ACROSS: f32 = 0.055;

    fn brick_height(&self, at: Place, _shared: &mut [f32; SHARED]) -> f32 {
        let laid = Self::bricks(at.u, at.v);
        let face = 0.58
            + 0.20 * self.brick_roll(laid.brick, TILT)
            + 0.14 * self.bump(at.u, at.v, 20, FACE);

        // The mortar sits back and its edges are rounded, so the light finds a lip
        // rather than a cut line.
        face * (1.0 - laid.mortar)
    }

    fn brick_color(&self, spot: &Spot, height: f32, _shared: &[f32; SHARED]) -> Vec3 {
        let u = spot.at.u;
        let v = spot.at.v;
        let laid = Self::bricks(u, v);

        let fired = self.brick_roll(laid.brick, TILT);
        let mut face = wash(tint(0.42, 0.24, 0.19), tint(0.60, 0.33, 0.24), fired);
        // A fired face is not flat: a fine field across it, and a few dark flecks where
        // the clay was.
        face = wash(
            face,
            tint(0.54, 0.41, 0.31),
            self.bump(u, v, 24, FACE) * 0.45,
        );
        if self.grain(spot, SPECKLE) > 0.93 {
            face = wash(face, tint(0.30, 0.18, 0.15), 0.5);
        }

        let lime = tint(0.74, 0.70, 0.62) * (0.90 + 0.20 * self.bump(u, v, 40, GRAIN));
        let mut color = wash(face, lime, laid.mortar);

        if spot.at.ruined("dusty") {
            // Powder lies on whatever stands up and takes the fire out of the faces.
            let mask = 0.25 + 0.45 * height;
            color = wash(color, tint(0.72, 0.66, 0.55), mask);
        }
        if spot.at.ruined("mossy") {
            // Growth holds to the joints and the bottom lip of each course, where the
            // water runs and sits.
            let low = 1.0 - shade(0.0, 0.34, laid.up);
            let mask = shade(0.40, 0.72, self.field(u, v, 5, MOSS))
                * laid.mortar.max(low * (1.0 - laid.mortar));
            color = wash(color, tint(0.17, 0.29, 0.13), mask);
        }

        color * (0.93 + 0.14 * self.grain(spot, GRAIN))
    }

    /// The roll one brick of the layout is cut from. Numbered by course and by brick in
    /// the course, which is a whole number of each across the tile, so a brick at the
    /// edge is rolled the same as the one its neighbour is on the other side.
    fn brick_roll(&self, brick: IVec2, index: usize) -> f32 {
        self.roll(
            brick.x as i64,
            brick.y as i64,
            Self::PER_COURSE,
            Self::COURSES,
            index,
        )
    }

    /// Where a point of the tile falls in the layout: which brick it is on, where inside
    /// its course, and how much of it is joint rather than face.
    fn bricks(u: f32, v: f32) -> Laid {
        let courses = Self::COURSES as f32;
        let per = Self::PER_COURSE as f32;

        let row = (v * courses).floor() as i32;
        let up = v * courses - row as f32;

        // Every other course starts half a brick along, so the joints never line up.
        let along = u * per - if row % 2 == 0 { 0.0 } else { 0.5 };
        let across = along - along.floor();

        let high = 1.0 - shade(0.0, Self::JOINT_HIGH, up.min(1.0 - up));
        let joint = 1.0 - shade(0.0, Self::JOINT_ACROSS, across.min(1.0 - across));

        Laid {
            // Raw brick numbers, unwrapped: the roll they are asked for wraps at the
            // edge of the tile, which is what makes the half brick at the left the end
            // of the last brick at the right instead of a brick of its own.
            brick: IVec2::new(along.floor() as i32, row),
            up,
            mortar: high.max(joint),
        }
    }
}

/// One point of a brick wall, measured out of the layout.
#[derive(Debug, Clone, Copy)]
struct Laid {
    /// Which brick the point is on, counted from the corner of the tile.
    brick: IVec2,
    /// How far up its course the point is, 0..1.
    up: f32,
    /// How much of this point is joint rather than face, with a soft edge.
    mortar: f32,
}

/// The three names a script writes, checked: what the surface is, what has happened to
/// it, and how finely it is painted. A name that is not one is refused here, once, before
/// a single texel is mixed.
fn named(
    kind: &str,
    variation: Option<&str>,
    quality: Option<&str>,
) -> Result<(TextureType, Option<&'static str>, TextureQuality), TextureError> {
    let kind = TextureType::from_name(kind).ok_or_else(|| TextureError::Type {
        asked: kind.to_string(),
    })?;
    let quality = wanted_quality(quality)?;
    Ok((kind, wanted_variation(kind, variation)?, quality))
}

/// The quality asked for, which is the one a fresh tile is painted at when a script
/// names none or writes an empty word.
fn wanted_quality(asked: Option<&str>) -> Result<TextureQuality, TextureError> {
    let Some(asked) = asked.map(str::trim).filter(|asked| !asked.is_empty()) else {
        return Ok(TextureQuality::Medium);
    };
    TextureQuality::from_name(asked).ok_or_else(|| TextureError::Quality {
        asked: asked.to_string(),
    })
}

/// The variation of one kind, asked for the way a script writes it.
fn wanted_variation(
    kind: TextureType,
    asked: Option<&str>,
) -> Result<Option<&'static str>, TextureError> {
    let Some(asked) = asked.map(str::trim).filter(|asked| !asked.is_empty()) else {
        return Ok(None);
    };
    let asked = asked.to_lowercase();
    let offered = kind.variations();
    offered
        .iter()
        .find(|name| **name == asked.as_str())
        .copied()
        .map(Some)
        .ok_or_else(|| TextureError::Variation {
            kind: kind.name(),
            asked,
            offered,
        })
}

/// The ways one named surface can be ruined, for a script that is asking what it may
/// write before it writes it. A surface that is not one is refused the way a picture of
/// it would be.
pub fn variations(kind: &str) -> Result<&'static [&'static str], TextureError> {
    TextureType::from_name(kind)
        .map(TextureType::variations)
        .ok_or_else(|| TextureError::Type {
            asked: kind.to_string(),
        })
}

/// The tile the parameters name, as tightly packed RGBA8, with how wide it was painted:
/// the picture before it is compressed, which is what a test reads.
pub fn pixels(
    kind: &str,
    variation: Option<&str>,
    quality: Option<&str>,
    seed: Seed,
) -> Result<(u32, Vec<u8>), TextureError> {
    let (kind, variation, quality) = named(kind, variation, quality)?;
    Ok((
        quality.size(),
        Canvas::new(quality, seed).paint(kind, variation),
    ))
}

/// The packed picture the parameters name: KTX2 of BC7 blocks with a full mip chain,
/// sRGB like any authored picture, ready for `obj.texture` to be pointed at.
pub fn generate(
    kind: &str,
    variation: Option<&str>,
    quality: Option<&str>,
    seed: Seed,
) -> Result<Vec<u8>, TextureError> {
    let (size, pixels) = pixels(kind, variation, quality, seed)?;
    carbine_ktx::pack(size, size, &pixels, true).map_err(|why| TextureError::Encoder {
        why: why.to_string(),
    })
}

/// Generates a tile and writes it, returning the path it wrote.
///
/// The name carries everything that went into it - what it is, what has gone wrong with
/// it, how finely it was painted and the seed it was painted from - so two different
/// answers never land on one file and the same answer lands on the file that is already
/// there. An `out` that names a folder writes into it; anything else is the exact file.
/// Left alone, the tile lands in the folder the game is run from.
pub fn save(
    kind: &str,
    variation: Option<&str>,
    quality: Option<&str>,
    seed: Seed,
    out: Option<&Path>,
) -> Result<PathBuf, TextureError> {
    let (kind, variation, quality) = named(kind, variation, quality)?;
    let bytes = generate(kind.name(), variation, Some(quality.name()), seed)?;

    let written = destination(kind, variation, quality, seed, out);
    // A bare file name has no directory to make, and `create_dir_all("")` is an error.
    if let Some(directory) = written
        .parent()
        .filter(|directory| !directory.as_os_str().is_empty())
    {
        std::fs::create_dir_all(directory).map_err(|why| TextureError::Write {
            path: written.display().to_string(),
            why: why.to_string(),
        })?;
    }
    std::fs::write(&written, bytes).map_err(|why| TextureError::Write {
        path: written.display().to_string(),
        why: why.to_string(),
    })?;

    Ok(written)
}

/// The file the parameters answer to: everything that went into the tile, so the picture
/// on disk says what it is and what it was painted from.
fn name(kind: TextureType, variation: Option<&str>, quality: TextureQuality, seed: Seed) -> String {
    format!(
        "{}_{}_{}_s{}",
        kind.name(),
        variation.unwrap_or("plain"),
        quality.name(),
        seed.0
    )
}

fn destination(
    kind: TextureType,
    variation: Option<&str>,
    quality: TextureQuality,
    seed: Seed,
    out: Option<&Path>,
) -> PathBuf {
    let file = name(kind, variation, quality, seed);
    match out {
        Some(directory) if names_a_folder(directory) => directory.join(file).with_extension("ktx2"),
        Some(exact) => exact.to_path_buf(),
        None => PathBuf::from(format!("{file}.ktx2")),
    }
}

/// Whether the caller means a folder rather than a file: one that already exists, or one
/// whose name ends in a separator. The same rule the Packager goes by.
fn names_a_folder(path: &Path) -> bool {
    let text = path.to_string_lossy();
    path.is_dir() || text.ends_with('/') || text.ends_with('\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> Seed {
        Seed::of("yard")
    }

    /// A directory of its own, so a rerun never reads a stale tile.
    fn workspace(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("carbine-texture-{name}"));
        std::fs::remove_dir_all(&directory).ok();
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    /// Every kind, with each of its ways to be ruined and with none of them.
    fn every_surface() -> Vec<(TextureType, Option<&'static str>)> {
        let mut asked = Vec::new();
        for kind in [
            TextureType::Brick,
            TextureType::Grass,
            TextureType::Rock,
            TextureType::Sand,
        ] {
            asked.push((kind, None));
            asked.extend(kind.variations().iter().map(|name| (kind, Some(*name))));
        }
        asked
    }

    /// The mean of one channel of a painted tile, which is what it looks like from a
    /// metre off.
    fn mean(tile: &[u8], channel: usize) -> f32 {
        let count = tile.len() / 4;
        (0..count)
            .map(|texel| tile[texel * 4 + channel] as f32)
            .sum::<f32>()
            / (count as f32 * 255.0)
    }

    #[test]
    fn the_names_a_script_writes_are_the_ones_the_engine_paints() {
        for name in TextureType::NAMES {
            let kind = TextureType::from_name(name).unwrap();
            assert_eq!(kind.name(), *name, "{name} does not answer as itself");
            assert!(!kind.variations().is_empty(), "{name} offers nothing");
        }

        // The words a designer reaches for instead of the advertised ones.
        assert_eq!(TextureType::from_name(" Lawn "), Some(TextureType::Grass));
        assert_eq!(TextureType::from_name("stone"), Some(TextureType::Rock));
        assert_eq!(TextureType::from_name("masonry"), Some(TextureType::Brick));
        assert_eq!(TextureType::from_name("desert"), Some(TextureType::Sand));
        assert_eq!(TextureType::from_name("water"), None);

        for name in TextureQuality::NAMES {
            let quality = TextureQuality::from_name(name).unwrap();
            assert_eq!(quality.name(), *name);
        }
        assert_eq!(
            TextureQuality::from_name("MED"),
            Some(TextureQuality::Medium)
        );
        assert_eq!(
            TextureQuality::from_name("ultra"),
            Some(TextureQuality::High)
        );
        assert_eq!(TextureQuality::from_name("extreme"), None);

        // A size and a depth of detail for each rung of the ladder. They move together,
        // which is what leaves one word for a script to pick.
        assert_eq!(TextureQuality::Low.size(), 256);
        assert_eq!(TextureQuality::Medium.size(), 512);
        assert_eq!(TextureQuality::High.size(), 1024);
        assert_eq!(TextureQuality::Low.octaves(), 3);
        assert_eq!(TextureQuality::Medium.octaves(), 5);
        assert_eq!(TextureQuality::High.octaves(), 7);
    }

    #[test]
    fn every_recipe_meets_its_own_edge() {
        // The same point of the tile named from either side of its edge has to answer
        // the same way, the per-texel roll included: a texel at column 0 and one a full
        // width across are the same texel of the next copy of the picture.
        for (kind, variation) in every_surface() {
            let canvas = Canvas::new(TextureQuality::Low, seed());
            let cells = canvas.size;

            // Across the right edge, up the top edge, and one tile and a bit over.
            let twins = [
                (0.0, 0.31, 1.0, 0.31, 0u32, 92, cells, 92),
                (0.77, 0.0, 0.77, 1.0, 41, 0, 41, cells),
                (0.13, 0.98, 1.13, 1.98, 7, 250, 7 + cells, 250 + cells),
            ];

            for (u, v, twin_u, twin_v, col, row, twin_col, twin_row) in twins {
                let here = Place { u, v, variation };
                let there = Place {
                    u: twin_u,
                    v: twin_v,
                    variation,
                };

                let mut here_fields = [0.0f32; SHARED];
                let mut there_fields = [0.0f32; SHARED];
                let stands = canvas.height(kind, here, &mut here_fields);
                let also = canvas.height(kind, there, &mut there_fields);
                assert!(
                    (stands - also).abs() < 1e-4,
                    "{} {:?} stands at {stands} here and {also} across its own edge",
                    kind.name(),
                    variation.unwrap_or("plain"),
                );

                let left = Spot { at: here, col, row };
                let right = Spot {
                    at: there,
                    col: twin_col,
                    row: twin_row,
                };
                let near = canvas.color(&left, kind, stands, &here_fields);
                let far = canvas.color(&right, kind, also, &there_fields);
                assert!(
                    (far - near).abs().max_element() < 1e-3,
                    "{} {:?} is a different colour across its own edge: {near} vs {far}",
                    kind.name(),
                    variation.unwrap_or("plain"),
                );
            }
        }
    }

    #[test]
    fn a_painted_tile_is_the_same_picture_on_both_sides_of_its_seam() {
        // The check a script's eye would do: the join between two copies of a tile is no
        // more noticeable than the sharpest join already inside it. A wall has joints in
        // it, so the seam is allowed to be as loud as a joint - what it is not allowed to
        // be is louder than every join in the picture, which is what a field that does not
        // wrap makes.
        for kind in [
            TextureType::Brick,
            TextureType::Grass,
            TextureType::Rock,
            TextureType::Sand,
        ] {
            let (size, tile) = pixels(kind.name(), None, Some("low"), seed()).unwrap();
            let count = size as usize;
            let lit = |col: usize, row: usize| -> f32 {
                let at = (row * count + col) * 4;
                tile[at] as f32 * 0.3 + tile[at + 1] as f32 * 0.6 + tile[at + 2] as f32 * 0.1
            };

            // How much two columns of the tile differ from each other, texel by texel,
            // and the same question about the rows.
            let columns = |first: usize, second: usize| -> f32 {
                (0..count)
                    .map(|row| (lit(first, row) - lit(second, row)).abs())
                    .sum::<f32>()
                    / count as f32
            };
            let rows = |first: usize, second: usize| -> f32 {
                (0..count)
                    .map(|col| (lit(col, first) - lit(col, second)).abs())
                    .sum::<f32>()
                    / count as f32
            };

            let sharpest = (1..count - 1)
                .map(|col| columns(col, col + 1))
                .fold(0.0f32, f32::max);
            let seam = columns(count - 1, 0);
            assert!(
                seam <= sharpest * 1.5,
                "{} breaks at its own edge: {seam} across the seam against {sharpest} at its \
                 sharpest join inside",
                kind.name(),
            );

            // Up the tile too, which is where a wave or a course of brick would show a
            // join that the fields alone hide.
            let highest = (1..count - 1)
                .map(|row| rows(row, row + 1))
                .fold(0.0f32, f32::max);
            let top = rows(count - 1, 0);
            assert!(
                top <= highest * 1.5,
                "{} breaks at its top: {top} against {highest} at its sharpest course",
                kind.name(),
            );
        }
    }

    #[test]
    fn the_same_seed_paints_the_same_tile_twice() {
        let (size, first) = pixels("grass", Some("moss"), Some("low"), seed()).unwrap();
        let (again_size, again) = pixels("grass", Some("moss"), Some("low"), seed()).unwrap();
        assert_eq!(size, again_size);
        assert_eq!(first, again, "a rerun of the same seed painted elsewhere");

        let (_, other) = pixels("grass", Some("moss"), Some("low"), Seed::of("yarde")).unwrap();
        let different = first
            .chunks(4)
            .zip(other.chunks(4))
            .filter(|(one, two)| one != two)
            .count();
        assert!(
            different > first.len() / 4 / 3,
            "a new seed changed only {different} texels of it"
        );

        // Every texel is a colour with an alpha left alone for the blend, and none of
        // them is a hole.
        assert_eq!(size * size * 4, first.len() as u32);
        assert!(
            first.chunks(4).all(|texel| texel[3] == 255),
            "a generated tile is opaque all over"
        );
    }

    /// The tile is painted in row bands handed to as many workers as it is worth, so the
    /// split itself has to be invisible: one worker or twenty, the same bytes in the same
    /// order. 200 and `count - 1` do not divide 256, so the last band is ragged too.
    #[test]
    fn the_same_tile_comes_back_from_any_number_of_workers() {
        let canvas = Canvas::new(TextureQuality::Low, seed());
        let count = canvas.size as usize;
        let whole = canvas.paint_in(TextureType::Grass, Some("moss"), count);
        for rows in [1, 3, 7, 200, count - 1] {
            assert_eq!(
                canvas.paint_in(TextureType::Grass, Some("moss"), rows),
                whole,
                "{rows} rows to a worker moved a texel"
            );
        }
    }

    #[test]
    fn each_kind_reads_as_the_surface_it_names() {
        for kind in [
            TextureType::Brick,
            TextureType::Grass,
            TextureType::Rock,
            TextureType::Sand,
        ] {
            let (size, tile) = pixels(kind.name(), None, Some("low"), seed()).unwrap();
            let (red, green, blue) = (mean(&tile, 0), mean(&tile, 1), mean(&tile, 2));
            let brightest = tile
                .chunks(4)
                .map(|texel| {
                    (texel[0] as f32 * 0.3 + texel[1] as f32 * 0.6 + texel[2] as f32 * 0.1) / 255.0
                })
                .fold(0.0f32, f32::max);
            let dimmest = tile
                .chunks(4)
                .map(|texel| {
                    (texel[0] as f32 * 0.3 + texel[1] as f32 * 0.6 + texel[2] as f32 * 0.1) / 255.0
                })
                .fold(1.0f32, f32::min);

            match kind {
                TextureType::Grass => assert!(
                    green > red && green > blue,
                    "{} is not green: r{red} g{green} b{blue}",
                    kind.name()
                ),
                TextureType::Sand => {
                    assert!(
                        red > green && green > blue,
                        "{} is not warm: r{red} g{green} b{blue}",
                        kind.name()
                    );
                    assert!(red > 0.45, "{} came out dark: {red}", kind.name());
                }
                TextureType::Rock => assert!(
                    (red - green).abs() < 0.08 && (green - blue).abs() < 0.08,
                    "{} is not stone: r{red} g{green} b{blue}",
                    kind.name()
                ),
                TextureType::Brick => assert!(
                    red - green > 0.05,
                    "{} is not fired clay: r{red} g{green} b{blue}",
                    kind.name()
                ),
            }

            // The light is baked in, because nothing in the renderer reads a second
            // picture for relief: a tile that is one flat wash has no shading in it.
            assert!(
                brightest - dimmest > 0.25,
                "{} has no relief: {dimmest} to {brightest} over {size} texels",
                kind.name(),
            );
        }
    }

    #[test]
    fn a_variation_is_seen_in_the_picture_and_not_only_in_the_name() {
        // A way to ruin a surface has to change what it looks like, and in the way its
        // name says. Growth is read as a balance rather than as a green channel, because
        // moss is a darker surface than the grass it lies over and still greener against
        // its own red.
        for (kind, name, moved) in [
            (TextureType::Grass, "dry", "straw"),
            (TextureType::Grass, "moss", "greener"),
            (TextureType::Sand, "dust", "lighter"),
            (TextureType::Sand, "pebble", "changed"),
            (TextureType::Rock, "cracked", "darker"),
            (TextureType::Rock, "mossy", "greener"),
            (TextureType::Brick, "dusty", "lighter"),
            (TextureType::Brick, "mossy", "greener"),
        ] {
            let (_, plain) = pixels(kind.name(), None, Some("low"), seed()).unwrap();
            let (_, ruined) = pixels(kind.name(), Some(name), Some("low"), seed()).unwrap();

            let changed = plain
                .chunks(4)
                .zip(ruined.chunks(4))
                .filter(|(one, two)| {
                    (one[0] as i16 - two[0] as i16).abs() > 8
                        || (one[1] as i16 - two[1] as i16).abs() > 8
                        || (one[2] as i16 - two[2] as i16).abs() > 8
                })
                .count();
            assert!(
                changed > plain.len() / 4 / 10,
                "{} with {name} left {} texels of {} alone",
                kind.name(),
                changed,
                name,
            );

            let balance = |tile: &[u8]| mean(tile, 1) - mean(tile, 0);
            let lit = |tile: &[u8]| mean(tile, 0) * 0.3 + mean(tile, 1) * 0.6 + mean(tile, 2) * 0.1;

            match moved {
                "greener" => assert!(
                    balance(&ruined) > balance(&plain) + 0.01,
                    "{name} on {} did not add growth: {:+} against {:+}",
                    kind.name(),
                    balance(&ruined),
                    balance(&plain),
                ),
                "straw" => assert!(
                    balance(&ruined) < balance(&plain) - 0.01,
                    "{name} on {} did not bleach it: {:+} against {:+}",
                    kind.name(),
                    balance(&ruined),
                    balance(&plain),
                ),
                "darker" => assert!(
                    lit(&ruined) < lit(&plain) - 0.01,
                    "{name} on {} did not hollow it: {} against {}",
                    kind.name(),
                    lit(&ruined),
                    lit(&plain),
                ),
                "lighter" => assert!(
                    lit(&ruined) > lit(&plain) + 0.01,
                    "{name} on {} did not powder it: {} against {}",
                    kind.name(),
                    lit(&ruined),
                    lit(&plain),
                ),
                _ => {}
            }
        }
    }

    #[test]
    fn the_layout_is_counted_not_measured_in_texels() {
        // Six courses and four bricks to a course, at every quality: a finer tile is the
        // same wall seen closer, not a wall with smaller bricks in it.
        let courses = Canvas::COURSES;
        let per_course = Canvas::PER_COURSE;

        let layout = |quality: TextureQuality| -> (i64, f32) {
            let canvas = Canvas::new(quality, seed());
            let count = canvas.size as usize;
            let mut joints = 0usize;
            for row in 0..count {
                for col in 0..count {
                    let laid = Canvas::bricks(
                        (col as f32 + 0.5) / count as f32,
                        (row as f32 + 0.5) / count as f32,
                    );
                    assert!(
                        laid.brick.x < per_course as i32 + 1 && laid.brick.x >= -1,
                        "a brick was counted outside the course: {:?}",
                        laid.brick
                    );
                    assert!(laid.brick.y < courses as i32 && laid.brick.y >= 0);
                    assert!((0.0..=1.0).contains(&laid.mortar), "{:?}", laid);
                    if laid.mortar > 0.5 {
                        joints += 1;
                    }
                }
            }
            (joints as i64, joints as f32 / (count * count) as f32)
        };

        let (low_joints, low) = layout(TextureQuality::Low);
        let (_, high) = layout(TextureQuality::High);
        assert!(
            (low - high).abs() < 0.02,
            "the joints cover {low} of a small tile and {high} of a large one: {low_joints} texels"
        );

        // Every other course is shifted half a brick, so no joint runs straight down
        // the wall: the same point across belongs to a different brick one course up.
        let low = Canvas::bricks(0.1, 0.5 / courses as f32);
        let high = Canvas::bricks(0.1, 1.5 / courses as f32);
        assert_ne!(
            low.brick.x, high.brick.x,
            "the courses line up: {low:?} {high:?}"
        );
        assert_eq!(low.brick.y, 0);
        assert_eq!(high.brick.y, 1);
    }

    #[test]
    fn a_surface_asks_and_the_engine_answers_what_it_offers() {
        let error = generate("water", None, Some("medium"), seed()).unwrap_err();
        assert!(error.to_string().contains("water"), "{error}");
        assert!(error.to_string().contains("brick, grass"), "{error}");

        let error = generate("grass", Some("mold"), Some("medium"), seed()).unwrap_err();
        assert!(error.to_string().contains("mold"), "{error}");
        assert!(
            error.to_string().contains("dry, moss"),
            "the refusal did not say what grass takes: {error}"
        );

        let error = generate("grass", None, Some("extreme"), seed()).unwrap_err();
        assert!(error.to_string().contains("extreme"), "{error}");
        assert!(error.to_string().contains("high, low"), "{error}");

        // A variation a kind does not have is refused the same way, and an empty one is
        // left out rather than read as a name nobody typed.
        assert!(generate("brick", Some("dust"), Some("medium"), seed()).is_err());
        assert!(generate("brick", Some("  "), Some("medium"), seed()).is_ok());
        assert_eq!(
            wanted_variation(TextureType::Brick, Some("MOSSY")).unwrap(),
            Some("mossy")
        );

        // A quality left out is the one a fresh tile is painted at, and writing an empty
        // word for it means the same. The default belongs to the engine, not to a script.
        assert_eq!(
            named("brick", None, None).unwrap().2,
            TextureQuality::Medium
        );
        assert_eq!(
            named("brick", None, Some(" ")).unwrap().2,
            TextureQuality::Medium
        );
        assert_eq!(
            pixels("brick", None, None, seed()).unwrap().0,
            TextureQuality::Medium.size()
        );

        // And asking what a surface takes answers with the same names it will accept.
        assert_eq!(variations("grass").unwrap(), ["dry", "moss"]);
        assert_eq!(variations(" Masonry ").unwrap(), ["dusty", "mossy"]);
        assert!(variations("water").is_err());
    }

    #[test]
    fn a_saved_tile_is_a_packed_image_the_loader_reads_back() {
        let directory = workspace("saved");

        let in_folder = save(
            "rock",
            Some("cracked"),
            Some("low"),
            seed(),
            Some(&directory),
        )
        .unwrap();
        assert_eq!(
            in_folder,
            directory.join(format!("rock_cracked_low_s{}.ktx2", seed().0))
        );

        let bytes = std::fs::read(&in_folder).unwrap();
        assert!(carbine_ktx::is_ktx2(&bytes));
        let read = carbine_ktx::read(&bytes).unwrap();
        assert_eq!((read.width(), read.height()), (256, 256));
        assert!(read.srgb(), "a painted tile is a picture, not light");
        assert_eq!(
            read.levels().len(),
            carbine_ktx::mip_count(256, 256) as usize
        );

        // The same parameters answer with the same file, so a rerun overwrites the
        // picture it made rather than littering the folder with a second one.
        assert_eq!(
            save(
                "rock",
                Some("cracked"),
                Some("low"),
                seed(),
                Some(&directory)
            )
            .unwrap(),
            in_folder
        );
        assert_ne!(
            save("rock", None, Some("low"), seed(), Some(&directory)).unwrap(),
            in_folder,
            "ruined stone is not plain stone"
        );

        // A name ending in a separator is a folder too, and one that is not there yet
        // is made on the way past.
        let built = format!(
            "{}{}",
            directory.join("built").display(),
            std::path::MAIN_SEPARATOR,
        );
        assert_eq!(
            save("sand", None, Some("low"), seed(), Some(Path::new(&built)))
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy(),
            format!("sand_plain_low_s{}.ktx2", seed().0)
        );

        // An exact file is honoured, and a bare name is the file the game will find
        // beside itself.
        let exact = directory.join("brickwall.ktx2");
        assert_eq!(
            save("brick", Some("mossy"), Some("low"), seed(), Some(&exact)).unwrap(),
            exact
        );
        assert!(exact.is_file());
        assert_eq!(
            destination(
                TextureType::Grass,
                None,
                TextureQuality::Medium,
                seed(),
                None
            ),
            PathBuf::from(format!("grass_plain_medium_s{}.ktx2", seed().0))
        );
    }

    #[test]
    fn a_folder_that_cannot_be_made_is_said_in_the_refusal() {
        let directory = workspace("refused");
        let in_the_way = directory.join("notes");
        std::fs::write(&in_the_way, "not a folder").unwrap();

        let error = save(
            "grass",
            None,
            Some("low"),
            seed(),
            Some(&in_the_way.join("tile.ktx2")),
        )
        .unwrap_err();
        assert!(matches!(error, TextureError::Write { .. }), "{error}");
        assert!(error.to_string().contains("tile.ktx2"), "{error}");

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_packed_tile_is_a_file_the_engine_can_be_pointed_at() {
        let bytes = generate("brick", Some("dusty"), Some("low"), seed()).unwrap();
        let read = carbine_ktx::read(&bytes).unwrap();

        assert_eq!((read.width(), read.height()), (256, 256));
        assert!(carbine_ktx::is_ktx2(&bytes));
        assert!(
            bytes.len() < 256 * 256 * 4,
            "BC7 is meant to be the smaller half"
        );
    }
}
