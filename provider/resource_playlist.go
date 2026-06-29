package provider

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/hashicorp/terraform-plugin-sdk/v2/diag"
	"github.com/hashicorp/terraform-plugin-sdk/v2/helper/schema"
)

func resourcePlaylist() *schema.Resource {
	return &schema.Resource{
		Description: "Manage a YouTube Music playlist.",
		CreateContext: resourcePlaylistCreate,
		ReadContext:   resourcePlaylistRead,
		UpdateContext: resourcePlaylistUpdate,
		DeleteContext: resourcePlaylistDelete,
		Importer: &schema.ResourceImporter{
			StateContext: schema.ImportStatePassthroughContext,
		},
		Schema: map[string]*schema.Schema{
			"title": {
				Type:        schema.TypeString,
				Required:    true,
				Description: "Playlist title",
			},
			"description": {
				Type:        schema.TypeString,
				Optional:    true,
				Default:     "",
				Description: "Playlist description",
			},
			"privacy": {
				Type:        schema.TypeString,
				Optional:    true,
				Default:     "private",
				Description: "Privacy status: private, public, or unlisted",
				ValidateFunc: func(val interface{}, key string) (warns []string, errs []error) {
					v := val.(string)
					if v != "private" && v != "public" && v != "unlisted" {
						errs = append(errs, fmt.Errorf("%q must be private, public, or unlisted, got: %s", key, v))
					}
					return
				},
			},
			"video_ids": {
				Type:        schema.TypeList,
				Optional:    true,
				Elem:        &schema.Schema{Type: schema.TypeString},
				Description: "Video IDs to add to the playlist",
			},
			"playlist_id": {
				Type:        schema.TypeString,
				Computed:    true,
				Description: "YT Music playlist ID",
			},
		},
	}
}

func resourcePlaylistCreate(ctx context.Context, d *schema.ResourceData, m interface{}) diag.Diagnostics {
	client := NewYTMusicClient(m.(*ProviderConfig))
	title := d.Get("title").(string)
	desc := d.Get("description").(string)
	privacy := d.Get("privacy").(string)

	input := PlaylistCreateInput{
		Title:       title,
		Description: stringPtr(desc),
		Privacy:     stringPtr(privacy),
	}
	if desc == "" {
		input.Description = nil
	}

	out, err := client.CreatePlaylist(input)
	if err != nil {
		return diag.Errorf("error creating playlist: %s", err)
	}

	d.SetId(out.ID)
	d.Set("playlist_id", out.ID)

	// Add initial tracks if specified
	if v, ok := d.GetOk("video_ids"); ok {
		ids := toStringList(v.([]interface{}))
		if len(ids) > 0 {
			if err := client.AddItems(out.ID, ids); err != nil {
				return diag.Errorf("error adding items to playlist: %s", err)
			}
		}
	}

	return resourcePlaylistRead(ctx, d, m)
}

func resourcePlaylistRead(ctx context.Context, d *schema.ResourceData, m interface{}) diag.Diagnostics {
	client := NewYTMusicClient(m.(*ProviderConfig))
	id := d.Id()

	// Verify playlist exists by getting details
	data, err := client.GetPlaylist(id)
	if err != nil {
		d.SetId("")
		return diag.Diagnostics{
			diag.Diagnostic{
				Severity: diag.Warning,
				Summary:  "Playlist not found, removed from state",
				Detail:   err.Error(),
			},
		}
	}

	d.Set("playlist_id", id)

	// Try to extract title from response for drift detection
	if data != nil {
		var raw map[string]interface{}
		if json.Unmarshal(*data, &raw) == nil {
			if name, ok := raw["name"].(string); ok {
				d.Set("title", name)
			}
			if desc, ok := raw["description"].(string); ok {
				d.Set("description", desc)
			}
		}
	}

	return nil
}

func resourcePlaylistUpdate(ctx context.Context, d *schema.ResourceData, m interface{}) diag.Diagnostics {
	client := NewYTMusicClient(m.(*ProviderConfig))
	id := d.Id()

	if d.HasChanges("title", "description", "privacy") {
		var title, desc, privacy *string
		if d.HasChange("title") {
			t := d.Get("title").(string)
			title = &t
		}
		if d.HasChange("description") {
			d := d.Get("description").(string)
			desc = &d
		}
		if d.HasChange("privacy") {
			p := d.Get("privacy").(string)
			privacy = &p
		}
		if err := client.EditPlaylist(id, title, desc, privacy); err != nil {
			return diag.Errorf("error updating playlist: %s", err)
		}
	}

	if d.HasChange("video_ids") {
		old, new := d.GetChange("video_ids")
		oldIDs := toStringList(old.([]interface{}))
		newIDs := toStringList(new.([]interface{}))

		// Add new IDs (simplified: just replace by removing all + adding new)
		// Real impl would diff old vs new for efficiency
		if len(oldIDs) > 0 {
			client.RemoveItems(id, oldIDs)
		}
		if len(newIDs) > 0 {
			if err := client.AddItems(id, newIDs); err != nil {
				return diag.Errorf("error adding items: %s", err)
			}
		}
	}

	return resourcePlaylistRead(ctx, d, m)
}

func resourcePlaylistDelete(ctx context.Context, d *schema.ResourceData, m interface{}) diag.Diagnostics {
	client := NewYTMusicClient(m.(*ProviderConfig))
	id := d.Id()

	if err := client.DeletePlaylist(id); err != nil {
		return diag.Errorf("error deleting playlist: %s", err)
	}

	d.SetId("")
	return nil
}

// ── Helpers ──────────────────────────────────────────────────────────

func stringPtr(s string) *string {
	if s == "" {
		return nil
	}
	return &s
}

func toStringList(list []interface{}) []string {
	out := make([]string, len(list))
	for i, v := range list {
		if s, ok := v.(string); ok {
			out[i] = s
		}
	}
	return out
}
