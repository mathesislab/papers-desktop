#!/bin/sh
# Open the pull request "papers edit --from" on perrette/papers, from a fork
# of alex-robinson (the branch edit-from of the papers checkout).
set -e
cd "$(dirname "$0")"
gh repo fork perrette/papers --clone=false
sleep 5
git -C /Users/alrobi001/models/papers push git@github.com:alex-robinson/papers.git edit-from
gh pr create --repo perrette/papers --base v3 --head alex-robinson:edit-from \
  --title "papers edit --from FILE; a rejected edit exits 1" --body-file pr-edit-from.md
