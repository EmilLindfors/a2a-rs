// Package main is the entrypoint for the terraform-provider-a2aagent plugin.
//
// terraform-provider-a2aagent is a config-as-artifact provider: it is the
// source of truth for agent definitions and emits TOML files that the `a2a`
// binary (from the a2a-agents crate) consumes. It does not provision runtime
// infrastructure — infra deployment stays in the user's existing tooling.
package main

import (
	"context"
	"flag"
	"log"

	"github.com/emillindfors/terraform-provider-a2aagent/internal/provider"
	"github.com/hashicorp/terraform-plugin-framework/providerserver"
)

var (
	// these are set at build time with -ldflags "-X main.version=..." etc.
	version string = "dev"
)

func main() {
	var debug bool
	flag.BoolVar(&debug, "debug", false, "set to true to run the provider with support for debuggers")
	flag.Parse()

	opts := providerserver.ServeOpts{
		Address: "registry.terraform.io/emillindfors/a2aagent",
		Debug:   debug,
	}

	err := providerserver.Serve(context.Background(), provider.New(version), opts)
	if err != nil {
		log.Fatal(err.Error())
	}
}
