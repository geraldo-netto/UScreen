# TODO

Actionable findings from a full-project review on 2026-09-16 (branch
`configurable-input-devices`, after commit b22952b). Build, cache and
`target/` directories were skipped. Every item was verified against the code
by the reviewer that raised it; line numbers are as of that commit.

Severity: **high** = crash, data loss, security, or a feature silently dead;
**medium** = wrong behaviour in a realistic case; **low** = misleading,
cosmetic, or minor. Effort: **S** under an hour, **M** under half a day,
**L** larger. Status: open, doing, done, wontfix.

71 items: 3 high, 28 medium, 40 low.

| id | status | severity | effort | short description |
|---|---|---|---|---|
| T004 | open | high | S | Fix inproc-encoder build: spawn_blocking closure captures &mut self via self.config.instance ([capture.rs:1164](host/src/capture.rs#L1164)) |
| T005 | open | high | S | Forward extra tablets' base ports to their session ports; second real tablet can never connect ([main.rs:971](host/src/main.rs#L971)) |
| T006 | open | high | S | Rewrite pkgver in the shipped PKGBUILD; it is still 1.1.0 while everything else is 1.2.3 ([build-packages.sh:47](packaging/build-packages.sh#L47)) |
| T007 | open | medium | S | Make the GitHub update check optional or correct SECURITY.md, which says it can be turned off ([MainActivity.kt:343](android/app/src/main/java/com/uscreen/MainActivity.kt#L343)) |
| T008 | open | medium | S | Use START_NOT_STICKY and tie wake/Wi-Fi locks to onStart/onStop, not activity lifetime ([StreamingService.kt:112](android/app/src/main/java/com/uscreen/StreamingService.kt#L112)) |
| T010 | open | medium | S | Do not build the decoder in setSurface() while the receiver is stopped ([VideoReceiver.kt:204](android/app/src/main/java/com/uscreen/VideoReceiver.kt#L204)) |
| T011 | open | medium | S | Stop claiming the tarball installer enables the systemd unit; install.sh never does ([installation.md:40](docs/installation.md#L40)) |
| T012 | open | medium | S | Scale-1 conversion writes with g_mode_w stride into buffers sized by g_out_w: heap overflow on odd width ([evdi_helper.c:328](host/evdi/evdi_helper.c#L328)) |
| T013 | open | medium | S | Print STREAM_SIZE after computing g_out_w/g_out_h; capture.rs waits 3s for it on every start ([evdi_helper.c:493](host/evdi/evdi_helper.c#L493)) |
| T014 | open | medium | S | find_uscreen_bin prefers a stale ~/.local/bin/uscreen over the daemon installed next to the GUI ([main.rs:160](gui/src/main.rs#L160)) |
| T015 | open | medium | S | The GUI's one-time setup skips the uinput udev rule and never checks /dev/uinput ([main.rs:250](gui/src/main.rs#L250)) |
| T016 | open | medium | S | Start/Stop and Apply bypass systemd when uscreen.service is the thing running the daemon ([main.rs:270](gui/src/main.rs#L270)) |
| T017 | open | medium | S | GUI never re-reads config.toml, so Apply overwrites changes the daemon or `uscreen wifi` made meanwhile ([main.rs:346](gui/src/main.rs#L346)) |
| T018 | open | medium | S | The 3s wait for STREAM_SIZE never waits: cloned watch receiver has a stale version ([capture.rs:1080](host/src/capture.rs#L1080)) |
| T019 | open | medium | S | Do not tear down the helper when start_encoder fails before spawning anything ([capture.rs:1101](host/src/capture.rs#L1101)) |
| T020 | open | medium | S | Record encoder_mode from the size start_encoder used, not a fresh active_mode() call ([capture.rs:1108](host/src/capture.rs#L1108)) |
| T021 | open | medium | M | Watch helper_child exit in the session select; inproc encoder never notices a dead helper ([capture.rs:1187](host/src/capture.rs#L1187)) |
| T022 | open | medium | S | Use terminate() (SIGTERM) for the helper on display-off, mode change and crash restart, not start_kill() ([capture.rs:1288](host/src/capture.rs#L1288)) |
| T023 | open | medium | S | Doctor never checks that evdi_helper (and its libevdi.so) can be found and executed ([doctor.rs:144](host/src/doctor.rs#L144)) |
| T024 | open | medium | S | check_processes reports FAIL for two helpers, which is the normal state with max_tablets > 1 ([doctor.rs:221](host/src/doctor.rs#L221)) |
| T025 | open | medium | S | KDE output mode check compares against cfg.width/height even when auto_resolution follows the tablet ([doctor.rs:465](host/src/doctor.rs#L465)) |
| T026 | open | medium | S | check_colour runs `adb shell` without -s, so all three checks vanish when two devices are attached ([doctor.rs:552](host/src/doctor.rs#L552)) |
| T027 | open | medium | S | Reset the partial frame when the FIFO writer closes mid-frame, or every later frame is misaligned ([encoder.rs:263](host/src/encoder.rs#L263)) |
| T028 | open | medium | S | extract_parameter_sets is H.264-only; with hevc_* on the inproc path clients wait 5s for a config that never comes ([encoder.rs:293](host/src/encoder.rs#L293)) |
| T029 | open | medium | M | Map input devices onto the EVDI output on X11 from the daemon (xinput map-to-output), retiring the manual script ([input.rs:712](host/src/input.rs#L712)) |
| T030 | open | medium | S | Check /proc/<pid>/comm before declaring a PID-file process a live daemon ([main.rs:150](host/src/main.rs#L150)) |
| T031 | open | medium | S | Treat input/stream server bind failure as fatal instead of logging and running on ([main.rs:374](host/src/main.rs#L374)) |
| T032 | open | medium | S | Exclude the current primary serial from `others` instead of assuming it is devices[0] ([main.rs:946](host/src/main.rs#L946)) |
| T033 | open | medium | S | Make `uscreen stop` wait for the daemon to exit; GUI Apply & restart races the untracked-daemon check ([main.rs:1411](host/src/main.rs#L1411)) |
| T034 | open | medium | S | CI never fails on the EVDI helper and never uploads it, although it is a shipped binary ([build.yml:45](.github/workflows/build.yml#L45)) |
| T035 | open | medium | S | make setup-system installs no udev rule and sets initial_device_count=1, unlike every other path ([Makefile:43](Makefile#L43)) |
| T036 | open | low | S | Set allowBackup=false or exclude the 'uscreen' prefs so the session token is not backed up ([AndroidManifest.xml:25](android/app/src/main/AndroidManifest.xml#L25)) |
| T037 | open | low | S | Scope cleartext permission to 127.0.0.1 via network_security_config instead of app-wide flag ([AndroidManifest.xml:30](android/app/src/main/AndroidManifest.xml#L30)) |
| T038 | open | low | S | Move the stylus-hover KDoc onto onGenericMotionEvent; it is attached to onNewIntent ([MainActivity.kt:274](android/app/src/main/java/com/uscreen/MainActivity.kt#L274)) |
| T039 | open | low | M | Only accept the 'token' extra from the shell; any app can overwrite it and force reconnects ([MainActivity.kt:300](android/app/src/main/java/com/uscreen/MainActivity.kt#L300)) |
| T040 | open | low | S | Disable the OrientationEventListener in onStop and re-enable in onStart ([MainActivity.kt:363](android/app/src/main/java/com/uscreen/MainActivity.kt#L363)) |
| T041 | open | low | S | Align MIN_BITRATE_KBPS (5000) with host config::MIN_BITRATE_KBPS (1000) or document the gap ([Prefs.kt:24](android/app/src/main/java/com/uscreen/Prefs.kt#L24)) |
| T042 | open | low | S | Add a pingInterval to the control-socket OkHttpClient to detect a dead link ([TouchCapture.kt:60](android/app/src/main/java/com/uscreen/TouchCapture.kt#L60)) |
| T043 | open | low | S | Uninstall recipe misses the icons install.sh drops in ~/.local/share/icons and the ~/.cache/uscreen fallback ([SECURITY.md:43](SECURITY.md#L43)) |
| T044 | open | low | S | Packages put modprobe.d and modules-load.d files under /usr/lib, not /etc as documented ([installation.md:68](docs/installation.md#L68)) |
| T045 | open | low | S | evdi_capture_test.c and evdi_test2.c include evdi_drm.h, which does not exist in host/evdi/ ([evdi_capture_test.c:12](host/evdi/evdi_capture_test.c#L12)) |
| T046 | open | low | S | evdi_grab_test.c is not a C file: it contains C++ lambdas, prose and a shell heredoc ([evdi_grab_test.c:492](host/evdi/evdi_grab_test.c#L492)) |
| T047 | open | low | S | Check pthread_create in conv_pool_init or bgra_to_nv12 deadlocks forever ([evdi_helper.c:318](host/evdi/evdi_helper.c#L318)) |
| T048 | open | low | S | Conversion hardcodes 32-bit BGRA but on_mode_changed accepts any bits_per_pixel/pixel_format ([evdi_helper.c:416](host/evdi/evdi_helper.c#L416)) |
| T049 | open | low | M | A single 250 ms read stall by a live ffmpeg gets its FIFO closed mid-frame ([evdi_helper.c:745](host/evdi/evdi_helper.c#L745)) |
| T050 | open | low | S | g_lat_us/g_lat_count written by the writer thread without the lock the stats reader holds ([evdi_helper.c:767](host/evdi/evdi_helper.c#L767)) |
| T051 | open | low | S | find_evdi_device() ignores card0 and picks an arbitrary evdi.N in readdir order ([evdi_helper.c:929](host/evdi/evdi_helper.c#L929)) |
| T052 | open | low | S | evdi_add_device() returns 0 on failure, never negative: the `< 0` check is dead and costs a 5 s wait ([evdi_helper.c:1011](host/evdi/evdi_helper.c#L1011)) |
| T053 | open | low | S | Bitrate slider floor (5 Mbps) disagrees with MIN_BITRATE_KBPS (1000) and silently rewrites saved values ([main.rs:685](gui/src/main.rs#L685)) |
| T054 | open | low | S | h264_vaapi hard-codes /dev/dri/renderD128 with no way to pick another GPU ([capture.rs:777](host/src/capture.rs#L777)) |
| T055 | open | low | S | hevc_vaapi is rejected as 'Unknown encoder' although Codec::from_encoder and its test treat it as supported ([capture.rs:928](host/src/capture.rs#L928)) |
| T056 | open | low | S | `tx.receiver_count() > 0` is always true: StreamServer::run holds an unread subscriber ([capture.rs:1402](host/src/capture.rs#L1402)) |
| T057 | open | low | S | 'evdi module not loaded' prints two overlapping hints, the second contradicting the first ([doctor.rs:119](host/src/doctor.rs#L119)) |
| T058 | open | low | S | uinput hint installs a source-tree path that packaged installs do not have and already ship ([doctor.rs:138](host/src/doctor.rs#L138)) |
| T059 | open | low | S | Tablet-app hint points at a debug APK in the source tree that package users do not have ([doctor.rs:404](host/src/doctor.rs#L404)) |
| T060 | open | low | S | check_autostart calls a missing unit 'disabled' and hints a command that will fail ([doctor.rs:534](host/src/doctor.rs#L534)) |
| T061 | open | low | S | Reject modes the EDID DTD cannot encode instead of silently truncating them ([edid.rs:93](host/src/edid.rs#L93)) |
| T062 | open | low | M | Tell the app which input devices exist so it stops sending touch/pen events nobody consumes ([input.rs:1146](host/src/input.rs#L1146)) |
| T063 | open | low | S | Report the codec of the running encoder, not the startup one, and validate Config.encoder ([input.rs:1217](host/src/input.rs#L1217)) |
| T064 | open | low | S | Cache only a successful D-Bus backend probe; a transient failure disables KWin integration for the whole run ([kwin.rs:56](host/src/kwin.rs#L56)) |
| T065 | open | low | S | Remove or implement the dead `start --daemon/-d`, `--display` and `--auto-vdisplay` flags ([main.rs:88](host/src/main.rs#L88)) |
| T066 | open | low | S | Apply the same clamps to CLI overrides that FileConfig::sanitize applies to the file; fix the doc range ([main.rs:192](host/src/main.rs#L192)) |
| T067 | open | low | S | Print the configured video/input ports in the startup banner, not hard-coded 8890/8891 ([main.rs:493](host/src/main.rs#L493)) |
| T068 | open | low | S | Await extra sessions' capture pipelines at shutdown instead of dropping their JoinHandles ([main.rs:531](host/src/main.rs#L531)) |
| T069 | open | low | S | Reset wifi_announced when the tablet disconnects, as the comment promises ([main.rs:1010](host/src/main.rs#L1010)) |
| T070 | open | low | S | Fix doc comments attached to the wrong items and drop dead adb_device_serial ([main.rs:1256](host/src/main.rs#L1256)) |
| T071 | open | low | S | write_frame issues four write syscalls per frame; the per-batch flush() is a no-op on TcpStream ([stream.rs:279](host/src/stream.rs#L279)) |
| T072 | open | low | S | Reap the xdg-open and uscreen-gui children spawned from tray menu callbacks ([tray.rs:175](host/src/tray.rs#L175)) |
| T073 | open | low | S | Drop the `edid` prerequisite of `install`: s9ultra.bin is generated and copied but never read ([Makefile:22](Makefile#L22)) |
| T074 | open | low | S | Portability check accepts GLIBC_2.36 while the docs promise Ubuntu 22.04 (glibc 2.35) ([build-release.sh:43](scripts/build-release.sh#L43)) |
| T075 | open | low | S | Check NOTES and the tag before the multi-minute build, not after ([publish-release.sh:57](scripts/publish-release.sh#L57)) |

## Details

**T004** — The `move` closure handed to tokio::task::spawn_blocking reads `self.config.instance`, which captures `self` (a `&mut CaptureManager`) and fails with E0521 (borrowed data escapes, requires 'static); the feature advertised in docs/development.md:107 has not compiled since e7a9c3e. Hoist `let fifo = fifo_path_for(self.config.instance);` above the closure like the other locals.

**T005** — spawn_extra_session's comment says the tablet side keeps using 8890/8891, but on_tablet_connected runs `adb reverse tcp:8892 tcp:8892`. The app hard-codes 127.0.0.1:8890 and ws://127.0.0.1:8891, so with max_tablets > 1 a second physical tablet gets a pipeline and a display but nothing listens on the ports it dials; only the loopback fake tablet works. Give setup_adb_forwarding separate remote/local ports: `adb reverse tcp:<base> tcp:<base+2*instance>`.

**T006** — control and the rpm spec get Version sed-replaced but the PKGBUILD is tarred as-is with pkgver=1.1.0 and source at tag v$pkgver, so uscreen-1.2.3-PKGBUILD.tar.gz builds 1.1.0; publish-release.sh never checks it and docs/development.md:149 omits it from the bump list. Sed pkgver like the others and add it to the publish check and checklist.

**T007** — SECURITY.md (and README/faq) state the app's update check is off with check_updates = false, but that is a host config key; the app calls api.github.com unconditionally on every process start. Add a Prefs toggle, or change the docs.

**T008** — START_STICKY makes the system restart the service after process death with no activity, re-acquiring a 4 h partial wake lock and an untimed Wi-Fi lock. MainActivity.onStop stops streaming but the locks stay held until onDestroy, so a backgrounded app keeps CPU and radio awake. Return START_NOT_STICKY and release the locks from onStop.

**T010** — setSurface() calls setupCodec() whenever mediaCodec is null, independent of isRunning, so every surfaceCreated/surfaceChanged (launch with no host, rotation in pen-only mode) creates a hardware decoder plus a MAX_PRIORITY thread polling every 10 ms that nothing releases until the next stop(). Guard with isRunning.

**T011** — This line and README.md:55 say install.sh runs `systemctl --user enable --now`, but scripts/install.sh:183-185 only copies the unit and runs daemon-reload. Either add the enable step to install.sh or correct both docs.

**T012** — bgra_to_nv12 passes g_mode_w/g_mode_h as the job's w/h and convert_strip uses j->w as the destination row stride, while buffers are malloc'd for g_out_w*g_out_h*3/2 with g_out_w = g_mode_w & ~1. With an odd mode width every Y row is one byte too long and the last chroma row writes past the buffer. Loop to ow/oh and use ow as the destination stride, or reject odd modes.

**T013** — capture.rs parses a `STREAM_SIZE w h` stdout line and blocks up to 3 s on stream_rx before every ffmpeg start, then warns 'Compositor reported no mode within 3s'. The helper never emits that line. Add printf("STREAM_SIZE %d %d\n") + fflush right after g_out_w/g_out_h are computed.

**T014** — A package install (/usr/bin/uscreen-gui) on a machine that once ran install.sh starts the old ~/.local/bin/uscreen from the Start button. The daemon avoids the same trap for evdi_helper (main.rs:606-612). Check the sibling of current_exe() first, then PATH, then ~/.local/bin.

**T015** — run_system_setup writes modprobe.d/modules-load.d and creates an EVDI device, but not the udev rule install.sh and the packages install; poll_status has no uinput check, so the daemon later fails with 'Failed to open /dev/uinput' while the window shows nothing amiss. Write the rule inline plus udevadm reload/trigger, and add a writability check like doctor.rs.

**T016** — stop_daemon/start_daemon always shell out to `uscreen stop`/`uscreen start`; with the unit active Stop leaves the unit inactive and Start spawns an unmanaged daemon, after which `systemctl --user start uscreen` fails. Check `systemctl --user is-active uscreen.service` and use systemctl in that case.

**T017** — cfg is loaded once in App::new and save() writes the whole struct, but the daemon rewrites the file on tablet-pushed settings and mode changes and `uscreen wifi` writes wifi_address. Anything changed after the window opened is reverted on Apply. Reload from disk in apply() and overlay only the GUI-edited fields.

**T018** — `self.stream_rx` is never borrow_and_update'd, and `stream_tx.send(None)` at line 671 bumps the version on every helper start, so `wait_rx.changed()` returns immediately and ffmpeg is spawned at the configured size before the helper reports the real one; the warning at 1088 is unreachable and a mismatch is only fixed by a later encoder restart. Call `wait_rx.mark_unchanged()` (or borrow_and_update) and re-check `is_none()` before the timeout.

**T019** — An unknown encoder name (bail at 928) or a missing ffmpeg binary is permanent, yet each retry kills and re-spawns the helper with 2s→30s backoff forever, i.e. an EVDI unplug/replug on the desktop every 30s. Keep the helper up on a spawn/config failure and only kill it when a running pipeline died.

**T020** — If STREAM_SIZE/MODE_CHANGED arrives between the `active_mode()` inside start_encoder (line 743) and this line, `encoder_mode` records the new size while ffmpeg was configured for the old one; the following `stream_rx.changed()` sees `now == encoder_mode`, takes `resume_same_encoder`, and the picture stays skewed for the whole session. Have start_encoder return the (w, h) it passed to ffmpeg.

**T021** — No select arm observes the helper process. With the ffmpeg path a dead helper is caught indirectly (ffmpeg gets EOF on the FIFO and exits), but the in-process reader treats read()==0 as 'no writer yet' (encoder.rs:263) and sleeps forever, so a crashed helper is never restarted until a settings change. Add a `helper_child.wait()` arm to trigger the restart path.

**T022** — Lines 992, 1019 and 1287 SIGKILL the helper, but `terminate` (1459) documents that SIGKILL skips the helper's SIGTERM handler and `evdi_disconnect` (evdi_helper.c:1107), leaving the connector attached until the kernel releases the fd; the next start_helper may find the card still attached. Await `Self::terminate(&mut h, "evdi_helper")` in those paths.

**T023** — main.rs find_helper() searches several locations and the daemon cannot start without it; doctor checks ffmpeg/adb but not the helper, so a missing helper or libevdi.so.1 is only discovered at `uscreen start`. Resolve the helper the same way and run it (or ldd) with a FAIL + install hint.

**T024** — spawn_extra_session starts one evdi_helper and one ffmpeg per tablet slot, but check_processes flags helpers.len() > 1 with a `pkill -x evdi_helper` hint that would kill a healthy second pipeline, and only counts ffmpeg on fifo_path() (instance 0). Compare against cfg.max_tablets and check fifo_path_for(i) for every instance.

**T025** — With auto_resolution=true (default) the tablet's native size goes into the live settings channel without being persisted, so a tablet that is not 2960x1848 runs the output at its own size while config holds the default; doctor prints FAIL 'picture will be skewed' for a working setup. Skip or downgrade when cfg.auto_resolution is set.

**T026** — check_tablet picks a serial and passes -s because a bare `adb shell` fails with 'more than one device' (the routine USB+Wi-Fi case after adb tcpip). check_colour's three calls have no -s, so they silently print nothing then. Return the chosen serial from check_tablet and use it.

**T027** — The helper closes and reopens the FIFO when a write stalls >250ms (evdi_helper.c:748) and the reader stays open, so read_frame sees Ok(0) with `filled > 0`, sleeps, and then appends the next frame's bytes to the old partial buffer; every subsequent frame is offset and encodes as garbage until the encoder is rebuilt. On Ok(0) with `filled > 0`, discard the partial frame.

**T028** — It classifies NALs with `b & 0x1f == 7|8`, which never matches HEVC VPS/SPS/PPS (types 32-34), so codec_config stays None while main.rs still tells the tablet the stream is hevc; every connecting client hits the 5s 'Codec config not available' timeout in stream.rs:162. Branch on Codec::from_encoder and use the HEVC header layout.

**T029** — map_devices_to_output only knows KWin; on X11 sessions (XDG_SESSION_TYPE=x11) the daemon already knows the connector, owns the devices and re-runs on every attach/mode change, so it should shell out to `xinput map-to-output` itself. scripts/map-input-x11.sh and the troubleshooting step then go away.

**T030** — The PID file persists across reboots and crashes; the liveness test is only `/proc/<pid>` exists, so after PID reuse `uscreen start` refuses with 'already running' and show_status reports running. other_daemons() already verifies comm == 'uscreen'; reuse it here and in show_status, and drop stale files.

**T031** — If TcpListener::bind fails (port in use), input_srv.run()/stream_srv.run() return Err, it is logged once and the daemon keeps running with no input server and, since the uinput lifecycle task is spawned inside run(), no virtual devices ever; status/tray say everything is fine. Send shutdown_tx or exit non-zero on Err.

**T032** — `others = devices.iter().skip(1)` assumes the primary is the first adb entry, but pick_device is sticky and prefers the device with the app, so a Wi-Fi primary plus a freshly plugged USB phone yields the primary tablet spawned a second extra session while the phone gets nothing. Filter `others` by `d != current`.

**T033** — stop_daemon sends SIGTERM, deletes the PID file and returns at once, while shutdown takes up to several seconds. gui apply() sleeps only 500 ms between stop and start, so the new daemon hits other_daemons() and bails 'already running (untracked)' while the GUI reports 'daemon restarted'. Poll /proc/<pid> (bounded) in stop_daemon; in the GUI surface a non-zero child exit.

**T034** — `make build-helper || echo Skipped` and the apt install swallow every failure, and the artifact step lists only uscreen and uscreen-gui, so a broken evdi_helper.c passes CI. Install libevdi-dev, drop the `|| echo` fallbacks and add host/evdi/evdi_helper to the upload.

**T035** — docs/development.md:12 says setup-system does 'modprobe.d / modules-load.d / udev rule', but it never installs packaging/60-uscreen-uinput.rules, so a source install on a non-Bazzite system fails with 'Failed to open /dev/uinput'. It also writes initial_device_count=1 where install.sh, packaging/uscreen-evdi.conf, gui and SECURITY.md use 2.

**T036** — Prefs.hostToken stores the daemon's session token in SharedPreferences, and allowBackup="true" ships it to device/cloud backups. Turn it off or add backup rules excluding the prefs.

**T037** — usesCleartextTraffic="true" allows plain HTTP/ws to any host; the only cleartext peer is the loopback WebSocket. A network_security_config domain-config for 127.0.0.1 keeps the default HTTPS-only for the update check.

**T038** — The doc block explaining why hover is caught at the activity level is followed by a second KDoc for onNewIntent, so it documents nothing; the actual override at line 320 has no comment.

**T039** — MainActivity is exported (needed for adb am start), so any installed app can start it with a bogus token extra: applyToken() persists it, tears down live connections, and auth fails until the host's relaunch backoff redelivers the real token. Accept the extra only from UID 2000, or keep the old token until the new one has authenticated.

**T040** — The accelerometer listener registered in onCreate is only stopped in onDestroy or when pinning; with the foreground service keeping the process alive, the sensor runs for as long as the app sits in the background.

**T041** — MAX_BITRATE_KBPS is annotated as kept in sync with the host, but the minimum is 5000 on the tablet versus 1000 on the host, so the slider cannot request lower bitrates and coerceIn silently raises a lower stored value.

**T042** — Neither side sends WebSocket pings and readTimeout is 0, so a link that dies without a FIN (host suspend, Wi-Fi adb drop) leaves isConnected true and events written into a dead socket until TCP gives up. .pingInterval(5, SECONDS) makes OkHttp fail the socket and trigger the reconnect.

**T043** — install.sh:173-176 installs two SVGs under ~/.local/share/icons/hicolor/scalable/apps/, and runtime.rs:12 falls back to ~/.cache/uscreen when XDG_RUNTIME_DIR is unset; neither is in the 'every file' list. Add both rm lines.

**T044** — This section and SECURITY.md:29-30 name /etc/modprobe.d/uscreen-evdi.conf and /etc/modules-load.d/uscreen.conf, but deb, rpm and PKGBUILD install to /usr/lib/...; only install.sh uses /etc. Say 'or /usr/lib/...' as the udev row already does.

**T045** — Both files include a header not in the tree, so neither builds; neither is referenced by the Makefile, build-release.sh or the PKGBUILD. Delete them or drop the include and add a make target.

**T046** — Lines 492-502 use lambdas, line 507 onwards is prose, followed by a second program inside a heredoc and a gcc command. It cannot compile, is not referenced by any build, and includes a non-existent evdi_drm.h. Delete it.

**T047** — g_pool_active is set to g_nthreads-1 on every frame and the caller waits until it reaches 0; if any pthread_create fails the count never drains and publish_frame blocks forever. On failure, set g_nthreads = i and stop spawning.

**T048** — g_mode_bpp is derived from mode.bits_per_pixel and used for the stride, yet convert_strip* always read 4 bytes per pixel in B,G,R order; a 24/16-bpp mode would produce garbage or over-read. Reject anything other than 32 bpp / XRGB8888-class formats.

**T049** — poll(POLLOUT, 250) timing out is treated as 'encoder dead': the write end is closed with a partial frame in the pipe, ffmpeg sees EOF and exits, and the daemon must respawn it. Any >250 ms hiccup of a healthy encoder costs a full restart and a keyframe gap. Use a longer or cumulative deadline, or check POLLERR/EPIPE.

**T050** — writer_thread appends g_lat_us[g_lat_count++] unlocked while run_event_loop resets g_lat_count under g_swap_mutex; a data race (UB). Take g_swap_mutex around the append, or use atomics.

**T051** — `if (card > 0)` skips /dev/dri/card0, so where evdi is the only DRM device the helper tries evdi_add_device() instead of reusing it; the loop also breaks on the first evdi.N in unsorted readdir order. Use `card >= 0` and pick the lowest card.

**T052** — libevdi's evdi_add_device returns the fwrite count (0 when /sys/devices/evdi/add cannot be opened). The error branch never fires, so the helper always sits in wait_for_device(5000) before printing the root-only hint. Test `<= 0` and skip the wait.

**T053** — The slider range is 5.0..=60.0 while config.rs allows 1000 kbps; egui clamps on display, so a config with bitrate 1000-4999 is bumped to 5000 as soon as the Video tab is drawn and the form turns dirty. Use MIN_BITRATE_KBPS/1000 as the lower bound.

**T054** — On iGPU+dGPU machines renderD128 is frequently the wrong node, so h264_vaapi fails on every start with no config knob. Add a `vaapi_device` setting (or honour LIBVA_DRM_DEVICE) and default to renderD128.

**T055** — The test at 1860 asserts hevc_vaapi maps to Hevc and the inproc path accepts any `vaapi` name, but the CLI path only has an `h264_vaapi` branch, so a user gets a permanent bail. Either add a hevc_vaapi branch (respecting ten_bit p010le) or drop it from the test and docs.

**T056** — main.rs:367 and :800 pass `video_tx.subscribe()` to the server, which only ever calls `resubscribe()` on it, so the gate (also encoder.rs:235) never skips a frame and `latency.on_encoded` runs with no client, making LatencyTracker log 'no frames acknowledged by the tablet' every 5s whenever a tablet is attached but the app is not connected. Hand the server a Sender clone and subscribe per client, or gate on > 1.

**T057** — Line 119 says install evdi-dkms then modprobe; line 120 says modprobe evdi and install evdi-dkms 'if that fails'. Keep one.

**T058** — Packages install 60-uscreen-uinput.rules under /usr/lib/udev/rules.d, so for a package user the rule exists and the real cause is usually not being at a seat or the tag not applied; the `sudo install -Dm644 packaging/...` hint fails outside a checkout. Check for the rule first and hint udevadm trigger / console login when present.

**T059** — Docs tell users to install the release uscreen.apk, but the hint is `adb install android/app/build/outputs/apk/debug/app-debug.apk`, which only exists after a local Gradle build. Hint the release APK / releases page.

**T060** — `systemctl --user is-enabled uscreen.service` prints nothing when the unit is not installed, so a user who never installed it gets 'disabled' and an enable command that errors 'Unit not found'. Distinguish not-found and point at the unit file.

**T061** — sanitize allows width/height up to 8192, but the DTD active-pixel fields are 12-bit (max 4095) and the pixel clock is clamped to u16::MAX, so anything wider than 4095 produces a corrupt timing. Clamp to 4095 and warn when the pixel clock overflows.

**T062** — With input_touch/input_pen off the app still streams every MotionEvent as JSON and the host parses, locks and drops each one. Add touch/pen booleans to InputResponse (greeting and mode push) and have TouchCapture return early, as isPenOnly already does.

**T063** — InputEvent::Config accepts an arbitrary `encoder` string, the capture manager switches to it live, but InputConfig.codec is fixed at startup, so later connect/mode messages tell the app the old codec and it builds the wrong decoder. The string is also persisted unvalidated. Derive codec from settings_tx.borrow().encoder and whitelist encoder names.

**T064** — backend() stores None in the OnceCell if neither busctl nor qdbus answered at the first call. A daemon autostarted at login with the tablet already plugged in can probe before org.kde.KWin is on the bus and then never maps input devices or suppresses the OSK until restarted. Retry when the cached value is None.

**T065** — `daemonize` is destructured as `{ .. }` and never read, and `display`/`auto_vdisplay` have no uses, yet all three appear in --help; gui/src/main.rs:266 also claims 'the daemon detaches', which it never does. Implement or delete.

**T066** — cli.fps/bitrate/quality/width/height/stream_scale bypass sanitize (`--fps 500`, `--bitrate 0` go straight into EncoderSettings and EDID), while docs/development.md:78 advertises `--fps (30–90)` and MIN_FPS is 10; the same doc calls list-displays 'list EVDI displays' though it prints kscreen-doctor/wpctl. Run merged values through the clamps and correct the doc.

**T067** — The 'Otherwise, run: adb reverse tcp:8890 tcp:8890' lines ignore video_port/input_port, so a user who changed ports is told the wrong manual command.

**T068** — Only cap_handle (tablet 1) is awaited with the 5 s timeout; adb_handle.abort() drops the extras map, whose JoinHandles are detached. When main returns, kill_on_drop SIGKILLs the extra helpers/ffmpeg without the SIGTERM path that runs evdi_disconnect. Track the extra capture handles and include them in the bounded wait.

**T069** — The flag is documented as 'said once per disappearance' but never cleared, so only the first Wi-Fi reconnect of a daemon run is logged. Set it false in the (Some(old), None) branch.

**T070** — The 'Serial of the first fully-online device' doc sits on `enum Transport`, 'Pick the tablet to drive' sits on is_fake_serial (1290), and 'Keeps watching for the tablet' heads ExtraSessionTemplate (670) instead of adb_monitor. adb_device_serial (1368) and InputServer::stop are #[allow(dead_code)] leftovers.

**T071** — Length, type, seq and payload go out as separate `write_all` calls on an unbuffered TcpStream with TCP_NODELAY, so each frame costs up to four segments, and the 'flush once per batch' comment at 237 describes nothing that happens. Build the 9-byte header in a stack buffer and use write_vectored.

**T072** — open_release_page and open_settings spawn with std::process::Command and drop the Child, so each click leaves a zombie for the daemon's lifetime. Wait on a thread, or double-fork/setsid.

**T073** — `install: build edid` runs scripts/gen-edid.py (needs python3) and copies edid/s9ultra.bin, but the daemon generates auto-*.bin files itself (edid.rs:154-160) and neither unit file passes --edid. A source install without python3 fails for nothing.

**T074** — The case pattern passes GLIBC_2.3[0-6], so a binary needing 2.36 is called portable, but README.md:50 and docs/installation.md:11 list Ubuntu 22.04+. Tighten the check to <=2.35 or state Debian 12+/Ubuntu 24.04+.

**T075** — The usage check and the `git rev-parse v$VERSION` check run after build-release.sh and build-packages.sh; `make publish` without NOTES builds everything and then exits with the usage message. Move both checks above the builds.
