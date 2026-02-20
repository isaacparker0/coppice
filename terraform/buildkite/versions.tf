terraform {
  required_version = ">= 1.6.3"

  required_providers {
    buildkite = {
      source  = "buildkite/buildkite"
      version = "~> 1.30"
    }
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "~> 2.76"
    }
    github = {
      source  = "integrations/github"
      version = "~> 6.11"
    }
    tls = {
      source  = "hashicorp/tls"
      version = "~> 4.2"
    }
  }
}
