"""`carbine.procedural.particles()` - the generator that turns a shape, a colour or
an image into a crowd flying through the world.

A system is a node, not a switch: it hangs in the tree, so it can be bolted to a
chimney and throw embers from the top of it, or stand by itself in the middle of the
sky. It is parented, moved, coloured, skinned, hidden and destroyed the way any
object is, and the knobs it adds are about its crowd.

What each drop is doing there - where it was born, how fast it falls, what stops it,
what it throws back when it does - is the core's business, so these tests read the
same numbers the editor will. Rain is the default because rain is what a crowd of
streaks falling out of a box looks like; `shape="circle", speed=1.2` is snow.
"""

import carbine
import pytest

SECOND = 1.0 / 60.0
WIDTH = 64

CLEAR = (22, 21, 20)


def pixel(data, x, y):
    offset = (y * WIDTH + x) * 4
    return tuple(data[offset : offset + 3])


def painted(data):
    """How much of the frame is not the empty colour."""
    return sum(1 for i in range(0, len(data), 4) if tuple(data[i : i + 4]) != (*CLEAR, 255))


def fall(crowd, seconds):
    """Steps the world one frame at a time and reports the most drops that were ever
    in the air and the most brought up short in a single step. A splash only shows for
    a few frames, and a hit only counts for the step that made it."""
    most_air = crowd.inAir
    most_hits = crowd.impacts
    for _ in range(round(seconds / SECOND)):
        carbine.step(SECOND)
        most_air = max(most_air, crowd.inAir)
        most_hits = max(most_hits, crowd.impacts)
    return most_air, most_hits


@pytest.fixture
def clear_air():
    """A world with nothing flying in it. The scene is one per process, so a crowd
    left active would rain on the next test's frame."""

    def sweep():
        for node in carbine.scene.root.children:
            if isinstance(node, carbine.Particle):
                node.destroy()
        carbine.physics.stopAll()
        carbine.player().enabled = False

    sweep()
    yield
    sweep()


def veil(**knobs):
    """A crowd that stays inside the two metre box the exact colour assertions are
    written against, with drops big enough to land on a pixel. Squares, because a
    streak is six times as long as it is wide and would cover the frame by itself."""
    knobs.setdefault("shape", "square")
    knobs.setdefault("area", (1.0, 1.0, 1.0))
    knobs.setdefault("size", 0.3)
    knobs.setdefault("count", 12)
    return carbine.procedural.particles(**knobs)


def mote(**knobs):
    """One drop, standing exactly where its node stands: the shape a pixel assertion
    can be written against."""
    knobs.setdefault("count", 1)
    knobs.setdefault("shape", "square")
    knobs.setdefault("area", (0.0, 0.0, 0.0))
    knobs.setdefault("size", 0.5)
    knobs.setdefault("color", "#ffffff")
    return carbine.procedural.particles(**knobs)


def corners(pixels):
    """The four corners of a drop standing in the middle of the frame, two pixels in
    from each axis: far enough off the middle to sit inside one block of the test skin,
    close enough in that a drop the dice made small still covers all four."""
    inside = [(-2, -2), (2, -2), (-2, 2), (2, 2)]
    return tuple(pixel(pixels, 32 + dx, 32 + dy) for dx, dy in inside)


def test_the_generator_lists_the_cuts_a_drop_can_take(clear_air):
    assert carbine.procedural.shapes() == ["circle", "square", "streak"]


def test_a_fresh_system_is_a_node_wearing_rain(clear_air):
    rain = carbine.procedural.particles()
    try:
        assert rain.kind == "particles"
        assert rain.name == "Particles"
        assert rain.shape == "streak"
        assert rain.count == 400
        assert rain.inAir == 400, "a fresh system is already flying its crowd"
        assert rain.size == pytest.approx(0.05)
        assert rain.speed == pytest.approx(9.0)
        assert tuple(rain.direction) == pytest.approx((0.0, 0.0, -1.0))
        assert tuple(rain.area) == pytest.approx((24.0, 24.0, 16.0))
        assert rain.lifetime == pytest.approx(2.0)
        assert rain.active is True
        assert rain.collide is True
        assert rain.splash == 0
        assert rain.recycle is True
        assert rain.impacts == 0
        assert rain.texture is None

        # A node, so it is in the tree the moment it exists.
        assert rain in carbine.scene.root.children
        assert isinstance(rain.parent, carbine.Object)
    finally:
        rain.destroy()


