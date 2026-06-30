# Terraform Provider for YT Music

Terraform provider + CLI tools for managing YouTube Music playlists programmatically.

**Architecture:** Go provider (terraform-plugin-sdk/v2) shells out to Rust sidecar (ytmusic-cli) via JSON stdin/stdout.

## Repository Structure

```
├── main.go                    # Go provider entrypoint
├── provider/                  # Terraform provider impl
├── ytmusic-cli/               # Rust CLI sidecar (JSON protocol)
├── genre-to-playlist/         # Rust CLI: batch populate from genre data
├── rym-to-genre/              # Rust CLI: convert RYM lists to genre JSON
└── examples/
    ├── main.tf                # Terraform config
    └── cookies.txt.example    # Cookie format reference
```

## Quick Start

### 1. Cookie setup

Export cookies from your browser (Netscape format) after logging into music.youtube.com:

```
cp examples/cookies.txt.example ~/.config/youtui/cookies.txt
# Replace placeholder values with real exported cookies
```

### 2. Build

```bash
make build
```

### 3. Use Terraform

```bash
cd examples
terraform init
terraform apply   # type 'yes'
```

### 4. Populate a playlist

```bash
genre-to-playlist --genre goregrind --per-band 5
```

## CLI Commands

### ytmusic-cli

JSON stdin/stdout protocol used by the Terraform provider.

```bash
# Auth check
ytmusic-cli auth-check

# List library playlists
ytmusic-cli playlist-list

# Get playlist details
ytmusic-cli playlist-get '{"id":"PL..."}'

# List all tracks in a playlist
ytmusic-cli playlist-tracks '{"id":"PL..."}'

# Clean a playlist (interactive: y/n/d/q per artist)
ytmusic-cli playlist-clean '{"id":"PL..."}'

# Remove all tracks by an artist
ytmusic-cli playlist-remove-artist '{"id":"PL...","artist":"Katy Perry"}'

# Search
ytmusic-cli search '{"query":"Møl","type":"artists"}'

# Debug: dump raw browse response
ytmusic-cli debug-browse '{"id":"PL..."}'
```

Cookie: set `YTMAPI_COOKIE=~/.config/youtui/cookies.txt` env var or pass `--cookie <file>`.

### genre-to-playlist

Batch populate playlists from genre JSON files.

```bash
# List available genres
genre-to-playlist --list-genres

# Dry run (don't create playlist)
genre-to-playlist --genre sasscore --dry-run

# Create + populate
genre-to-playlist --genre goregrind --per-band 5

# Use existing playlist ID
genre-to-playlist --genre skramz --per-band 100 --auto-split \
  --playlist-id PLSP0ygdlaKPg

# Per-band limit: how many songs to take per artist
# Higher = more coverage per artist, more API calls
genre-to-playlist --genre goregrind --per-band 10

# Auto-split: create Pt.2, Pt.3 when 5000 song cap hit
genre-to-playlist --genre skramz --per-band 100 --auto-split

# Use cache (skip search phase on re-run)
genre-to-playlist --genre skramz --use-cache
```

### rym-to-genre

Convert RYM (RateYourMusic) text dumps to genre JSON files.

```bash
# Parse RYM list -> genre JSON
rym-to-genre rym-to-genre/raw/goregrind.txt genres/goregrind.json

# Custom genre name/description
rym-to-genre rym-to-genre/raw/mylist.txt genres/mygenre.json \
  --name "My Genre" --description "RYM essentials"
```

Raw RYM text files go in `rym-to-genre/raw/`. Use Ctrl+A Ctrl+C from the RYM page to get the complete list including header.

## Adding a Genre

1. **Create genre data**: either write a JSON file or parse a RYM list:
   ```bash
   rym-to-genre rym-to-genre/raw/mygenre.txt genre-to-playlist/genres/mygenre.json
   ```

2. **Wire Terraform resource** in `examples/main.tf`:
   ```hcl
   resource "ytmusic_playlist" "mygenre" {
     title       = "Genre: My Genre"
     description = "RYM Essentials\n\n--\nGenerated via github.com/caos-obliquo/terraform-provider-ytmusic"
     privacy     = "unlisted"
   }
   ```
   Then `terraform apply`.

3. **Populate**:
   ```bash
   genre-to-playlist --genre mygenre --per-band 5
   ```

## Genre JSON Format

```json
{
  "name": "goregrind",
  "description": "RYM Goregrind Essentials (441 entries)",
  "entries": [
    {"artist": "Carcass", "album": "Reek of Putrefaction", "year": 1988},
    {"artist": "Impetigo", "album": "Ultimo mondo cannibale", "year": 1990}
  ]
}
```

Or band-only format (no specific albums):

```json
{
  "name": "black-metal",
  "genre": "black metal",
  "bands": ["Mayhem", "Burzum", "Darkthrone"]
}
```

## Genre Validation

genre-to-playlist dynamically validates genre tags via:
1. **Last.fm** album tags (primary)
2. **MusicBrainz** genre tags (fallback)
3. **Band-name search** for validated entries (catches more songs)

Rejected entries show `✗` with reason. Uncertain entries show `?` (added anyway).

Set Last.fm API key: `YTMAPI_LASTFM_KEY=your_key` or in `~/.config/youtui/config.toml [scrobbling].api_key`.

## Playlist Management

### Dedup Auto-Handling

When adding songs to a playlist, duplicates are automatically skipped:
- `DuplicateHandlingMode::Skip` on API calls
- Paginated fetch for existing tracks (optional dedup)
- Backoff on rate limits (30s/60s/120s)

### Cleaning a Playlist

For sasscore playlist (wrong artists leaked in from old search logic):

```bash
# Interactive: review each artist, y=remove, n=keep, d=done, q=quit
ytmusic-cli playlist-clean '{"id":"PLffpwpOuFBzw"}'

# Or non-interactive: remove all by an artist
ytmusic-cli playlist-remove-artist '{"id":"PLffpwpOuFBzw","artist":"Katy Perry"}'
```

## Environment Variables

| Variable | Purpose |
|---|---|
| `YTMAPI_COOKIE` | Path to Netscape cookie file |
| `YTMAPI_LASTFM_KEY` | Last.fm API key for genre validation |

## Architecture

```
Terraform (HCL)
  └── Go provider (terraform-plugin-sdk/v2)
       └── ytmusic-cli (Rust, JSON stdin/stdout)
            └── ytmapi-rs (YouTube Music API client)
                 └── YouTube Music (internal API)
```

*Inspired by terraform-provider-spottypso* (made with ❤️ and hatred for YT Music's undocumented API)
