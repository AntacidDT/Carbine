//! Proof that what `carbine_ktx::pack` writes is real KTX2 and real BC7, checked by other
//! people's implementations rather than by our own.

use bcn_decode::{Format as BlockFormat, decode_rgba8_to_vec};
use carbine_ktx::{Error, MAGIC, pack, read};
use ktx2::dfd::{Basic, Block};
use ktx2::{Format, Reader, TransferFunction};

/// One entry of the level index, so a test can point at the descriptor after it.
const LEVEL_INDEX: usize = 24;
/// The identifier plus the fixed header fields, which is where the level index begins.
const HEADER: usize = 80;

/// A picture with hard edges, a diagonal, a ramp and a hole, so a codec cannot pass by
/// averaging a flat colour.
fn picture(width: u32, height: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let ramp = (x * 255 / width.max(1)) as u8;
            let edge = if x > y { 255 } else { 0 };
            let alpha = if x + y < (width + height) / 2 { 255 } else { 0 };
            pixels.extend_from_slice(&[ramp, edge, 255 - ramp, alpha]);
        }
    }
    pixels
}

fn psnr(source: &[u8], decoded: &[u8]) -> f64 {
    let mut squared = 0.0f64;
    let mut samples = 0.0f64;
    for (source, decoded) in source.chunks(4).zip(decoded.chunks(4)) {
        for channel in 0..3 {
            let d = source[channel] as f64 - decoded[channel] as f64;
            squared += d * d;
            samples += 1.0;
        }
    }
    let mse = squared / samples;
    if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0 * 255.0 / mse).log10()
    }
}

#[test]
fn a_packed_image_is_a_ktx2_file_to_everyone_else() {
    let bytes = pack(16, 8, &picture(16, 8), true).unwrap();
    let oracle = Reader::new(&bytes).unwrap();
    let header = oracle.header();

    assert_eq!(header.format, Some(Format::BC7_SRGB_BLOCK));
    assert_eq!((header.pixel_width, header.pixel_height), (16, 8));
    assert_eq!(header.type_size, 1);
    assert_eq!(header.pixel_depth, 0);
    assert_eq!(header.layer_count, 0);
    assert_eq!(header.face_count, 1);
    assert_eq!(header.supercompression_scheme, None);
    assert_eq!(oracle.color_model(), Some(ktx2::ColorModel::BC7));
    assert_eq!(oracle.transfer_function(), Some(TransferFunction::SRGB));
    assert_eq!(oracle.is_alpha_premultiplied(), Some(false));

    let ours = read(&bytes).unwrap();
    assert_eq!(ours.width(), 16);
    assert_eq!(ours.height(), 8);
    assert!(ours.srgb());
    assert_eq!(ours.levels().len(), header.level_count as usize);
    for (level, (theirs, ours)) in oracle.levels().zip(ours.levels()).enumerate() {
        assert_eq!(theirs.data, ours.bytes, "level {level} disagrees on bytes");
        assert_eq!(theirs.uncompressed_byte_length as usize, ours.bytes.len());
    }
}

#[test]
fn a_linear_image_declares_itself_linear() {
    let bytes = pack(8, 8, &picture(8, 8), false).unwrap();
    let oracle = Reader::new(&bytes).unwrap();

    assert_eq!(oracle.header().format, Some(Format::BC7_UNORM_BLOCK));
    assert_eq!(oracle.transfer_function(), Some(TransferFunction::Linear));
    assert!(!read(&bytes).unwrap().srgb());
}

#[test]
fn the_descriptor_matches_the_khronos_generator() {
    for srgb in [true, false] {
        let format = if srgb {
            Format::BC7_SRGB_BLOCK
        } else {
            Format::BC7_UNORM_BLOCK
        };
        let (basic, type_size) = Basic::from_format(format).unwrap();
        let block = Block::Basic(basic);
        let mut expected = Vec::new();
        expected.extend_from_slice(
            &u32::try_from(block.serialized_length())
                .unwrap()
                .to_le_bytes(),
        );
        let end = expected.len() + block.serialized_length();
        expected.resize(end, 0);
        block.to_bytes(&mut expected[4..]);

        let bytes = pack(4, 4, &picture(4, 4), srgb).unwrap();
        let levels = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        let dfd_at = HEADER + levels * LEVEL_INDEX;
        let ours = &bytes[dfd_at..dfd_at + expected.len()];
        assert_eq!(
            ours,
            expected.as_slice(),
            "the DFD we write is not the one Khronos generates"
        );
        assert_eq!(type_size, 1);
    }
}

