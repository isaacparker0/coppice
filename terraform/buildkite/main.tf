variable "buildbuddy_api_key" {
  type      = string
  sensitive = true
}

variable "docr_push_token" {
  type      = string
  sensitive = true
}

resource "buildkite_team" "default" {
  name                          = "Everyone"
  privacy                       = "VISIBLE"
  default_team                  = true
  members_can_create_pipelines  = true
  members_can_create_registries = true
  members_can_create_suites     = true
  default_member_role           = "MEMBER"
}

resource "buildkite_cluster" "default" {
  name = "Default cluster"
}

resource "buildkite_cluster_queue" "default" {
  cluster_id = buildkite_cluster.default.id
  key        = "default"
}

resource "buildkite_cluster_agent_token" "ci_runner" {
  cluster_id  = buildkite_cluster.default.id
  description = "coppice-ci-runner-01"
}

resource "buildkite_cluster_secret" "buildbuddy_api_key" {
  cluster_id = buildkite_cluster.default.uuid
  key        = "BUILDBUDDY_API_KEY"
  value      = var.buildbuddy_api_key
  policy     = <<-EOT
    - pipeline_slug: ci
    - pipeline_slug: playground-deploy
      build_branch: main
  EOT
}

resource "buildkite_cluster_secret" "docr_push_token" {
  cluster_id = buildkite_cluster.default.uuid
  key        = "DOCR_PUSH_TOKEN"
  value      = var.docr_push_token
  policy     = <<-EOT
    - pipeline_slug: playground-deploy
      build_branch: main
  EOT
}

resource "buildkite_pipeline" "ci" {
  name       = "ci"
  repository = "git@github.com:isaacparker0/coppice.git"

  cluster_id      = buildkite_cluster.default.id
  default_team_id = buildkite_team.default.id

  branch_configuration = "main"

  skip_intermediate_builds                 = true
  skip_intermediate_builds_branch_filter   = "*"
  cancel_intermediate_builds               = true
  cancel_intermediate_builds_branch_filter = "!main"

  steps = file("${path.module}/../../.buildkite/pipelines/ci.yaml")

  provider_settings = {
    trigger_mode                   = "code"
    build_branches                 = true
    build_pull_requests            = true
    publish_commit_status          = true
    publish_commit_status_per_step = false
  }
}

resource "github_repository_webhook" "buildkite_ci" {
  repository = "coppice"
  active     = true
  events = [
    "pull_request",
    "push",
  ]

  configuration {
    url          = buildkite_pipeline.ci.webhook_url
    content_type = "json"
    insecure_ssl = "0"
  }
}

resource "buildkite_pipeline" "playground_deploy" {
  name       = "playground-deploy"
  repository = "git@github.com:isaacparker0/coppice.git"

  cluster_id      = buildkite_cluster.default.id
  default_team_id = buildkite_team.default.id

  branch_configuration = "main"

  skip_intermediate_builds               = true
  skip_intermediate_builds_branch_filter = "main"

  steps = file("${path.module}/../../.buildkite/pipelines/playground_deploy.yaml")
}

resource "tls_private_key" "buildkite_checkout" {
  algorithm = "ED25519"
}

resource "github_repository_deploy_key" "buildkite_checkout" {
  repository = "coppice"
  title      = "buildkite-checkout"
  key        = tls_private_key.buildkite_checkout.public_key_openssh
  read_only  = true
}

resource "digitalocean_droplet" "buildkite_runner" {
  name   = "coppice-ci-runner-01"
  region = "nyc3"
  size   = "s-4vcpu-8gb"
  image  = "ubuntu-24-04-x64"

  user_data = templatefile("${path.module}/buildkite_runner_setup.sh.tftpl", {
    buildkite_agent_token           = buildkite_cluster_agent_token.ci_runner.token
    github_checkout_private_key_pem = tls_private_key.buildkite_checkout.private_key_openssh
  })
}
