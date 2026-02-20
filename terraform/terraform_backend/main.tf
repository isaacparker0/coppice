locals {
  bucket_name = "coppice-opentofu-state"
  region      = "nyc3"
  state_key   = "terraform_backend/terraform.tfstate"
}

resource "digitalocean_spaces_bucket" "state" {
  name   = local.bucket_name
  region = local.region
  acl    = "private"

  force_destroy = false

  versioning {
    enabled = true
  }

  lifecycle {
    prevent_destroy = true
  }
}
