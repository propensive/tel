package tel

import soundness.*

import probably.TestEvent

import stdios.fileDescriptorStdio
import termcapDefinitions.basicTermcap

// tel's own suite, run WITHOUT fume: a `Suite` has no `main` of its own (the host — normally fume —
// drives it through `invoke`), so this is the plain-`java` entry point `make test-plain` uses,
// printing one line per completed test and exiting with the suite's status (0 = passed,
// 1 = failures, 2 = the suite threw). `fume run -c <test jar>` (`make test`, and CI) remains the
// full experience, discovering the suite from the assembly's `META-INF/services/probably.Suite`
// index, which the beneficence plugin writes.
@main
def runTests(): Unit =
  val passes = Atomic(0)
  val failures = Atomic(0)

  val status = Tests.invoke(t"", event => event match
    case TestEvent.TestCompleted(test, _, _, outcome, _, _) =>
      if outcome.outcome == t"pass" || outcome.outcome == t"aspire-pass" then passes.ere(_ + 1)
      else failures.ere(_ + 1)

      Out.println(t"[${outcome.outcome}] ${test.path.join(t" / ")}")

    case TestEvent.DetailMessage(_, message) =>
      Out.println(t"    $message")

    case TestEvent.DetailCompare(_, expected, found, _) =>
      Out.println(t"    expected: $expected")
      Out.println(t"    found:    $found")

    case TestEvent.RunTerminated(error, _, _) =>
      Out.println(t"suite threw: ${error.components.map(_.message).join(t"; ")}")

    case _ => ())

  Out.println(t"${passes()} passed, ${failures()} failed")
  Exit(status).terminate()
