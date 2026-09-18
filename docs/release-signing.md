# Fork Android release identity

The maintainer designated an independent fork identity on 2026-09-18, and its
permanent signing key has been provisioned. **Integration remains open as T250:**
current code still builds `com.uscreen`, the host addresses that package, and
the publication workflow does not yet enforce the certificate below.

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

Before T250 is complete, migrate host component targets and Android packaging,
document settings migration, and add permanent package-targeting and
wrong-certificate publication regressions. The certificate fingerprint must be
checked on the actual release APK before publication. Do not configure this
fork release key for the current upstream application ID.
