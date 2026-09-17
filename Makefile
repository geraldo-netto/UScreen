.PHONY: all build build-helper install clean run android adb edid status stop list dist setup-system publish release-metadata

VERSION = 1.2.3

CARGO = cargo
# T220: forward the parallel jobserver on real builds; keep -n/-q/-t inert.
# https://doc.rust-lang.org/rustc/jobserver.html
make_mode = $(firstword -$(MAKEFLAGS))
cargo_recursive = $(if $(or $(findstring n,$(make_mode)),$(findstring q,$(make_mode)),$(findstring t,$(make_mode))),,+)
CC = gcc
ADB = adb
# Release bundles pin the same libevdi as the portable build. Override its path when needed.
LIBEVDI ?= $(shell $(CC) -print-file-name=libevdi.so.1.15.0)
BIN_DIR = $(value HOME)/.local/bin
DATA_DIR = $(value HOME)/.local/share/uscreen
# Keep literal user paths out of shell evaluation.
quote = '$(subst ','"'"',$(1))'

all: build

# Generic -O3 (no -march=native): release binaries must run on any x86-64 CPU
build-helper:
	$(CC) -O3 -o host/evdi/evdi_helper host/evdi/evdi_helper.c -levdi -lpthread -Ihost/evdi -Wl,-rpath,'$$ORIGIN'
	@echo "✓ EVDI helper: host/evdi/evdi_helper"

build: build-helper
	$(cargo_recursive)$(CARGO) build --release
	@echo "✓ Binaries: $(PWD)/target/release/uscreen and uscreen-gui"

install: build
	mkdir -p $(call quote,$(BIN_DIR))
	# rm first: cp into a running binary fails with "text file busy"
	rm -f $(call quote,$(BIN_DIR)/uscreen) $(call quote,$(BIN_DIR)/uscreen-gui) $(call quote,$(BIN_DIR)/evdi_helper)
	cp target/release/uscreen $(call quote,$(BIN_DIR)/uscreen)
	cp target/release/uscreen-gui $(call quote,$(BIN_DIR)/uscreen-gui)
	cp host/evdi/evdi_helper $(call quote,$(BIN_DIR)/evdi_helper)
	@printf '✓ Binaries installed to %s\n' $(call quote,$(BIN_DIR))
	mkdir -p $(call quote,$(value HOME)/.local/share/applications)
	bash scripts/write-desktop-entry.sh $(call quote,$(BIN_DIR)/uscreen-gui) scripts/uscreen.desktop > $(call quote,$(value HOME)/.local/share/applications/uscreen.desktop)
	mkdir -p $(call quote,$(value HOME)/.local/share/icons/hicolor/scalable/apps)
	cp packaging/icons/uscreen.svg packaging/icons/uscreen-pen.svg $(call quote,$(value HOME)/.local/share/icons/hicolor/scalable/apps/) 2>/dev/null || true
	@echo "✓ Desktop entry and icons installed (UScreen in the app menu)"
	mkdir -p $(call quote,$(value HOME)/.config/systemd/user/) 2>/dev/null || true
	cp scripts/uscreen.service $(call quote,$(value HOME)/.config/systemd/user/) 2>/dev/null || true
	systemctl --user daemon-reload 2>/dev/null || true
	@echo "✓ systemd user service installed"

# One-time system setup (needs sudo): pre-create an EVDI device at boot so
# the daemon never needs root, and load the required modules.
setup-system:
	sudo mkdir -p /etc/modprobe.d /etc/modules-load.d
	echo "options evdi initial_device_count=2" | sudo tee /etc/modprobe.d/uscreen-evdi.conf
	printf "evdi\nuinput\n" | sudo tee /etc/modules-load.d/uscreen.conf
	sudo install -Dm644 packaging/60-uscreen-uinput.rules /etc/udev/rules.d/60-uscreen-uinput.rules
	sudo udevadm control --reload
	sudo udevadm trigger --name-match=uinput
	sudo modprobe evdi || true
	sudo modprobe uinput || true
	@if [ "$$(cat /sys/devices/evdi/count 2>/dev/null || echo 0)" = "0" ]; then \
		echo 1 | sudo tee /sys/devices/evdi/add; \
	fi
	@echo "✓ System setup done (EVDI device available now and at every boot)"

