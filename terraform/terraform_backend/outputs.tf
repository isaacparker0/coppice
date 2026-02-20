output "bucket_name" {
  value = digitalocean_spaces_bucket.state.name
}

output "endpoint" {
  value = "https://${local.region}.digitaloceanspaces.com"
}
