//! KTX2 holding BC7 — the only image format Carbine loads.
//!
//! A packed image reaches the GPU as its final bytes: no CPU codec runs while a scene is
//! loading, and VRAM keeps 8 bits a texel instead of 32. Anything that is not already in
//! this container has to come through [`pack`] first, which is what the Packager does.

use std::fmt;

use rusty_dds::{Dds, DecodeContent, EncodeLayout};

/// `«KTX 20»\r\n\x1a\n` — the KTX2 file identifier.
pub const MAGIC: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];

/// The identifier plus the fixed header fields.
const HEADER: usize = 80;
const LEVEL_INDEX: usize = 24;
/// 4-byte section length, then one block: 8-byte header, 16-byte basic descriptor, 1 sample.
const DFD: usize = 44;
const BLOCK_BYTES: usize = 16;
const VK_BC7_UNORM: u32 = 145;
const VK_BC7_SRGB: u32 = 146;
/// The Khronos color model for a block-compressed BC7 descriptor.
const BC7_COLOR_MODEL: u8 = 134;
const COLOR_MODEL_OFFSET: usize = 12;

#[derive(Debug)]
pub enum Error {
    /// Fewer bytes than the KTX2 header needs.
    TooShort,
    NotKtx2,
    /// A file that is legal KTX2 but not the shape Carbine writes.
    Unsupported(String),
    Truncated {
        level: u32,
    },
    WrongLevelSize {
        level: u32,
        expected: usize,
        found: usize,
    },
    Encoder(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooShort => write!(f, "it is shorter than the 80 byte KTX2 header"),
            Error::NotKtx2 => write!(f, "it is not a KTX2 file"),
            Error::Unsupported(what) => write!(f, "{what}"),
            Error::Truncated { level } => write!(f, "level {level} runs past the end of the file"),
            Error::WrongLevelSize {
                level,
                expected,
                found,
            } => {
                write!(
                    f,
                    "level {level} holds {found} bytes of BC7, expected {expected}"
                )
            }
            Error::Encoder(why) => write!(f, "the BC7 encoder refused the image: {why}"),
        }
    }
}

impl std::error::Error for Error {}

/// How many levels a full chain of this size has, biggest first.
pub fn mip_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// The packed size of one level: BC7 covers the edges by padding out to whole blocks.
fn level_bytes(width: u32, height: u32) -> usize {
    let blocks = width.div_ceil(4) as usize * height.div_ceil(4) as usize;
    blocks * BLOCK_BYTES
}

/// True once a buffer is long enough to hold the identifier and starts with it.
pub fn is_ktx2(bytes: &[u8]) -> bool {
    bytes.len() >= MAGIC.len() && bytes.starts_with(&MAGIC)
}

/// Compress a tightly packed RGBA8 image into a KTX2 file with a full mip chain.
///
/// `srgb` marks the texels as display-encoded, which is what an authored picture or a
/// decal is; data textures that the script does arithmetic on want `false`.
pub fn pack(width: u32, height: u32, rgba8: &[u8], srgb: bool) -> Result<Vec<u8>, Error> {
    if width == 0 || height == 0 {
        return Err(Error::Unsupported("an image with no pixels".into()));
    }
    let source = width as usize * height as usize * 4;
    if rgba8.len() < source {
        return Err(Error::Unsupported(format!(
            "{width}x{height} wants {source} bytes of RGBA8 and got {}",
            rgba8.len()
        )));
    }

    let levels = mip_count(width, height);
    let mut chain = Vec::with_capacity(levels as usize);
    let mut pixels = rgba8.to_vec();
    let mut size = (width, height);
    for level in 0..levels {
        let layout = EncodeLayout::flat_2d(DecodeContent::Bc7, size.0, size.1);
        let encoded = Dds::encode_from_rgba8(&pixels, layout).map_err(encoder_error)?;
        let expected = level_bytes(size.0, size.1);
        if encoded.data.len() != expected {
            return Err(Error::Encoder(format!(
                "level {level} came back as {} bytes, not the {expected} its block grid wants",
                encoded.data.len()
            )));
        }
        chain.push(encoded.data);
        if level + 1 < levels {
            pixels = halve(&pixels, size.0, size.1);
            size = ((size.0 / 2).max(1), (size.1 / 2).max(1));
        }
    }

    let index_end = HEADER + levels as usize * LEVEL_INDEX;
    // The spec puts level 0 on a whole-block boundary; every level after it is already a
    // whole number of blocks, so aligning the start aligns the whole chain.
    let data_start = (index_end + DFD).div_ceil(16) * 16;
    let total = data_start + chain.iter().map(|level| level.len()).sum::<usize>();

    let mut bytes = vec![0u8; total];
    write_header(&mut bytes, width, height, levels, srgb, index_end);
    bytes[index_end..index_end + DFD].copy_from_slice(&descriptor(srgb));

    let mut at = data_start;
    for (level, blocks) in chain.iter().enumerate() {
        bytes[at..at + blocks.len()].copy_from_slice(blocks);
        put64(&mut bytes, HEADER + level * LEVEL_INDEX, at as u64);
        put64(
            &mut bytes,
            HEADER + level * LEVEL_INDEX + 8,
            blocks.len() as u64,
        );
        // No supercompression, so the stored length is the length on the GPU.
        put64(
            &mut bytes,
            HEADER + level * LEVEL_INDEX + 16,
            blocks.len() as u64,
        );
        at += blocks.len();
    }

    Ok(bytes)
}

