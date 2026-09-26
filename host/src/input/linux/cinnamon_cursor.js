// T604: Cinnamon hides the mouse on touchscreen addition/use. Keep it visible
// only while this Blent touchscreen exists; device removal retires every hook.
(function (name) {
    const {Clutter, GLib, Meta} = imports.gi;
    const seat = Clutter.get_default_backend().get_default_seat();
    const tracker = Meta.CursorTracker.get_for_display(global.display);
    let owned = null, visibility = 0, added = 0, removed = 0, timer = 0;
    function show() {
        if (!tracker.get_pointer_visible()) tracker.set_pointer_visible(true);
    }
    function cleanup() {
        if (visibility) tracker.disconnect(visibility);
        if (added) seat.disconnect(added);
        if (removed) seat.disconnect(removed);
        if (timer) GLib.source_remove(timer);
        visibility = added = removed = timer = 0;
    }
    function adopt(device) {
        if (owned || device.get_device_name() !== name ||
                device.get_device_type() !== Clutter.InputDeviceType.TOUCHSCREEN_DEVICE) return;
        owned = device;
        if (timer) GLib.source_remove(timer);
        timer = 0;
        visibility = tracker.connect('visibility-changed', show);
        show();
    }
    added = seat.connect('device-added', (_seat, device) => adopt(device));
    removed = seat.connect('device-removed', (_seat, device) => {
        if (device === owned) { show(); cleanup(); }
    });
    timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 10000, () => {
        timer = 0;
        cleanup();
        return GLib.SOURCE_REMOVE;
    });
    seat.list_devices().forEach(adopt);
    return true;
})(__BLENT_DEVICE__)
