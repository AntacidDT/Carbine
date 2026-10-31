import carbine
import pytest

WIDTH = 64
HEIGHT = 64

CLEAR = (22, 21, 20)


def pixel(data, x, y, width=WIDTH):
    offset = (y * width + x) * 4
    return tuple(data[offset : offset + 3])


def test_duplicate_makes_one_when_nobody_named_a_number():
    original = carbine.new("cube", name="Crate")
    try:
        copies = carbine.duplicate(original)
        assert len(copies) == 1
        assert copies[0].kind == "cube"
        assert copies[0].name == "Crate"
        assert copies[0].id != original.id
        assert original.name == "Crate", "the original keeps its own name"
    finally:
        carbine.destroy("all")


def test_duplicate_makes_the_number_asked_for():
    original = carbine.new("cube")
    try:
        before = carbine.scene.nodeCount
        assert len(carbine.duplicate(original, 50)) == 50
        assert carbine.scene.nodeCount == before + 50
        assert carbine.duplicate(original, 0) == [], "asking for none makes none"
        assert carbine.scene.nodeCount == before + 50
    finally:
        carbine.destroy("all")


def test_a_copy_starts_where_the_original_stands_and_then_goes_its_own_way():
    original = carbine.new("cube", name="Crate")
    try:
        original.position = (2.0, 3.0, 4.0)
        original.size = (2.0, 2.0, 6.0)
        original.rotationEuler = (0.0, 0.0, 30.0)
        original.color = "#8b93a6"
        original.transparent = 200.0
        original.gravity = True
        original.gravityPull = 1.6
        original.collisionShape = "capsule"

        copy = carbine.duplicate(original)[0]
        assert copy.position.toTuple() == (2.0, 3.0, 4.0)
        assert copy.size.toTuple() == (2.0, 2.0, 6.0)
        assert copy.rotationEuler.toTuple() == pytest.approx((0.0, 0.0, 30.0))
        assert copy.colorHex == "#8b93a6"
        assert copy.transparent == 200.0
        assert copy.gravity is True
        assert copy.gravityPull == pytest.approx(1.6)
        assert copy.collision is True
        assert copy.collisionShape == "capsule"

        copy.position = (0.0, 0.0, 0.0)
        assert original.position.toTuple() == (2.0, 3.0, 4.0)
    finally:
        carbine.destroy("all")


def test_a_plain_copy_shares_nothing_after_the_copy_is_made():
    original = carbine.new("cube")
    try:
        copy = carbine.duplicate(original)[0]
        copy.color = "#00ff00"
        assert original.colorHex == "#eb7a34", "a duplicate is a copy, not a twin"
    finally:
        carbine.destroy("all")


def test_a_copy_of_an_actor_is_an_actor_with_its_own_health():
    actor = carbine.spawnEntity("cube", name="Grunt", health=40.0, damage=7.0)
    try:
        copy = carbine.duplicate(actor)[0]
        assert isinstance(copy, carbine.Entity), "an actor copies as an actor"
        assert copy.health == 40.0
        assert copy.damage == 7.0
        assert copy.alive is True

        copy.health = 3.0
        assert actor.health == 40.0, "what a twin is worth is its own business"
    finally:
        carbine.destroy("all")


def test_a_copy_of_a_crowd_is_a_crowd_of_its_own():
    system = carbine.procedural.particles(count=30, speed=2.5, recycle=False)
    try:
        copy = carbine.duplicate(system)[0]
        assert isinstance(copy, carbine.Particle), "a system copies as a system"
        assert copy.count == 30
        assert copy.speed == 2.5
        assert copy.recycle is False
        assert copy.inAir == 30

        copy.count = 5
        assert system.count == 30
    finally:
        carbine.destroy("all")


def test_a_branch_comes_across_as_copies():
    crate = carbine.new("cube", name="Crate")
    lid = carbine.new("cube", name="Lid", parent=crate)
    try:
        lid.position = (0.0, 0.0, 1.0)
        copy = carbine.duplicate(crate)[0]
        assert [child.name for child in copy.children] == ["Lid"]
        assert copy.children[0].id != lid.id
        assert len(crate.children) == 1, "the original keeps its own lid"
        assert carbine.scene.nodeCount == 4
    finally:
        carbine.destroy("all")


def test_linkDuplicate_spreads_one_write_over_the_whole_group():
    original = carbine.new("cube", name="Crate")
    try:
        twins = carbine.linkDuplicate(original, 4)
        assert len(twins) == 4

        original.color = "#1f3557"
        assert [twin.colorHex for twin in twins] == ["#1f3557"] * 4

        # Written from a twin, it still reaches the original and the rest.
        twins[1].transparent = 40.0
        assert original.transparent == 40.0
        assert [twin.transparent for twin in twins] == [40.0] * 4

        twins[2].gravity = True
        twins[2].collisionShape = "sphere"
        assert original.gravity is True
        assert original.collision is True
        assert [twin.collisionShape for twin in twins] == ["sphere"] * 4
    finally:
        carbine.destroy("all")


