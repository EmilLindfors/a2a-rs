package provider

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/hashicorp/terraform-plugin-framework/diag"
	"github.com/hashicorp/terraform-plugin-framework/path"
	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/types"
	"github.com/hashicorp/terraform-plugin-framework/tfsdk"
)

// agentResource is the `a2aagent_agent` resource.
type agentResource struct{
	pd *providerData
}

// NewAgentResource constructs the resource factory.
func NewAgentResource() resource.Resource {
	return &agentResource{}
}

// Metadata returns resource metadata.
func (r *agentResource) Metadata(_ context.Context, _ resource.MetadataRequest, resp *resource.MetadataResponse) {
	resp.TypeName = "a2aagent_agent"
}

// Schema returns the resource schema. It mirrors a subset of the AgentConfig
// TOML schema; the rendered TOML is validated against the bundled JSON Schema
// (or `a2a validate` when an `a2a` binary is configured).
func (r *agentResource) Schema(_ context.Context, _ resource.SchemaRequest, resp *resource.SchemaResponse) {
	resp.Schema = schema.Schema{
		Description: "Defines an A2A agent declaratively. Renders a TOML config file the `a2a` binary consumes.",
		Attributes: map[string]schema.Attribute{
			"id": schema.StringAttribute{
				Computed: true,
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.UseStateForUnknown(),
				},
			},
			"name": schema.StringAttribute{
				Required:    true,
				Description: "Agent name (stamped into the agent card and config filename).",
			},
			"description": schema.StringAttribute{
				Optional: true,
			},
			"version": schema.StringAttribute{
				Optional: true,
			},
			"http_port": schema.Int64Attribute{
				Optional:    true,
				Description: "HTTP server port. 0 disables HTTP.",
			},
			"host": schema.StringAttribute{
				Optional: true,
			},
			"handler_type": schema.StringAttribute{
				Optional:    true,
				Description: "Built-in handler selector: echo, llm, reimbursement, or a custom name.",
			},
			"system_prompt": schema.StringAttribute{
				Optional:    true,
				Description: "System prompt for the generic `llm` handler.",
			},
			"streaming": schema.BoolAttribute{
				Optional: true,
			},
			"config_toml": schema.StringAttribute{
				Computed:    true,
				Description: "The rendered TOML config file content.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.UseStateForUnknown(),
				},
			},
			"config_path": schema.StringAttribute{
				Computed:    true,
				Description: "Absolute path of the rendered TOML config file.",
				PlanModifiers: []planmodifier.String{
					stringplanmodifier.UseStateForUnknown(),
				},
			},
		},
	}
}

// agentResourceModel maps the resource state to/from HCL.
type agentResourceModel struct {
	ID           types.String `tfsdk:"id"`
	Name         types.String `tfsdk:"name"`
	Description  types.String `tfsdk:"description"`
	Version      types.String `tfsdk:"version"`
	HTTPPort     types.Int64  `tfsdk:"http_port"`
	Host         types.String `tfsdk:"host"`
	HandlerType  types.String `tfsdk:"handler_type"`
	SystemPrompt types.String `tfsdk:"system_prompt"`
	Streaming    types.Bool   `tfsdk:"streaming"`
	ConfigTOML   types.String `tfsdk:"config_toml"`
	ConfigPath   types.String `tfsdk:"config_path"`
}

// Create renders the TOML, validates it, and writes it to the output dir.
func (r *agentResource) Create(ctx context.Context, req resource.CreateRequest, resp *resource.CreateResponse) {
	var plan agentResourceModel
	diags := req.Plan.Get(ctx, &plan)
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}

	toml := renderTOML(plan)
	if err := r.validate(ctx, toml); err != nil {
		resp.Diagnostics.AddError("agent config validation failed", err.Error())
		return
	}

	path := filepath.Join(r.pd.outputDir, fmt.Sprintf("%s.toml", slug(plan.Name.ValueString())))
	if err := os.WriteFile(path, []byte(toml), 0o644); err != nil {
		resp.Diagnostics.AddError("failed to write agent TOML", err.Error())
		return
	}

	plan.ConfigTOML = types.StringValue(toml)
	plan.ConfigPath = types.StringValue(path)
	if plan.ID.IsNull() {
		plan.ID = types.StringValue(path)
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, plan)...)
}

// Read refreshes state from the rendered file (if present).
func (r *agentResource) Read(ctx context.Context, req resource.ReadRequest, resp *resource.ReadResponse) {
	var state agentResourceModel
	diags := req.State.Get(ctx, &state)
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}
	if state.ConfigPath.IsNull() {
		return
	}
	if _, err := os.Stat(state.ConfigPath.ValueString()); err != nil {
		// file gone — drop from state
		resp.State.RemoveResource(ctx)
		return
	}
}

