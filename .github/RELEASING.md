# Releasing

Releases are fully automated. To ship a release, add one of these labels to a PR before merging:

| Label | Example |
|---|---|
| `release:patch` | `0.1.8` -> `0.1.9` (bug fixes) |
| `release:minor` | `0.1.8` -> `0.2.0` (new features) |
| `release:major` | `0.1.8` -> `1.0.0` (breaking changes) |

If no release label is added, merging the PR does nothing extra.

## What happens when you merge

1. `auto-release` fires, bumps `package.json`, and pushes a `chore: release vX.Y.Z` commit plus a `vX.Y.Z` tag to main.
2. `release` fires from the tag, builds the macOS `.dmg` and Windows `.exe`, and publishes them as a GitHub Release.
3. `deploy` fires from the main push. It only runs when the commit message starts with `chore: release`, so it deploys to GitHub Pages exactly once per release and never on regular merges.

## GitHub Pages deploy

The deploy workflow triggers on every push to main but runs only when the commit is a release commit (`chore: release vX.Y.Z`). This is intentional: GitHub Pages environment protection rules require deployments to come from a branch ref, not a tag ref, so the deploy is tied to the release commit on main rather than the tag.

## Secrets required

- `RELEASE_PAT`: a fine-grained PAT with `Contents: write` for this repo. Needed so the bot commit and tag can bypass branch protection rules that require a PR.
