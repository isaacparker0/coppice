terraform {
  required_version = ">= 1.6.3"

  required_providers {
    buildkite = {
      source  = "buildkite/buildkite"
      version = "~> 1.30"
    }
  }
}
