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

### Step 1 — Create genre data file

#### Option A — from RYM text dump (recommended)

1. Export your RYM genre page as plain text to `a.txt`
2. Convert to JSON:
   ```bash
   cd rym-to-genre
   cargo run --release -- ../a.txt --genre Goregrind --output ../genre-to-playlist/genres/goregrind.json
   ```
3. The tool auto-extracts artist/album/year triples and deduplicates.

#### Option B — hand-crafted JSON

Create `genre-to-playlist/genres/<name>.json`:

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

### Step 2 — Wire into Terraform config

Add a resource block to `examples/main.tf`:

```hcl
resource "ytmusic_playlist" "goregrind" {
  title       = "Genre: Goregrind"
  description = "Extreme grindcore with gore-themed lyrics"
  privacy     = "unlisted"

  lifecycle {
    ignore_changes = all    # prevents accidental modification after populate
  }
}

output "goregrind_playlist_id" {
  value = ytmusic_playlist.goregrind.playlist_id
}
```

Run Terraform to create the empty playlist:

```bash
cd examples/
terraform apply          # outputs playlist_id like "PLRALkHBpmpKQ"
```

### Step 3 — Populate with genre-to-playlist

```bash
cd genre-to-playlist

# Dry-run first
cargo run --release -- \
  --playlist-id PLRALkHBpmpKQ \
  --cookie ~/.config/youtui/cookies.txt \
  --genre goregrind \
  --dry-run

# Then populate
cargo run --release -- \
  --playlist-id PLRALkHBpmpKQ \
  --cookie ~/.config/youtui/cookies.txt \
  --genre goregrind \
  --per-band 3 --max-songs 5000 --privacy unlisted
```

**That's it.** Terraform owns the playlist lifecycle (create / destroy / import).
`genre-to-playlist` owns the song population. Once populated, `lifecycle.ignore_changes`
prevents Terraform from modifying the playlist content.

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

Export cookies from music.youtube.com (Netscape format, **not** HTTP header format — no `Cookie:` prefix).

See `examples/cookies.txt.example` for the expected format.

Set via `cookie_file` in Terraform config or `YTMAPI_COOKIE` env var.
