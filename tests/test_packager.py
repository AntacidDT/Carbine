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


OBJ = "o Triangle\nv 0 0 0\nv 2 0 0\nv 0 3 0\nf 1 2 3\n"
STL = (
    "solid slab\nfacet normal 0 0 1\nouter loop\n"
    "vertex 0 0 0\nvertex 2 0 0\nvertex 0 3 0\n"
    "endloop\nendfacet\nendsolid slab\n"
)


def test_an_authored_obj_is_packed_beside_its_source(tmp_path):
    source = tmp_path / "triangle.obj"
    source.write_text(OBJ)

    built = carbine.packager.model(source)

    assert built == str(tmp_path / "triangle.glb")
    with open(built, "rb") as file:
        assert file.read(4) == b"glTF", "the one container the engine's loader reads"


def test_an_authored_stl_is_packed_beside_its_source(tmp_path):
    source = tmp_path / "slab.stl"
    source.write_text(STL)

    built = carbine.packager.model(source)

    assert built == str(tmp_path / "slab.glb")
    assert pathlib.Path(built).stat().st_size > 0


def test_the_packed_model_can_be_worn_with_its_vertices(tmp_path):
    source = tmp_path / "triangle.obj"
    source.write_text(OBJ)
    built = carbine.packager.model(source)

    holder = carbine.new("Model")
    try:
        holder.model = built
        assert holder.model == built
        assert len(holder.children) == 1
        child = holder.children[0]
        assert child.name == "Triangle"
        assert child.mesh.vertexCount == 3
        assert child.mesh.triangleCount == 1
        assert child.mesh.positions[1].x == pytest.approx(2.0)
        assert child.mesh.positions[2].y == pytest.approx(3.0)
    finally:
        holder.destroy()


def test_a_model_can_go_into_a_folder_or_an_exact_name(tmp_path):
    source = tmp_path / "chair.obj"
    source.write_text(OBJ)
    folder = tmp_path / "built"
    folder.mkdir()

    assert carbine.packager.model(source, out=folder) == str(folder / "chair.glb")
    fresh = tmp_path / "fresh"
    assert carbine.packager.model(source, out=f"{fresh}{os.sep}") == str(
        fresh / "chair.glb"
    )
    named = tmp_path / "seat.glb"
    assert carbine.packager.model(source, out=named) == str(named)


def test_a_file_that_is_not_a_model_is_refused(tmp_path):
    notes = tmp_path / "notes.txt"
    notes.write_text("words, not triangles")

    with pytest.raises(ValueError, match="notes.txt") as error:
        carbine.packager.model(str(notes))
    assert "could not be read as a model" in str(error.value), error
    assert "obj and stl" in str(error.value), error

    with pytest.raises(ValueError, match="could not be read as a model"):
        carbine.packager.model(str(tmp_path / "ghost.obj"))
