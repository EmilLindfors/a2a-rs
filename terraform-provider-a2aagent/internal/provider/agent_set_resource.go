package provider

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/hashicorp/terraform-plugin-framework/resource"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/listplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/planmodifier"
	"github.com/hashicorp/terraform-plugin-framework/resource/schema/stringplanmodifier"
	"github.com/hashicorp/terraform-plugin-framework/types"
)

// agentSetResource groups multiple agents and emits a manifest listing their
// TOML config paths. The `a2a` binary accepts repeated `--config` args to run
// many agents in one process (see a2a-agents/bin/a2a.rs).
type agentSetResource struct{
	pd *providerData
}

func NewAgentSetResource() resource.Resource { return &agentSetResource{} }

func (r *agentSetResource) Metadata(_ context.Context, _ resource.MetadataRequest, resp *resource.MetadataResponse) {
	resp.TypeName = "a2aagent_agent_set"
}

func (r *agentSetResource) Schema(_ context.Context, _ resource.SchemaRequest, resp *resource.SchemaResponse) {
	resp.Schema = schema.Schema{
		Description: "Groups multiple agents and emits a manifest file listing their config paths.",
		Attributes: map[string]schema.Attribute{
			"id": schema.StringAttribute{
				Computed: true,
				PlanModifiers: []planmodifier.String{stringplanmodifier.UseStateForUnknown()},
			},
			"name": schema.StringAttribute{Required: true},
			"config_paths": schema.ListAttribute{
				ElementType: types.StringType,
				Required:    true,
				PlanModifiers: []planmodifier.List{listplanmodifier.RequiresReplace()},
			},
			"manifest_path": schema.StringAttribute{
				Computed: true,
				PlanModifiers: []planmodifier.String{stringplanmodifier.UseStateForUnknown()},
			},
		},
	}
}

type agentSetModel struct {
	ID           types.String `tfsdk:"id"`
	Name         types.String `tfsdk:"name"`
	ConfigPaths  types.List   `tfsdk:"config_paths"`
	ManifestPath types.String `tfsdk:"manifest_path"`
}

func (r *agentSetResource) Create(ctx context.Context, req resource.CreateRequest, resp *resource.CreateResponse) {
	var plan agentSetModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	manifest := filepath.Join(r.pd.outputDir, fmt.Sprintf("%s.manifest.txt", slug(plan.Name.ValueString())))
	var lines []string
	for _, p := range plan.ConfigPaths.Elements() {
		lines = append(lines, p.String())
	}
	if err := os.WriteFile(manifest, []byte(strings.Join(lines, "\n")), 0o644); err != nil {
		resp.Diagnostics.AddError("failed to write manifest", err.Error())
		return
	}
	plan.ManifestPath = types.StringValue(manifest)
	plan.ID = types.StringValue(manifest)
	resp.Diagnostics.Append(resp.State.Set(ctx, plan)...)
}

func (r *agentSetResource) Read(ctx context.Context, req resource.ReadRequest, resp *resource.ReadResponse) {
	var state agentSetModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	if state.ManifestPath.IsNull() {
		return
	}
	if _, err := os.Stat(state.ManifestPath.ValueString()); err != nil {
		resp.State.RemoveResource(ctx)
	}
}

func (r *agentSetResource) Update(ctx context.Context, req resource.UpdateRequest, resp *resource.UpdateResponse) {
	var plan agentSetModel
	resp.Diagnostics.Append(req.Plan.Get(ctx, &plan)...)
	if resp.Diagnostics.HasError() {
		return
	}
	manifest := filepath.Join(r.pd.outputDir, fmt.Sprintf("%s.manifest.txt", slug(plan.Name.ValueString())))
	var lines []string
	for _, p := range plan.ConfigPaths.Elements() {
		lines = append(lines, p.String())
	}
	if err := os.WriteFile(manifest, []byte(strings.Join(lines, "\n")), 0o644); err != nil {
		resp.Diagnostics.AddError("failed to write manifest", err.Error())
		return
	}
	plan.ManifestPath = types.StringValue(manifest)
	plan.ID = types.StringValue(manifest)
	resp.Diagnostics.Append(resp.State.Set(ctx, plan)...)
}

func (r *agentSetResource) Delete(ctx context.Context, req resource.DeleteRequest, resp *resource.DeleteResponse) {
	var state agentSetModel
	resp.Diagnostics.Append(req.State.Get(ctx, &state)...)
	if resp.Diagnostics.HasError() {
		return
	}
	if !state.ManifestPath.IsNull() {
		_ = os.Remove(state.ManifestPath.ValueString())
	}
}

func (r *agentSetResource) Configure(_ context.Context, req resource.ConfigureRequest, resp *resource.ConfigureResponse) {
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