#[test]
fn a_stored_mip_is_the_one_above_halved() {
    let bytes = pack(64, 24, &picture(64, 24), true).unwrap();
    let texture = read(&bytes).unwrap();
    let levels = texture.levels();

    assert_eq!(levels.len(), 7);
    assert_eq!((levels[0].width, levels[0].height), (64, 24));
    assert_eq!((levels[1].width, levels[1].height), (32, 12));
    assert_eq!((levels[6].width, levels[6].height), (1, 1));
    assert_eq!(levels[6].bytes.len(), 16);
    for level in levels {
        let blocks = level.width.div_ceil(4) * level.height.div_ceil(4);
        assert_eq!(
            level.bytes.len(),
            blocks as usize * 16,
            "a {}x{} level that does not fill its blocks",
            level.width,
            level.height
        );
    }
}

#[test]
fn an_edge_that_is_not_a_whole_block_still_packs() {
    let bytes = pack(5, 5, &picture(5, 5), true).unwrap();
    let texture = read(&bytes).unwrap();

    assert_eq!(texture.levels().len(), 3);
    assert_eq!(texture.levels()[0].bytes.len(), 64);
    let decoded = decode_rgba8_to_vec(BlockFormat::Bc7, texture.levels()[0].bytes, 5, 5).unwrap();
    assert_eq!(decoded.len(), 5 * 5 * 4);
}

#[test]
fn the_gpu_reads_back_the_picture_that_was_packed() {
    let source = picture(64, 64);
    let bytes = pack(64, 64, &source, true).unwrap();
    let texture = read(&bytes).unwrap();

    let decoded = decode_rgba8_to_vec(BlockFormat::Bc7, texture.levels()[0].bytes, 64, 64).unwrap();
    assert_eq!(decoded.len(), source.len());

    // Alpha is the channel a script blends on, so it has to survive nearly exactly;
    // BC7 spends its bits on colour and gives a hard-edged picture a few levels a channel.
    let mut worst_alpha = 0u32;
    for (source, decoded) in source.chunks(4).zip(decoded.chunks(4)) {
        worst_alpha = worst_alpha.max(source[3].abs_diff(decoded[3]) as u32);
    }
    assert!(worst_alpha <= 2, "alpha drifted by {worst_alpha}");

    let colour = psnr(&source, &decoded);
    assert!(colour > 35.0, "a colour psnr of {colour:.1} dB");
}

#[test]
fn a_flat_colour_keeps_its_own_through_a_block() {
    // The renderer's pixel tests lean on this: a skin of flat halves arrives as those
    // halves, give or take a BC7 step, because every block it is cut from holds one
    // colour and one alpha and the encoder has nothing to spend bits on.
    let mut pixels = Vec::new();
    for row in 0..8 {
        for _ in 0..8 {
            pixels.extend_from_slice(if row < 4 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 255, 255]
            });
        }
    }
    let bytes = pack(8, 8, &pixels, true).unwrap();
    let level = read(&bytes).unwrap();

    let decoded = decode_rgba8_to_vec(BlockFormat::Bc7, level.levels()[0].bytes, 8, 8).unwrap();
    for (texel, want) in decoded.chunks(4).zip(pixels.chunks(4)) {
        for channel in 0..4 {
            assert!(
                texel[channel].abs_diff(want[channel]) <= 2,
                "a flat texel wanted {want:?} and got {texel:?}"
            );
        }
    }
}

#[test]
fn a_file_that_is_not_one_of_ours_is_refused_by_name() {
    let good = pack(8, 8, &picture(8, 8), true).unwrap();
    assert!(read(&good).is_ok());
    assert!(is_not_an_image());

    let mut wrong_format = good.clone();
    wrong_format[12..16].copy_from_slice(&43u32.to_le_bytes()); // R8G8B8A8_SRGB
    assert!(matches!(read(&wrong_format), Err(Error::Unsupported(_))));

    let mut supercompressed = good.clone();
    supercompressed[44..48].copy_from_slice(&1u32.to_le_bytes()); // Basis Universal
    assert!(matches!(read(&supercompressed), Err(Error::Unsupported(_))));

    let mut cubemap = good.clone();
    cubemap[36..40].copy_from_slice(&6u32.to_le_bytes());
    assert!(matches!(read(&cubemap), Err(Error::Unsupported(_))));

    let mut lying_descriptor = good.clone();
    let dfd_at = u32::from_le_bytes(good[48..52].try_into().unwrap()) as usize;
    lying_descriptor[dfd_at + 12] = 133; // BC6H, not BC7
    assert!(matches!(
        read(&lying_descriptor),
        Err(Error::Unsupported(_))
    ));

    let mut short = good.clone();
    short.truncate(good.len() - 1);
    assert!(matches!(
        read(&short),
        Err(Error::Truncated { .. } | Error::WrongLevelSize { .. })
    ));

    assert!(matches!(read(&[0u8; 40]), Err(Error::TooShort)));
    let not_ktx2 = [b'x'; 100];
    assert!(matches!(read(&not_ktx2), Err(Error::NotKtx2)));
}

fn is_not_an_image() -> bool {
    let png = [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    !carbine_ktx::is_ktx2(&png) && carbine_ktx::is_ktx2(&MAGIC)
}