def test_every_knob_can_be_written_as_it_is_made(clear_air):
    chimney = carbine.new("cube", name="Chimney")
    chimney.position = (0.0, 0.0, 4.0)
    hearth = carbine.procedural.particles(seed="hearth")
    try:
        embers = carbine.procedural.particles(
            name="Embers",
            parent=chimney,
            shape="circle",
            count=24,
            size=0.1,
            speed=2.5,
            direction=(0.0, 0.0, 1.0),
            area=(0.5, 0.5, 0.5),
            lifetime=4.0,
            seed="hearth",
            color="#eb7a34",
            active=True,
            collide=False,
            splash=3,
            recycle=False,
        )
        try:
            assert embers.name == "Embers"
            assert embers.shape == "circle"
            assert embers.count == 24
            assert embers.size == pytest.approx(0.1)
            assert embers.speed == pytest.approx(2.5)
            assert tuple(embers.direction) == pytest.approx((0.0, 0.0, 1.0))
            assert tuple(embers.area) == pytest.approx((0.5, 0.5, 0.5))
            assert embers.lifetime == pytest.approx(4.0)
            assert embers.active is True
            assert embers.collide is False
            assert embers.splash == 3
            assert embers.recycle is False
            assert embers.colorHex == "#eb7a34"
            assert embers.seed == hearth.seed, "a word is a dice, and the same word repeats"
            assert embers.parent == chimney
            assert tuple(embers.position) == pytest.approx((0.0, 0.0, 0.0))

            # A node rides its branch, so its place is measured from the chimney
            # rather than from the scene origin.
            embers.position = (0.0, 0.0, 0.5)
            assert tuple(embers.position) == pytest.approx((0.0, 0.0, 0.5))
        finally:
            embers.destroy()
    finally:
        hearth.destroy()
        chimney.destroy()


def test_a_knob_left_out_keeps_what_the_engine_gives(clear_air):
    only = carbine.procedural.particles(shape="square")
    try:
        assert only.shape == "square"
        assert only.count == 400, "the rest of it is still the rain the engine writes"
        assert only.collide is True
    finally:
        only.destroy()


def test_a_shape_the_engine_does_not_know_is_refused(clear_air):
    with pytest.raises(ValueError, match="unknown particle shape"):
        carbine.procedural.particles(shape="hail")

    rain = carbine.procedural.particles()
    try:
        with pytest.raises(ValueError, match="unknown particle shape"):
            rain.shape = "pentagon"
        assert rain.shape == "streak", "and the system keeps what it had"
    finally:
        rain.destroy()


def test_switching_off_leaves_the_node_and_clears_the_air(flat, clear_air):
    dry = carbine.readPixels()
    rain = veil(color="#ffffff")
    try:
        assert rain.inAir == 12
        assert painted(carbine.readPixels()) > 0, "the crowd is in front of the lens"

        rain.active = False
        assert rain.inAir == 0, "a stopped crowd is out of the air"
        assert carbine.readPixels() == dry
        assert rain.count == 12, "but the system itself was stopped, not deleted"
        assert rain.name == "Particles"
        assert rain.shape == "square"
        assert rain.active is False

        rain.active = True
        assert rain.inAir == 12, "and it flies again from where it stands"
    finally:
        rain.destroy()


def test_a_seed_names_the_veil_and_writing_it_back_gives_the_same_one(flat, clear_air):
    snow = veil(shape="circle", seed="monsoon", color="#ffffff")
    try:
        monsoon = carbine.readPixels()
        assert painted(monsoon) > 0

        snow.seed = 7
        assert snow.seed == 7, "a number is as good as a word"
        seven = carbine.readPixels()

        snow.seed = 7
        assert carbine.readPixels() == seven, "the same number lays the same weather"

        snow.seed = "monsoon"
        assert carbine.readPixels() == monsoon, "and the word lays the one it named"

        snow.seed = 8
        assert carbine.readPixels() != seven, "a different dice lays a different crowd"

        # A word hashes to a number the full width of the dice, and a script reads it
        # off one system to write it into the next.
        rolled = snow.seed + 1
        snow.seed = rolled
        assert snow.seed == rolled, "and reading a seed back is not the end of it"

        snow.seed = 18_446_744_073_709_551_615
        assert snow.seed == 18_446_744_073_709_551_615, "the whole of u64 is a legal dice"

        with pytest.raises(TypeError, match="a seed is a word or a whole number"):
            snow.seed = 3.5
    finally:
        snow.destroy()


