# terraform-provider-ytmusic

Terraform provider + CLI tools for YouTube Music.

Go provider shells out to Rust sidecar (ytmusic-cli) via JSON stdin/stdout.
genre-to-playlist populates playlists from genre JSON.
rym-to-genre converts RYM text dumps to genre JSON.

## Build & install

```bash
make all      # builds Go provider + ytmusic-cli
make install  # copies to ~/.terraform.d/plugins/
```

## Terraform

```
resource "ytmusic_playlist" "name" {
  title       = "Genre: Name"
  description = "..."
  privacy     = "unlisted"    # private / public / unlisted

  lifecycle {
    ignore_changes = all      # prevent overwrite after populate
  }
}

data "ytmusic_search" "song" {
  query   = "artist song"
  type    = "song"
}

output "playlist_id" {
  value = ytmusic_playlist.name.playlist_id
}
```

Auth: cookies.txt from music.youtube.com (Netscape format, no `Cookie:` prefix).
Set via `cookie_file` in config or `YTMAPI_COOKIE` env var.

## genre-to-playlist

Batch-search + populate YT Music playlists from genre JSON files.

### Usage

```bash
cd genre-to-playlist/

# List available genres
cargo run --release -- --list-genres

# Dry-run (search + sample, no write)
cargo run --release -- --genre sasscore --cookie cookies.txt --dry-run

# Create + populate
cargo run --release -- --genre goregrind --cookie cookies.txt --per-band 10

# Populate existing (created via terraform apply)
cargo run --release -- --playlist-id PLxxx --genre goregrind --cookie cookies.txt

# Auto-split into Pt.2/Pt.3 when 5000 cap hit
cargo run --release -- --playlist-id PLxxx --genre goregrind --per-band 10 --auto-split
```

### Flags

`--genre <name>` Genre name (matches genres/<name>.json).
`--cookie <path>` Cookies file path (or YTMAPI_COOKIE env).
`--playlist-id <id>` Populate existing playlist (skip create).
`--max-songs <n>` Max songs (default 5000, YT Music cap).
`--per-band <n>` Songs per band (default auto = max/bands, clamped 1-100).
`--dry-run` Search + sample only, no write.
`--privacy <mode>` private/public/unlisted (create only).
`--name <str>` Custom playlist name (default "Genre: <name>").
`--genres-dir <dir>` Genre JSON directory (default "genres").
`--auto-split` Create Pt.2/Pt.3 when near 5000 cap.
`--prune` Remove non-matching tracks from existing playlist.

### Search pipeline

Per item:
1. Query: `{artist} {album}` (entry) or `{band} {genre}` (band)
2. Genre validate: Last.fm album tags (primary) → MusicBrainz tags (fallback). Fail-open.
3. Broad mode (if validated): re-search artist-only for 3x per-band limit
4. Hard blocklist: rejects mainstream pop artists
5. Diversity sample: per-item shuffle, capped at per-band limit

### Dedup

Fetches existing playlist tracks via paginated continuation stream. Filters add list to only new video IDs.

### Rate limits

50 songs/batch. 3 retries (1s/3s/9s). 500ms throttle (750ms >500 songs, 1000ms >1000).

### Genre data format

**Band-based** — search `{band} {genre}`:
```json
{"genre": "Black Metal", "description": "...", "bands": ["Mayhem", "Burzum"]}
```

**Entry-based** (precise) — search `{artist} {album}`, filter by album name:
```json
{"genre": "Sasscore", "entries": [
  {"artist": "The Blood Brothers", "album": "...Burn, Piano Island, Burn"},
  {"artist": "SeeYouSpaceCowboy...", "album": "The Romance of Affliction"}
]}
```

Year optional in entries. Album-match only — never falls back to artist-only search (prevents pop leaks).

## rym-to-genre

Convert RYM text dump to genre JSON.

```bash
cd rym-to-genre
cargo run --release -- input.txt --genre Name --output ../genre-to-playlist/genres/name.json
```

Flags: `--genre` (req), `--output` (default stdout), `--description`, `--force`, `--verbose`.

Parses RYM format: blank-line-separated entries with artist/album/year.

## Adding a new genre

Files to touch:

| # | File | Change |
|---|------|--------|
| 1 | `genre-to-playlist/genres/<name>.json` | **New file** — genre data |
| 2 | `examples/main.tf` | **Edit** — add resource + output block |

No Go provider or genre-to-playlist code changes needed.

### Step 1 — Create genre data

Option A — from RYM text dump:
```bash
cd rym-to-genre
cargo run --release -- ../a.txt --genre Goregrind --description "..." \
  --output ../genre-to-playlist/genres/goregrind.json
```

Option B — hand-craft:
```bash
vim genre-to-playlist/genres/goregrind.json
```

Format: bands or entries (see above).

### Step 2 — Wire terraform

Edit `examples/main.tf`:
```hcl
resource "ytmusic_playlist" "goregrind" {
  title       = "Genre: Goregrind"
  description = "Extreme grindcore with gore-themed lyrics\n\n--\nGenerated via github.com/caos-obliquo/terraform-provider-ytmusic"
  privacy     = "unlisted"

  lifecycle { ignore_changes = all }
}

output "goregrind_playlist_id" {
  value = ytmusic_playlist.goregrind.playlist_id
}
```

Apply:
```bash
cd examples/
terraform apply
```

### Step 3 — Populate

```bash
cd genre-to-playlist

# Dry-run
cargo run --release -- --playlist-id PLxxx --cookie cookies.txt --genre goregrind --dry-run

# Populate
cargo run --release -- --playlist-id PLxxx --cookie cookies.txt --genre goregrind --per-band 10
```

### Step 4 — Update (add more songs later)

```bash
cargo run --release -- --playlist-id PLxxx --cookie cookies.txt --genre goregrind --per-band 20
```
Dedup skips already-existing tracks. Only new ones added.

## Project layout

```
main.go                   Go plugin entry
provider/                 Terraform resources + data sources
ytmusic-cli/src/          Rust sidecar (JSON stdin/stdout)
genre-to-playlist/
  src/main.rs             CLI + search + add pipeline
  src/genre_validator.rs  Last.fm + MusicBrainz validation
  genres/                 Genre data files
rym-to-genre/             RYM text dump → genre JSON
examples/
  main.tf                 Terraform config (add genre resources here)
  cookies.txt.example     Format reference
Makefile                  build/install/cross-compile
```
