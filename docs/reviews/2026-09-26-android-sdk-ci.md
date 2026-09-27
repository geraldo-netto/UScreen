# Retired Android SDK package in CI (T639)

Build run [36273300317](https://github.com/geraldo-netto/blent/actions/runs/36273300317)
failed in Android SDK setup before any application build or coverage collection:

```text
sdkmanager tools
Warning: Failed to find package 'tools'
```

The upstream [setup-android v3 action](https://github.com/android-actions/setup-android/blob/v3/action.yml)
defaults its package selection to `tools platform-tools`. The build now explicitly
selects supported `platform-tools`; the action still installs command-line tools,
and Gradle selects the application's declared SDK/build dependencies.

Permanent regression
`BuildOutputTest.test_t639_android_ci_avoids_retired_sdk_tools` failed before the
workflow change with `T639: default SDK packages include retired tools`, then
passed unchanged. It guards every Android SDK setup step against implicit legacy
selection and explicit retired `tools`, while requiring supported platform tools.
Production sources and collected function counters are unchanged.
