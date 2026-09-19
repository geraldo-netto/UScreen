# Fork Android release identity

The maintainer designated an independent fork identity on 2026-09-18, and its
permanent signing key has been provisioned. Android builds the fork package
below. Host discovery, launch, token delivery and capability queries target
that package; Kotlin classes retain their `com.uscreen` namespace. Release
bundling and publication verify the APK against the designated certificate.

| Property | Value |
| --- | --- |
| Maintainer | [geraldo-netto](https://github.com/geraldo-netto) |
| Selected application ID | `io.github.geraldo_netto.uscreen` |
| Key alias | `uscreen-fork-release` |
| Key algorithm | RSA, 4096 bits |
| Certificate signature | SHA256withRSA |
| Certificate validity (UTC) | 2026-09-18 19:35:05 through 2054-02-03 19:35:05 |
| Public certificate | [release-certificate.pem](release-certificate.pem) |
| Certificate SHA-256 | `1B:34:ED:11:5E:47:6F:4D:17:8B:49:F6:07:6C:F9:ED:6C:C0:7D:47:4F:92:30:EC:95:2B:C9:7B:BB:A7:04:00` |

The application ID uses an underscore because Android allows letters, digits
and underscores in its dot-separated segments, but not hyphens. It identifies
a separate app from `com.uscreen`; see [Android's application-ID rules](https://developer.android.com/build/configure-app-module).
The selected migration allows both apps to be installed. It must not uninstall
the existing app or promise automatic transfer of its private settings.
Android's [signing documentation](https://developer.android.com/studio/publish/app-signing)
describes update identity and key custody. Keep the same fork application ID
and compatible signing identity for future fork updates.

## Key custody

Only the public certificate belongs in Git. The encrypted PKCS12 keystore and
its generated password are outside the repository, with owner-only directory
and file permissions. A byte-identical backup is stored on a separate local
filesystem; both copies contain the password needed for recovery, so filesystem
access protects that password. Local custody paths are recorded alongside the
key in `identity.json`. Retain an additional offline copy for disaster recovery;
the local backup is not an offline backup.

Provisioning verification compared every backup file, checked permissions and
filesystem separation, and read the certificate using the backup password. The
backup private key signed a temporary JAR, which passed strict verification
against the primary keystore. This checks key usability and backup recovery;
it does not establish the signing identity of any existing or published APK.

Inspect the tracked public certificate without accessing private material:

```bash
openssl x509 -in docs/release-certificate.pem -noout -fingerprint -sha256 -dates
```

## Build and verify

Provide an owner-only properties file outside the repository containing
`storeFile`, `storePassword`, `keyAlias` and `keyPassword`. Use the provisioned
keystore and alias above; do not generate a replacement. `storeFile` should be
an absolute path. The environment variable selects the properties file without
putting passwords on the command line:

```bash
USCREEN_KEYSTORE_PROPERTIES=/private/path/keystore.properties \
  ./android/gradlew -p android assembleRelease
python3 scripts/verify-release-apk.py android/app/build/outputs/apk/release/app-release.apk
```

The verifier uses stock Android SDK `apksigner` and `aapt2`, from PATH or the
latest stable installed build-tools directory. It discovers the SDK through
`ANDROID_SDK_ROOT`, `ANDROID_HOME`, then `android/local.properties`. Verification
requires a valid APK signature, exactly one signer matching the tracked public
certificate, the fork package, `com.uscreen.MainActivity` as launcher, and a
non-debuggable APK. Missing tools, unsigned/debug APKs and mismatched identities
fail closed. Both release bundle builders verify before copying the APK;
publication verifies the staged APK before any release API writes.

## Migration from the upstream package

Install the fork APK alongside the existing `com.uscreen` app. The host now
looks specifically for `io.github.geraldo_netto.uscreen`, so an upstream-only
installation is reported as missing. The fork has separate Android private
data. Note the old app's brightness, refresh, decoder and other preferences,
then select them in the fork app as needed; there is no automatic settings
transfer. The installer does not remove the old app. Both apps can remain
installed, but use the fork with the updated host.

An explicit launch uses the application ID and the original class namespace:

```bash
adb shell am start -n io.github.geraldo_netto.uscreen/com.uscreen.MainActivity
```

Debug builds use the same fork application ID with a different signing key.
They cannot replace an official release in place. Preserve app data and check
which APK is installed before choosing a migration; do not automatically
uninstall an app to bypass a signature mismatch. Future official releases use
the same designated package and signing identity.
