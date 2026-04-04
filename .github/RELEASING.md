# Releasing

Releases are fully automated. To ship a release, add one of these labels to a PR before merging:

| Label | Example |
|---|---|
| `release:patch` | `0.1.8` -> `0.1.9` (bug fixes) |
| `release:minor` | `0.1.8` -> `0.2.0` (new features) |
| `release:major` | `0.1.8` -> `1.0.0` (breaking changes) |

When the PR is merged, the `auto-release` workflow will:
1. Bump the version in `package.json` and push a commit to main
2. Create and push a `vX.Y.Z` tag

The tag triggers the `release` workflow, which builds the macOS `.dmg` and Windows `.exe` and publishes them as a GitHub Release.

If no release label is added, merging the PR does nothing extra.

## Order of operations

The release only triggers after the PR is fully merged and all required status checks have passed (branch protection enforces this before the merge is allowed). The sequence is:

1. PR checks pass, merge is allowed
2. `auto-release` fires, bumps version, pushes a commit and tag to main
3. `release` fires from the tag, builds and publishes the binaries
