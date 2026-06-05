# Platform-specific binary wheels for codei

Each `pypi-publish/*` directory contains a platform-specific wheel.
The main `codei` package declares dependencies on these wheels using
environment markers, so pip installs only the correct one.

## Structure

```
pypi-publish/
├── codei_cli-VERSION-cp38-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl
├── codei_cli_aarch64-VERSION-cp38-abi3-manylinux_2_17_aarch64.manylinux2014_aarch64.whl
├── codei_cli_macos_x86_64-VERSION-py3-none-macosx_10_15_x86_64.whl
├── codei_cli_macos_arm64-VERSION-py3-none-macosx_11_0_arm64.whl
├── codei_cli_win_x86_64-VERSION-py3-none-win_amd64.whl
└── codei_cli_win_arm64-VERSION-py3-none-win_arm64.whl
```

## Building

Run `scripts/publish.sh` from the py-cli root to build and upload all wheels.
