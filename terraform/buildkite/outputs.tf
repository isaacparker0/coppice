output "ci_runner_agent_token" {
  value     = buildkite_cluster_agent_token.ci_runner.token
  sensitive = true
}
