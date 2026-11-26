"""`carbine.procedural.flipbook()` — an animated `.ktx2` with a frame per layer.

The frames are painted from one seed and packed as BC7 layers with their own mip
chains. The frame rate lives in the file, so pointing a skin at it is enough to make
a surface move; `textureFps` can write a rate over the painted one.
"""

import struct

import carbine
import pytest


@pytest.fixture
def quiet():
    """The scene is one per process; a skinned map left behind changes later frames."""

    def sweep():
        carbine.destroy("all")
        carbine.player().enabled = False

    sweep()
    yield
    sweep()


def test_a_flipbook_is_a_layered_ktx2(quiet, tmp_path):
    written = carbine.procedural.flipbook(
        "grass", frames=4, fps=12.0, quality="low", seed="flicker", out=tmp_path
    )

    with open(written, "rb") as file:
        header = file.read(80)
    assert header[:12] == b"\xabKTX 20\xbb\r\n\x1a\n"
    assert struct.unpack_from("<I", header, 32)[0] == 4, "four array layers"
    assert b"carbine.fps" in open(written, "rb").read()


def test_the_same_seed_paints_the_same_frames(quiet, tmp_path):
    first = carbine.procedural.flipbook(
        "brick", frames=3, fps=8, quality="low", seed="wall", out=tmp_path / "a.ktx2"
    )
    again = carbine.procedural.flipbook(
        "brick", frames=3, fps=8, quality="low", seed="wall", out=tmp_path / "b.ktx2"
    )
    other = carbine.procedural.flipbook(
        "brick", frames=3, fps=8, quality="low", seed="walL", out=tmp_path / "c.ktx2"
    )

    assert open(first, "rb").read() == open(again, "rb").read()
    assert open(first, "rb").read() != open(other, "rb").read()


def test_a_bad_flipbook_is_refused(quiet, tmp_path):
    with pytest.raises(ValueError, match="at least two"):
        carbine.procedural.flipbook("grass", frames=1)
    with pytest.raises(ValueError, match="positive"):
        carbine.procedural.flipbook("grass", fps=0)


def test_a_flipbook_can_skin_a_world_object(quiet, headless, tmp_path):
    written = carbine.procedural.flipbook(
        "grass", frames=4, fps=12, quality="low", seed="skin", out=tmp_path
    )
    tile = carbine.new("plane")

    tile.texture = written
    assert tile.texture == written
    assert tile.textureFps == pytest.approx(0.0), "zero uses the file's own rate"
    carbine.render()

    tile.textureFps = 20.0
    assert tile.textureFps == pytest.approx(20.0)
    carbine.render()
    tile.textureFps = 0.0
    carbine.render()

    assert len(carbine.readPixels()) == 64 * 64 * 4
    tile.destroy()


def test_a_flipbook_can_skin_a_map(quiet, headless, tmp_path):
    written = carbine.procedural.flipbook(
        "rock", frames=3, fps=6, quality="low", seed="ground", out=tmp_path
    )
    ground = carbine.procedural.map()
    ground.texture = written
    ground.textureFps = 10.0
    assert ground.textureFps == pytest.approx(10.0)
    carbine.render()
    assert len(carbine.readPixels()) == 64 * 64 * 4
    ground.destroy()