/// The next mip down: a 2x2 box average, rounding a cut-off edge up to keep every texel.
///
/// Averaging happens on the stored bytes, so an sRGB chain mixes display values rather
/// than light. Every importer Carbine replaces does the same, and the difference is only
/// visible in a mip the camera almost never rests on.
fn halve(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (out_width, out_height) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![0u8; (out_width * out_height * 4) as usize];
    for y in 0..out_height {
        for x in 0..out_width {
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let (sx, sy) = (x * 2 + dx, y * 2 + dy);
                    if sx >= width || sy >= height {
                        continue;
                    }
                    let at = (sy * width + sx) as usize * 4;
                    for channel in 0..4 {
                        sum[channel as usize] += pixels[at + channel as usize] as u32;
                    }
                    count += 1;
                }
            }
            let at = (y * out_width + x) as usize * 4;
            for channel in 0..4 {
                out[at + channel] = ((sum[channel] + count / 2) / count) as u8;
            }
        }
    }
    out
}

fn encoder_error(why: rusty_dds::Error) -> Error {
    Error::Encoder(why.to_string())
}

fn write_header(bytes: &mut [u8], width: u32, height: u32, levels: u32, srgb: bool, dfd_at: usize) {
    bytes[..MAGIC.len()].copy_from_slice(&MAGIC);
    put32(bytes, 12, if srgb { VK_BC7_SRGB } else { VK_BC7_UNORM });
    put32(bytes, 16, 1);
    put32(bytes, 20, width);
    put32(bytes, 24, height);
    put32(bytes, 28, 0);
    put32(bytes, 32, 0);
    put32(bytes, 36, 1);
    put32(bytes, 40, levels);
    put32(bytes, 44, 0);
    put32(bytes, 48, dfd_at as u32);
    put32(bytes, 52, DFD as u32);
}

/// The one descriptor shape Carbine writes: BC7, 4x4 blocks of 16 bytes, straight alpha.
fn descriptor(srgb: bool) -> [u8; DFD] {
    let mut d = [0u8; DFD];
    put32(&mut d, 0, (DFD - 4) as u32);
    // Khronos vendor, basic block type, version 2, 40 bytes including this header.
    put32(&mut d, 4, 0);
    put16(&mut d, 8, 2);
    put16(&mut d, 10, (DFD - 4) as u16);
    d[COLOR_MODEL_OFFSET] = BC7_COLOR_MODEL;
    d[13] = 1; // BT.709 primaries
    d[14] = if srgb { 2 } else { 1 }; // sRGB, or linear
    d[15] = 0; // straight alpha
    // A block dimension is stored one less than it is.
    d[16..20].copy_from_slice(&[3, 3, 0, 0]);
    d[20] = BLOCK_BYTES as u8;
    // One sample spanning the whole 128 bit block: channel 0, unsigned, full range.
    d[30] = 127;
    put32(&mut d, 40, u32::MAX);
    d
}

/// A KTX2 file Carbine can put straight on the GPU.
#[derive(Debug)]
pub struct Texture<'a> {
    width: u32,
    height: u32,
    srgb: bool,
    levels: Vec<Level<'a>>,
}

/// One mip level, still in BC7 blocks.
#[derive(Debug)]
pub struct Level<'a> {
    pub width: u32,
    pub height: u32,
    pub bytes: &'a [u8],
}

impl<'a> Texture<'a> {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Whether the texels are display-encoded, which picks the GPU's own format.
    pub fn srgb(&self) -> bool {
        self.srgb
    }

    /// The stored chain, biggest level first.
    pub fn levels(&self) -> &[Level<'a>] {
        &self.levels
    }
}

