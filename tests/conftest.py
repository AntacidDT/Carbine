import struct

import carbine
import pytest

WIDTH = 64
HEIGHT = 64


@pytest.fixture
def headless():
    """An offscreen 64 by 64 target. The scene itself is one per process, so a
    test that moves the camera or the backdrop leaves it moved for the next."""
    try:
        adapter = carbine.init(WIDTH, HEIGHT, window=False)
    except RuntimeError as error:
        pytest.skip(f"no GPU renderer available: {error}")
    try:
        yield adapter
    finally:
        carbine.close()


@pytest.fixture
def flat(headless):
    """The framing the exact colour assertions are written against: no backdrop,
    the light straight overhead, and an orthographic camera showing two metres of
    world across the frame. One metre is 32 pixels and the origin is dead centre.

    A test needs this framing, not the next one, so the camera and the backdrop are
    handed back as they were found afterwards."""
    sky = carbine.atmosphere.sky()
    camera = carbine.camera()
    found = (
        sky.enabled,
        tuple(sky.sun),
        camera.mode,
        tuple(camera.position),
        tuple(camera.target),
        camera.viewHeight,
    )

    sky.enabled = False
    sky.sun = (0.0, 0.0, 1.0)
    camera.mode = "orthographic"
    camera.position = (0.0, 0.0, 10.0)
    camera.target = (0.0, 0.0, 0.0)
    camera.viewHeight = 2.0

    yield headless

    (
        sky.enabled,
        sky.sun,
        camera.mode,
        camera.position,
        camera.target,
        camera.viewHeight,
    ) = found


def bitmap(rows):
    """A 24 bit BMP from `(red, green, blue)` rows, top row first. The suite authors
    pictures by hand because Carbine only reads packed ones - the Packager is what
    turns a file like this into a `.ktx2`, and no test needs a decoder."""
    height, width = len(rows), len(rows[0])
    stride = (width * 3 + 3) // 4 * 4
    padding = b"\x00" * (stride - width * 3)
    body = b"".join(
        b"".join(bytes(pixel)[::-1] for pixel in row) + padding
        for row in reversed(rows)
    )
    header = struct.pack("<2sIHHI", b"BM", 14 + 40 + len(body), 0, 0, 14 + 40)
    info = struct.pack(
        "<IiiHHIIiiII", 40, width, height, 1, 24, 0, len(body), 0, 0, 0, 0
    )
    return header + info + body


@pytest.fixture
def authored(tmp_path):
    """Writes a named BMP of `(red, green, blue)` rows into the test's own folder and
    answers its path, for tests that care about the file before it is packed."""

    def draw(name, rows):
        path = tmp_path / name
        path.write_bytes(bitmap(rows))
        return str(path)

    return draw


@pytest.fixture
def picture(authored):
    """Four colour blocks: red top left, green top right, blue bottom left, white
    bottom right. Eight pixels a side, so the middle of a block sits well away from
    every edge and the sampler has nothing to blend across."""
    size = 8

    def colour(x, y):
        if y < size // 2:
            return (255, 0, 0) if x < size // 2 else (0, 255, 0)
        return (0, 0, 255) if x < size // 2 else (255, 255, 255)

    return authored("quadrants.bmp", [[colour(x, y) for x in range(size)] for y in range(size)])


@pytest.fixture
def skin(picture):
    """The authored picture above packed to `.ktx2`, which is how every image in the
    suite reaches the engine: the Packager converts it, the loader reads it back."""
    return carbine.packager.image(picture)