def test_moving_the_node_leaves_the_air_and_restart_relays_it(flat, clear_air):
    drop = mote(name="Mote")
    try:
        drop.position = (0.0, 0.0, 3.0)
        drop.restart()
        assert pixel(carbine.readPixels(), 32, 32) != CLEAR, "the drop stands on the node"

        # A drop already flying keeps its own world, so moving the node does not
        # pick the crowd up out of the air with it.
        drop.position = (0.5, 0.0, 3.0)
        assert pixel(carbine.readPixels(), 32, 32) != CLEAR, "and it did not"
        assert pixel(carbine.readPixels(), 48, 32) == CLEAR

        drop.restart()
        assert pixel(carbine.readPixels(), 32, 32) == CLEAR, "restart lays it at the node"
        assert pixel(carbine.readPixels(), 48, 32) != CLEAR
    finally:
        drop.destroy()


def test_a_system_on_a_riding_node_lays_its_crowd_at_the_branch(flat, clear_air):
    """The box is centred on where the node *stands*, which for a child is where its
    parent carries it, so a veil hung on a chimney moves with the chimney.

    The parent is another system, emptied to count=0, so the only thing that can
    paint a pixel here is the single drop itself."""
    chimney = carbine.procedural.particles(name="Chimney", count=0)
    drop = mote(name="Embers", parent=chimney)
    try:
        assert chimney.inAir == 0, "a crowd of none hangs in the tree and paints nothing"

        chimney.position = (0.5, 0.0, 0.0)
        drop.restart()
        assert pixel(carbine.readPixels(), 48, 32) != CLEAR, "the crowd flew with its node"
        assert pixel(carbine.readPixels(), 32, 32) == CLEAR

        chimney.position = (-0.5, 0.0, 0.0)
        drop.restart()
        assert pixel(carbine.readPixels(), 16, 32) != CLEAR
        assert pixel(carbine.readPixels(), 48, 32) == CLEAR
    finally:
        drop.destroy()
        chimney.destroy()


def test_a_wall_between_the_crowd_and_the_lens_hides_it(flat, clear_air):
    drop = mote(name="Mote")
    wall = carbine.new("cube", name="Wall")
    wall.size = (2.0, 2.0, 1.0)
    wall.position = (0.0, 0.0, 1.0)
    wall.color = "#00ff00"
    try:
        assert pixel(carbine.readPixels(), 32, 32) == (0, 255, 0), (
            "the crowd is drawn in the world, not over it"
        )
    finally:
        drop.destroy()
        wall.destroy()


def test_a_hidden_system_leaves_the_frame(flat, clear_air):
    dry = carbine.readPixels()
    rain = veil(color="#ffffff")
    try:
        flying = carbine.readPixels()
        assert painted(flying) > 0

        rain.visible = False
        assert carbine.readPixels() == dry
        assert rain.inAir == 12, "hidden means unseen, not stopped"

        rain.visible = True
        assert carbine.readPixels() == flying
    finally:
        rain.destroy()


def test_a_crowd_that_collides_counts_what_it_brought_up_short(clear_air):
    rain = carbine.procedural.particles(
        name="Shower", count=8, area=(0.4, 0.4, 0.0), size=0.05, lifetime=30.0
    )
    try:
        rain.position = (0.0, 0.0, 10.0)
        rain.restart()
        _, hits = fall(rain, 2.0)
        assert hits > 0, "the floor is everyone's, and the crowd hit it"

        rain.collide = False
        rain.restart()
        _, hits = fall(rain, 2.0)
        assert hits == 0, "switched off, the drops go straight through"
    finally:
        rain.destroy()


def test_a_hit_can_throw_droplets_back(clear_air):
    rain = carbine.procedural.particles(
        name="Shower", count=3, area=(0.2, 0.2, 0.0), size=0.05, lifetime=30.0, splash=4
    )
    try:
        rain.position = (0.0, 0.0, 10.0)
        rain.restart()
        air, hits = fall(rain, 3.0)
        assert hits > 0 and air > 3, "the splash counts as being in the air with the rain"

        rain.splash = 0
        rain.restart()
        air, hits = fall(rain, 3.0)
        assert hits > 0 and air == 3, "a drop that only stops throws nothing back"
    finally:
        rain.destroy()


def test_recycle_off_runs_the_crowd_out_and_clears_the_sky(clear_air):
    puff = veil(count=20, lifetime=0.2, collide=False, recycle=False)
    try:
        assert puff.inAir == 20
        fall(puff, 1.0)
        assert puff.inAir == 0, "one crowd, thrown once"
        assert puff.count == 20 and puff.active, "the system is still there, just spent"

        puff.recycle = True
        puff.restart()
        assert puff.inAir == 20, "and a recycled system lays itself again"
    finally:
        puff.destroy()


