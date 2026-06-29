# terraform-provider-ytmusic

Terraform provider + CLI tools for YouTube Music.

## Architecture

```
Terraform (Go plugin) ──stdin/stdout──► ytmusic-cli (Rust sidecar)
                                           └── ytmapi-rs
```

Go provider shells out to Rust binary via JSON protocol. No HTTP from Go.

## Quick start

```bash
# Build everything
make all

# Install locally
make install

# Create a playlist with Terraform
cd examples/
terraform apply
```

## Resources

| Resource | Description |
|----------|-------------|
| `ytmusic_playlist` | CRUD playlist. Fields: `title`, `description`, `privacy`, `video_ids` |

## Data sources

| Data source | Description |
|-------------|-------------|
| `ytmusic_search` | Search songs/artists/albums/playlists. Fields: `query`, `type`, `results` |

## genre-to-playlist CLI

Batch-create playlists from genre band/album lists.

```bash
cd genre-to-playlist/

# List available genres
cargo run --release -- --list-genres

# Dry-run (preview without creating)
cargo run --release -- \
  --genre sasscore \
  --cookie /path/to/cookies.txt \
  --dry-run

# Create + populate in one step
cargo run --release -- \
  --genre sasscore \
  --cookie /path/to/cookies.txt \
  --per-band 3 --max-songs 400 --privacy unlisted

# Populate an existing playlist (created via Terraform)
cargo run --release -- \
  --playlist-id PLffpwpOuFBzw \
  --genre sasscore \
  --cookie /path/to/cookies.txt
```

### Workflow with Terraform

```
1. terraform apply        → creates empty playlist, outputs ID
2. genre-to-playlist \    → searches + populates
     --playlist-id <ID> \
     --genre sasscore
3. terraform destroy      → removes playlist from YT Music
```

### Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--genre` | required | Genre name matching a file in `genres/` |
| `--cookie` | `YTMAPI_COOKIE` | Path to cookies.txt |
| `--playlist-id` | — | Populate existing playlist (skip create) |
| `--max-songs` | 5000 | Max total songs |
| `--per-band` | auto | Songs per band (auto = max/size capped 1-100) |
| `--dry-run` | false | Search + sample only, no write |
| `--privacy` | private | private / public / unlisted |
| `--name` | "Genre: \<name\>" | Custom playlist name |

## Adding a genre

1. Create `genre-to-playlist/genres/<name>.json`

Two formats:

**Band-based** (search "{band} {genre}"):
```json
{
  "genre": "Black Metal",
  "description": "Optional description",
  "bands": ["Mayhem", "Burzum", "Darkthrone"]
}
```

**Entry-based** (curated artist/album, more precise):
```json
{
  "genre": "Sasscore",
  "entries": [
    {"artist": "SeeYouSpaceCowboy...", "album": "The Romance of Affliction", "year": 2021},
    {"artist": "The Blood Brothers", "album": "...Burn, Piano Island, Burn"}
  ]
}
```

`year` is optional. Entry mode searches `{artist} {album} {year}` and filters results by album name.

2. Run:

```bash
cargo run --release -- --list-genres    # verify it shows up
cargo run --release -- --genre <name> --dry-run   # preview
cargo run --release -- --genre <name>             # create
```

## Structure

```
├── main.go                         # Go plugin entry
├── provider/                       # Terraform resources + data sources
├── ytmusic-cli/src/main.rs         # Rust sidecar (JSON protocol)
├── genre-to-playlist/              # Batch playlist creator
│   ├── src/main.rs
│   └── genres/                     # Genre data files
└── examples/
```

## Auth

Export cookies from music.youtube.com (Netscape format). Set via `cookie_file` in Terraform config or `YTMAPI_COOKIE` env var.
