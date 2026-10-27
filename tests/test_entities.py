import carbine
import pytest


def test_spawnEntity_gives_an_actor_a_body():
    actor = carbine.spawnEntity("rectangle", name="Grunt", health=40.0, damage=7.0)
    try:
        assert actor.kind == "rectangle"
        assert actor.name == "Grunt"
        assert actor.health == 40.0
        assert actor.damage == 7.0
        assert actor.object is not None
        assert actor.object.id == actor.id
    finally:
        actor.destroy()


def test_a_bodiless_entity_is_logic_only():
    controller = carbine.spawnEntity(name="controller", position=(1.0, 2.0, 0.0))
    try:
        assert controller.object is None
        assert controller.kind == "entity"
        assert controller.position.toTuple() == (1.0, 2.0, 0.0)
    finally:
        controller.destroy()


def test_an_entity_can_hang_from_an_object():
    pedestal = carbine.new("rectangle", name="Pedestal")
    try:
        rider = carbine.spawnEntity("square", parent=pedestal)
        assert rider.object.parent == pedestal
        assert pedestal.children == [rider.object]
        rider.destroy()
    finally:
        pedestal.destroy()


def test_gravity_is_a_multiplier_with_earth_at_ten():
    crate = carbine.spawnEntity("rectangle", name="Crate")
    try:
        assert crate.gravity is False
        assert crate.anchored is False
        assert crate.gravityPull == 10.0

        crate.gravity = True
        crate.anchored = True
        assert crate.gravity is True
        assert crate.anchored is True

        crate.gravityPull = 1.6
        assert crate.gravityPull == pytest.approx(1.6, abs=1e-6)

        crate.gravityPull = -1.0
        assert crate.gravityPull == 0.0
    finally:
        crate.destroy()


def test_killing_an_entity_keeps_its_body():
    before = carbine.scene.entityCount
    actor = carbine.spawnEntity("rectangle", name="Soldier", health=30.0)
    assert carbine.scene.entityCount == before + 1

    body = actor.object
    actor.kill()

    assert carbine.scene.entityCount == before
    assert body is not None
    assert body.name == "Soldier"
    with pytest.raises(RuntimeError):
        actor.health
    body.destroy()


def test_killEntity_accepts_a_handle_from_anywhere():
    actor = carbine.spawnEntity("square")
    carbine.killEntity(actor)
    assert actor.object is not None
    actor.destroy()


def test_a_plain_object_is_never_an_entity():
    card = carbine.new("rectangle")
    try:
        with pytest.raises(ValueError, match="not an entity"):
            carbine.killEntity(card)
    finally:
        card.destroy()


def test_entity_identity_is_the_node():
    actor = carbine.spawnEntity("rectangle")
    try:
        assert actor == actor.object
        assert len({actor, actor.object}) == 1
        assert hash(actor) == actor.id
    finally:
        actor.destroy()
