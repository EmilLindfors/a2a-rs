// Package provider implements the terraform-provider-a2aagent provider.
package provider

import (
	"context"
	"fmt"
	"os"
	"path/filepath"

	"github.com/hashicorp/terraform-plugin-framework/datasource"
	"github.com/hashicorp/terraform-plugin-framework/provider"
	"github.com/hashicorp/terraform-plugin-framework/provider/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/types"
)

// A2aagentProvider is the provider implementation.
type A2aagentProvider struct{
	version string
}

// A2aagentProviderModel maps provider configuration HCL.
type A2aagentProviderModel struct {
	// Directory to write rendered agent TOML files into. Defaults to ".".
	OutputDir types.String `tfsdk:"output_dir"`
	// Optional path to an `a2a` binary for schema/validation shelling-out.
	// When set, the provider validates configs by running `a2a validate`.
	A2aBin types.String `tfsdk:"a2a_bin"`
}

// Metadata returns provider metadata.
func (p *A2aagentProvider) Metadata(_ context.Context, _ provider.MetadataRequest, resp *provider.MetadataResponse) {
	resp.TypeName = "a2aagent"
	resp.Version = p.version
}

// Schema returns the provider schema.
func (p *A2aagentProvider) Schema(_ context.Context, _ provider.SchemaRequest, resp *provider.SchemaResponse) {
	resp.Schema = schema.Schema{
		Attributes: map[string]schema.Attribute{
			"output_dir": schema.StringAttribute{
				Optional:    true,
				Description: "Directory to write rendered agent TOML files into. Defaults to the current working directory.",
			},
			"a2a_bin": schema.StringAttribute{
				Optional:    true,
				Description: "Path to an `a2a` binary used for config validation via `a2a validate`. Optional; falls back to JSON Schema validation.",
			},
		},
	}
}

// Configure configures the provider.
func (p *A2aagentProvider) Configure(ctx context.Context, req provider.ConfigureRequest, resp *provider.ConfigureResponse) {
	var cfg A2aagentProviderModel
	diags := req.Config.Get(ctx, &cfg)
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}

	out := cfg.OutputDir.ValueString()
	if out == "" {
		wd, err := os.Getwd()
		if err != nil {
			resp.Diagnostics.AddError("failed to get working directory", err.Error())
			return
		}
		out = wd
	}
	abs, err := filepath.Abs(out)
	if err != nil {
		resp.Diagnostics.AddError("failed to resolve output_dir", err.Error())
		return
	}
	if err := os.MkdirAll(abs, 0o755); err != nil {
		resp.Diagnostics.AddError(fmt.Sprintf("failed to create output_dir %q", abs), err.Error())
		return
	}

	resp.DataSourceData = &providerData{outputDir: abs, a2aBin: cfg.A2aBin.ValueString()}
	resp.ResourceData = &providerData{outputDir: abs, a2aBin: cfg.A2aBin.ValueString()}
}

// Resources returns the provider's resources.
func (p *A2aagentProvider) Resources(_ context.Context) []func() resource.Resource {
	return []func() resource.Resource{
		NewAgentResource,
		NewAgentSetResource,
	}
}

// DataSources returns the provider's data sources.
func (p *A2aagentProvider) DataSources(_ context.Context) []func() datasource.Resource {
	return nil
}

type providerData struct {
	outputDir string
	a2aBin    string
}

// New creates a new provider instance.
func New(version string) provider.Provider {
	return &A2aagentProvider{version: version}
}
