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

	var rawResults []interface{}
	if err := json.Unmarshal(*data, &rawResults); err == nil {
		d.Set("results", rawResults)
	} else {
		// Try single object
		var rawObj map[string]interface{}
		if err := json.Unmarshal(*data, &rawObj); err == nil {
			d.Set("results", []interface{}{rawObj})
		}
	}

	d.SetId(query)
	return nil
}
