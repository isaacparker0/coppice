terraform {
  required_version = ">= 1.6.0"

  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "~> 2.76"
    }
  }
}

provider "digitalocean" {
  token = var.digitalocean_token
}

variable "digitalocean_token" {
  type      = string
  sensitive = true
}

variable "digitalocean_registry_read_token" {
  type      = string
  sensitive = true
}

variable "ssh_key_fingerprint" {
  type = string
}

variable "ssh_allow_cidr" {
  type = string
}

locals {
  name             = "coppice-playground"
  region           = "nyc1"
  size             = "s-1vcpu-2gb"
  image_repository = "registry.digitalocean.com/coppice/playground"
  nginx_config     = file("${path.module}/nginx.conf")
  ubuntu_image     = "ubuntu-24-04-x64"
  firewall_name    = "coppice-playground-firewall"
}

resource "digitalocean_droplet" "playground" {
  name     = local.name
  region   = local.region
  size     = local.size
  image    = local.ubuntu_image
  ssh_keys = [var.ssh_key_fingerprint]

  user_data = templatefile("${path.module}/user_data.sh.tftpl", {
    image_repository = local.image_repository
    registry_token   = var.digitalocean_registry_read_token
    nginx_config     = local.nginx_config
  })
}

resource "digitalocean_firewall" "playground" {
  name        = local.firewall_name
  droplet_ids = [digitalocean_droplet.playground.id]

  inbound_rule {
    protocol         = "tcp"
    port_range       = "22"
    source_addresses = [var.ssh_allow_cidr]
  }

  inbound_rule {
    protocol         = "tcp"
    port_range       = "80"
    source_addresses = ["0.0.0.0/0", "::/0"]
  }

  inbound_rule {
    protocol         = "tcp"
    port_range       = "443"
    source_addresses = ["0.0.0.0/0", "::/0"]
  }

  outbound_rule {
    protocol              = "tcp"
    port_range            = "1-65535"
    destination_addresses = ["0.0.0.0/0", "::/0"]
  }

  outbound_rule {
    protocol              = "udp"
    port_range            = "1-65535"
    destination_addresses = ["0.0.0.0/0", "::/0"]
  }

  outbound_rule {
    protocol              = "icmp"
    destination_addresses = ["0.0.0.0/0", "::/0"]
  }
}

output "droplet_ipv4" {
  value = digitalocean_droplet.playground.ipv4_address
}