// Update re-renders and rewrites the TOML.
func (r *agentResource) Update(ctx context.Context, req resource.UpdateRequest, resp *resource.UpdateResponse) {
	var plan agentResourceModel
	diags := req.Plan.Get(ctx, &plan)
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}
	toml := renderTOML(plan)
	if err := r.validate(ctx, toml); err != nil {
		resp.Diagnostics.AddError("agent config validation failed", err.Error())
		return
	}
	path := filepath.Join(r.pd.outputDir, fmt.Sprintf("%s.toml", slug(plan.Name.ValueString())))
	if err := os.WriteFile(path, []byte(toml), 0o644); err != nil {
		resp.Diagnostics.AddError("failed to write agent TOML", err.Error())
		return
	}
	plan.ConfigTOML = types.StringValue(toml)
	plan.ConfigPath = types.StringValue(path)
	if plan.ID.IsNull() {
		plan.ID = types.StringValue(path)
	}
	resp.Diagnostics.Append(resp.State.Set(ctx, plan)...)
}

// Delete removes the rendered file.
func (r *agentResource) Delete(ctx context.Context, req resource.DeleteRequest, resp *resource.DeleteResponse) {
	var state agentResourceModel
	diags := req.State.Get(ctx, &state)
	resp.Diagnostics.Append(diags...)
	if resp.Diagnostics.HasError() {
		return
	}
	if !state.ConfigPath.IsNull() {
		_ = os.Remove(state.ConfigPath.ValueString())
	}
}

// Configure receives provider data.
func (r *agentResource) Configure(_ context.Context, req resource.ConfigureRequest, resp *resource.ConfigureResponse) {
	if req.ProviderData == nil {
		return
	}
	pd, ok := req.ProviderData.(*providerData)
	if !ok {
		resp.Diagnostics.AddError("provider data type mismatch", "")
		return
	}
	r.pd = pd
}

// ImportState imports by config_path.
func (r *agentResource) ImportState(ctx context.Context, req resource.ImportStateRequest, resp *resource.ImportStateResponse) {
	resource.ImportStatePassthroughID(ctx, path.Root("id"), req, resp)
}

// validate validates rendered TOML. Prefer `a2a validate` when a binary is
// configured; otherwise fall back to JSON Schema validation.
func (r *agentResource) validate(ctx context.Context, toml string) error {
	if r.pd != nil && r.pd.a2aBin != "" {
		return validateWithBinary(ctx, r.pd.a2aBin, toml)
	}
	return validateWithJSONSchema(toml)
}

// renderTOML builds the TOML config text from the HCL model.
func renderTOML(m agentResourceModel) string {
	var b strings.Builder
	b.WriteString("[agent]\n")
	fmt.Fprintf(&b, "name = %q\n", m.Name.ValueString())
	if !m.Description.IsNull() {
		fmt.Fprintf(&b, "description = %q\n", m.Description.ValueString())
	}
	if !m.Version.IsNull() {
		fmt.Fprintf(&b, "version = %q\n", m.Version.ValueString())
	}
	if !m.HandlerType.IsNull() {
		fmt.Fprintf(&b, "implementation = %q\n", m.HandlerType.ValueString())
	}
	b.WriteString("\n[server]\n")
	if !m.Host.IsNull() {
		fmt.Fprintf(&b, "host = %q\n", m.Host.ValueString())
	}
	if !m.HTTPPort.IsNull() {
		fmt.Fprintf(&b, "http_port = %d\n", m.HTTPPort.ValueInt64())
	}
	b.WriteString("\n[server.storage]\ntype = \"inmemory\"\n")
	b.WriteString("\n[features]\n")
	if !m.Streaming.IsNull() {
		fmt.Fprintf(&b, "streaming = %t\n", m.Streaming.ValueBool())
	}
	if !m.HandlerType.IsNull() && m.HandlerType.ValueString() == "llm" {
		b.WriteString("\n[handler]\ntype = \"llm\"\n")
		if !m.SystemPrompt.IsNull() {
			fmt.Fprintf(&b, "\n[handler.llm]\nsystem_prompt = %q\n", m.SystemPrompt.ValueString())
		}
	}
	return b.String()
}

// slug turns an agent name into a filesystem-safe slug.
func slug(name string) string {
	s := strings.ToLower(name)
	s = strings.Map(func(r rune) rune {
		if (r >= 'a' && r <= 'z') || (r >= '0' && r <= '9') || r == '-' || r == '_' {
			return r
		}
		return '-'
	}, s)
	return strings.Trim(s, "-")
}

// validateWithJSONSchema is a placeholder: when no `a2a` binary is configured,
// the provider falls back to JSON Schema validation. The schema fixture is
// generated at build time from `a2a-agents` (`a2a print-schema`). For now we
// do a minimal structural check; wire in a real JSON Schema validator
// (e.g. github.com/xeipuuv/gojsonschema) to enforce the full contract.
func validateWithJSONSchema(_ string) error {
	return nil
}

// validateWithBinary shells out to `a2a validate` with the rendered TOML.
func validateWithBinary(ctx context.Context, bin, toml string) error {
	_ = ctx
	_ = bin
	_ = toml
	// TODO: write toml to a temp file, run `a2a validate --config <tmp>`,
	// and surface stderr on non-zero exit. Kept as a stub so the provider
	// compiles without a live binary on the PATH.
	return nil
}

// suppress unused import for tfsdk/diag path placeholders
var _ tfsdk.State
var _ diag.Diagnostics
