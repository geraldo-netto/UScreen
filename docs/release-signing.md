# Blent Android release identity

Blent uses a fresh application identity selected on 2026-09-26 and the
permanent signing key provisioned on 2026-09-18. No settings migration or
compatibility with the former application is provided. Android builds the fork package
below. Host discovery, launch, token delivery and capability queries target
that package; Kotlin classes retain their `com.blent` namespace. Release
bundling and publication verify the APK against the designated certificate.

| Property | Value |
| --- | --- |
| Maintainer | [geraldo-netto](https://github.com/geraldo-netto) |
| Selected application ID | `io.github.geraldo_netto.blent` |
| Key alias | `uscreen-fork-release` |
| Key algorithm | RSA, 4096 bits |
| Certificate signature | SHA256withRSA |
| Certificate validity (UTC) | 2026-09-18 19:35:05 through 2054-02-03 19:35:05 |
| Public certificate | [release-certificate.pem](release-certificate.pem) |
| Certificate SHA-256 | `1B:34:ED:11:5E:47:6F:4D:17:8B:49:F6:07:6C:F9:ED:6C:C0:7D:47:4F:92:30:EC:95:2B:C9:7B:BB:A7:04:00` |

The application ID uses an underscore because Android allows letters, digits
and underscores in its dot-separated segments, but not hyphens. It identifies
a separate app from the former UScreen packages; see [Android's application-ID rules](https://developer.android.com/build/configure-app-module).
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

The local custody README and `identity.json` describe the active fork identity
and completed T250 package migration/certificate gate. Their stale pre-migration
notes were corrected in both copies on 2026-09-19 (T536); all custody files were
verified byte-identical across copies, with owner-only file permissions and
unchanged key, password and public certificate. Keep these notes synchronized
when the implemented signing workflow changes.

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
BLENT_KEYSTORE_PROPERTIES=/private/path/keystore.properties \
  ./android/gradlew -p android assembleRelease
python3 scripts/verify-release-apk.py android/app/build/outputs/apk/release/app-release.apk
```

The verifier uses stock Android SDK `apksigner` and `aapt2`, from PATH or the
latest stable installed build-tools directory. It discovers the SDK through
`ANDROID_SDK_ROOT`, `ANDROID_HOME`, then `android/local.properties`. Verification
requires a valid APK signature, exactly one signer matching the tracked public
certificate, the fork package, `com.blent.MainActivity` as launcher, and a
non-debuggable APK. Missing tools, unsigned/debug APKs and mismatched identities
fail closed. Both release bundle builders verify before copying the APK;
publication verifies the staged APK before any release API writes.

## ADB-managed installation

After verifying the signed APK, install with `adb -s SERIAL install -r -g blent.apk`.
`-g` grants the runtime permissions declared by the APK (currently Camera);
it grants no new permission absent from the manifest and does not start capture.
Normal permissions are handled by Android at installation. Special/signature
permissions and USB authorization are separate platform controls. A manually
installed APK still needs runtime permission prompts. Camera Start/Stop and
foreground/background consent remain required regardless of installation route.

## Fresh Blent installation

Install the signed Blent APK as `io.github.geraldo_netto.blent`. It has separate
private data from the former UScreen packages and starts with fresh defaults.
No settings migration or old-version compatibility is provided. Stop the old
app before using Blent with the renamed host; old app data need not be deleted.
The host targets only the new Blent package.

An explicit launch uses the new application ID and class namespace:

```bash
adb shell am start -n io.github.geraldo_netto.blent/com.blent.MainActivity
```

Debug builds use the same fork application ID with a different signing key.
They cannot replace an official release in place. Preserve app data and check
which APK is installed before choosing a migration; do not automatically
uninstall an app to bypass a signature mismatch. Future official releases use
the same designated package and signing identity.
