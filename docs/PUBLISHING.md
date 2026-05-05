# Publishing Guide

This guide is for the one-time publication of this template repository as `github.com/safelibs/port-template`. Run the commands from the repository root after the template contract is complete and committed on `main`.

## One-Time Publication Commands

```sh
gh auth status
gh repo create safelibs/port-template --public --source=. --remote=origin --description "Template for Safelibs port-* repositories"
git push -u origin main
gh repo edit safelibs/port-template --description "Template for Safelibs port-* repositories" --visibility public
gh api --method PATCH repos/safelibs/port-template --field is_template=true
origin_url="$(git remote get-url origin)"; case "$origin_url" in git@github.com:safelibs/port-template.git|https://github.com/safelibs/port-template.git|ssh://git@github.com:safelibs/port-template.git) ;; *) echo "unexpected origin: $origin_url"; exit 1;; esac
test "$(git ls-remote origin refs/heads/main | awk '{print $1}')" = "$(git rev-parse HEAD)"
gh repo view safelibs/port-template --json visibility,isTemplate,defaultBranchRef --jq '.visibility == "PUBLIC" and .isTemplate == true and .defaultBranchRef.name == "main"' | grep -qx true
gh run list --repo safelibs/port-template --workflow ci-release.yml --limit 1
gh release view --repo safelibs/port-template "build-$(git rev-parse --short=12 HEAD)" --json tagName,assets,url
```

## Expected Initial Push Behavior

The initial push may include scaffold commits made before the CI workflow and completed template files existed. During the initial branch creation push, `.github/workflows/ci-release.yml` is expected to skip only those incomplete bootstrap commits when they are missing completed-template files.

The final pushed head commit is different: it must satisfy the completed template contract. CI must run the hook sequence (`install-build-deps`, `check-layout`, `build-debs`, `run-upstream-tests`, `run-port-tests`, `run-validation-tests`), upload every `dist/*.deb` artifact, and publish the GitHub Release tagged `build-<short-sha>` for that head commit.

If the head commit does not produce a release, inspect the latest `ci-release.yml` run before using the repository as a template.

## Repository Settings To Confirm

After running the publication commands, confirm:

- The repository is public.
- The repository is marked as a GitHub template.
- The default branch is `main`.
- `origin` points to `safelibs/port-template`.
- The remote `main` SHA matches the local `HEAD`.
- The latest workflow run completed for the pushed head commit.
- The `build-<short-sha>` GitHub Release exists and includes every built `.deb` asset.
