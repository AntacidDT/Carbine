"""Damage: what a body owes after it rides into another, and what a script does.

Two routes into the same numbers — contact, which the solver reports during
`step`, and `entity.hit(other)`, which a script writes. Both respect the target's
`hitPause`, and both end the actor when its health runs out.
"""

import carbine
import pytest

SECOND = 1.0 / 60.0
HANDS = ("w", "a", "s", "d", "space", "escape")


def let_go():
    for name in HANDS:
        carbine.injectKey(name, False)


def settle(seconds=5.0):
    for _ in range(round(seconds / SECOND)):
        carbine.step(SECOND)


def actor(*, at=(0.0, 0.0, 0.0), health=40.0, damage=0.0, collides=True, falls=False):
    """A metre cube with a collider and hit points: the smallest thing that can
    both bite and be bitten."""
    body = carbine.spawnEntity(
        "cube", position=at, health=health, damage=damage
    )
    body.size = (1.0, 1.0, 1.0)
    body.collision = collides
    body.gravity = falls
    return body


@pytest.fixture
def walking():
    """A player a test can drive, handed back disabled and empty."""
    player = carbine.player()
    let_go()
    player.enabled = False
    player.body = None
    player.mode = "first"
    player.position = (0.0, 0.0, 0.0)
    player.yaw = 0.0
    player.pitch = 0.0
    player.speed = 5.0
    player.enabled = True
    yield player
    let_go()
    player.enabled = False
    player.body = None
    player.mode = "first"
    player.position = (0.0, 0.0, 0.0)
    player.yaw = 0.0
    player.pitch = 0.0
    player.speed = 5.0


def test_a_new_actor_starts_alive_and_holds_its_breath():
    victim = actor(health=40.0)
    try:
        assert victim.alive is True
        assert victim.hitPause == pytest.approx(0.5)
        assert victim.stayAfterDeath is True
    finally:
        victim.destroy()


def test_a_scripted_hit_takes_the_attackers_damage_off_the_target():
    attacker = actor(health=40.0, damage=7.0)
    victim = actor(at=(4.0, 0.0, 0.0), health=40.0)
    try:
        assert attacker.hit(victim) == 7.0
        assert victim.health == 33.0
        assert victim.alive is True
        assert attacker.health == 40.0, "the attacker is not in the fight with itself"
    finally:
        victim.destroy()
        attacker.destroy()


def test_the_hurt_pause_holds_the_next_hit_off():
    attacker = actor(health=40.0, damage=7.0)
    victim = actor(at=(4.0, 0.0, 0.0), health=40.0)
    try:
        assert attacker.hit(victim) == 7.0
        assert attacker.hit(victim) == 0.0, "still hurting"

        for _ in range(4):
            carbine.step(0.1)
        assert attacker.hit(victim) == 0.0, "a pause paid a tenth short"

        carbine.step(0.1)
        assert attacker.hit(victim) == 7.0
        assert victim.health == 26.0
    finally:
        victim.destroy()
        attacker.destroy()


def test_an_actor_with_nothing_to_give_leaves_the_target_fresh():
    blunt = actor(health=40.0, damage=0.0)
    victim = actor(at=(4.0, 0.0, 0.0), health=40.0)
    try:
        assert blunt.hit(victim) == 0.0
        assert victim.health == 40.0
    finally:
        victim.destroy()
        blunt.destroy()


def test_a_plain_object_cannot_be_hit_and_has_nothing_to_hit_with():
    pillar = carbine.new("cube")
    attacker = actor(health=40.0, damage=7.0)
    try:
        assert not hasattr(pillar, "hit"), "scenery does not bite"

        with pytest.raises(RuntimeError):
            attacker.hit(pillar)
    finally:
        attacker.destroy()
        pillar.destroy()


def test_two_actors_riding_together_bite_once_per_pause():
    hazard = actor(at=(0.0, 0.0, 1.0), health=1000.0, damage=1.0)
    victim = actor(at=(0.0, 0.0, 3.0), health=100.0, falls=True)
    try:
        settle(3.0)
        assert victim.health < 100.0, "lying on the hazard costs a hit point"
        assert victim.health > 90.0, "one per half second, not one per frame"
        assert hazard.health == 1000.0, "and the victim bites nothing back"
    finally:
        victim.destroy()
        hazard.destroy()


def test_a_fight_ends_with_a_corpse_that_falls_and_stops_blocking():
    hazard = actor(at=(0.0, 0.0, 1.0), health=1000.0, damage=10.0)
    fighter = actor(at=(0.0, 0.0, 3.0), health=25.0, damage=8.0, falls=True)
    try:
        settle(4.0)
        assert fighter.alive is False, "the fighter is down"
        assert hazard.alive is True
        assert hazard.health < 1000.0, "it bit back while it could"

        settled = hazard.health
        settle(5.0)
        assert hazard.health == settled, "a corpse cannot hurt"
        assert fighter.collision is False, "and is no longer in anyone's way"
        assert fighter.gravity is True
        assert fighter.position.z == pytest.approx(0.5, abs=1e-2), "it drops through"
        assert fighter.touching() == []
    finally:
        fighter.destroy()
        hazard.destroy()


