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

resource "ytmusic_playlist" "sasscore" {
  title       = "Genre: Sasscore"
  description = "Aggressive, chaotic hardcore with sass/gay/queer themes\n\n--\nGenerated via github.com/caos-obliquo/terraform-provider-ytmusic"
  privacy     = "unlisted"
}

# ── Outputs ───────────────────────────────────────────────────────────

output "sasscore_playlist_id" {
  description = "Feed this to genre-to-playlist --playlist-id"
  value = ytmusic_playlist.sasscore.playlist_id
}
