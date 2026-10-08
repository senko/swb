#!/usr/bin/env bash
# Prepares the persistent review worktree: a git worktree next to the main
# repository with its own target directory, so that reviews and
# verifications build incrementally and never share build output with
# another tree (docs/testing.md, "Review worktree").
#
# Usage:
#   tools/review-tree.sh path            print the path of the worktree
#   tools/review-tree.sh reset [COMMIT]  clean tree at COMMIT (default: HEAD
#                                        of the main repository)
#   tools/review-tree.sh apply PATCH     reset to HEAD, then apply PATCH
#   tools/review-tree.sh staged          reset to HEAD, then apply the
#                                        staged diff of the main repository
#
# The worktree is $SWB_REVIEW_TREE, default ../swb-review next to the main
# repository. The script creates it when it does not exist, and marks it
# (a file `swb-review-tree` in its git directory). It changes only a marked
# worktree, so that a wrong $SWB_REVIEW_TREE cannot discard the work in
# another tree. `reset` removes untracked files but keeps ignored ones
# (target/, out/, tools/.venv), so the next build is incremental. Build in
# the worktree with `env -u CARGO_TARGET_DIR just check`.
set -euo pipefail

main=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
tree=${SWB_REVIEW_TREE:-$(dirname "$main")/swb-review}
if [ "$(realpath -m "$tree")" = "$(realpath "$main")" ]; then
  echo "review-tree.sh: the review worktree must not be the main repository" >&2
  exit 2
fi

ensure() {
  git -C "$main" worktree prune
  if [ ! -e "$tree" ]; then
    git -C "$main" worktree add --detach "$tree" HEAD >&2
    touch "$(git -C "$tree" rev-parse --absolute-git-dir)/swb-review-tree"
  fi
  if ! git -C "$main" worktree list --porcelain | grep -qxF "worktree $(realpath "$tree")" ||
    [ ! -e "$(git -C "$tree" rev-parse --absolute-git-dir)/swb-review-tree" ]; then
    echo "review-tree.sh: $tree is not the review worktree (no swb-review-tree mark)" >&2
    exit 2
  fi
}

reset() { # COMMIT
  ensure
  git -C "$tree" checkout --quiet --detach "$1"
  git -C "$tree" reset --quiet --hard "$1"
  git -C "$tree" clean --quiet -fd
}

head=$(git -C "$main" rev-parse HEAD)
case ${1:-} in
path)
  echo "$tree"
  ;;
reset)
  reset "$(git -C "$main" rev-parse "${2:-HEAD}")"
  echo "$tree at $(git -C "$tree" rev-parse --short HEAD)"
  ;;
apply)
  patch=$(realpath "${2:?usage: tools/review-tree.sh apply PATCH}")
  reset "$head"
  git -C "$tree" apply --index "$patch"
  echo "$tree at $(git -C "$tree" rev-parse --short HEAD) + $patch"
  ;;
staged)
  reset "$head"
  git -C "$main" diff --cached --binary | git -C "$tree" apply --index
  echo "$tree at $(git -C "$tree" rev-parse --short HEAD) + staged diff"
  ;;
*)
  sed -n '/^# Usage:/,/^# The worktree/p' "$0" | sed '$d; s/^# \{0,1\}//' >&2
  exit 2
  ;;
esac
