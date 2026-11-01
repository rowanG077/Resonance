#!/usr/bin/env bash
# Run only after every build/test/package job has succeeded.
set -euo pipefail

: "${GH_REPO:?}" "${RELEASE_COMMIT:?}" "${VERSION:?}"
if [[ ! "$VERSION" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  echo 'Expected a version in the form vX.X.X.' >&2
  exit 1
fi

assets=(
  "target/dist/resonance-$VERSION-windows-x64.zip"
  "target/dist/resonance-$VERSION-macos-arm64.tar.gz"
  "target/dist/resonance-$VERSION-linux-x64.tar.gz"
  "target/dist/resonance-$VERSION-linux-arm64.tar.gz"
)
for asset in "${assets[@]}" docs/installation.md; do
  [[ -s "$asset" ]] || { echo "Missing release input: $asset" >&2; exit 1; }
done

# List refs first so authentication/network errors cannot look like a missing tag.
tag=$(gh api "repos/$GH_REPO/git/matching-refs/tags/$VERSION" \
  --jq ".[] | select(.ref == \"refs/tags/$VERSION\") | .ref")
if [[ -n "$tag" ]]; then
  commit=$(gh api "repos/$GH_REPO/commits/$VERSION" --jq .sha)
  if [[ "$commit" != "$RELEASE_COMMIT" ]]; then
    echo "Tag $VERSION already points to $commit; refusing to move it to $RELEASE_COMMIT. Use a new release version." >&2
    exit 1
  fi
else
  gh api --method POST "repos/$GH_REPO/git/refs" \
    -f "ref=refs/tags/$VERSION" -f "sha=$RELEASE_COMMIT"
fi

# Resume an interrupted draft, but never replace a published release's binaries.
draft=$(gh api --paginate "repos/$GH_REPO/releases?per_page=100" \
  --jq ".[] | select(.tag_name == \"$VERSION\") | .draft")
if [[ "$draft" == false ]]; then
  echo "Release $VERSION is already published at $RELEASE_COMMIT; nothing to do."
  exit 0
fi
if [[ -z "$draft" ]]; then
  gh release create "$VERSION" --verify-tag --draft \
    --title "Resonance $VERSION" --notes-file docs/installation.md
fi

cp docs/installation.md target/dist/README.md
(
  cd target/dist
  sha256sum "${assets[@]#target/dist/}" > SHA256SUMS
)
gh release upload "$VERSION" "${assets[@]}" target/dist/README.md target/dist/SHA256SUMS --clobber
gh release edit "$VERSION" --draft=false
