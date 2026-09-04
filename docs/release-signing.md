# Release signing (macOS and Windows)

Releases are cut by pushing a version tag; `dist` (cargo-dist 0.32) builds the
five targets, and two extra jobs sign the macOS and Windows binaries before the
GitHub Release is created.

```
tag 0.2.0
   ├── test                    the full suite on the three OSes (gate)
   ├── plan                    dist works out what to build
   ├── build-local-artifacts   per-target builds + archives (dist)
   ├── build-global-artifacts  checksums + shell/powershell installers (dist)
   ├── custom-sign-macos       ← .github/workflows/sign-macos.yml
   │      reopens the *apple-darwin*.tar.gz archives, signs every Mach-O binary
   │      inside them (Developer ID + hardened runtime + timestamp), notarizes
   │      them, repacks the archives under the same names, refreshes the
   │      .sha256 files and overwrites the workflow artifacts
   ├── custom-sign-windows     ← .github/workflows/sign-windows.yml
   │      the same round trip for the *pc-windows-*.zip archives: every .exe
   │      inside is Authenticode-signed with Azure Trusted Signing (SHA256 +
   │      RFC-3161 timestamp), the archive is repacked and its .sha256 refreshed
   └── host                    creates the release from those artifacts
```

`dist` has no code-signing support of its own and builds + packages in a single
command, so there is no hook between "binary exists" and "binary is inside the
archive". Reopening the finished archive is what makes signing possible without
forking dist's pipeline. Both jobs are wired in through
`host-jobs = ["./sign-macos", "./sign-windows"]` in `dist-workspace.toml`, so
`dist generate` reproduces the wiring.

`custom-sign-windows` runs **after** `custom-sign-macos`, not beside it: both
rewrite the shared `artifacts-build-global` artifact, and two concurrent uploads
under the same artifact name would race. `dist generate` will not reproduce that
ordering — put the `needs: custom-sign-macos` back if you regenerate.

The macOS signing, notarization and keychain teardown steps come from the shared
composite actions in [`digiogithub/ci-actions`](https://github.com/digiogithub/ci-actions)
(pinned at `@v1`), the same ones the pando and git-in-track release pipelines
use; the Windows side uses `azure/trusted-signing-action@v0` against the same
signing account those two repositories share.

## What a user gets

`docsai` inside the macOS archives is signed with the DIGIO Developer ID and
notarized. A bare Mach-O cannot carry a stapled notarization ticket, so a
downloaded copy is validated by Gatekeeper's **online** check against the
registered CDHash — i.e. the first run needs network. If offline first-run ever
matters, the archive has to become a `.pkg`, which can be stapled.

`docsai.exe` inside the Windows archive carries an Authenticode signature from
`CN=Digio Soluciones Digitales SL` with an RFC-3161 timestamp, so SmartScreen
and Windows Defender see a known publisher instead of an unsigned binary.

Linux artifacts are unsigned; there is no OS gatekeeper to satisfy there.

## The macOS secret

One repository secret, `MACOS_SIGNING_BUNDLE`: a base64 `.tar.gz` of
`~/DIGIO_Software_Signing_Keys`, holding both Developer ID `.p12` files and the
`kvagerc` env file with their passwords and the notary credentials
(`NOTARY_APPLE_ID`, `NOTARY_TEAM_ID`, `NOTARY_APP_PASSWORD`).

```bash
ssh mac-mini-de-digio 'tar czf - -C ~ DIGIO_Software_Signing_Keys | base64' \
  | gh secret set MACOS_SIGNING_BUNDLE --repo digiogithub/docsai
```

On the runner the certificates are imported into an ephemeral keychain that is
deleted with `if: always()`; the raw `.p12` files are removed as soon as they
are imported, and every password read from `kvagerc` is masked in the log.

## Windows: no secret, an OIDC identity

`custom-sign-windows` runs in the `release` GitHub environment and mints an OIDC
token that a federated credential on the Entra application accepts, so there is
nothing long-lived to leak or rotate. The configuration lives entirely in
repository **variables**:

| Variable | Value |
| --- | --- |
| `AZURE_CLIENT_ID` | `3d9ac0eb-08e2-40b4-b95b-d9c3303d04bf` |
| `AZURE_TENANT_ID` | `30e30347-734c-4e52-85c3-a3c0a0d3389d` |
| `AZURE_SUBSCRIPTION_ID` | `5a0ec42a-5724-4790-ba2b-519c808820e4` |
| `AZURE_SIGNING_ENDPOINT` | `https://weu.codesigning.azure.net/` |
| `AZURE_SIGNING_ACCOUNT` | `digio-art-sign-acc` |
| `AZURE_SIGNING_CERT_PROFILE` | `digio` |

The Trusted Signing account, certificate profile and role assignment are shared
with pando and git-in-track; only the federated credential is per repository:

```bash
az ad app federated-credential create --id 3d9ac0eb-08e2-40b4-b95b-d9c3303d04bf \
  --parameters '{"name":"docsai-release-environment-immutable",
                 "issuer":"https://token.actions.githubusercontent.com",
                 "subject":"repo:digiogithub@8157515/docsai@1319602413:environment:release",
                 "audiences":["api://AzureADTokenExchange"]}'
```

Note the **subject format**. This repository presents the immutable-ID form,
`repo:<org>@<org id>/<repo>@<repo id>:environment:release`, not the slug form
older repositories use. Copying a working subject from another repository
produces `AADSTS700213: No matching federated identity record found for
presented assertion subject ...`. Read the form the repository actually emits
before writing the credential:

```bash
gh api repos/digiogithub/docsai/actions/oidc/customization/sub
```

Both forms are registered here, so the credential keeps working if GitHub ever
flips the repository back to slugs.

## Keeping it in step with dist

`sign-macos.yml` and `sign-windows.yml` address the workflow artifacts by dist's
own names (`artifacts-build-local-<target>`, `artifacts-build-global`). Adding or
renaming a **darwin** or **windows** target in `dist-workspace.toml` means adding
the matching upload step in the corresponding job; the unpacking side is
glob-driven and needs no change. Each job fails loudly if it finds no archive for
its platform, or no binary inside one, so a silent "released unsigned" is not
possible.

After signing, the archives are re-extracted and every binary is re-checked — a
`(runtime)` signature on macOS, a `Valid` Authenticode status on Windows — so a
repack that lost the signature fails the release instead of shipping.
