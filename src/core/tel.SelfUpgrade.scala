package tel

import java.util.concurrent as juc

import soundness.*

import alphabets.hexLowerCase
import charsets.utf8Charset
import codepages.utf8Codepage
import errorDiagnostics.stackTracesDiagnostics
import filesystemBackends.javaBaseFilesystem
import httpBackends.javaNetHttp
import internetAccess.online
import logging.silentLogging
import probates.cancelProbate
import providers.javaBaseProvider
import systems.javaBaseSystem
import textSanitizers.skipSanitizer

// Self-upgrade, done as Pyrocosm's tools do it (`pyrocosm.Tool` and `pyrocosm.Release`), for a
// tool that does not depend on Pyrocosm. This file is the same in lira and tel but for its
// package and `SelfUpgrade.name`, and is meant to move into Ethereal, beside `Upgrade`, once
// a Soundness release can carry it.
//
// Once a day, in the background of the daemon, a released build fetches the manifest of its
// newest release (`UpgradeManifest`). While that is newer than the running build, the `upgrade`
// subcommand is offered by tab-completion; it is accepted at any time. `upgrade` fetches the
// manifest afresh, downloads the executable for this platform, checks its digest, and stages it
// for the launcher (`Upgrade.stage`), which verifies its signature against the keys in the
// RUNNING executable and swaps it in at the start of the next invocation. That invocation
// reports what the launcher did, once (`report`). Builds are compared by the build id the
// launcher gives the daemon, `major*1000000 + minor*1000 + patch`: a development build has none,
// and never checks.
object UpgradeManifest:
  case class Executable(url: Text, sha256: Text)

  // The newest non-prerelease release's manifest. GitHub's `latest` redirect needs no API call,
  // and so is not rate-limited; snapshots are pre-releases, and are never `latest`.
  def url(name: Text): Text =
    t"https://github.com/propensive/$name/releases/latest/download/upgrade.tsv"

  // Parses a manifest: tab-separated rows of `version`, `build`, `signed-by` (which may be
  // empty) and `<platform> <url> <sha256>`; blank lines and `#` comments are skipped. `Unset`
  // if the version or build is missing, or the build is not a number.
  def parse(text: Text): Optional[UpgradeManifest] =
    val rows: List[List[Text]] =
      text.cut(t"\n").map(_.trim).filter: line =>
        line != t"" && !line.starts(t"#")
      . map(_.cut(t"\t"))

    def field(key: Text): Optional[Text] =
      rows.filter(_.prim == key).prim.let:
        case _ :: value :: _ => value
        case _               => t""

    val executables: Map[Text, Executable] =
      rows.bind:
        case List(platform, url, sha256) if platform.contains(t"-") =>
          List(platform -> Executable(url, sha256))

        case _ =>
          Nil

      . to[Map]

    field(t"version").let: version =>
      field(t"build").let { build => safely(build.as[Long]) }.let: build =>
        val signedBy: Optional[Text] =
          field(t"signed-by").let { key => if key == t"" then Unset else key }
        UpgradeManifest(version, build, signedBy, executables)

  // Downloads `executable` and checks its digest; `Unset` if the download fails or the bytes do
  // not have the SHA-256 the manifest promised.
  def download(executable: Executable): Optional[Data] =
    safely(executable.url.as[HttpUrl].fetch().receive[Data]).let: data =>
      if data.digest[Sha2[256]].serialize[Hex] == executable.sha256.lower then data else Unset

  // The platform label of the running JVM, as the release names its executables: `linux-x64`,
  // `linux-arm64`, `macos-x64`, `macos-arm64` or `windows-x64`; `Unset` for anything else.
  def platform(using System): Optional[Text] =
    val os: Text = safely(System.properties.os.name[Text]()).or(t"").lower
    val arch: Text = safely(System.properties.os.arch[Text]()).or(t"").lower

    val system: Optional[Text] =
      if os.contains(t"mac") || os.contains(t"darwin") then t"macos"
      else if os.contains(t"win") then t"windows"
      else if os.contains(t"linux") then t"linux"
      else Unset

    val architecture: Optional[Text] =
      if arch == t"x86_64" || arch == t"amd64" then t"x64"
      else if arch == t"aarch64" || arch == t"arm64" then t"arm64"
      else Unset

    system.let: system =>
      architecture.let { architecture => t"$system-$architecture" }

// A published release, as its `upgrade.tsv` manifest describes it (see propensive/.github#32):
// the version, the build id its executables carry, the key they were signed with, if any, and
// one executable per platform, with the SHA-256 the download is checked against. The manifest
// is not signed and need not be: the launcher verifies the executable it names against the key
// in the RUNNING binary, so a forged manifest can make an upgrade fail but never apply a bad one.
case class UpgradeManifest
  ( version:     Text,
    build:       Long,
    signedBy:    Optional[Text],
    executables: Map[Text, UpgradeManifest.Executable] ):

  def executable(platform: Text): Optional[UpgradeManifest.Executable] =
    executables.at(platform)

