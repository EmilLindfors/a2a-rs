//go:build tools
// +build tools

// Package tools tracks build-time tool dependencies for `go generate`.
package tools

import (
	_ "github.com/hashicorp/terraform-plugin-framework"
)
