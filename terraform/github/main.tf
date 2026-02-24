resource "github_repository" "coppice" {
  name       = "coppice"
  visibility = "public"

  allow_merge_commit          = false
  allow_rebase_merge          = false
  delete_branch_on_merge      = true
  merge_commit_message        = "BLANK"
  merge_commit_title          = "PR_TITLE"
  squash_merge_commit_message = "BLANK"
  squash_merge_commit_title   = "PR_TITLE"
}
