# Build the LSP server as a plain (clean, no shell-preamble) assembly JAR.
assembly:
	mill tel.core.assembly

# Produce the Ethereal native launcher `tel` from the assembly. Ethereal's self-packaging path
# (triggered by `-Dbuild.executable`) reads the assembled JAR and writes a small native launcher that
# starts (or reconnects to) a resident daemon and forwards stdin/stdout/signals to it.
tel: assembly
	java -Dbuild.executable=tel -jar out/tel/core/assembly.dest/out.jar

# Install the launcher onto the PATH so the Zed extension can find it via `worktree.which`.
install: tel
	cp tel ${HOME}/.local/bin/

# Run the language server on stdio via the native launcher (handy for a manual JSON-RPC smoke test).
# Ethereal applications must be started through their launcher, not `java -jar`.
run: tel
	./tel lsp

dev:
	mill -w tel.core.compile

.PHONY: assembly install run dev
