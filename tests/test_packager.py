"""`carbine.packager` — the importer that turns an authored file into the format the
engine loads.

Images come to Carbine as `.ktx2`: BC7 blocks with the whole mip chain in one container,
which is what the GPU reads with no decoder in the way. The engine carries one image
loader, so a picture in any other format is not decoded late inside a frame — it is
converted on the way in, here by the same call a script makes and in Enginebay by the
same code behind it."""

import os
import pathlib

import carbine
import pytest

KTX2 = bytes([0xAB]) + b"KTX 20\xBB\x0D\x0A\x1A\x0A"


def test_an_authored_picture_is_packed_beside_its_source(authored):
    picture = authored(
        "wood.bmp", [[(x * 8, y * 8, 128) for x in range(8)] for y in range(8)]
    )

    packed = carbine.packager.image(picture)

    assert packed == picture.removesuffix(".bmp") + ".ktx2"
    with open(packed, "rb") as file:
        assert file.read(12) == KTX2, "the container the engine's one loader reads"


def test_a_path_object_is_as_good_as_a_string(authored, tmp_path):
    """`pathlib.Path` is how a script holds a file, so the Packager takes one on the
    way in as well as a plain string."""
    picture = pathlib.Path(authored("slate.bmp", [[(4, 20, 90)] * 8 for _ in range(8)]))

    built = tmp_path / "built" / "slate.ktx2"
    packed = carbine.packager.image(picture, out=built)

    assert packed == str(built)
    assert os.path.getsize(packed) > 0


def test_a_packed_picture_is_smaller_than_the_pixels_it_carries(authored):
    """The efficiency argument in one number: a 64 by 64 gradient costs 12 KB as BMP
    and less than that as BC7 with a full mip chain on top — and the copy that lands in
    video memory is the file itself, with no RGBA8 staging step to halve the saving."""
    rows = [[(x * 4, y * 4, 255 - x * 4) for x in range(64)] for y in range(64)]
    picture = authored("gradient.bmp", rows)

    packed = carbine.packager.image(picture)

    assert os.path.getsize(packed) < os.path.getsize(picture)


def test_a_folder_or_an_exact_name_can_be_asked_for(authored, tmp_path):
    picture = authored("brick.bmp", [[(9, 6, 4) for _ in range(8)] for _ in range(8)])

    built = tmp_path / "built"
    built.mkdir()
    assert carbine.packager.image(picture, out=str(built)) == str(built / "brick.ktx2")

    fresh = tmp_path / "fresh"
    assert carbine.packager.image(picture, out=f"{fresh}{os.sep}") == str(
        fresh / "brick.ktx2"
    ), "a name with a separator on the end is a folder to make"

    named = tmp_path / "flooring.ktx2"
    assert carbine.packager.image(picture, out=str(named)) == str(named)


def test_the_answer_is_the_path_a_skin_wants(picture):
    """Packed on the way in, worn straight away: what the Packager returns is exactly
    what an object or a crowd takes, with nothing else for the engine to read."""
    packed = carbine.packager.image(picture)
    tile = carbine.new("plane")
    crowd = carbine.procedural.particles(image=packed)
    try:
        assert tile.texture is None
        tile.texture = packed
        assert tile.texture == packed
        assert crowd.texture == packed
    finally:
        crowd.destroy()
        tile.destroy()


def test_a_file_that_is_not_a_picture_is_refused(tmp_path):
    notes = tmp_path / "notes.txt"
    notes.write_text("words, not pixels")

    with pytest.raises(ValueError, match="notes.txt") as error:
        carbine.packager.image(str(notes))
    assert "could not be read as a picture" in str(error.value), error
    assert "png, jpg, webp, tga and bmp" in str(error.value), error

    with pytest.raises(ValueError, match="could not be read as a picture"):
        carbine.packager.image(str(tmp_path / "ghost.png"))