def test_a_crowd_can_be_grown_and_cut_back(clear_air):
    rain = veil(count=10)
    try:
        assert rain.count == 10
        assert rain.inAir == 10

        rain.count = 25
        assert rain.inAir == 25, "the new drops join the ones already flying"

        rain.count = 5
        assert rain.inAir == 5, "and the extras are taken out"

        rain.count = 50_000
        assert rain.count == 20_000, "the engine stops listening past its own cap"
    finally:
        rain.destroy()


def test_the_way_a_system_points_is_the_way_its_crowd_travels(clear_air):
    spray = carbine.procedural.particles()
    try:
        spray.direction = (0.0, 0.0, 1.0)
        assert tuple(spray.direction) == pytest.approx((0.0, 0.0, 1.0))

        spray.direction = (2.0, 0.0, 0.0)
        assert tuple(spray.direction) == pytest.approx((1.0, 0.0, 0.0)), "only the way counts"

        spray.direction = (0.0, 0.0, 0.0)
        assert tuple(spray.direction) == pytest.approx((0.0, 0.0, -1.0)), (
            "no way at all means down, which is what rain wants"
        )

        spray.rotationEuler = (90.0, ..., ...)
        assert tuple(spray.rotationEuler) == pytest.approx((90.0, 0.0, 0.0))
    finally:
        spray.destroy()


def test_a_system_hangs_in_the_tree_like_any_other_node(clear_air):
    chimney = carbine.new("cube", name="Chimney")
    embers = carbine.procedural.particles(name="Embers", parent=chimney)
    spark = carbine.procedural.particles(name="Spark")
    pot = carbine.new("cube", name="Pot")
    panel = carbine.ui.new("square")
    try:
        assert embers.parent == chimney
        assert chimney.children == [embers]

        embers.parent = None
        assert embers.parent == carbine.scene.root
        assert chimney.children == []

        embers.parent = chimney
        assert embers in chimney.children

        embers.addChild(spark)
        assert spark.parent == embers
        assert embers.children == [spark], "a system can hold a crowd of its own"
        embers.removeChild(spark)
        assert spark.parent == carbine.scene.root

        embers.addChild(pot)
        assert isinstance(embers.children[0], carbine.Object), "and a body"
        assert pot in embers.children

        with pytest.raises(ValueError, match="cannot share a branch"):
            panel.children = [embers]
    finally:
        panel.destroy()
        pot.destroy()
        spark.destroy()
        embers.destroy()
        chimney.destroy()


def test_a_particle_handle_reads_back_as_what_it_is(clear_air):
    rain = carbine.procedural.particles(name="Shower")
    try:
        found = [node for node in carbine.scene.root.children if node.name == "Shower"]
        assert len(found) == 1
        assert isinstance(found[0], carbine.Particle)
        assert found[0] == rain, "the same node, however it was picked up"
        assert hash(found[0]) == hash(rain)
        assert found[0].inAir == rain.inAir

        rain.name = "Storm"
        assert found[0].name == "Storm", "one node, not two copies of it"
    finally:
        rain.destroy()


def test_colour_and_opacity_are_written_the_way_an_object_writes_them(clear_air):
    rain = carbine.procedural.particles()
    try:
        rain.color = "#eb7a34"
        assert rain.colorHex == "#eb7a34"
        assert tuple(rain.color) == pytest.approx((235 / 255, 122 / 255, 52 / 255), abs=1e-3)

        rain.color = (0.1, 0.2, 0.3)
        assert tuple(rain.color) == pytest.approx((0.1, 0.2, 0.3))

        # One axis at a time, the way every other vector on a node is written.
        rain.color = (..., 0.4, ...)
        assert tuple(rain.color) == pytest.approx((0.1, 0.4, 0.3))

        rain.transparent = 128.0
        assert rain.transparent == pytest.approx(128.0)

        with pytest.raises(ValueError, match="not a colour"):
            rain.color = "nope"
    finally:
        rain.destroy()


def test_a_skin_can_be_named_and_taken_back_off(clear_air, skin):
    rain = carbine.procedural.particles()
    try:
        assert rain.texture is None
        rain.texture = skin
        assert rain.texture == skin

        rain.texture = None
        assert rain.texture is None, "flat again, painted in its own colour"

        born = carbine.procedural.particles(image=skin)
        try:
            assert born.texture == skin, "or the picture can come with the crowd"
        finally:
            born.destroy()
    finally:
        rain.destroy()


