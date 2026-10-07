# Release checklist

Use this checklist for each release candidate. It is a procedure, not a record that the listed checks have passed. Leave items unchecked until verified against the candidate artifact.

## Local build and package

- [ ] Confirm the intended source commit and tag `vX.Y.Z`; the crate and bundle versions use numeric `X.Y.Z`.
- [ ] Run `cargo fmt --check`.
- [ ] Run `cargo clippy` and review warnings without broadening scope into unrelated cleanup.
- [ ] Run `cargo test`.
- [ ] Fetch the pinned Sparkle dependency with `scripts/fetch-sparkle.sh`.
- [ ] Build a local ad-hoc package with `VERSION=X.Y.Z scripts/bundle-macos.sh`. This does not notarize the app.
- [ ] Confirm the local output includes `Rista-X.Y.Z.zip`, `Rista-X.Y.Z.dmg`, and `SHA256SUMS`; appcast generation requires `SPARKLE_PRIVATE_ED_KEY`.
- [ ] Verify `codesign --verify --deep --strict 'dist/Rista.app'`.
- [ ] Confirm ZIP/DMG bundle filename and `CFBundleName` are ASCII `Rista.app` / `Rista`, while `CFBundleDisplayName` remains `Rísta`. Sparkle's exact path comparison can fail when an accented filename is normalized differently by LaunchServices.
- [ ] Confirm the bundle includes `Contents/Resources/LICENSE` with the correct copyright holder, and its iconset displays clearly at small and large sizes.
- [ ] Verify checksums with `(cd dist && shasum -a 256 -c SHA256SUMS)`.
- [ ] Confirm the appcast enclosure names only the final ZIP, with the ASCII asset name `Rista-X.Y.Z.zip`.
- [ ] Verify the appcast Ed25519 signature against the public key embedded as `SUPublicEDKey` in the bundled `Info.plist`.

## Notarized GitHub release

The tag-triggered workflow runs on `v*`. It fails closed unless the tag version matches `Cargo.toml`, all signing credentials are available, and notarization succeeds.

Configure these GitHub Actions repository secrets:

- `DEVELOPER_ID_P12` — base64-encoded Developer ID Application certificate and private key in PKCS#12 format.
- `DEVELOPER_ID_P12_PASSWORD` — password for that PKCS#12 file.
- `APPSTORE_API_PRIVATE_KEY` — App Store Connect API private key as PEM or base64-encoded PEM.
- `SPARKLE_PRIVATE_ED_KEY` — Sparkle EdDSA signing seed.

Configure these GitHub Actions repository variables:

- `APPLE_TEAM_ID`
- `APPSTORE_API_KEY_ID`
- `APPSTORE_ISSUER_ID`

Never print, paste, or commit secret values. The workflow imports the Developer ID certificate into an ephemeral runner keychain and removes the keychain and API key file during cleanup.

- [ ] Confirm `NOTARIZE=1` is used only with a real Developer ID Application identity and App Store Connect API key.
- [ ] Confirm the notary service authenticates before the expensive build begins.
- [ ] Sign inside-out: Sparkle `Installer.xpc` and `Downloader.xpc` with their existing entitlements, `Autoupdate`, `Updater.app`, `Sparkle.framework`, then `Rista.app`. Do not use `codesign --deep` to sign.
- [ ] Use hardened runtime and a secure timestamp for every Mach-O signed with the Developer ID identity.
- [ ] Submit the app ZIP and require notary status `Accepted`; on failure preserve the submission result and fetch the notary log when available.
- [ ] Staple and validate the app, then require `spctl --assess --type execute`.
- [ ] Create the final ZIP from the stapled app. Build a basic compressed UDZO DMG containing that exact app and an `/Applications` symlink.
- [ ] Sign the DMG with the Developer ID identity and timestamp; submit it separately and require `Accepted`.
- [ ] Staple and validate the DMG, then require `spctl --assess --type open --context context:primary-signature` and `hdiutil verify`.
- [ ] Generate `SHA256SUMS` only after the final ZIP and DMG bytes are stable. Generate the signed appcast afterward from a directory containing only the final ZIP.
- [ ] Publish `Rista-X.Y.Z.zip`, `Rista-X.Y.Z.dmg`, `SHA256SUMS`, and `appcast.xml` only after every signing, notarization, staple, and validation gate passes.

The v0.1.0 and v0.1.1 releases remain ad-hoc signed and not notarized. The published v0.1.2 ZIP and DMG have passed signing, notarization, stapling, and Gatekeeper checks. Repeat verification for each new release.

## Installed-artifact validation

- [ ] Download the actual release assets; do not substitute a locally built ZIP for the published artifact.
- [ ] Verify the downloaded ZIP and DMG against the downloaded `SHA256SUMS`, comparing the digest even if the host normalizes an asset filename.
- [ ] Verify the downloaded app and DMG signatures, stapled tickets, Gatekeeper assessments, and the appcast signature using the embedded public key, without accessing or exposing private keys.
- [ ] Install and exercise the downloaded app on macOS 13+ Apple Silicon using the manual cases in [TEST-CASES.md](TEST-CASES.md).
- [ ] Confirm `.md` and `.markdown` Finder opens and Dock/Finder reopen work after the final app window closes.
- [ ] Confirm the appcast and package URLs are reachable by intended users.
- [ ] Exercise the published update end-to-end: offer, download, Install and Relaunch, old PID exit, new PID/version, and unchanged vault/settings. Do not count replacement of the on-disk bundle alone as success.
- [ ] For v0.1.0–v0.1.2 installs, document the one-time quit and outer-bundle rename to `Rista.app` (or fresh DMG installation and removal of the old copy). New package bytes cannot repair the old running updater's host-path matching. Keep ordinary-name and renamed-bundle test results distinct; unattended scheduling is a separate check.

Linux and Windows are intended targets, but there are no verified packages or platform-specific release checks yet.
