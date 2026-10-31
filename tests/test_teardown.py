"""Tearing a scene down: one call for the whole tree, and one for the run itself.

The engine keeps one scene per process, so these tests hand back what they take; a node,
a weather or a camera left standing here stands there for the rest of the suite too."""

import carbine
import pytest


def test_destroy_takes_a_handle_and_everything_under_it():
    before = carbine.scene.nodeCount
    crate = carbine.new("cube", name="Crate")
    bolt = carbine.new("sphere", name="Bolt", parent=crate)
    plate = carbine.new("cube", name="Plate")
    assert carbine.scene.nodeCount == before + 3

    carbine.destroy(crate)

    assert carbine.scene.nodeCount == before + 1
    for node in (crate, bolt):
        with pytest.raises(carbine.DestroyedError):
            node.name

    plate.destroy()
    assert carbine.scene.nodeCount == before


def test_destroy_refuses_the_root_it_could_not_replace():
    before = carbine.scene.nodeCount

    with pytest.raises(ValueError, match="root"):
        carbine.destroy(carbine.scene.root)

    assert carbine.scene.nodeCount == before


def test_destroy_all_clears_the_tree_and_the_weather():
    cube = carbine.new("cube")
    card = carbine.ui.new("square")
    crowd = carbine.procedural.particles(shape="circle")
    rain = carbine.atmosphere.new("rain")
    quake = carbine.physics.new("earthquake")
    quake.start()
    assert carbine.scene.nodeCount >= 4

    carbine.destroy("all")

    assert carbine.scene.nodeCount == 0
    for handle in (cube, card, crowd, rain):
        with pytest.raises(carbine.DestroyedError):
            handle.name
    assert carbine.atmosphere.running() == []
    assert carbine.physics.running() == []


def test_destroy_all_leaves_the_settings_a_game_tuned_alone():
    camera = carbine.camera()
    player = carbine.player()
    found = (camera.mode, tuple(camera.position), carbine.atmosphere.fog)

    player.enabled = True
    camera.position = (4.0, 5.0, 6.0)
    carbine.atmosphere.fog = 40.0
    carbine.new("cube")

    carbine.destroy("all")

    assert player.enabled is True
    assert tuple(camera.position) == (4.0, 5.0, 6.0)
    assert carbine.atmosphere.fog == 40.0

    player.enabled = False
    camera.mode, camera.position, carbine.atmosphere.fog = found


def test_destroy_needs_a_handle_or_the_word_all():
    for wrong in ("everything", "ALLNodes", 3, None):
        with pytest.raises((TypeError, ValueError)):
            carbine.destroy(wrong)


def test_end_closes_the_scene_and_the_window(headless):
    camera = carbine.camera()
    player = carbine.player()
    carbine.new("cube")
    carbine.atmosphere.new("snow")
    carbine.physics.new("wind").start()
    carbine.atmosphere.fog = 30.0
    player.enabled = True
    camera.position = (4.0, 5.0, 6.0)

    carbine.end()

    assert carbine.scene.nodeCount == 0
    assert carbine.scene.entityCount == 0
    assert carbine.atmosphere.running() == []
    assert carbine.physics.running() == []
    assert carbine.atmosphere.fog == 0.0
    assert player.enabled is False
    assert tuple(camera.position) != (4.0, 5.0, 6.0)
    assert carbine.running() is False
    assert carbine.dt() == 0.0
    with pytest.raises(RuntimeError, match="init"):
        carbine.viewport()

    # `end()` ends the scene, not the process: the emptied world is still there for a
    # fresh window to draw into, which is what an editor's restart asks for.
    carbine.init(64, 64, window=False)
    assert carbine.scene.nodeCount == 0
    assert len(carbine.readPixels()) == 64 * 64 * 4
