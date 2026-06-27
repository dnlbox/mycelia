# Homebrew Packaging

This directory carries the staging formula for testing the Homebrew UX before a
Homebrew/core submission.

To create a personal tap:

```text
gh repo create dnlbox/homebrew-mycelia --public
git clone git@github.com:dnlbox/homebrew-mycelia.git
cd homebrew-mycelia
mkdir -p Formula
cp ../mycelia/packaging/homebrew/Formula/mycelia.rb Formula/mycelia.rb
git add Formula/mycelia.rb
git commit -m "Add mycelia formula"
git push origin main
```

Then install through the tap:

```text
brew tap dnlbox/mycelia
brew install mycelia
brew test mycelia
```

The permanent target remains Homebrew/core. Keep the formula source-based:
it should build from a tagged source archive and must not shell out to
`install.sh`.
