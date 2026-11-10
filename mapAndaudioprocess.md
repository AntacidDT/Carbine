# Map generator + Audio — working process log

Written while working so a context freeze can be caught up from this file alone.
Update after every chunk. No commits unless asked.

## The ask

1. `carbine.procedural.map` — terrain/maps instead of flat unusable places.
2. Audio: `packager.audio` converts mp4/mp3/wav → `.ogg`, then the engine parses
   the `.ogg` with its own parser and plays it.
3. Map first, audio second.

## Standing rules (do not violate)

- Rust core does every calculation; Python is a thin scripting layer.
- Own runtime parsers (Ogg + codec decoding hand-rolled). Pure-Rust crates ARE
  allowed in the *converter* (Packager) — precedent: rusty_dds, tobj, stl_io.
- Generator rules: configure fully BEFORE producing; `seed` accepts/returns the
  full unsigned u64 range (words hash to top of u64); deterministic.
- No `image` crate outside carbine-pack.
- Verification chain: `cargo fmt --all`, clippy 0 warnings,
  `cargo test -p carbine-core -- --test-threads=1`,
  `.venv/bin/maturin develop`, whole `.venv/bin/pytest` (singletons leak — run all).
- Never Read PNGs/screenshots; user runs demos himself.
- No commits/pushes unless explicitly asked.

## Design decisions so far

### Map (terrain)
- Terrain = one node wearing a heightfield MeshData (grid of quads→triangles),
  generated from seeded fbm in carbine-core (procgen substrate already has
  noise/fbm/wrapping noise from tasks #47/#112).
- Collision: NOT mesh collider (thousands of convex pieces = unusable).
  Analytic heightfield grounding: sample height under player/bodies at the two
  floor checks — player.rs `step_player` (position.z <= GROUND_HEIGHT, ~line 347)
  and physics.rs `integrate` (rest = position.z + bottom, ~line 146).
  Floor height becomes `max(GROUND_HEIGHT, terrain.height_at(x, y))`.
- Knobs: size (extent), relief (height amplitude), quality (grid resolution),
  seed; biome/variation later per spec memory (variants, biomes, tree count).
- API shape: `carbine.procedural.map(...)` like particles/texture generators.

### Audio
- Packager converter: pure-Rust decode (symphonia candidate for mp3/mp4/wav).
  No pure-Rust Vorbis encoder exists → encode FLAC (flacenc) and mux into Ogg
  with our own muxer. Output `.ogg` container, FLAC payload.
- Runtime: own Ogg page parser + own FLAC decoder crate (carbine-ogg or
  similar), no runtime codec crates. cpal for output device (OS binding is
  acceptable like wgpu/winit).
- API: Sound as a node (lights/particles precedent) — decide exact surface when
  we get there.
- Crate availability NOT yet verified (cargo search pending).

## Recon done

- Read: procgen spec memory, Cargo.toml, mesh.rs (MeshData), primitives.rs (cube
  UV contract), carbine-pack/lib.rs (PackError/destination pattern to mirror),
  player.rs:300–429, physics.rs:55–234 (step order + floor contacts).
- Step order: step_player → count_down_hits → run_disasters → step_vehicles →
  integrate → recut → push_apart → hurt_by_contacts → place_seats → ride_seat →
  follow_camera → place_body → run_particles.

## Progress

- [x] Recon: grounding points located, packager pattern read.
- [x] Read procgen substrate + particle/model generator binding as template.
- [x] Implement terrain generator in carbine-core (mesh + height_at).
      `crates/carbine-core/src/terrain.rs`: kind "terrain", TerrainQuality
      low/medium/high (cells 32/64/128, octaves 3/4/5), seeded fbm heightfield,
      MeshData grid, Terrain component, create_terrain[_with], setters rebuild
      on a fresh mesh handle, floor_at(x,y) analytic grounding.
- [x] Analytic terrain grounding in player + physics.
      player.rs step_player + physics.rs integrate now use self.floor_at(x,y)
      = max(GROUND_HEIGHT, terrain height under spot). Terrain is anchored,
      has no collider (not a solver body). 9 terrain tests pass serial.
- [ ] PyO3 binding `carbine.procedural.map` + Terrain handle + stubs + tests + demo.
- [ ] Full verification chain for map.
- [ ] Audio: verify crates, implement packager.audio.
- [ ] Audio: own Ogg/FLAC parser crate.
- [ ] Audio: playback (cpal) + Sound API + stubs/tests/demo.
- [ ] Full verification chain for audio.

## Current state

Core terrain generator implemented and tested (9/9). Next: Python binding
`carbine.procedural.map(...)` returning a Terrain node handle, register in
native lib.rs, add to kinds(), write .pyi stubs, pytest, demo. Then audio.
texture.rs has uncommitted fbm-sharing optimization (task #123, verified green).
NO commits made (none requested).