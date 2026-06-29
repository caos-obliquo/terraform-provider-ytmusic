terraform {
  required_providers {
    ytmusic = {
      source = "caos-obliquo/ytmusic"
    }
  }
}

provider "ytmusic" {
  cookie_file = "/home/caos/.config/youtui/cookies.txt"
}

# ── Playlist resources ────────────────────────────────────────────────
# Note: video_ids requires known values at plan time.
# Search results are computed (known after apply), so they can't feed
# directly into resource.video_ids in the same plan.
# To populate a playlist from search results:
#   1. `terraform apply` → outputs search result video_ids
#   2. Copy desired IDs into video_ids below
#   3. `terraform apply` again

resource "ytmusic_playlist" "black_metal" {
  title       = "Genre: Black Metal"
  description = "Auto-generated from genre pipeline"
  privacy     = "private"
  # video_ids = ["VIDEO_ID_1", "VIDEO_ID_2"]
}

resource "ytmusic_playlist" "ambient" {
  title       = "Genre: Ambient"
  description = "Auto-generated from genre pipeline"
  privacy     = "private"
}

# ── Data sources ──────────────────────────────────────────────────────

data "ytmusic_search" "black_metal_songs" {
  query = "black metal"
  type  = "songs"
}

data "ytmusic_search" "ambient_songs" {
  query = "ambient"
  type  = "songs"
}

# ── Outputs ───────────────────────────────────────────────────────────

output "black_metal_playlist_id" {
  value = ytmusic_playlist.black_metal.playlist_id
}

output "black_metal_search_results" {
  description = "Search results for black metal songs (copy video_ids to populate playlist)"
  value = data.ytmusic_search.black_metal_songs.results[*].video_id
}

output "ambient_search_results" {
  description = "Search results for ambient songs"
  value = data.ytmusic_search.ambient_songs.results[*].video_id
}
