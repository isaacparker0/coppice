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

resource "buildkite_pipeline" "ci" {
  name            = "ci"
  repository      = "git@github.com:isaacparker0/coppice.git"
  cluster_id      = buildkite_cluster.default.id
  default_team_id = buildkite_team.default.id
  steps           = file("${path.module}/../../.buildkite/pipelines/ci.yaml")
}
