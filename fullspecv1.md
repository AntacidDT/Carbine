# Carbine — Full Specification v1

The complete engine surface as it stands: every subsystem, every API entry, and the
mechanisms underneath them. Written against the live code — the Python stub
(`pkg/carbine/__init__.pyi`) is the contract, the Rust crates are the truth behind it.

Status markers: everything below is **implemented** unless a section says
otherwise. The one piece still in flight (audio) is collected at the end.

---

## 1. What Carbine is

- A 3D game engine: **Rust does every calculation**, Python is a thin scripting layer.
  The native module (`carbine._native`, PyO3) holds the scene, the solver, the
  generators and the renderer; a script only asks and writes.
- **z-up world**, metres and seconds throughout. `size = (length, width, height)`
  along x, y, z. Perspective camera is the default; orthographic is a mode.
- **One format per class of asset**: objects `.glb`, images `.ktx2`, audio `.ogg`
  (audio in flight). The engine carries one small loader per class, never a codec
  zoo. The Packager converts authored files into those formats.
- **Own runtime parsers**: GLB reader, KTX2 reader, hand-rolled GJK/EPA collision —
  no Rapier/parry, no runtime codec crates. Pure-Rust third-party crates are allowed
  in *converters* only (BC7 encoder, model converters).
- Desktop first, GPLv3 with exception, self-hosted accounts planned, 5-player P2P
  planned, RON for authored data. Editor layer ("Enginebay") rides the same core.
- Keep it simple: understandable APIs over realism. `gravityPull = 10` is Earth;
  no gravity vectors, presets or drag knobs.

### Workspace

| Crate | Job |
|---|---|
| `carbine-core` | Scene/ECS (bevy_ecs), transforms, physics, player, vehicles, particles, lights, atmospheres, procgen substrate, texture + terrain generators |
| `carbine-render` | wgpu renderer, winit window, sky/shadow/UI passes, frame pacing, input collection |
| `carbine-glb` | Own GLB container reader + minimal writer, Y-up→Z-up crossing |
| `carbine-ktx` | Own KTX2 reader + BC7 packer interface (decoder-free loader) |
| `carbine-pack` | The Packager: authored file in, engine format out (only crate that knows what a PNG is) |
| `carbine-native` | PyO3 bindings — the whole Python surface |

Python package: `pkg/carbine` (`__init__.py` + `_native` + `.pyi` stubs).
`import carbine` stands alone; `atexit` closes the engine cleanly if the script never does.

---

## 2. Module-level API (`carbine.*`)

### Lifecycle

| Entry | Signature | Meaning |
|---|---|---|
| `init` | `init(width=1280, height=720, *, window=True, title="Carbine") -> str` | Start the engine; returns the GPU adapter name (`"unknown"` if no device was reached). `window=False` is the headless/offscreen path. |
| `render` | `render() -> None` | Draw one frame (and collect input). |
| `running` | `running() -> bool` | Whether the window is still open. |
| `viewport` | `viewport() -> tuple[int, int]` | Current drawable size in pixels. |
| `readPixels` | `readPixels() -> bytes` | Last frame's pixels (replay/tooling). |
| `close` | `close() -> None` | Shut the renderer down. Registered with `atexit`. |
| `destroy` | `destroy(target: Object \| Entity \| Particle \| Terrain \| str) -> None` | A handle: that node + its whole subtree. `"all"`: every authored node (objects, actors, UI, crowds, weather, maps) + all disasters. Camera, backdrop, player settings and the sun stay — they are settings, not content. |
| `end` | `end() -> None` | `init()` in reverse: brand-new scene, player gives up camera and pointer, window closes. Frame limits survive (engine settings, not scene content). |

### Creation and copies

| Entry | Signature | Meaning |
|---|---|---|
| `new` | `new(kind, *, name=None, parent=None) -> Object \| Terrain` | One world node of `kind` (see `kinds()`). Returns the class the node is: an `Object` for geometry/light/model/vehicle/seat, a `Terrain` for `"terrain"` (a map laid with the engine's defaults — use `procedural.map()` to name its size/relief/seed as it is laid). |
| `spawnEntity` | `spawnEntity(kind=None, *, name=None, parent=None, position=None, health=1.0, damage=0.0) -> Entity` | An actor: a node with hit points. `kind` skins its body. |
| `killEntity` | `killEntity(entity) -> None` | Deal death through the engine's own rules. |
| `duplicate` | `duplicate(target, count=1) -> list[...]` | Independent copies beside the original, standing where it stood, at rest. Everything carried: size, colour, skin, gravity, collider cut, UI knobs, crowd knobs + its air, a map's hills, and the branch under it as copies. Each copy then owns its place, turning, size and health. Returns the class the node is (Object/Entity/Particle/Terrain). `count=0` makes none. |
| `linkDuplicate` | `linkDuplicate(target, count=1) -> list[...]` | Copies tied in one configuration group (Blender-style linked data). **Shared writes** (what a thing *is*): colour, `transparent`, `texture`, `textureTiling`, `gravity`, `gravityPull`, `mass`, `anchored`, `collision`, `collisionType`, actor `hitPause`/`stayAfterDeath`, UI `corner`/`polygon`/`outline`, vehicle `type`/`topSpeed`/`turnRate`/`grip`, every particle knob except `active`, a map's `relief`/`quality`/`seed`/`size`, Text content/size/wrap/outline. **Local writes** (where/what's left): `position`, `rotation`, `size` (bodies), `scale`, `health`, `damage`, `visible`, crowd `restart()`. A copy of a member joins its group; plain `duplicate` leaves the group; the original gains a group only when first link-copied. Linked maps share their hills but not their place. |

