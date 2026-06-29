package main

import (
	"context"
	"flag"
	"log"

	"github.com/caos-obliquo/terraform-provider-ytmusic/provider"
	"github.com/hashicorp/terraform-plugin-sdk/v2/plugin"
)

func main() {
	var debugMode bool
	flag.BoolVar(&debugMode, "debug", false, "set to true to run the provider with support for debuggers like delve")
	flag.Parse()

	opts := &plugin.ServeOpts{
		ProviderFunc: provider.Provider,
	}

	if debugMode {
		err := plugin.Debug(context.Background(), "registry.terraform.io/caos-obliquo/ytmusic", opts)
		if err != nil {
			log.Fatal(err.Error())
		}
		return
	}

	plugin.Serve(opts)
}