object SelfUpgrade:
  val name: Text = t"tel"

  // The exit statuses of `upgrade` when it does nothing, as Pyrocosm's tools use them.
  val NoUpgrade: Exit = Exit.Fail(9)
  val UpgradeFailed: Exit = Exit.Fail(10)

  // The newest release this daemon knows of, from the last check (`refresh`), and whether the
  // daily check has been started in this daemon.
  private val latest: juc.atomic.AtomicReference[UpgradeManifest | Null] =
    juc.atomic.AtomicReference(null)

  private val checking: juc.atomic.AtomicBoolean = juc.atomic.AtomicBoolean(false)

  // How old a cached manifest may be before it is fetched again: the check is daily.
  private val checkInterval: Long = 24*60*60*1000L

  // The version a build id stands for.
  def version(build: Long): Text =
    t"${(build/1000000).show}.${(build/1000%1000).show}.${(build%1000).show}"

  // Starts the daily check for a newer release, once per daemon, for a released build. The
  // task runs under the daemon's monitor, so it outlives the invocation that started it, and it
  // never raises: a failed check leaves the last known release in place.
  def check()(using resident: Resident, monitor: Monitor, environment: Environment): Unit =
    if resident.buildId > 0 && checking.compareAndSet(false, true) then
      async:
        while true do
          refresh(force = false)
          snooze(24*Hour)

  // The cached manifest, `$XDG_CACHE_HOME/<name>/upgrade.tsv`, so that a daemon restarted
  // within a day of the last check does not fetch again.
  private def cacheDirectory(using Environment): Optional[Path on Linux] =
    safely(Xdg.cacheHome[Path on Linux] / Name[Linux](name))

  private def cache(using Environment): Optional[Path on Linux] =
    cacheDirectory.let { directory => safely(directory / Name[Linux](t"upgrade.tsv")) }

  // The newest release, from the cache if it is fresh enough and `force` is not set, and
  // otherwise fetched (and cached). Whatever is learned is remembered for `available`; `Unset`
  // if nothing could be read.
  def refresh(force: Boolean)(using Environment): Optional[UpgradeManifest] =
    val file: Optional[Path on Linux] = cache

    val cached: Optional[UpgradeManifest] =
      if force then Unset else file.let: file =>
        safely(summon[FilesystemBackend on Linux].stat(file, true)).let: stat =>
          val age: Long = java.lang.System.currentTimeMillis() - stat.modified
          if age < checkInterval then safely(file.read[Text]).let(UpgradeManifest.parse(_))
          else Unset

    val release: Optional[UpgradeManifest] = cached.or:
      safely(UpgradeManifest.url(name).as[HttpUrl].fetch().receive[Text]).let: text =>
        UpgradeManifest.parse(text).also:
          cacheDirectory.let: directory =>
            safely:
              if !directory.existent() then directory.create[Directory](CreateFlag.Parents)
              (directory / Name[Linux](t"upgrade.tsv")).write(text)

    release.let(latest.set(_))
    release

  // The newest release this daemon knows of, if it is newer than the running build.
  def available(using resident: Resident): Optional[UpgradeManifest] =
    Optional(latest.get()).let: release =>
      if resident.buildId > 0 && release.build > resident.buildId then release else Unset

  // Reports, once and on standard error, what the launcher did with a staged upgrade: this
  // invocation's launcher checked `.pending` before it connected, so its verdict is already on
  // disk. Nothing is printed for a tab-completion or the help tree's probe.
  def report()(using cli: Cli, environment: Environment): Unit =
    cli match
      case invocation: Invocation =>
        given Stdio = invocation.stdio

        Upgrade.outcome.let: outcome =>
          outcome.result match
            case Upgrade.Outcome.Result.Applied =>
              Err.println(t"$name: upgraded to ${version(outcome.candidate)}")

            case result =>
              val reason: Message = result.communicate
              Err.println(t"$name: the staged upgrade was not applied: $reason")

          Upgrade.acknowledge()

      case _ =>
        ()

  // `upgrade`: fetches the manifest afresh, and stages the newest release if it is newer.
  def upgrade()(using invocation: Invocation, resident: Resident, environment: Environment)
  :   Exit =

    refresh(force = true)

    available.let(stage(_)).or(noUpgrade())

  // `upgrade` when nothing newer is known: the manifest has just been fetched afresh, so this is
  // the newest release, or the check could not reach it.
  private def noUpgrade()(using invocation: Invocation, resident: Resident): Exit =
    given Stdio = invocation.stdio

    if resident.buildId > 0
    then Out.println(t"No release newer than $name ${version(resident.buildId)} is known")
    else Out.println(t"This build of $name is not a release, so it cannot be upgraded")

    NoUpgrade

  // `upgrade` when `release` is newer: downloads its executable for this platform, checks the
  // digest, and stages it for the launcher to verify and apply. An executable built without a
  // key cannot be upgraded in place, and is told how to reinstall instead.
  private def stage(release: UpgradeManifest)(using invocation: Invocation, environment: Environment)
  :   Exit =

    given Stdio = invocation.stdio

    if Upgrade.pending then
      Out.println(t"An upgrade is already staged, and takes effect when $name next runs")
      Exit.Ok
    else if !Upgrade.enabled then
      Out.println(t"$name ${release.version} is available, but this executable has no key.")
      Out.println(t"Reinstall it with: curl -fsSL https://propensive.dev/$name | sh")
      UpgradeFailed
    else
      val platform: Optional[Text] = UpgradeManifest.platform
      val executable: Optional[UpgradeManifest.Executable] = platform.let(release.executable(_))

      executable.let: executable =>
        Out.println(t"Downloading $name ${release.version} for ${platform.or(t"?")}...")

        UpgradeManifest.download(executable).let: data =>
          recover:
            case error: Upgrade.Error =>
              Out.println(t"The upgrade could not be staged: ${error.message}")
              UpgradeFailed

          . protect:
              Upgrade.stage(data)
              val version: Text = release.version
              Out.println(t"$name $version is staged, and takes effect when $name next runs")
              Exit.Ok

        . or:
            Out.println(t"The download failed, or did not have the digest the release promised")
            UpgradeFailed

      . or:
          Out.println(t"$name ${release.version} has no executable for this platform")
          UpgradeFailed
