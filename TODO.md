# TODO

| id | status | severity | effort | description |
|---|---|---|---|---|
| T221 | blocked | medium | small | Local USB setup: RugKing Pad 2 Pro (vendor `1782`) appears in `adb devices` with `no permissions`; its USB node is root-owned without user access, and `android-sdk-platform-tools-common` is absent. User already belongs to `plugdev`; verified package candidate includes vendor `1782`. Requires administrator authentication (`sudo -n` reports password required): install that package, reload udev rules, reconnect tablet, authorize debugging, and verify ADB state `device` before removing this item. |