run: build
	sudo modprobe -q evdi || true
	sudo modprobe -q uinput || true
	./target/release/uscreen start

adb:
	$(ADB) reverse tcp:8890 tcp:8890
	$(ADB) reverse tcp:8891 tcp:8891
	@echo "✓ ADB ports: 8890 (video), 8891 (input)"

android:
	cd android && ./gradlew assembleDebug --no-daemon
	@echo "✓ APK: android/app/build/outputs/apk/debug/app-debug.apk"

android-install: android
	$(ADB) install -r android/app/build/outputs/apk/debug/app-debug.apk
	@echo "✓ APK installed on device"

edid:
	mkdir -p edid
	python3 scripts/gen-edid.py 2960 1848 60 edid/s9ultra.bin
	@echo "✓ EDID generated: edid/s9ultra.bin"

list:
	./target/release/uscreen list-displays

status:
	./target/release/uscreen status

stop:
	./target/release/uscreen stop

# Release tarball: prebuilt binaries + installer. Upload to GitHub releases
# together with the release APK (android/app/build/outputs/apk/release/).
# Publish a complete release: builds everything, refuses if any of the five
# files is missing, uploads to a draft, verifies digests, then publishes.
# Usage: GH_TOKEN=... make publish NOTES=path/to/notes.md
publish:
	./scripts/publish-release.sh $(call quote,$(value NOTES))

# Write VERSION and the release date into the website, llms.txt, sitemap and
# CITATION.cff. Usage: make release-metadata DATE=2026-09-15 (default: today)
release-metadata:
	./scripts/update-release-metadata.sh $(VERSION) $(or $(DATE),$(shell date +%F))

dist:
	@# Portable binaries (built against Debian 12 glibc) when the build
	@# container exists; otherwise a local build, which only runs on
	@# distributions at least as new as this machine.
	@if distrobox list 2>/dev/null | grep -q ' uscreen-build '; then \
		./scripts/build-release.sh && ./packaging/build-packages.sh; \
	else \
		echo "!! no uscreen-build container: building locally (NOT portable — see scripts/build-release.sh)"; \
		$(MAKE) dist-local; \
	fi

dist-local: build
	rm -rf dist/uscreen-$(VERSION)
	rm -f dist/uscreen-$(VERSION)-linux-x86_64.tar.gz
	mkdir -p dist/uscreen-$(VERSION)/bin dist/uscreen-$(VERSION)/scripts dist/uscreen-$(VERSION)/packaging
	cp target/release/uscreen target/release/uscreen-gui host/evdi/evdi_helper dist/uscreen-$(VERSION)/bin/
	cp -L "$(LIBEVDI)" dist/uscreen-$(VERSION)/bin/libevdi.so.1.15.0
	ln -sf libevdi.so.1.15.0 dist/uscreen-$(VERSION)/bin/libevdi.so.1
	cp scripts/install.sh scripts/write-desktop-entry.sh scripts/uscreen.desktop scripts/uscreen.service scripts/copy-distribution-docs.sh dist/uscreen-$(VERSION)/scripts/
	cp packaging/distribution-docs.txt packaging/uscreen-evdi.conf packaging/uscreen-modules.conf packaging/uscreen.service packaging/60-uscreen-uinput.rules dist/uscreen-$(VERSION)/packaging/
	mkdir -p dist/uscreen-$(VERSION)/packaging/icons && cp packaging/icons/uscreen.svg packaging/icons/uscreen-pen.svg dist/uscreen-$(VERSION)/packaging/icons/
	./scripts/copy-distribution-docs.sh dist/uscreen-$(VERSION)/
	cd android && ./gradlew assembleRelease -q && cp app/build/outputs/apk/release/app-release.apk ../dist/uscreen-$(VERSION)/uscreen.apk
	tar -C dist -czf dist/uscreen-$(VERSION)-linux-x86_64.tar.gz uscreen-$(VERSION)
	@echo "✓ Release: dist/uscreen-$(VERSION)-linux-x86_64.tar.gz"

clean:
	cd host && $(CARGO) clean
	rm -f host/evdi/evdi_helper
	rm -rf dist
	@echo "✓ Cleaned"
