# Release & Semantic Versioning Guide

This project utilizes an automated CI/CD pipeline via GitHub Actions to build, bundle, and release the **Kick Synthesizer** plugin. Following the steps below ensures that cross-platform binaries are generated correctly and versioned according to [Semantic Versioning (SemVer)](https://semver.org/) standards.

---

## 1. Triggering a New Release

The build workflow is triggered specifically by **Git Tags**. When you are ready to publish a new version, create a tag starting with a lowercase `v`.
Make sure to push to main branch first before tagging.

### Step A: Create a Tag
Use the `-a` flag for an annotated tag. This allows you to include a brief summary of the changes.
```bash
git tag -a v0.3.1 -m "Release v0.3.1"
```

### Step B: Push to GitHub
Pushing the tag specifically (not just the branch) initiates the GitHub Action.
```bash
git push origin v0.3.1
```

---
## 2. Automated Versioning Logic
The workflow includes a `get_version` step that handles the naming conventions automatically:
- Stripping the Prefix: The script strips the `v` from the tag name (e.g., `v1.2.3` becomes `1.2.3`).
- Release Naming: The GitHub Release will be titled "**Release v1.2.3**".
- Filename Generation: Artifacts are renamed for clarity, resulting in filenames like:
  - `kick_synth-v1.2.3-win64.zip` 
  - `kick_synth-v1.2.3-macos.zip`

---
## 3. Bundle & Zip Handling
The workflow builds multiple plugin formats and packages them per platform.

### Windows (x64)

The Windows build generates:
- `kick_synth.vst3` — a VST3 plugin (single DLL file)
- `kick_synth.clap` — a CLAP plugin (single DLL file)

Both are zipped into a standard archive for easy extraction.

### macOS (Universal)

The macOS build produces a universal binary (Apple Silicon + Intel) and generates:
- `kick_synth.vst3/` — a VST3 bundle
- `kick_synth.clap/` — a CLAP bundle
- `Kick Synth.component/` — an Audio Unit v2 bundle (wraps the CLAP via [clap-wrapper](https://github.com/free-audio/clap-wrapper))

The Audio Unit bundle is self-contained: the CLAP binary is embedded inside it under `Contents/PlugIns/`, so installing only the `.component` is sufficient for Logic Pro and other AU hosts.

### IMPORTANT: Preserving Permissions
The workflow uses `zip -ry` for macOS:
- The `-r` flag ensures the entire folder structure is captured.
- The `-y` flag preserves symbolic links and the executable bit. Without this, the plugin will fail to load in DAWs because the binary inside the bundle will not have permission to execute.

---

## 4. Post-Release: Gatekeeper Bypass (macOS)
Since these builds are not code-signed or notarized, macOS users must clear the "quarantine" flag after installing the plugin. For example:
```bash
# VST3
sudo xattr -rd com.apple.quarantine /Library/Audio/Plug-Ins/VST3/kick_synth.vst3
# CLAP
sudo xattr -rd com.apple.quarantine /Library/Audio/Plug-Ins/CLAP/kick_synth.clap
# Audio Unit
sudo xattr -rd com.apple.quarantine /Library/Audio/Plug-Ins/Components/"Kick Synth.component"
```
