# Example: two declarative agents + a manifest

terraform {
  required_providers {
    a2aagent = {
      source  = "registry.terraform.io/emillindfors/a2aagent"
      version = "~> 0.1"
    }
  }
}

provider "a2aagent" {
  output_dir = "${path.module}/generated"
  # Optional: point at an `a2a` binary for stricter validation:
  # a2a_bin = "${path.module}/../target/debug/a2a"
}

resource "a2aagent_agent" "echo" {
  name        = "echo-agent"
  description = "A minimal echo agent."
  handler_type = "echo"
  http_port   = 8080
}

resource "a2aagent_agent" "llm" {
  name         = "llm-agent"
  description  = "A config-driven LLM agent."
  handler_type = "llm"
  http_port    = 8081
  system_prompt = "You are a concise, helpful assistant."
  streaming    = true
}

resource "a2aagent_agent_set" "fleet" {
  name = "fleet"
  config_paths = [
    a2aagent_agent.echo.config_path,
    a2aagent_agent.llm.config_path,
  ]
}