### Registers and lookups

| Entry | Meaning |
|---|---|
| `scene` | The `Scene` register (root, sun, counts). |
| `ui` | The `Ui` register (pixel overlay). |
| `physics` | The `Physics` register (disasters). |
| `atmosphere` | The `Atmosphere` register (sky, light, time, fog, weather). |
| `procedural` | The `Procedural` generators. |
| `packager` | The `Packager` importer. |
| `camera()` | The `Camera` handle. |
| `player()` | The `Player` controller handle. |
| `mouse()` | The `Mouse` read-only state. |
| `kinds()` | What `new` accepts: `cube, plane, sphere, rectangle, square, light, model, vehicle, seat, driverSeat, terrain`. |
| `collisionTypes()` | `["box", "sphere", "mesh"]`. |
| `lightTypes()` | `["straight", "blocky"]` (shade-edge looks). |
| `handlingTypes()` | `["arcade", "drift"]`. |

### Simulation clock and input

| Entry | Signature | Meaning |
|---|---|---|
| `dt` | `dt() -> float` | Seconds the last step ran. |
| `step` | `step(seconds) -> None` | Advance the world explicitly (replays/tooling); clamped to `MAX_STEP`. |
| `key` | `key(name) -> bool` | Held now. Keys are **positional**: `"w"` is the key a French keyboard labels `z`. |
| `pressed` / `released` | `(name) -> bool` | This frame only. |
| `keys` | `keys() -> list[str]` | Every key name the engine knows. |
| `grabMouse` | `grabMouse(grab) -> bool` | Take/give the pointer; reports what the compositor actually allowed. Only a player holds it. |
| `injectKey` | `injectKey(name, down) -> None` | Synthetic input (replay/tooling). |
| `injectMouse` | `injectMouse(dx, dy) -> None` | Synthetic relative motion. |
| `injectButton` | `injectButton(name, down) -> None` | Synthetic mouse button. |

### Module properties (frame pacing + light look)

Live on the module itself (`_Carbine(ModuleType)`), because a game names them before
it has a window:

| Property | Type | Default | Meaning |
|---|---|---|---|
| `fpsCap` | `int \| None` | `None` | Ceiling the window must not pass; `None`/`0` = no ceiling. |
| `monitorSync` | `bool` | `False` | Hand the pace to the screen. |
| `monitorHz` | `int` (ro) | — | What the panel asks; 0 on some desktops until the first `render()`, re-asked every second (unplugging noticed). |
| `fpsLimit` | `int \| None` (ro) | `None` | The rate the two settle at: **min(cap, panel)**; `None` when neither holds. |
| `fps` | `float` (ro) | `0.0` | Measured FPS, refreshed every 0.25 s; zero until sampled and after init/end. |
| `lightType` | `str` | `"straight"` | Shade-edge look: `"straight"` weaves cast readings (turned by pixel position, far readings count less, cut kept off the cast border); `"blocky"` keeps single-texel steps. Same cost either way — cost is `atmosphere.lightQuality`. |

---

## 3. Math types

### `Vec3`
`Vec3(x=0.0, y=0.0, z=0.0)`; fields `x, y, z`; `len() == 3`, iterable
(`Vec3Iterator`), `+`, `-`, scalar `*` both ways, `toTuple() -> (x, y, z)`.
Every vector *write* in the API also accepts a plain `(x, y, z)` tuple in which
`None` or `...` **preserves that axis**.

### `Quat`
`Quat(x=0, y=0, z=0, w=1)`; `Quat.identity()`, `Quat.fromEuler(x, y, z)` (degrees),
`toEuler() -> (x, y, z)` degrees. Rotations read as Quat, write as Quat or euler tuple.

### `Mesh` (read-only view of a node's geometry)
`positions: list[Vec3]`, `normals: list[Vec3]`, `uvs: list[(u, v)]`,
`indices: list[int]`, `vertexCount`, `triangleCount`,
`bounds -> (min, max) | None`.

### `DestroyedError(RuntimeError)`
Raised by any handle whose node has been destroyed. (A destroyed **Disaster**
answers `ValueError` instead — it was never a node.)

---

## 4. Scene graph — the one tree

Everything is a **node**: objects, actors, lights, seats, vehicles, particle
systems, maps, UI elements. One tree under `scene.root`; transforms compose down it,
so a turret rides its tank and embers ride their chimney.

- `parent` reads back as the class the parent really is (Object / Entity / Particle /
  Terrain); writing `None` moves to root. A root-level node reads its parent as the
  root Object handle (`scene.root`, id 0), not `None`.
- `children` is the walk order — also the **UI paint order** (last sits on top).
  Writing a list (or single node) replaces the branch: listed order becomes walk
  order, omitted children move to root, nodes hanging elsewhere are adopted.
  **Nothing is destroyed** by re-listing.
- `addChild` appends; `removeChild` detaches to root (leaves the world — `destroy`
  is the call that means removal).
- **One-branch rule**: the overlay is pixels, the world is metres; a branch never
  crosses between them except at the root, where both hang.
- `find(name)` searches the whole branch downward (imports name their parts, so
  this is how a turret/door/seat is reached again).

