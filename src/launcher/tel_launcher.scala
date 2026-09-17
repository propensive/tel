package tel

import soundness.*

// The invocation point, alone in its own build module. `externalize` (from burdock, re-exported
// through `soundness.*`) records the SHA-256 of every jar on THIS module's compile classpath into
// `META-INF/burdock.deps` at compile time. Because the `launcher` module depends on `tel-core` as a
// PUBLISHED coordinate (see `build.mill`), the exact jar bytes on the classpath match the released
// ones, so `soundness.repackage` rewrites it into an on-demand `Burdock-Require` download rather
// than inlining its classes. The whole command dispatch lives in `tel.TelServer.run` in the
// (published) `core` module.
@main
def tel(): Unit = externalize(TelServer.run())
