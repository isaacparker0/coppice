resource "digitalocean_app" "playground" {
  deployment_per_page = 20

  spec {
    name   = "coppice-playground"
    region = "nyc"

    alert {
      rule     = "DEPLOYMENT_FAILED"
      disabled = false
    }

    alert {
      rule     = "DOMAIN_FAILED"
      disabled = false
    }

    service {
      name               = "playground"
      instance_count     = 1
      instance_size_slug = "apps-s-1vcpu-0.5gb"
      http_port          = 8080

      image {
        registry_type = "DOCR"
        registry      = "coppice"
        repository    = "playground"
        tag           = "live"
      }
    }
  }
}