### `Scene` register
| Entry | Meaning |
|---|---|
| `root` (ro) | The root Object. |
| `sun` (ro) | The daylight node every world is born with — colour it, turn it (turning = aiming), brighten it, `brightness = 0` for night. `None` if destroyed. `atmosphere.time(...)` turns it. |
| `nodeCount` (ro) | Authored nodes (root and born sun excluded). |
| `entityCount` (ro) | Actors among them. |

---

## 5. Object — the world node API

`carbine.new(kind)` returns an `Object`. Kind words are read forgivingly
(trim + lowercase). Knobs that belong to another role (light/vehicle/seat/UI/text)
**raise** on a node that isn't one — the handle tells you what it is.

### Identity & tree
`id: int`, `name: str`, `kind: str` (ro), `parent`, `children`, `addChild`,
`removeChild`, `find(name)`, `destroy()`.

### Transform
| Property | Type | Notes |
|---|---|---|
| `position` | Vec3 | Metres from scene origin (through the parent chain). |
| `size` | Vec3 *(float for Text)* | Live (length, width, height). Writing size scales the node to those metres against its mesh's natural extent. Text: font size 1..400 px. |
| `scale` | Vec3 | The scale itself. |
| `rotation` | Quat | Write Quat or euler degrees tuple. |
| `rotationEuler` | Vec3 | Degrees. |

