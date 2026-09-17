package tel

import java.lang as jl
import java.util.concurrent.atomic as juca

import soundness.*
import probably.TestEvent

// tel's own suite, run WITHOUT fume: a `Suite` has no `main` of its own (the host — normally fume —
// drives it through `invoke`), so this is the plain-`java` entry point `make test-plain` uses,
// printing one line per completed test and exiting with the suite's status (0 = passed,
// 1 = failures, 2 = the suite threw). `fume run -c <test jar>` (`make test`, and CI) remains the
// full experience, discovering the suite from the assembly's `META-INF/services/probably.Suite`
// index, which the beneficence plugin writes.
@main
def runTests(): Unit =
  val passes = juca.AtomicInteger(0)
  val failures = juca.AtomicInteger(0)
  val out = jl.System.out.nn

  val status = Tests.invoke(t"", event => event match
    case TestEvent.TestCompleted(test, _, _, outcome, _, _) =>
      if outcome.outcome == t"pass" || outcome.outcome == t"aspire-pass" then passes.incrementAndGet()
      else failures.incrementAndGet()
      out.println(t"[${outcome.outcome}] ${test.path.join(t" / ")}".s)

    case TestEvent.DetailMessage(_, message) =>
      out.println(t"    $message".s)

    case TestEvent.DetailCompare(_, expected, found, _) =>
      out.println(t"    expected: $expected".s)
      out.println(t"    found:    $found".s)

    case TestEvent.RunTerminated(error, _, _) =>
      out.println(t"suite threw: ${error.components.map(_.message).join(t"; ")}".s)

    case _ => ())

  out.println(t"${passes.get} passed, ${failures.get} failed".s)
  jl.System.exit(status)
