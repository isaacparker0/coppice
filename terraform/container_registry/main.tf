resource "digitalocean_container_registry" "primary" {
  name                   = "coppice"
  subscription_tier_slug = "basic"
}
