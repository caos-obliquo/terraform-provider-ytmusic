package provider

import (
	"context"

	"github.com/hashicorp/terraform-plugin-sdk/v2/diag"
	"github.com/hashicorp/terraform-plugin-sdk/v2/helper/schema"
)

func Provider() *schema.Provider {
	return &schema.Provider{
		Schema: map[string]*schema.Schema{
			"cookie_file": {
				Type:        schema.TypeString,
				Required:    true,
				Description: "Path to YouTube Music cookies.txt file",
				DefaultFunc: schema.EnvDefaultFunc("YTMAPI_COOKIE", nil),
			},
			"cli_path": {
				Type:        schema.TypeString,
				Optional:    true,
				Description: "Path to ytmusic-cli binary (default: ytmusic-cli on PATH)",
				DefaultFunc: schema.EnvDefaultFunc("YTMUSIC_CLI_PATH", "ytmusic-cli"),
			},
		},
		ResourcesMap: map[string]*schema.Resource{
			"ytmusic_playlist": resourcePlaylist(),
		},
		DataSourcesMap: map[string]*schema.Resource{
			"ytmusic_search": dataSourceSearch(),
		},
		ConfigureContextFunc: providerConfigure,
	}
}

type ProviderConfig struct {
	CookieFile string
	CLIPath    string
}

func providerConfigure(ctx context.Context, d *schema.ResourceData) (interface{}, diag.Diagnostics) {
	config := &ProviderConfig{
		CookieFile: d.Get("cookie_file").(string),
		CLIPath:    d.Get("cli_path").(string),
	}

	// Validate: try auth check
	client := NewYTMusicClient(config)
	if err := client.AuthCheck(); err != nil {
		return config, diag.Diagnostics{
			diag.Diagnostic{
				Severity: diag.Warning,
				Summary:  "YT Music auth check failed",
				Detail:   err.Error(),
			},
		}
	}

	return config, nil
}