### Appearance
| Property | Type | Notes |
|---|---|---|
| `color` | Vec3 \| str | Vec3, RGB tuple, **named colour** or hex (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`). |
| `colorHex` (ro) | str | `#rrggbb`. |
| `texture` | str \| None | A packed `.ktx2` path painted over the colour; any other format refused (pack it first). `None` = flat. |
| `textureTiling` | float | **Metres per tile**, default 1.0. Every face measured along its own two axes, laid once per metre — a tile stays the size it was painted for on any face. `0` smears one whole picture per face (how a model's own UVs arrive). Multi-tile faces are *run in*: repeats nudged off their lattice and worn to their own tone, so a long wall reads as one surface. |
| `model` | str \| None | A `.glb` worn here: the file's nodes come in as **children**, named what the file named them; `None` takes that branch off, leaving script-authored children. `path#Name` hangs one named branch of a hangar file. Only `.glb` is read (FBX refused with advice). |
| `visible` | bool | False hides the node and everything under it. |
| `transparent` | float | Opacity 0..255. |
| `mesh` (ro) | Mesh | The node's own geometry. |

### Physics participation
| Property | Type | Default | Notes |
|---|---|---|---|
| `anchored` | bool | false | Holds its ground: no gravity, no shoves, no rolling/pivoting. |
| `gravity` | bool | **false** | Nothing falls until a script says so. |
| `gravityPull` | float | 10 | m/s²; 10 Earth, 1.6 Moon, 0 hangs. |
| `mass` | float | box × 200 kg/m³ | Kilograms. Decides how much of a push a body takes — **never** how fast gravity pulls it. |
| `collision` | bool | false | In the solver or out; nothing collides until asked. |
| `collisionType` | str | box | `"box"` = the size drawn around it; `"sphere"` = ball as wide as its widest way; `"mesh"` = the body's own surfaces, **one convex solid per surface** (the room inside a loaded vehicle stays empty). Writing switches collision on; reading before that gives the type it would be cut as. |
| `touching()` | list | — | Bodies in contact as of the last step. |

### Light knobs (only on `new("Light")`)
| Property | Notes |
|---|---|
| `shape` (ro) | `"point"` (lamp) or `"sun"` (daylight). The world's sun is `scene.sun`; a lamp never becomes a second one. |
| `brightness` | 1 at birth, 0 dark. |
| `reach` | Metres a lamp lights; past it, nothing. |
| `type` | `"straight"`/`"blocky"` per-light override; unwritten follows `carbine.lightType`. |

### Vehicle knobs (only on a vehicle)
`driveable: bool` — `new("Vehicle")` is one from birth (a hatchback 4.4 × 2 × 1.2 m,
in the solver, held by gravity); **anything else becomes one by writing
`driveable = True`**, which hangs a drive on the node and asks nothing else of it —
a loaded tank keeps its size, mass and collider.

| Property | Notes |
|---|---|
| `type` | Handling: `"arcade"` (the nose decides the line) or `"drift"` (tyres can be beaten, the car slides across its old line). |
| `topSpeed` | m/s full throttle chases. |
| `turnRate` | deg/s the nose turns at full lock. |
| `grip` | 0 ice .. 1 gripper tarmac — how much of a sideways shove the tyres keep. Arcade reads it as 1 and never slides. |
| `throttle`, `steer`, `brake` | Each −1..1: accelerator forwards, wheel across, brake down. **Wishes, not speeds** — the handling works out where the car goes. A keyboard-reading script writes them every frame. These three are one car's own business: writes do **not** travel through a link group. |
| `driver` (ro) | First driving seat with somebody in it; `None` for an empty car. |

### Seat knobs (only on a seat)
| Entry | Notes |
|---|---|
| `drives` (ro) | True for `new("DriverSeat")` (drives the vehicle it hangs on), false for `new("Seat")` (rides along). The kind word said it at creation. |
| `seated` (ro) | Whoever rides here, as the handle they really are; `None` empty. |
| `seatedIn` (ro) | (On any node) which seat this node rides in, `None` on its own feet. |
| `sit(seat)` | Sits this node in the seat. It **stays on its own branch** — name, children, nothing re-parented; the step holds it at the seat's place. Whoever was in the seat gets out. |
| `stand()` | Gets out exactly where the seat had it, **keeping the seat's speed** — stepping out of a moving car is a moment of running. Safe to call while on foot. |

Seat kind words are forgiving: `"chair"`, `"passenger"`, `"Driver's Seat"` all work.

### UI knobs (only on overlay nodes — world objects raise)
| Property | Notes |
|---|---|
| `corner` | Corner rounding, pixels. |
| `polygon` | Side count (shape preset). |
| `outline` | Text: bool (black outline); shapes: float pixel thickness. |
| `size` | Pixels (Vec3 for shapes, float font size for Text). |

### Text-only properties
| Property | Notes |
|---|---|
| `content` | The string. Bundled SDF font covers ASCII + Latin-1; unsupported chars show `?`. |
| `wordWrap` | bool. |
| `width` | Wrap width px, clamped 1..16384. |
| `outlineWidth` | Stored 0..32 px, drawn up to 0.6 × font size. |
Numeric Text inputs reject NaN/infinity with `ValueError`. Text position is
**top-left**.

---

## 6. Entity — the actor API

`carbine.spawnEntity(...)` returns an `Entity`: a node with hit points. It hangs in
the same tree and rides whatever it is pinned to, body or no body. It carries the
full node surface — `id, name, kind, parent, children, addChild, removeChild,
position, size, rotation, rotationEuler, color, texture, textureTiling, model,
find, visible, anchored, gravity, gravityPull, mass, collision, collisionType,
touching, seatedIn, sit, stand, destroy` — with the same meanings as Object, plus:

| Entry | Type | Notes |
|---|---|---|
| `object` (ro) | Object \| None | The body node the actor wears. |
| `health` | float | HP remaining. At zero the engine has already dealt with death. Writing a positive number back is the **only revive**. What death means for the run (end it, spectate, let the player up) is the **script's** decision, not the engine's. |
| `damage` | float | HP this actor deals, on contact and through `hit`. |
| `alive` (ro) | bool | A corpse answers false, takes no more hits, and cannot hurt anything. |
| `hitPause` | float | Seconds one hit holds the next off (contact or scripted). Default 0.5. |
| `stayAfterDeath` | bool | True: body lies in the world, inert but still falling. False: node removed with everything under it. |
| `hit(other) -> float` | method | Bites `other` for this actor's `damage`; answers HP actually landed — 0 when the target was dead/still hurting, or this actor is a corpse. |
| `kill()` | method | Death through the engine's rules. |

**Engine vs script**: mechanics live in the core, game rules in the script. A dead
worn body is dropped (visibility restored, taken off the player), not judged.
Contact damage lands on whatever the player wears — a player a script wants hurt
wears an Entity as `player.body`.

---

## 7. Vehicles & seats — the mechanism

- A **drive is a component any node can wear** (`driveable`). `new("Vehicle")` is a
  ready hatchback; an imported `.glb` tank becomes a vehicle by writing
  `driveable = True` and keeps everything it arrived with.
- **Arcade**: the nose decides the line; grip read as 1; the car never slides.
  **Drift**: sideways velocity survives according to `grip`, so the car can be
  thrown across its old line. Both calibrated.
- Driver wishes (`throttle/steer/brake`) are read **before anything moves** in the
  step, so the speed asked for is the speed integrated this step.
- A **DriverSeat drives the vehicle it hangs on**; a plain Seat only rides. Seats
  are nodes: hang them anywhere in the vehicle's branch.
- **Sitting**: the rider keeps its own branch; each step welds it to the seat's
  world place after the solver moves the car. A rider neither falls nor shoves;
  its jump key does nothing until it stands. The player's eyes ride the same seat,
  and a third-person camera in a car keeps behind the **chassis nose**, not the
  driver's mouse.
- **Standing**: left where the seat was, carrying the seat's velocity. A dying
  actor is stood out by the engine with a shove, asked or not.
- Anchored vehicles refuse shoves — and refuse to roll or pivot.
- Floor drag never second-guesses tyres: a driven body's floor contact does not
  apply the sliding grip that un-driven bodies get.

---

## 8. Player — the WASD-and-mouse controller

`carbine.player()`; turn it on and the engine drives the camera.

| Property | Type | Notes |
|---|---|---|
| `enabled` | bool | Master switch. |
| `mode` | str | `"first"` (camera in the eyes) / `"third"` (behind the body). |
| `position` | Vec3 | The feet; a write arrives **at rest** (no throwing). |
| `yaw`, `pitch` | float | Degrees; yaw about vertical, pitch never passes it. |
| `speed` | float | m/s at full tilt. |
| `sensitivity` | float | Degrees per pixel of mouse. |
| `eyeHeight` | float | Metres. |
| `distance` | float | Third-person only: metres behind the body. |
| `gravityPull` | float | 10 Earth, 1.6 Moon. |
| `jumpStrength` | float | Upward kick, m/s. |
| `grounded` (ro) | bool | Standing on something this step. |
| `velocity` (ro) | Vec3 | Current pace. |
| `body` | Object \| None | The node drawn as the player (hidden in first person, moved every step). Write Object or Entity; read Object. Contact damage lands on it. When that actor dies the engine takes it off, leaves it in the world, restores its visibility; the controller has no HP and walks on until the script says otherwise. |
| `grabMouse` | bool | Escape hands the pointer back; a click asks again. |
| `jump()` | method | Pushes off the ground (not while seated). |
| `isKeyDown(name)` | method | Convenience read. |

Mechanism: keys set a **target velocity** the body chases (`1 − e^(−12·dt)`), so
releasing a key slides to a stop instead of snapping. Grounding is the floor under
the feet — flat ground, or the terrain surface beneath a map. Quake shake moves the
**view
only**, never `position` — the collider walks the floor while the eyes rattle.
A script that places the camera itself owns it; the follow step stands down.

---

## 9. Camera

`carbine.camera()` — metres, z-up. `position` floats above the world, `target` is
the point the lens aims at (that's what gives the frame depth).

| Property | Notes |
|---|---|
| `position`, `target` | Vec3. |
| `mode` | `"perspective"` (default) or `"orthographic"`. |
| `fov`, `near`, `far` | Perspective lens. |
| `viewHeight` | Orthographic only: vertical metres of visible world. |
| `lookAt(target)` | Aim (Vec3 or tuple). |

---

## 10. Input & Mouse

Module functions `key/pressed/released/keys`, `grabMouse`, and the three injectors
(see §2). Positional key names; `pressed`/`released` are last-frame-only.

`carbine.mouse()` — read-only, as the last frame saw it:
`dx, dy` (pixels since last frame — only useful while grabbed), `x, y`,
`left, right, middle` (bool), `wheel` (float), `grabbed` (bool).

---

## 11. Physics — collision, contacts, damage

### The solver
- Hand-rolled **support-function collision (GJK/EPA)** — no third-party physics.
- Nothing collides until `collision = True`. Collider types: **box** (the drawn
  size), **sphere** (widest way), **mesh** (one convex piece per surface — hull,
  turret, wheel are three solids with real air between them).
- Cuts are **cached between steps** and re-cut only when the node or its model
  branch moves (`recut`); a standing thousand-point body costs nothing per step.
- Contacts push pairs apart over several passes, scenery holding its ground;
  **mass decides the share** of a push, and the pair **trades pace** along the
  contact normal instead of teleporting. A pair whose answer cannot be checked
  (degenerate maths, too-flat shapes) is left alone rather than pushed worse.
- Two nodes in the same branch never shove each other (a turret doesn't dig
  itself out of its hull).
- Gravity: `velocity.z -= gravityPull·dt`; bodies rest on their **lowest drawn
  point** (an import's wheels, not its pivot). Landing applies floor drag unless
  the body is being driven.

### `Scene.step` order (the whole mechanism, once per step)
1. `step_player` — the camera follows it, so it goes first
2. `count_down_hits` — hit-pause timers
3. `run_disasters` — the weather's shove
4. `step_vehicles` — driver wishes read before anything moves
5. `integrate` per node — gravity, spin, floor contact
6. `recut` — every body carried into world metres once
7. `push_apart` — overlap resolution + pace trade
8. `hurt_by_contacts` — what the touches cost
9. `place_seats` → `ride_seat` — riders held where their seats ended up
10. `follow_camera` → `place_body` — eyes and drawn body read the settled world
11. `run_particles` — crowds fly last, over a world that is where it will be drawn

### Contact damage
Bodies with `damage` bite what they touch; `hitPause` holds repeat hits off.
`Entity.hit(other)` is the scripted door to the same rules. Death is the engine's
mechanic (corpse or removal per `stayAfterDeath`); what death *means* is the script's.

### Disasters — `carbine.physics`
Not nodes: no place in the tree, nothing to draw. An acceleration the whole world
is asked about once a step; only bodies that asked for gravity and aren't anchored
are shoved.

`Physics.new(kind, *, name=None) -> Disaster`; `kinds()`, `running()`, `stopAll()`
(what the weather already did stays done).

| Disaster entry | Notes |
|---|---|
| `id`, `kind` (ro) | `"earthquake"`, `"wind"`, `"tornado"`, `"blast"` (aliases: quake, twister, gale/windstorm, explosion). Kind never changes — write another. |
| `name` | Defaults to the kind. |
| `magnitude` | **Every step up the scale doubles the shove.** 5 = everyday weather, 7 = the news, past 10 the engine stops listening (clamped). |
| `duration` | Seconds; 0 (wind/funnel default) runs until `stop()`; a blast arrives with < 0.5 s. |
| `elapsed` (ro) | Seconds already run. |
| `epicentre` | Where it's worst; ignored while `node` is set. |
| `reach` | Metres before unfelt; 0 = whole world. |
| `direction` | Which way a wind blows; other kinds find their own. |
| `node` | The event rides this node's world place (a funnel bolted to a fan follows it). `None` restores the epicentre. |
| `running` | Whether the solver is asking it now. |
| `start()`, `stop()`, `destroy()` | Lifecycle. Destroyed handles answer `ValueError`. |
| `accelerationAt(point) -> Vec3` | How hard/which way it shoves at one point; zero while off. A script may ask the weather a question. |

Quake shove is bursty; damage reads the decaying peak (calibrated).

---

## 12. Atmosphere — `carbine.atmosphere`

The environment register: time, visibility, light budget, sky, and the weather
that falls (written by the particle generator).

| Entry | Notes |
|---|---|
| `sky() -> Sky` | The backdrop handle. |
| `lightQuality` | `"auto"` (default), `"low"`, `"medium"`, `"high"`, `"extra"`. Auto reads the card: **> 4 GB → high; 0.1–4 GB → medium; smaller or shared memory → low**. Aliases: best/default, fast, mid/balanced, great, max/ultra. |
| `lightQualities()` | The rungs. |
| `light() -> Light` | What the light turned out to be (read-only report). |
| `time(clock=None) -> float` | Hours past midnight. Write `"07:30"` or `18`; reading returns hours; past midnight wraps. Turns the sun. |
| `fog` | 0 bright day .. 100 can't see your hand. Fog is the air — nothing to start/stop. |
| `fogColor` | Vec3/tuple/named/hex. |
| `new(kind, *, name=None) -> Particle` | One of the known weathers, **already flying and hung on the sky** (box rides with the eyes). Handle is a Particle with every particle knob. |
| `kinds()` | `["ash", "dust", "rain", "snow"]` (aliases: cinder/emberfall; sand/grit; drizzle/shower/downpour; blizzard/sleet). |
| `running()` | Sky systems falling over the view now (local crowds don't answer). |
| `stopAll()` | Every sky system off; each stays written and startable. Clears the air, not the fog. |

### `Sky`
`enabled`, `zenith`, `horizon` (colour writes), `noise` (grain), `sun: Vec3`
(direction), `ambient`, `colors(zenith, horizon)` convenience.

### `Light` (read-only report — nothing here is a knob)
`quality` (never `"auto"` — the machine has been read by ask time), `style`
(= `carbine.lightType`), `shadows` (sun pre-renders a cast), `shadowMap` (texels a
side, 0 when none), `softness` (readings per pixel; 1 hard edge, more fades),
`reach` (metres around the eyes the cast covers), `gloss` (0..1 sheen a square face
throws back), `skyFill` (unlit faces borrow the backdrop), `device`
(`discrete/integrated/cpu/virtual/other`, None until a renderer starts), `memory`
(bytes the card owns outright; None for shared or coy cards).

### Light nodes
Lights are **nodes** (`carbine.new("Light")`): brightness/colour/reach/type. The
born sun node mirrors `Sky.sun`; lamps light but cast no shade yet. Light quality
ladder decides how much work a frame takes; `lightType` decides the edge look at
the same cost.

---

## 13. Particles — crowds as nodes

A system is a **node**, not a register: it hangs in the tree (a chimney parent =
embers from its top), moves, takes colour, skins its drops, is destroyed like any
object. It has no body, so its knobs are about the crowd.

### Generator: `carbine.procedural.particles(...) -> Particle`
All keyword-only, all optional; one left out keeps the fresh-system value.
`particles()` alone is a rain veil; `particles(shape="circle", color="#fff8f0",
speed=1.2)` is snow. Knobs: `name, parent, shape, count, size, speed, direction,
area, lifetime, seed, color, image, active, collide, splash, recycle`. The crowd is
laid **once every knob is written** (the seed last of the placement knobs), so the
veil on screen is the one the script named. `procedural.shapes()` lists the cuts.

### `Particle` handle
Tree/identity: `id, name, kind ("particles"), parent, children, addChild,
removeChild, position, rotation, rotationEuler, color, colorHex, texture,
transparent, visible, destroy`.

| Knob | Notes |
|---|---|
| `shape` | `"streak"` (long along travel, 6 × size), `"square"` (keeps edges), `"circle"` (soft). Aliases — circle: disc/dot/round/snow/spark; square: box/quad/ash/leaf; streak: dash/line/drop/rain. |
| `count` | In the air. Growing lays new drops without moving the old; shrinking takes the extras out. Cap 20,000. |
| `size` | Metres across a drop. |
| `speed` | m/s along `direction`. |
| `direction` | World metres; zero = straight down. Turning the node turns the fall. |
| `area` | Box new drops are placed in, centred on the node. |
| `lifetime` | Seconds a drop lives. |
| `seed` | Word or full u64. Writing re-lays the whole veil. |
| `active` | Off keeps node + settings + tree place, empties the air. |
| `collide` | Drops stopped by bodies (floor + colliders) or fall through. |
| `splash` | Droplets an impact throws back (0 none; ≤ 32 per hit; 4,096 splash cap; 0.3 s life). |
| `recycle` | Off: the system runs out and the sky clears on its own. |
| `onSky` | Box rides with the eyes — the weather veil. |
| `inAir` (ro) | Live count now. |
| `impacts` (ro) | Drops brought up short last step. |
| `restart()` | Re-lay the crowd where the node stands now (after moving/turning/re-hanging). |

Mechanism: drops are depth-tested world billboards; collision is budgeted
(2,048 queries a step — a storm that overshoots by a frame is unnoticed, one that
takes a frame to answer is not). Each drop keeps a random part of the colour so a
thousand drops don't read as one flat sheet. Image particles skin the quad from a
`.ktx2`.

---

## 14. UI — the pixel overlay

`carbine.ui.new(kind="square", *, name=None, parent=None) -> Object`. The overlay
is measured in **pixels**, not metres, and hangs off the scene root on its own
branch (the one-branch rule: a UI element never sits inside a world branch, root
excepted). Every UI element is an `Object` handle with the shared node surface
(`id, name, kind, parent, children, addChild, removeChild, position, rotation,
color, colorHex, transparent, visible, destroy`) plus the overlay knobs below.
World objects **raise** on the overlay knobs; UI elements raise on physics knobs.

Paint order is children order — the last child sits on top. `position` is the
element's anchor in pixels (Text anchors top-left; shapes centre on it).

### The presets

All case-insensitive. Every shape is the same unit quad — the UI pass decides how
it is filled, so `polygon`/`corner` turn a square into a circle without a second
mesh.

| Kind | Sides | What it draws |
|---|---|---|
| `square` | 4 | Equal-sided card; the default. |
| `rectangle` | 4 | A card sized freely by `size = (w, h, 1)`. |
| `triangle` | 3 | Three-sided polygon. |
| `hexagon` | 6 | Six-sided polygon. |
| `circle` | 64 | A disc (a 64-gon reads as round). |
| `text` | — | The SDF glyph path, not a polygon (see below). |

### Shape properties (square / rectangle / triangle / hexagon / circle)

| Property | Type | Notes |
|---|---|---|
| `position` | Vec3 | Pixels; the shape centres on x, y. |
| `size` | Vec3 | `(width, height, 1)` in pixels. |
| `color` | Vec3 \| str | Fill colour (named / hex / tuple). |
| `transparent` | float | Opacity 0..255. |
| `corner` | float | Corner rounding in pixels (a circle is already round). |
| `polygon` | int | Side count override — turns the preset into another n-gon. |
| `outline` | float | Outline thickness in pixels. |
| `visible` | bool | Hide without destroying. |

### Text — `ui.new("Text")`

A baked **SDF atlas** (ASCII + Latin-1); unsupported glyphs render `?`. Position is
**top-left**. Numeric inputs reject NaN/infinity with `ValueError`.

| Property | Type | Notes |
|---|---|---|
| `content` | str | The string drawn. |
| `size` | float | Font size in pixels, 1..400 (scalar, not a vector). |
| `color` | Vec3 \| str | Glyph colour. |
| `outline` | bool | Black outline on/off (a bool here, a float on shapes). |
| `outlineWidth` | float | Outline pixels, stored 0..32, drawn up to 0.6 × font size. |
| `wordWrap` | bool | Wrap long lines. |
| `width` | float | Wrap width in pixels, clamped 1..16384. |
| `position` | Vec3 | Pixels, top-left anchor. |
| `transparent` | float | Opacity 0..255. |
| `visible` | bool | Hide without destroying. |

`carbine.fps` (measured FPS) written into a Text node's `content` each frame is the
live readout pattern.

---

## 15. Procedural — `carbine.procedural`

Generators make content instead of asking for it. Rules they all keep:
- **Configure fully before producing** — the thing laid is the one the knobs name.
- **Seeds are the full unsigned u64 range**; a word hashes (FNV-1a, never changed)
  to the top of it, so a seed read off one system writes straight into another.
  Same seed = same answer on every machine in a session.
- The substrate lives once in core: splitmix64 mixer, lattice hash, value noise,
  fbm octaves (≤ 8), wrapping tile noise, index-salted scatter (growing a scatter
  never moves the places it had).

### `particles(...)` and `shapes()` — see §13.

### `texture(surface, *, variation=None, quality=None, seed=None, out=None) -> str`
Paints a surface and writes the `.ktx2` an object skins with; answers the path.
- `surface` ∈ `textureTypes()` = `["brick", "grass", "rock", "sand"]`.
- `variation` ∈ `textureVariations(surface)`: grass — dry/moss; sand — dust/pebble;
  rock — cracked/mossy; brick — dusty/mossy. Unknown names raise `ValueError`
  listing what's offered; names read forgivingly (lawn, hi, small all land).
- `quality` ∈ `textureQualities()`: **low = 256² / 3 octaves, medium = 512² / 5
  (default), high = 1024² / 7**. Patterns are *counted*, not measured — six courses
  of brick are six courses at any quality.
- `seed`: same word paints the same surface for anybody.
- `out`: folder → into it; anything else → exact file; left alone → the run folder,
  named for everything asked so a rerun overwrites its own picture. Unmakeable
  folder → `OSError`. `pathlib.Path` accepted.
- Bake is spread over the cores; height and colour passes **share the fbm fields**
  (one noise evaluation per texel feeds both). Output is BC7 + full mip chain —
  the bytes the GPU wants, no decode at scene load.

### `map(*, name=None, parent=None, size=None, relief=None, quality=None, seed=None) -> Terrain`
Lays a heightfield of seeded hills as ground to walk on — somewhere to stand
instead of a flat plane. Every knob is optional; one left out keeps the fresh-map
value, so `map()` alone is a 64 × 64 m hill field and `map(size=(256, 256),
relief=20, seed="valley")` is a wide valley. The ground is laid **once every knob
is written**. `new("terrain")` lays the same defaults but returns without naming
them. `mapQualities()` lists the rungs.

| Knob | Default | Notes |
|---|---|---|
| `size` | `(64, 64)` | Reach along length and width, metres. A `(length, width)` tuple or a Vec3 (first two axes read). |
| `relief` | `6.0` | How tall the hills stand above the node, metres; 0 lays flat. |
| `quality` | `"medium"` | Grid fineness — see the ladder below. Unknown name → `ValueError`. |
| `seed` | engine default | Word or full u64; same seed lays the same ground for anybody. |

Quality ladder (`mapQualities()` = `["low", "medium", "high"]`): **low = 32 cells /
3 octaves, medium = 64 / 4, high = 128 / 5**. A finer grid is the *same hills seen
closer*, not different hills — the fbm is counted across the map (three features
wide), so quality changes resolution, never the shape.

### `Terrain` — the map node handle

A map is **scenery, not a body**: it holds its place, is never shoved by the
solver, and is walked on without being cut into collider pieces. It hangs in the
tree, is parented, moved, turned, coloured, skinned and destroyed like any node.
Its added knobs are about the ground rather than a shape.

Tree/identity: `id, name, kind ("terrain"), parent, children, addChild,
removeChild, position, rotation, rotationEuler, color, colorHex, texture,
textureTiling, transparent, visible, destroy`.

| Knob | Type | Notes |
|---|---|---|
| `size` | `(float, float)` \| Vec3 | Reach in metres; reads back as a `(length, width)` pair. Writing re-lays the ground at the new reach — hills keep their shape (counted across the map, not by the metre). |
| `relief` | float | Hill height above the node, metres; 0 flat. Writing re-lays. |
| `quality` | str | `"low"`/`"medium"`/`"high"`; writing re-lays finer/coarser. |
| `seed` | int \| str | The dice the hills were rolled from; writing lays the map that seed names. |
| `position` | Vec3 | The map is laid centred on the node, so moving it carries the ground — and the floor anything standing on it is caught by — along. |
| `rotation` | Quat | Turning the node turns the ground; on its side it is a cliff face. |
| `texture` / `textureTiling` | str \| None / float | Skin like any object; the tile is laid by the metre across the map, so a grass skin reads as one lawn. |
| `visible` | bool | Hidden, it **still holds its floor** — a walker stands on a map they cannot see. |

**Analytic grounding**: the floor under any spot is `max(flat ground, terrain
surface beneath it)`, hooked into the player's feet and every falling body's
underside. Maps are walked on directly — no thousands of collider pieces. Each
re-layout (size/relief/quality/seed write) rebuilds the mesh on a fresh GPU handle.
In a link group the ground knobs (`size/relief/quality/seed`) broadcast to every
member while `position` stays local — linked twins share their hills but stand in
their own places. A `duplicate` of a map is real walkable ground too.

---

## 16. Packager — `carbine.packager`

An authored file in, a Carbine-format file out. The same work Enginebay does on
drop; a script can do it in real time. Only this layer knows what a PNG is — the
runtime loaders stay decoder-free.

### `image(path, *, out=None) -> str`
Picture → `.ktx2` (BC7 blocks, full mip chain). Reads **png, jpg, webp, tga, bmp**;
anything else `ValueError`; unwriteable target `OSError`. No `out`: beside the
source, same stem (`wood.png → wood.ktx2`). `out` naming a folder (exists, or
trailing separator) packs into it keeping the stem; anything else is the exact
file. `pathlib.Path` fine for both.

### Model conversion
Engine reads **only GLB** (settled); the Packager converts obj/stl via pure-Rust
crates (tobj / stl_io). FBX ruled out. Imports stay a node tree.

### Audio — in flight: `audio(path)` mp4/mp3/wav → `.ogg`, then the engine's own
Ogg parser reads it back (see §18).

---

## 17. Frame pacing & teardown semantics

- Pacing: `fpsCap`/`monitorSync` absent by default (`None`/`False`); the limit is
  **min(cap, panel)**; Wayland names Hz only after frame one; `monitorHz` re-asked
  every second. `fpsLimit` reports the settled rate; `fps` the measured one.
- `destroy("all")` drops **authored content only**; camera/backdrop/player/sun are
  settings and stay. `end()` = new scene + window shut, keeps the frame limits.
- `atexit.register(close)` — a game that never calls `close()` still leaves the
  window and device cleanly (renderer dropped before the GPU library tears down).

---

## 18. In flight (not yet on the Python surface)

### Audio
Plan: `packager.audio` converts mp4/mp3/wav → `.ogg` (pure-Rust converter deps;
FLAC-in-Ogg likely, no pure-Rust Vorbis encoder exists), then the engine parses the
`.ogg` with **its own Ogg page parser + codec decoder** and plays it (cpal for the
device, Sound as a node). Nothing shipped yet.

### Also planned, undeclared
Map variants/biomes and a tree count; building generator; flipbook loops (ktx2 has
no layers yet); video on hold.

---

## 19. Named colours

Accepted anywhere a colour is written (objects, sky, fog, particles, UI):
red, dark red, light red, light orange, orange, dark orange, yellow, banana,
light yellow, skin color, white, black, grey/gray (+ light/dark), green
(+ light/dark), blue (+ light/dark), cyan, teal, purple, pink, brown, cream, navy,
gold — plus hex `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`.

---

## 20. Conventions that explain the edges

- **Handles tell you what they are**: light knobs raise on a cube, UI knobs raise
  on a world object, vehicle knobs raise on anything not driven. A destroyed node
  answers `DestroyedError`; a destroyed disaster `ValueError`.
- **Writes arrive at rest**: placing the player doesn't throw it; copies stand
  still from where the original stood.
- **Wishes vs speeds**: driver inputs are wishes the handling resolves; keys are
  target velocities the body chases.
- **Nothing is destroyed by accident**: re-listing children moves them to root;
  `removeChild` detaches; only `destroy`/`end`/death-with-removal take things out.
- **The engine deals mechanics, the script deals meaning**: death, revive, what a
  corpse is for, what the weather is for — all script decisions on top of fixed
  core rules.