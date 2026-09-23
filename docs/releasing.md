# Releases and documentation

The Release workflow prepares a shared version, checks it, publishes all seven
crates, creates a GitHub Release, deploys versioned documentation, and synchronizes
develop. Run it from develop with a stable version such as 0.2.0. A dispatch
authorizes that complete sequence.

## Initial setup

1. Make release.yml, docs.yml and the reusable checks available on the default
   branch and develop. GitHub lists manually dispatched workflows from the default
   branch; select develop when running Release.
2. Enable GitHub Actions and allow the job-specific token permissions declared in
   the workflows. Git operations use GITHUB_TOKEN and github-actions[bot].
   Branch rules must permit the bot to create release branches, fast-forward main,
   create tags and synchronize develop. The workflow does not bypass or edit rules.
3. Configure Pages with **GitHub Actions** as its source. Create/configure the
   release and github-pages environments. To use the single-dispatch flow,
   environment rules must allow these jobs without another manual approval.
4. For the first publication, configure the repository secret
   CARGO_BOOTSTRAP_TOKEN with a short-lived crates.io token authorized to publish
   the seven crate names. Select bootstrap. Do not put the token in commands,
   files, issue text or logs.
5. After all crates exist, configure their crates.io Trusted Publishers for this
   repository, the release.yml workflow and the release environment. Select oidc
   on subsequent runs, then remove the bootstrap secret. Authentication uses the
   official rust-lang/crates-io-auth-action; it does not silently fall back.

All developer commits made with a personal identity remain GPG-signed. Automated
release commits and merges use the bot's name/email and need no signature.
Automatic tags are lightweight and unsigned. No personal signing key is needed
in Actions, and the workflow does not require a GitHub Verified badge.

## Run a release

Ensure main is an ancestor of develop and all earlier releases have been
synchronized. Use **Actions → Release → Run workflow**, select develop, enter
X.Y.Z without a v prefix, and choose bootstrap or oidc. CLI equivalent:

~~~sh
gh workflow run release.yml --ref develop -f version=0.2.0 -f authentication=oidc
~~~

The first release may use the workspace's initial 0.1.0. Subsequent releases
advance the shared version. Existing release branches or tags stop preparation
for inspection. Prereleases and build metadata are not accepted.

The preparation job updates the workspace version, internal dependency versions,
and installation documentation. It commits these changes on release/vX.Y.Z,
using the bot, and publishes that branch. CI, GnuPG interoperability and fuzz
validate that exact commit. Package verification and documentation build before
registry uploads. Each check job resolves its own dependencies and uses --locked
after resolution; the build job's lock is passed to the publication job within
the same Actions run.

When every check passes, main advances by fast-forward and vX.Y.Z points to the
same commit. Cargo publishes, in order:

1. assuan-sexpr
2. assuan-protocol
3. assuan-transport
4. assuan-client
5. assuan-server
6. assuan-macros
7. assuan-library

Each upload must succeed and its registry version, non-yanked status and checksum
must match the archive Cargo just produced. An upload timeout is not evidence
that nothing was published. There is no skip-existing behavior or automatic
partial-publication recovery. OIDC credentials are acquired immediately before
promotion/publication; expiry stops the job rather than changing authentication.

After the seven confirmations, gh creates a GitHub Release containing
docs-vX.Y.Z.zip and docs-vX.Y.Z.sha256. Documentation is deployed by an explicit
call to docs.yml, so the flow does not depend on a tag push triggering another
workflow. Finally, develop receives a fast-forward or ordinary unsigned bot
merge. A conflict leaves the published release intact and reports failure.

## Documentation and LLM files

The site contains /X.Y.Z/ guides, Markdown sources, examples, API rustdoc, llms.txt
and llms-full.txt. Paths include the repository prefix on project Pages sites.
llms.txt is the navigation index; llms-full.txt combines the selected public
guides and examples. Detailed API coverage remains in rustdoc.

