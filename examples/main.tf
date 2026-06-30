terraform {
  required_providers {
    ytmusic = {
      source = "caos-obliquo/ytmusic"
    }
  }
}

provider "ytmusic" {
  cookie_file = "/home/caos/.config/youtui/cookies.txt"
  cli_path    = "/home/caos/.terraform.d/plugins/registry.terraform.io/caos-obliquo/ytmusic/0.1.0/linux_amd64/ytmusic-cli"
}

# ── Workflow ──────────────────────────────────────────────────────────
# 1. terraform apply                  → creates empty playlist, outputs ID
# 2. genre-to-playlist --playlist-id <ID> --genre sasscore   → populates it
#
# Terraform owns the playlist lifecycle (create/destroy/import).
# genre-to-playlist CLI owns the song population (search + add).

# Terraform creates the playlist. genre-to-playlist populates it.
# Once created, lifecycle prevents accidental modification.

resource "ytmusic_playlist" "goregrind" {
  title       = "Genre: Goregrind"
  description = "Extreme grindcore with gore-themed lyrics\n\n--\nGenerated via github.com/caos-obliquo/terraform-provider-ytmusic"
  privacy     = "unlisted"

  lifecycle {
    ignore_changes = all
  }
}

resource "ytmusic_playlist" "skramz" {
  title       = "Genre: Skramz"
  description = "First-wave screamo (2273 entries)"
  privacy     = "unlisted"

  lifecycle {
    ignore_changes = all
  }
}

# ── Outputs ───────────────────────────────────────────────────────────

output "goregrind_playlist_id" {
  description = "Feed this to genre-to-playlist --playlist-id"
  value = ytmusic_playlist.goregrind.playlist_id
}

output "skramz_playlist_id" {
  description = "Feed this to genre-to-playlist --playlist-id"
  value = ytmusic_playlist.skramz.playlist_id
}
