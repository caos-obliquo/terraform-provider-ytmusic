package provider

import (
	"context"
	"encoding/json"

	"github.com/hashicorp/terraform-plugin-sdk/v2/diag"
	"github.com/hashicorp/terraform-plugin-sdk/v2/helper/schema"
)

func dataSourceSearch() *schema.Resource {
	return &schema.Resource{
		Description: "Search YouTube Music for songs, albums, artists, or playlists.",
		ReadContext: dataSourceSearchRead,
		Schema: map[string]*schema.Schema{
			"query": {
				Type:        schema.TypeString,
				Required:    true,
				Description: "Search query",
			},
			"type": {
				Type:        schema.TypeString,
				Optional:    true,
				Default:     "songs",
				Description: "Search type: songs, artists, albums, or playlists",
			},
			"results": {
				Type:        schema.TypeList,
				Computed:    true,
				Description: "Search results",
				Elem: &schema.Resource{
					Schema: map[string]*schema.Schema{
						"title": {
							Type:     schema.TypeString,
							Computed: true,
						},
						"artist": {
							Type:     schema.TypeString,
							Computed: true,
						},
						"video_id": {
							Type:     schema.TypeString,
							Computed: true,
						},
						"album": {
							Type:     schema.TypeString,
							Computed: true,
						},
					},
				},
			},
		},
	}
}

// albumObj matches the nested album object from ytmapi-rs search results.
type albumObj struct {
	Name string `json:"name"`
}

func dataSourceSearchRead(ctx context.Context, d *schema.ResourceData, m interface{}) diag.Diagnostics {
	client := NewYTMusicClient(m.(*ProviderConfig))
	query := d.Get("query").(string)
	searchType := d.Get("type").(string)

	data, err := client.Search(query, searchType)
	if err != nil {
		return diag.Errorf("search error: %s", err)
	}

	if data == nil {
		d.SetId(query)
		return nil
	}

	// Parse JSON array from sidecar response
	var raw []json.RawMessage
	if err := json.Unmarshal(*data, &raw); err != nil {
		// Try single object
		var single map[string]interface{}
		if err2 := json.Unmarshal(*data, &single); err2 != nil {
			return diag.Errorf("parse search results: %s", err)
		}
		raw = []json.RawMessage{*data}
	}

	results := make([]interface{}, 0, len(raw))
	for _, item := range raw {
		// Extract flat fields
		var flat struct {
			Title   string   `json:"title"`
			Artist  string   `json:"artist"`
			VideoID string   `json:"video_id"`
			Album   albumObj `json:"album"`
		}
		if err := json.Unmarshal(item, &flat); err != nil {
			continue
		}
		r := map[string]interface{}{
			"title":    flat.Title,
			"artist":   flat.Artist,
			"video_id": flat.VideoID,
			"album":    flat.Album.Name,
		}
		results = append(results, r)
	}

	d.Set("results", results)
	d.SetId(query)
	return nil
}
