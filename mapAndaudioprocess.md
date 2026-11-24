# Map generator + Audio + Video — working process log

Written while working so a context freeze can be caught up from this file alone.
Update after every chunk. No commits unless asked.

## The ask

1. `carbine.procedural.map` — terrain/maps instead of flat unusable places.
2. Audio: engine reads `.wav` and `.mp3` directly; no conversion step, no `.ogg`.
3. Video: engine reads `.mp4`, shows it on a rectangle and plays its sound track.
4. Map first, audio second, video third.

## Standing rules (do not violate)

- Rust core does every calculation; Python is a thin scripting layer.
- Own runtime parsers where they pay (GLB, KTX2, WAV, collision). Codec crates ARE
  allowed for MP3/AAC (Symphonia) and MP4 pictures (system ffmpeg); cpal owns the
  output device. The `.ogg` plan was dropped: nobody has `.ogg`, and converting to
  it added a container with no runtime benefit.
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
- Format pivot: `.wav` + `.mp3`, no `.ogg`, no Packager step. The mixer reads what
  people actually have.
- `crates/carbine-audio`: hand-rolled WAV parser (PCM 8/16/24/32, float 32/64,
  extensible), MP3 via Symphonia, MP4/AAC audio track via Symphonia, and a voice
  mixer with linear resampling and mono/stereo mapping.
- `carbine-native`: cpal output on a thread-local stream; the callback only locks
  the mixer. No device is not an error — `step()` advances the mixer on the game
  clock, so headless tests see sounds start, move and end.
- API: `carbine.new("Sound")` is a node with `source`, `volume`, `looping`,
  `playing`, `time`, `duration`, `play()`, `stop()`.

### Video
- `.mp4` only. `play()` probes with `ffprobe`, spawns `ffmpeg` on a worker thread
  for RGBA frames, and decodes the AAC track into the same mixer.
- `carbine.new("Video")` is a rectangle (one metre high, film aspect) whose draw
  item asks for a per-node GPU texture; frames are uploaded as the playhead passes.
- `carbine-render` owns the video texture cache; `Gpu::upload_video` pads rows to
  the 256-byte copy alignment.
- Looping restarts both the decoder and the audio voice. A film with no audio track
  plays silently. Missing ffmpeg/ffprobe raises `ValueError` in `play()`.
- Decoded frames are capped at 1280 px wide so phone 4K clips do not eat memory.
- UI media: `ui.new("Image")` and `ui.new("Video")` are overlay quads. The UI shader
  samples a per-element picture (a white pixel for plain shapes, a packed `.ktx2`
  for images, a video frame for films), clipped to the element's shape. Films keep
  their pixel size; only world screens are reshaped by the film's aspect.
- `examples/video.py` is the walk-around demo: the MIT carbine character GLB as the
  player's body, a procedural map, a world screen, a HUD video, a packed logo image
  and a random mp4 from the home directory.

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
- [x] PyO3 binding `carbine.procedural.map` + Terrain handle + stubs + tests + demo.
- [x] Full verification chain for map.
- [x] Audio: `carbine-audio` crate (WAV + MP3 + MP4 audio + mixer).
- [x] Audio: cpal output, `carbine.new("Sound")`, stubs, tests, demo.
- [x] Video: `carbine.new("Video")`, ffmpeg frames, AAC audio, GPU texture upload.
- [x] Full verification chain for map + audio + video.

## Current state

Map, audio and video are implemented and green. Map: core heightfield, analytic
grounding, `procedural.map()`, 17 pytest cases. Audio: `.wav`/`.mp3`, Sound node,
mixer, cpal; 15 pytest cases. Video: `.mp4`, Video node, ffmpeg frames, AAC sound;
15 pytest cases, plus UI image/video cases. Verification: `cargo fmt --all`, clippy 0 warnings,
`cargo test -p carbine-core` 371 passed, `cargo test -p carbine-audio` 17 passed,
whole `.venv/bin/pytest` 427 passed, 1 skipped. NO commits made (none requested).