def test_a_linked_copy_keeps_its_own_place_its_own_size_and_wounds():
    actor = carbine.spawnEntity("cube", health=40.0)
    try:
        twins = carbine.linkDuplicate(actor, 3)
        twins[0].position = (5.0, 0.0, 0.0)
        twins[0].rotationEuler = (0.0, 0.0, 90.0)
        twins[0].size = (2.0, 2.0, 2.0)
        twins[1].health = 0.0

        assert actor.position.toTuple() == (0.0, 0.0, 0.0)
        assert actor.rotationEuler.toTuple() == pytest.approx((0.0, 0.0, 0.0))
        assert actor.size.toTuple() == (1.0, 1.0, 1.0), "a city needs towers of odd heights"
        assert actor.alive is True, "one twin dying is one twin's business"
    finally:
        carbine.destroy("all")


def test_copying_a_twin_joins_the_group_it_was_copied_from():
    original = carbine.new("cube")
    try:
        first = carbine.linkDuplicate(original)[0]
        second = carbine.linkDuplicate(first)[0]

        second.color = "#abcdef"
        assert original.colorHex == "#abcdef"
        assert first.colorHex == "#abcdef"
    finally:
        carbine.destroy("all")


def test_a_plain_copy_of_a_twin_leaves_the_group():
    original = carbine.new("cube")
    try:
        carbine.linkDuplicate(original)
        copy = carbine.duplicate(original)[0]

        copy.color = "#000000"
        assert copy.colorHex == "#000000"
        assert original.colorHex == "#eb7a34", "it took the colour, not the sharing"
    finally:
        carbine.destroy("all")


def test_a_linked_crowd_shares_its_knobs_and_parks_on_its_own():
    system = carbine.procedural.particles(count=20)
    try:
        twin = carbine.linkDuplicate(system)[0]

        twin.size = 0.75
        twin.area = (8.0, 8.0, 4.0)
        twin.seed = "storm"
        assert system.size == 0.75
        assert system.area.toTuple() == (8.0, 8.0, 4.0)
        assert system.seed == twin.seed

        twin.active = False
        assert twin.inAir == 0
        assert system.active is True
        assert system.inAir == 20
    finally:
        carbine.destroy("all")


def test_a_linked_panel_shares_its_shape_and_keeps_its_corner_of_the_screen():
    panel = carbine.ui.new("square", name="Panel")
    try:
        panel.corner = 12.0
        panel.polygon = 6
        panel.outline = 2.0
        twin = carbine.linkDuplicate(panel)[0]

        twin.corner = 3.0
        twin.position = (100.0, 20.0, 0.0)
        assert panel.corner == 3.0
        assert panel.position.toTuple() == (0.0, 0.0, 0.0)
        assert twin.polygon == 6

        copy = carbine.duplicate(panel)[0]
        assert copy.corner == 3.0
        copy.outline = 9.0
        assert panel.outline == 2.0, "the plain copy never joined in"
    finally:
        carbine.destroy("all")


def test_a_hurt_twin_leaves_its_twins_settings_alone():
    killer = carbine.spawnEntity("cube", health=40.0, damage=50.0)
    original = carbine.spawnEntity("cube", health=40.0)
    try:
        original.collision = True
        original.stayAfterDeath = True
        twin = carbine.linkDuplicate(original)[0]
        assert twin.stayAfterDeath is True

        assert killer.hit(twin) == 50.0
        assert twin.alive is False
        assert twin.collision is False, "the corpse is out of the solver"
        assert twin.gravity is True, "and falls on its own account"
        assert original.alive is True
        assert original.collision is True, "the engine does not hand a corpse's settings back"
        assert original.gravity is False
    finally:
        carbine.destroy("all")


def test_the_scene_root_is_not_a_thing_anyone_copies():
    with pytest.raises(ValueError):
        carbine.duplicate(carbine.scene.root)
    with pytest.raises(ValueError):
        carbine.linkDuplicate(carbine.scene.root, 3)


def test_copying_a_destroyed_node_is_refused():
    original = carbine.new("cube")
    carbine.destroy(original)

    with pytest.raises(carbine.DestroyedError):
        carbine.duplicate(original)
    with pytest.raises(carbine.DestroyedError):
        carbine.linkDuplicate(original)


def test_a_fresh_scene_forgets_which_bodies_shared():
    original = carbine.new("cube")
    carbine.linkDuplicate(original)
    carbine.destroy("all")

    reborn = carbine.new("cube")
    try:
        twin = carbine.linkDuplicate(reborn)[0]
        twin.color = "#123456"
        assert reborn.colorHex == "#123456"
    finally:
        carbine.destroy("all")


def test_linked_bodies_draw_the_same_picture(flat):
    original = carbine.new("rectangle", name="Card")
    try:
        original.size = (0.5, 0.5, 1.0)
        original.position = (0.5, 0.0, 0.0)
        twin = carbine.linkDuplicate(original)[0]
        twin.position = (-0.5, 0.0, 0.0)

        twin.color = "#ff0000"
        pixels = carbine.readPixels()
        assert pixel(pixels, 52, 32) == (255, 0, 0)
        assert pixel(pixels, 12, 32) == (255, 0, 0), "one paint, and both cards are red"

        twin.visible = False
        pixels = carbine.readPixels()
        assert pixel(pixels, 52, 32) == (255, 0, 0)
        assert pixel(pixels, 12, 32) == CLEAR, "hiding one twin hides that one"
    finally:
        carbine.destroy("all")