def test_a_crowd_wears_the_image_it_is_given(flat, clear_air, skin):
    """A picture on a drop, read off the frame. Every drop the dice throws is dimmed
    by a shade of its own, so the assertion is which colour each corner carries rather
    than a number: the four blocks of the test skin have to land the same way round
    they land on a textured wall."""
    drop = mote(name="Mote")
    try:
        assert len(set(corners(carbine.readPixels()))) == 1, (
            "unskinned, every corner of the drop is its own colour"
        )

        drop.texture = skin
        top_left, top_right, bottom_left, bottom_right = corners(carbine.readPixels())
        assert top_left[0] > max(top_left[1], top_left[2]), ("top left", top_left)
        assert top_right[1] > max(top_right[0], top_right[2]), ("top right", top_right)
        assert bottom_left[2] > max(bottom_left[0], bottom_left[1]), (
            "bottom left",
            bottom_left,
        )
        assert max(bottom_right) - min(bottom_right) <= 2, (
            "bottom right",
            bottom_right,
        )

        drop.texture = None
        assert len(set(corners(carbine.readPixels()))) == 1, "and off again"
    finally:
        drop.destroy()


def test_a_skin_the_engine_cannot_read_leaves_the_drop_its_own_colour(
    flat, clear_air, tmp_path
):
    """A file that claims to be packed and then falls apart costs the crowd its skin,
    not its colour: the renderer keeps its plain white texture, and white times the
    drop's own colour is the drop's own colour."""
    path = tmp_path / "broken.ktx2"
    path.write_bytes(bytes([0xAB]) + b"KTX 20\xBB\x0D\x0A\x1A\x0A" + b"not a picture")
    drop = mote(name="Mote", color="#eb7a34")
    try:
        own = pixel(carbine.readPixels(), 32, 32)
        assert own[0] > own[1] > own[2] > 0, own

        drop.texture = str(path)
        assert drop.texture == str(path), "the scene took the path on its header"
        assert pixel(carbine.readPixels(), 32, 32) == own, "and the drop still wears it"
    finally:
        drop.destroy()


def test_a_crowd_in_any_other_format_is_refused(clear_air, tmp_path):
    """The one image format is enforced where a drop is given its picture, including
    when the crowd arrives with it: a PNG never reaches the renderer, so the engine
    carries one loader instead of forty."""
    path = tmp_path / "flake.png"
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + b"\x00" * 24)

    with pytest.raises(ValueError, match="carbine.packager.image"):
        carbine.procedural.particles(image=str(path))

    rain = carbine.procedural.particles()
    try:
        with pytest.raises(ValueError, match="not a KTX2 file"):
            rain.texture = str(path)
        assert rain.texture is None
    finally:
        rain.destroy()


def test_a_destroyed_system_is_gone_not_resting(clear_air):
    rain = carbine.procedural.particles()
    ident = rain.id
    rain.destroy()

    assert rain.id == ident, "the handle keeps its number, and nothing else"
    for read in (lambda: rain.inAir, lambda: rain.count, lambda: rain.active, lambda: rain.name):
        with pytest.raises(carbine.DestroyedError):
            read()
    with pytest.raises(carbine.DestroyedError):
        rain.active = True
    with pytest.raises(carbine.DestroyedError):
        rain.destroy()
    assert isinstance(carbine.procedural.particles(), carbine.Particle)


def test_a_crowd_paints_but_never_pushes(clear_air):
    crate = carbine.new("cube", name="Crate")
    crate.size = (0.4, 0.4, 0.4)
    crate.position = (0.0, 0.0, 5.0)
    crate.gravity = True
    # Hang it where nothing can hold it, so the only thing that could move it is the
    # rain - and a thousand drops are only a picture.
    crate.gravityPull = 0.0
    rain = carbine.procedural.particles(count=400, area=(6.0, 6.0, 8.0))
    rain.position = (0.0, 0.0, 6.0)
    rain.restart()
    try:
        fall(rain, 2.0)
        assert tuple(crate.position) == (0.0, 0.0, 5.0), crate.position
    finally:
        rain.destroy()
        crate.destroy()


def test_the_generator_hands_back_nodes_rather_than_its_own_register(clear_air):
    assert isinstance(carbine.procedural, carbine.Procedural)
    assert repr(carbine.procedural) == "<Procedural generators>"

    first = carbine.procedural.particles(name="First")
    second = carbine.procedural.particles(name="Second")
    try:
        ids = (first.id, second.id)
        both = [node.name for node in carbine.scene.root.children if node.id in ids]
        assert both == ["First", "Second"], "two systems are two nodes, both in the tree"
        assert first != second
        assert first.inAir == second.inAir == 400
        assert repr(first).startswith('<Particle "First" count=400 active id=')
    finally:
        first.destroy()
        second.destroy()
