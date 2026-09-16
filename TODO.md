# TODO

| id | status | severity | effort | description |
|---|---|---|---|---|
| T141 | blocked | medium | small | Resolve installer desktop-path inconsistency: `make install` quotes the GUI executable in its desktop entry, but `scripts/install.sh::install_files` emits an unquoted Exec path. Installing under a home directory containing spaces creates a broken app-menu launcher. Unblock by correctly encoding the executable path in both entry points; add a permanent sandboxed regression that launches the generated entry from a path with spaces and reserved desktop-entry characters. |