/// Read a KTX2 file, accepting only what [`pack`] writes.
pub fn read(bytes: &[u8]) -> Result<Texture<'_>, Error> {
    if bytes.len() < HEADER {
        return Err(Error::TooShort);
    }
    if !bytes.starts_with(&MAGIC) {
        return Err(Error::NotKtx2);
    }

    let vk_format = get32(bytes, 12);
    let srgb = match vk_format {
        VK_BC7_SRGB => true,
        VK_BC7_UNORM => false,
        other => {
            return Err(Error::Unsupported(format!(
                "only BC7 images load, and this one calls itself vkFormat {other}"
            )));
        }
    };
    if get32(bytes, 16) != 1 {
        return Err(Error::Unsupported(
            "its data type is not a single byte".into(),
        ));
    }
    let width = get32(bytes, 20);
    let height = get32(bytes, 24);
    if width == 0 || height == 0 {
        return Err(Error::Unsupported("it has no pixels".into()));
    }
    if get32(bytes, 28) > 1 {
        return Err(Error::Unsupported("a 3D texture".into()));
    }
    if get32(bytes, 32) > 1 {
        return Err(Error::Unsupported("an array of textures".into()));
    }
    if get32(bytes, 36) != 1 {
        return Err(Error::Unsupported("a cubemap".into()));
    }
    let levels = get32(bytes, 40);
    if levels == 0 {
        return Err(Error::Unsupported(
            "a file that leaves the mip chain to be built at load time".into(),
        ));
    }
    if get32(bytes, 44) != 0 {
        return Err(Error::Unsupported(
            "level data that is supercompressed as well as block compressed".into(),
        ));
    }
    if get64(bytes, 64) + get64(bytes, 72) > 0 {
        return Err(Error::Unsupported("supercompression global data".into()));
    }

    let index_end = HEADER + levels as usize * LEVEL_INDEX;
    let dfd_at = get32(bytes, 48) as usize;
    let dfd_end = dfd_at + get32(bytes, 52) as usize;
    if index_end > dfd_at || dfd_end > bytes.len() {
        return Err(Error::Unsupported(
            "its level index and data format descriptor overlap or run off the end".into(),
        ));
    }
    read_descriptor(&bytes[dfd_at..dfd_end], srgb)?;

    let kvd_end = get32(bytes, 56) as usize + get32(bytes, 60) as usize;
    if kvd_end > bytes.len() {
        return Err(Error::Unsupported(
            "its key/value data runs off the end of the file".into(),
        ));
    }

    let mut chain = Vec::with_capacity(levels as usize);
    let (mut width_at_level, mut height_at_level) = (width, height);
    for level in 0..levels {
        let at = HEADER + level as usize * LEVEL_INDEX;
        let offset = get64(bytes, at) as usize;
        let length = get64(bytes, at + 8) as usize;
        if get64(bytes, at + 16) as usize != length {
            return Err(Error::Unsupported(format!(
                "level {level} claims a different size once uncompressed"
            )));
        }
        let expected = level_bytes(width_at_level, height_at_level);
        if length != expected {
            return Err(Error::WrongLevelSize {
                level,
                expected,
                found: length,
            });
        }
        let end = offset
            .checked_add(length)
            .ok_or(Error::Truncated { level })?;
        if end > bytes.len() {
            return Err(Error::Truncated { level });
        }
        chain.push(Level {
            width: width_at_level,
            height: height_at_level,
            bytes: &bytes[offset..end],
        });
        width_at_level = (width_at_level / 2).max(1);
        height_at_level = (height_at_level / 2).max(1);
    }

    Ok(Texture {
        width,
        height,
        srgb,
        levels: chain,
    })
}

/// The descriptor has to agree with the header, or the file is lying about its blocks.
fn read_descriptor(dfd: &[u8], srgb: bool) -> Result<(), Error> {
    const BLOCK: usize = 8 + 16 + 16;
    if dfd.len() < BLOCK || get32(dfd, 4) != 0 || get16(dfd, 10) as usize != BLOCK {
        return Err(Error::Unsupported(
            "a data format descriptor that is not one Khronos basic block".into(),
        ));
    }
    let color_model = dfd[COLOR_MODEL_OFFSET];
    if color_model != BC7_COLOR_MODEL {
        return Err(Error::Unsupported(format!(
            "a descriptor built for color model {color_model}, not BC7"
        )));
    }
    if dfd[16..20] != [3, 3, 0, 0] || dfd[20] as usize != BLOCK_BYTES {
        return Err(Error::Unsupported(
            "a descriptor whose blocks are not 4x4 of 16 bytes".into(),
        ));
    }
    if dfd[14] != if srgb { 2 } else { 1 } {
        return Err(Error::Unsupported(
            "a descriptor whose transfer function contradicts its format".into(),
        ));
    }
    Ok(())
}

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn get16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

fn get32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn get64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_is_as_deep_as_the_image_is_wide() {
        assert_eq!(mip_count(1, 1), 1);
        assert_eq!(mip_count(4, 4), 3);
        assert_eq!(mip_count(64, 64), 7);
        assert_eq!(mip_count(1024, 1), 11);
        assert_eq!(mip_count(0, 0), 1);
    }

    #[test]
    fn a_partial_block_still_costs_a_whole_one() {
        assert_eq!(level_bytes(4, 4), 16);
        assert_eq!(level_bytes(1, 1), 16);
        assert_eq!(level_bytes(5, 5), 64);
        assert_eq!(level_bytes(64, 64), 4096);
    }
}
