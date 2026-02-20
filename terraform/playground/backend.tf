terraform {
  backend "s3" {
    bucket       = "coppice-opentofu-state"
    key          = "playground/terraform.tfstate"
    region       = "us-east-1"
    use_lockfile = true

    endpoint                    = "https://nyc3.digitaloceanspaces.com"
    skip_region_validation      = true
    skip_credentials_validation = true
    skip_requesting_account_id  = true
    skip_s3_checksum            = true
  }
}