def test_stayAfterDeath_leaves_a_corpse_or_removes_the_body():
    killer = actor(health=40.0, damage=100.0)
    lingering = actor(at=(4.0, 0.0, 0.0), health=20.0)
    gone = actor(at=(8.0, 0.0, 0.0), health=20.0)
    gone.stayAfterDeath = False
    hat = carbine.new("cube", parent=gone.object)
    before = carbine.scene.nodeCount
    try:
        killer.hit(lingering)
        assert lingering.alive is False
        assert carbine.scene.nodeCount == before, "a corpse stays in the world"

        assert killer.hit(gone) == 100.0
        assert (
            carbine.scene.nodeCount == before - 2
        ), "a body that does not stay takes whatever was pinned under it"
        with pytest.raises(carbine.DestroyedError):
            gone.alive
        with pytest.raises(carbine.DestroyedError):
            hat.name

        lingering.destroy()
    finally:
        killer.destroy()


def test_a_dead_actor_takes_no_further_hits():
    killer = actor(health=40.0, damage=50.0)
    victim = actor(at=(4.0, 0.0, 0.0), health=20.0)
    try:
        assert killer.hit(victim) == 50.0
        assert victim.health == -30.0
        assert killer.hit(victim) == 0.0, "nothing beats a body already down"
        assert victim.health == -30.0
    finally:
        victim.destroy()
        killer.destroy()


def test_a_dead_actor_has_nothing_to_hit_with():
    strong = actor(health=40.0, damage=50.0)
    weak = actor(at=(4.0, 0.0, 0.0), health=10.0, damage=6.0)
    try:
        strong.hit(weak)
        assert weak.alive is False

        assert weak.hit(strong) == 0.0, "a corpse has no fist to swing"
        assert strong.health == 40.0
    finally:
        weak.destroy()
        strong.destroy()


def test_hitPause_and_stayAfterDeath_round_trip():
    victim = actor(health=40.0)
    try:
        victim.hitPause = 1.5
        victim.stayAfterDeath = False
        assert victim.hitPause == pytest.approx(1.5)
        assert victim.stayAfterDeath is False

        victim.hitPause = -1.0
        assert victim.hitPause == 0.0, "a negative pause is no pause at all"
    finally:
        victim.destroy()


def test_the_player_pays_for_what_it_walks_into(walking):
    suit = actor(health=30.0, collides=False)
    lava = actor(at=(2.0, 0.0, 1.0), health=1000.0, damage=4.0)
    walking.body = suit
    try:
        carbine.injectKey("w", True)
        settle(2.0)

        assert suit.health < 30.0, "the node the player wears wears the damage"
        assert suit.health > 10.0, "half a second between bites"
        assert walking.position.x < 1.8, f"and the pillar held: {walking.position.toTuple()}"
        assert lava.health == 1000.0
    finally:
        walking.body = None
        suit.destroy()
        lava.destroy()


def test_the_script_decides_what_a_dead_body_means(walking):
    """The engine drops a worn body that dies and leaves it visible where it fell.
    Whether that ends the run, turns the player into a spectator, or is only a
    setback is the script's call, since health is a number it can write back."""
    suit = actor(health=10.0, collides=False)
    lava = actor(at=(2.0, 0.0, 1.0), health=1000.0, damage=100.0)
    walking.body = suit
    try:
        assert suit.visible is False, "the eyes are inside it"

        carbine.injectKey("w", True)
        settle(1.0)
        carbine.injectKey("w", False)

        assert suit.alive is False, "one bite of a hundred is enough"
        assert walking.body is None, "the engine took the body off"
        assert suit.visible is True, "and left it in the frame where it fell"

        # Up again, somewhere else, with the number the engine just zeroed.
        walking.position = (0.0, -8.0, 0.0)
        suit.health = 30.0
        walking.body = suit
        assert walking.body == suit
        assert suit.alive is True
        assert suit.visible is False, "back in the eyes, back out of the frame"

        settle(1.0)
        assert suit.health == 30.0, "out of the lava, and whole again"
    finally:
        walking.body = None
        suit.destroy()
        lava.destroy()


def test_a_player_wearing_nothing_takes_no_contact_damage(walking):
    """There is no node to attribute a touch to, so a bare player is a ghost.
    A script that wants its player hurt gives it a body to wear."""
    lava = actor(at=(2.0, 0.0, 1.0), health=1000.0, damage=4.0)
    try:
        carbine.injectKey("w", True)
        settle(2.0)
        assert walking.position.x < 1.8, "the pillar still stopped it"
        assert lava.health == 1000.0
    finally:
        lava.destroy()