Every completed stable GitHub Release must retain its documentation ZIP and
checksum asset. The deploy downloads and verifies all those bundles, preserving
previous versions without rebuilding them. Missing/corrupt bundles fail the
deployment. Drafts and prereleases are excluded. The root index and LLM files
use the greatest stable SemVer, not the most recently uploaded release.

For local generation, use PowerShell 7.6.6 and the pinned Rust toolchain:

~~~powershell
cargo generate-lockfile
cargo doc --locked --workspace --all-features --no-deps
./scripts/build-docs.ps1 -Version 0.1.0 -BaseUrl https://baliestri.github.io/assuan-library-rs/
./scripts/verify-docs.ps1
~~~

Use the version in Cargo.toml. Generated files stay under target/release-docs;
the build refuses to overwrite a version directory. Supply a fresh -Output path
for another build.

## Failures and recovery

The job summary lists the completed and failed phases. Check that summary and
the named job before choosing recovery. Release operations across crates.io,
GitHub and Pages are not atomic.

| Failure | Recovery |
| --- | --- |
| Preparation fails before a release branch exists | Fix the reported configuration/version issue and dispatch a new run. |
| Release branch exists but checks/build failed | Re-run failed jobs of the original run when the candidate remains valid. A new dispatch deliberately rejects the existing branch. If source changes are needed, inspect refs and publication status before the maintainer decides how to replace an unpublished candidate. |
| main/tag exist but publication stopped | Inspect all seven versions in crates.io. Do not rerun uploads blindly or move the tag. Use the partial-publication procedure below. |
| All crates exist but GitHub Release creation failed | Confirm package provenance and tag SHA, then inspect whether the release/assets were created despite the error. Create missing material from the original run's bundle only after that inspection; do not replace divergent assets. |
| GitHub Release exists but Pages failed | Correct the Pages issue and run Documentation. This only collects existing release bundles and redeploys the site. |
| develop synchronization failed | Merge the published tag into develop, resolve conflicts and use a signed personal commit when acting as the maintainer. Push the merge before starting another release. |

For documentation-only recovery, use **Actions → Documentation → Run workflow**
from the maintained default branch, or:

~~~sh
gh workflow run docs.yml
~~~

For a partial registry publication:

1. Record the original prepared commit, tag and every confirmed crate from the
   job log. A crate at which Cargo failed may still have been uploaded.
2. Inspect the registry entries and download any already-published archives.
   Confirm the version, expected source files and .cargo_vcs_info.json commit
   against the original release commit. If provenance cannot be established,
   stop for a maintainer decision.
3. Check out that exact commit in a clean checkout and retrieve the original
   build lock/artifacts if still available. Use the pinned toolchain. Actions
   artifacts expire; the system does not promise indefinite automatic recovery.
4. After verifying which versions are absent, publish only those crates with
   the official Cargo command in dependency order, using explicitly configured
   credentials. For example, cargo publish -p assuan-client --locked --registry
   crates-io is appropriate only after its prerequisites are confirmed and its
   own version is absent. Confirm each resulting registry entry/checksum.
5. After all seven are confirmed, complete the GitHub Release with the original
   documentation bundle/checksum, run Documentation, and synchronize develop.

A failed check is not permission to use --allow-dirty, --no-verify, force push,
replace an existing tag or overwrite different release assets.

## Validation limits

Run ./scripts/test-release.ps1 for local regression tests. Git workflow steps
run against disposable local repositories; registry and GitHub API responses are
mocked for failure scenarios. actionlint validates workflow syntax and expressions.
Package verification and documentation generation run locally without uploads.

These checks do not prove that your repository's permissions, Trusted Publishers
or Pages settings allow a real release. The first authorized dispatch is the
operational validation; local implementation work never publishes a test release.

## References

- [Bot commits with the built-in token](https://github.com/actions/checkout/blob/main/README.md#push-a-commit-using-the-built-in-token)
- [Trusted Publishing](https://crates.io/docs/trusted-publishing)
- [Cargo publish](https://doc.rust-lang.org/cargo/commands/cargo-publish.html)
- [Custom Pages workflows](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)
