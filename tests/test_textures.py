"""`carbine.procedural.texture` — the generator that paints a surface.

A game needs walls, ground and rocks, and the usual way to get them is to find a
website, sort the pictures by licence, download the one that tiles, and hope the
size suits. This replaces all of that with one call: the engine rolls the surface
from a seed, bakes its own relief into the colour, and writes the `.ktx2` an object
is skinned with. What the tests can check without a screen is that the answers are
the ones on offer, that a seed is a whole description of the picture, and that the
file that lands on disk is the format the engine reads."""

import os

import carbine
import pytest

KTX2 = bytes([0xAB]) + b"KTX 20\xBB\x0D\x0A\x1A\x0A"


def tile(folder, surface, **asked):
    """Paints into `folder` and answers the path, so a test that wants two pictures of
    the same surface can ask for them under two names."""
    return carbine.procedural.texture(surface, out=str(folder), **asked)


def test_a_painted_surface_is_a_packed_image_the_engine_reads(tmp_path):
    written = tile(tmp_path, "grass", seed="pine")

    assert written.endswith(".ktx2")
    with open(written, "rb") as file:
        assert file.read(12) == KTX2, "the container the engine's one loader reads"
    assert os.path.getsize(written) > 0


def test_the_name_offered_is_the_answer_in_the_lists():
    types = carbine.procedural.textureTypes()
    assert types == ["brick", "grass", "rock", "sand"]
    assert carbine.procedural.textureQualities() == ["high", "low", "medium"]

    for surface in types:
        variations = carbine.procedural.textureVariations(surface)
        assert variations, f"{surface} has nothing to go wrong with it"


def test_everything_on_offer_paints(tmp_path):
    """A list a script can only read and not use is a trap, so each name in each list
    has to be a name the generator takes."""
    for surface in carbine.procedural.textureTypes():
        for variation in carbine.procedural.textureVariations(surface):
            written = tile(tmp_path, surface, variation=variation, seed="offer")
            assert os.path.exists(written)


def test_a_seed_is_a_whole_description_of_the_picture(tmp_path):
    """The same asking, twice, in two files: if a byte differs, a friend who types the
    same seed does not get the surface that was shown to them."""
    first = tile(tmp_path, "rock", variation="cracked", quality="low", seed="pine")
    again = os.path.join(tmp_path, "again.ktx2")
    second = carbine.procedural.texture(
        "rock", variation="cracked", quality="low", seed="pine", out=again
    )

    assert open(first, "rb").read() == open(second, "rb").read()

    elsewhere = os.path.join(tmp_path, "other.ktx2")
    other = carbine.procedural.texture(
        "rock", variation="cracked", quality="low", seed="cabin", out=elsewhere
    )
    assert open(first, "rb").read() != open(other, "rb").read(), "the seed did not matter"


def test_a_number_and_a_word_are_both_seeds(tmp_path):
    """A seed read off one screen is typed straight back into another, so the word and
    the number it hashed to have to paint the same surface - and a seed the engine
    cannot keep is refused rather than quietly cut down."""
    from_word = tile(tmp_path, "sand", seed="dune")
    number = int(os.path.basename(from_word).rsplit("_s", 1)[1].split(".")[0])

    typed = os.path.join(tmp_path, "typed.ktx2")
    carbine.procedural.texture("sand", seed=number, out=typed)

    assert open(from_word, "rb").read() == open(typed, "rb").read(), "the number lost"

    with pytest.raises(TypeError):
        carbine.procedural.texture("sand", seed=[1, 2], out=str(tmp_path))


def test_a_quality_is_a_size_and_an_amount_of_detail(tmp_path):
    sizes = {
        quality: os.path.getsize(tile(tmp_path, "brick", quality=quality, seed="wall"))
        for quality in ("low", "medium", "high")
    }

    assert sizes["low"] < sizes["medium"] < sizes["high"], sizes


def test_the_tile_is_named_for_everything_it_was_asked_for(tmp_path):
    written = tile(tmp_path, "grass", variation="dry", quality="high", seed="pine")
    name = os.path.basename(written)

    assert name.startswith("grass_dry_high_s"), name
    assert name.endswith(".ktx2")

    # And a rerun overwrites its own picture instead of adding a second one.
    assert tile(tmp_path, "grass", variation="dry", quality="high", seed="pine") == written
    assert len([entry for entry in os.listdir(tmp_path) if entry.startswith("grass_")]) == 1


def test_a_name_is_read_the_way_a_script_types_it(tmp_path):
    """`lawn` is grass and `hi` is high, so a guess from the surface it names is taken.
    The file is still named for what the engine settled on."""
    written = tile(tmp_path, "lawn", quality="hi", seed="pine")
    name = os.path.basename(written)

    assert name.startswith("grass_plain_high_s"), name


def test_a_folder_or_an_exact_name_can_be_asked_for(tmp_path):
    built = tmp_path / "built"
    built.mkdir()

    written = tile(built, "sand", seed="dune")
    assert os.path.dirname(written) == str(built)
    assert os.path.basename(written).startswith("sand_plain_medium_s")
    assert tile(built, "sand", seed="dune") == written, "a rerun wrote a second tile"
    assert len(os.listdir(built)) == 1

    fresh = tmp_path / "fresh"
    written = carbine.procedural.texture("sand", seed="dune", out=f"{fresh}{os.sep}")
    assert os.path.dirname(written) == str(fresh), "a name with a separator is a folder"
    assert os.path.exists(written)

    # A `pathlib.Path` is how a script holds a file, so an exact name arrives both ways.
    named = tmp_path / "flooring.ktx2"
    assert carbine.procedural.texture("sand", seed="dune", out=named) == str(named)
    assert os.path.exists(named)


def test_a_surface_the_engine_cannot_paint_is_refused_with_the_list(tmp_path):
    with pytest.raises(ValueError, match="cheese") as error:
        tile(tmp_path, "cheese")
    assert "brick, grass, rock, sand" in str(error.value), error

    with pytest.raises(ValueError, match="cheese"):
        carbine.procedural.textureVariations("cheese")


def test_a_variation_that_belongs_to_another_surface_is_refused(tmp_path):
    """`pebble` is a way to ruin sand, not a way to ruin a wall, and the refusal has to
    say which of its own the surface takes rather than accepting the word."""
    with pytest.raises(ValueError, match="pebble") as error:
        tile(tmp_path, "brick", variation="pebble")
    assert "brick takes: dusty, mossy" in str(error.value), error

    with pytest.raises(ValueError, match="glazed"):
        tile(tmp_path, "brick", quality="glazed")


def test_a_folder_that_cannot_be_made_is_an_os_error(tmp_path):
    blocked = tmp_path / "blocked"
    blocked.write_text("a file where a folder should be")

    with pytest.raises(OSError, match="could not be written"):
        carbine.procedural.texture("rock", out=f"{blocked / 'inside'}{os.sep}")


def test_left_alone_the_tile_lands_where_the_game_is_run_from(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)

    written = carbine.procedural.texture("grass", seed="pine")

    assert os.path.dirname(written) == "", written
    assert os.path.exists(tmp_path / written)


def test_the_answer_is_the_path_a_skin_wants(tmp_path, headless):
    """Generated, and worn straight away: what the call returns is exactly what an
    object takes, with nothing else for the engine to read."""
    written = tile(tmp_path, "grass", variation="moss", seed="pine")
    ground = carbine.new("plane")
    try:
        assert ground.texture is None
        ground.texture = written
        assert ground.texture == written
    finally:
        ground.destroy()
