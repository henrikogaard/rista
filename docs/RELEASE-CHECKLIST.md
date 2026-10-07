# Release checklist

Use this checklist for each release candidate. It is a procedure, not a record that the listed checks have passed. Leave items unchecked until verified against the candidate artifact.

## Build and package

- [ ] Confirm the intended source commit and tag `vX.Y.Z`; the crate and bundle versions use numeric `X.Y.Z`.
- [ ] Run `cargo fmt --check`.
- [ ] Run `cargo clippy` and review warnings without broadening scope into unrelated cleanup.
- [ ] Run `cargo test`.
- [ ] Fetch the pinned Sparkle dependency with `scripts/fetch-sparkle.sh`.
- [ ] Build the versioned macOS package with `VERSION=X.Y.Z scripts/bundle-macos.sh`.
- [ ] For a signed appcast, confirm the GitHub Actions secret `SPARKLE_PRIVATE_ED_KEY` is configured. Never print, paste, or commit the key. Local packaging without the key skips appcast generation.
- [ ] Verify `codesign --verify --deep --strict 'dist/Rísta.app'`.
- [ ] Confirm the bundle includes `Contents/Resources/LICENSE` with the correct copyright holder, and its iconset displays clearly at small and large sizes.
- [ ] Verify the generated checksum with `(cd dist && shasum -a 256 -c SHA256SUMS)`.
- [ ] Confirm the archive and checksum use the ASCII asset name `Rista-X.Y.Z.zip`, and the appcast enclosure URL names that same archive.
- [ ] Verify the appcast Ed25519 signature against the public key embedded as `SUPublicEDKey` in the bundled `Info.plist`.

The tag-triggered GitHub Actions workflow runs on `v*`, removes the leading `v` for the bundle version, and publishes `Rista-X.Y.Z.zip`, `SHA256SUMS`, and `appcast.xml`. Its default code signature is ad hoc; v0.1.0 is not Developer ID signed or notarized.

## Installed-artifact validation

- [ ] Download the actual release assets; do not substitute a locally built ZIP for the published artifact.
- [ ] Verify the downloaded ZIP against the downloaded `SHA256SUMS`, comparing the digest even if the host normalizes the asset filename.
- [ ] Verify the downloaded app bundle signature and the appcast signature using the embedded public key, without accessing or exposing the private key.
- [ ] Install and exercise the downloaded app on macOS 13+ Apple Silicon using the manual cases in [TEST-CASES.md](TEST-CASES.md).
- [ ] Confirm `.md` and `.markdown` Finder opens and Dock/Finder reopen work after the final app window closes.
- [ ] Confirm the appcast and package URLs are reachable by intended users. The repository is private, so the GitHub release feed is not suitable for unauthenticated update clients without publicly accessible hosting.
- [ ] Confirm distribution signing and notarization requirements for the intended audience; the current v0.1.0 artifact is ad hoc signed and not notarized.

Linux and Windows are intended targets, but there are no verified packages or platform-specific release checks yet.
