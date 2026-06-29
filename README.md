# Terraform Provider for YouTube Music

> Manage YouTube Music playlists and search content as Terraform resources.

## Why This Exists

YouTube Music has no official API. Unofficial APIs require reverse-engineered auth tokens
that are hard to manage in Infrastructure-as-Code workflows. This provider solves that by
wrapping [ytmapi-rs](https://crates.io/crates/ytmapi-rs) — a pure-Rust YT Music client
using Google's internal API — behind a Terraform plugin.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                  Terraform                           │
│  terraform-provider-ytmusic (Go)                     │
│    ┌──────────────────────────────────────────┐      │
│    │  resource_playlist.go                     │      │
│    │  data_source_search.go                    │      │
│    │  ytmusic_client.go                        │      │
│    └────────────────────┬─────────────────────┘      │
│                         │ JSON stdin/stdout           │
│                         ▼                            │
│    ┌──────────────────────────────────────────┐      │
│    │  ytmusic-cli (Rust sidecar)              │      │
│    │  ┌────────────────────────────────────┐  │      │
│    │  │  ytmapi-rs  (YT Music API client) │  │      │
│    │  └────────────────────────────────────┘  │      │
│    └──────────────────────────────────────────┘      │
└─────────────────────────────────────────────────────┘
```

**Why two languages?** The Terraform Plugin SDK is Go-native, but the best
YT Music reverse-engineered API client is
[ytmapi-rs](https://crates.io/crates/ytmapi-rs) (Rust). Rather than porting
hundreds of hours of reverse-engineering work, the Go provider shells out
to a Rust sidecar (`ytmusic-cli`) via JSON-over-stdin/stdout. This keeps
both codebases idiomatic and maintainable.

## Prerequisites

- [Terraform](https://www.terraform.io/downloads) >= 1.0
- [Go](https://go.dev/dl/) >= 1.21 (to build the provider)
- [Rust](https://rustup.rs/) (to build the sidecar)
- A YouTube Music cookies file (see [Authentication](#authentication))

## Quick Start

```bash
# 1. Clone and build
git clone https://github.com/caos-obliquo/terraform-provider-ytmusic
cd terraform-provider-ytmusic
make all

# 2. Install locally
make install

# 3. Create a Terraform config
cat > main.tf << 'EOF'
terraform {
  required_providers {
    ytmusic = {
      source = "caos-obliquo/ytmusic"
    }
  }
}

provider "ytmusic" {
  cookie_file = "/path/to/your/cookies.txt"
}

resource "ytmusic_playlist" "example" {
  title       = "My Playlist"
  description = "Managed by Terraform"
  privacy     = "private"
}
EOF

# 4. Apply
terraform init
terraform apply
```

## Authentication

YouTube Music requires authentication via browser cookies.

1. Log into [music.youtube.com](https://music.youtube.com) in Chrome/Chromium
2. Install a cookie export extension (e.g., "Get cookies.txt" for Chrome)
3. Export cookies in Netscape format to a file
4. Set `cookie_file` in the provider config or `YTMAPI_COOKIE` env var

## Resources

### `ytmusic_playlist`

Manage YouTube Music playlists with full CRUD.

| Attribute   | Type   | Required | Description                               |
|-------------|--------|----------|-------------------------------------------|
| `title`     | string | yes      | Playlist name                             |
| `description` | string | no     | Playlist description                      |
| `privacy`   | string | no       | `"private"`, `"public"`, or `"unlisted"`  |
| `video_ids` | list   | no       | Video IDs to seed the playlist            |
| `playlist_id` | string | computed | YT Music playlist ID (read after create) |

**Import existing playlists:**
```bash
terraform import ytmusic_playlist.example VLPLAYLISTID
```

## Data Sources

### `ytmusic_search`

Search YouTube Music catalog.

| Attribute | Type   | Required | Description                                    |
|-----------|--------|----------|------------------------------------------------|
| `query`   | string | yes      | Search term                                    |
| `type`    | string | no       | `"songs"` (default), `"artists"`, `"albums"`, `"playlists"` |
| `results` | list   | computed | List of results with `title`, `artist`, `video_id`, `album` |

**Usage:**
```hcl
data "ytmusic_search" "genre" {
  query = "black metal"
  type  = "songs"
}

output "video_ids" {
  value = data.ytmusic_search.genre.results[*].video_id
}
```

## Building

```bash
make all          # Build both Rust sidecar and Go provider
make build-rust   # Build only the Rust sidecar
make build-go     # Build only the Go provider
make install      # Build + install to ~/.terraform.d/plugins/
```

## Project Structure

```
├── main.go                    # Go plugin entry point
├── provider/
│   ├── provider.go            # Provider registration + config
│   ├── ytmusic_client.go      # Go→Rust bridge (JSON stdin/stdout)
│   ├── resource_playlist.go   # ytmusic_playlist CRUD
│   └── data_source_search.go  # ytmusic_search data source
├── ytmusic-cli/
│   ├── Cargo.toml
│   └── src/main.rs            # Rust sidecar (JSON protocol)
├── examples/
│   └── main.tf                # Example Terraform config
├── Makefile
└── README.md
```

## License

MIT
