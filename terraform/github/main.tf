resource "github_repository" "coppice" {
  name       = "coppice"
  visibility = "private"

  allow_merge_commit          = false
  allow_rebase_merge          = false
  squash_merge_commit_message = "BLANK"
  squash_merge_commit_title   = "PR_TITLE"
}
