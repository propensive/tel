# Publish the library to the local ~/.ivy2 (the launcher resolves `tel-core` from there; burdock
# will NOT externalize a locally-published copy unless its bytes match a release asset).
publishLocal:
	./mill tel.core.publishLocal

# Build the invocation-point `launcher` module as a plain (clean, no shell-preamble) assembly JAR.
# `launcher` depends on tel-core as a PUBLISHED coordinate resolved from ~/.ivy2/local, so the
# library is published there FIRST — otherwise the launcher silently builds against whatever was
# last published (a release's jar, say, whose bytes then externalize to that release's download,
# and local changes never reach the executable). `clean tel.launcher` for the same reason: the
# coordinate is fixed, so Mill's cached resolution would not notice the fresh publish.
assembly: publishLocal
	./mill clean tel.launcher
	./mill tel.launcher.assembly

# Publish tel to GitHub Releases: the tel-core jar first, then — once its digest is indexed — the
# repackaged `tel` executables, added to the same release. See release-launcher.sh in
# propensive/.github (run through etc/shared) for the two-step ordering and its verification.
release:
	./etc/shared release-launcher.sh tel "tel-core" $(VERSION)

# Repackage the launcher assembly into a self-fetching launcher with Burdock. The
# `burdock.externalize` macro wrapping `TelServer.run()` (in src/launcher/tel_launcher.scala) has
# already embedded `META-INF/burdock.deps` at compile time; running the repackager rewrites the JAR
# in place so published dependencies become on-demand `Burdock-Require` URLs and unpublished ones
# are inlined from `~/.cache/burdock`.
#
# Three publication homes are consulted: Maven Central (hashes resolved via deps.dev) for the
# third-party dependencies, and — via the `--github` hints — the release assets of the tel,
# Soundness and proscala repositories, whose per-jar SHA-256 digests the repackager matches against
# the classpath. The Soundness jars synced into ~/.ivy2/local are the release assets byte-for-byte,
# and the proscala release publishes the same jars its tarball carries, so both the components and
# the fork toolchain externalize; tel-core externalizes only once released (`make release`), and is
# inlined otherwise. Pyrocosm is deliberately NOT among the hints: tel does not depend on it. Set
# GITHUB_TOKEN to lift the API rate limit.
tel.jar: assembly
	cp out/tel/launcher/assembly.dest/out.jar tel.jar
	java -cp tel.jar soundness.repackage --github propensive/tel,propensive/soundness,propensive/proscala

# Package the repackaged JAR as a native executable for this machine with the pinned `xeq` builder
# script (fetched into dist/xeq and verified against etc/xeq.tsv).
tel: tel.jar xeq-fetch
	dist/xeq build --jar tel.jar --out tel

# Fetch the pinned `xeq` builder script into dist/xeq.
xeq-fetch:
	./etc/shared xeq-fetch.sh

# Install the launcher onto the PATH so the Zed extension can find it via `worktree.which`.
# Remove-then-copy, NOT a bare `cp`: overwriting the existing file reuses its inode, and macOS
# caches code-signing state per vnode — after an in-place rewrite every exec of the launcher is
# killed with SIGKILL until the file is replaced. Deleting first makes the copy a fresh inode.
install: tel
	rm -f ${HOME}/.local/bin/tel
	cp tel ${HOME}/.local/bin/

# Run the language server on stdio via the native launcher (handy for a manual JSON-RPC smoke test).
# Ethereal applications must be started through their launcher, not `java -jar`.
run: tel
	./tel lsp

# Compile and run the test suite with fume, which discovers the suite from the assembly named in
# .pyrocosm/fume/config.tel (relative to this directory). Extra selection terms go in TESTS, e.g.
# `make test TESTS='tag:hover'`. CI runs the same command (the shared workflow installs the fume
# pinned in etc/tools). `make test-plain` is a fume-less fallback: `tel.runTests`
# (src/test/tel_test_main.scala) drives `Tests.invoke` in-process, since a probably `Suite` has had
# no `main` of its own since Soundness 0.65.0.
test:
	./mill tel.test.assembly
	fume run -c out/tel/test/assembly.dest/out.jar $(TESTS)

test-plain:
	./mill tel.test.assembly
	java -cp out/tel/test/assembly.dest/out.jar tel.runTests

# Install every library pinned in etc/refs — releases and snapshots alike, transitively — into the
# local ivy repository, as CI does, so the build resolves exactly the pinned jars rather than
# whatever a sibling checkout's `publishLocal` last installed under the same version. A snapshot not
# yet on GitHub is built from the sibling checkout named by the pin's commit.
sync-deps:
	./etc/shared sync-deps.sh

# Check every source against Consequent Style and the project's own rules with flair (the release
# pinned in etc/tools; `make tools` installs it), as configured in .pyrocosm/flair/config.tel.
# Findings are warnings and the count is not yet zero, so CI does not run this; PATHS restricts the
# check to files beneath them.
check:
	flair check $(PATHS)

# Install the commands pinned in etc/tools (fume, flair) through their releases' installers.
tools:
	./etc/shared tools.sh

# Publish HEAD's library as a snapshot — a `snapshot-<hex>` pre-release named by the filtered tree
# of the commit, at version `<telVersion>-<hex>` — for a dependent repository to pin in its
# etc/refs before the next release. `LOCAL=1` stages and installs without publishing. The last line
# printed is the pin. See snapshot.sh in propensive/.github.
snapshot:
	./etc/shared snapshot.sh tel "$$(sed -n 's/.*val telVersion = "\(.*\)".*/\1/p' build.mill)"

# Delete snapshot pre-releases older than DAYS (default 60) days.
snapshot-prune:
	./etc/shared snapshot-prune.sh tel $(DAYS)

dev:
	./mill -w tel.core.compile

.PHONY: publishLocal assembly release xeq-fetch install run test test-plain sync-deps check tools snapshot snapshot-prune dev
