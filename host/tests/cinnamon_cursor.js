// T604: executable Muffin event model; use the actual injected policy source.
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');
const source = fs.existsSync(process.argv[2]) ? fs.readFileSync(process.argv[2], 'utf8') : 'true';
class Signals {
    constructor() { this.handlers = new Map(); this.next = 1; }
    connect(event, callback) { const id = this.next++; this.handlers.set(id, [event, callback]); return id; }
    disconnect(id) { assert(this.handlers.delete(id)); }
    emit(event, ...args) {
        for (const [name, callback] of [...this.handlers.values()]) {
            if (name === event) callback(this, ...args);
        }
    }
}
function fixture(visible = true, existing = [], allowExisting = true) {
    const tracker = new Signals(); tracker.visible = visible; tracker.writes = 0;
    tracker.get_pointer_visible = () => tracker.visible;
    tracker.set_pointer_visible = value => {
        if (tracker.visible === value) return;
        tracker.visible = value; tracker.writes++; tracker.emit('visibility-changed');
    };
    const seat = new Signals(); seat.list_devices = () => existing;
    const timers = new Map(); let nextTimer = 1;
    const GLib = { PRIORITY_DEFAULT: 0, SOURCE_REMOVE: false,
        timeout_add: (_, delay, callback) => { assert.equal(delay, 10000); const id = nextTimer++; timers.set(id, callback); return id; },
        source_remove: id => assert(timers.delete(id)) };
    const context = { imports: { gi: { GLib, Clutter: { InputDeviceType: { TOUCHSCREEN_DEVICE: 2 },
        get_default_backend: () => ({ get_default_seat: () => seat }) },
        Meta: { CursorTracker: { get_for_display: () => tracker } } } }, global: { display: {} } };
    assert.equal(vm.runInNewContext(source.replace('__BLENT_DEVICE__', JSON.stringify('Blent Touch')).replace('__BLENT_EXISTING__', String(allowExisting)), context), true);
    return { tracker, seat, timers, context };
}
function device(name = 'Blent Touch', type = 2) { return { get_device_name: () => name, get_device_type: () => type }; }
function add(f, d) { f.tracker.set_pointer_visible(false); f.seat.emit('device-added', d); }
function clean(f) { assert.equal(f.seat.handlers.size, 0); assert.equal(f.tracker.handlers.size, 0); assert.equal(f.timers.size, 0); }
// Muffin hides on touchscreen addition; policy must repair that exact event.
for (const initiallyVisible of [true, false]) {
    const f = fixture(initiallyVisible); const own = device(); add(f, own);
    assert.equal(f.tracker.visible, true, 'T604: owned touchscreen addition hid the Linux mouse');
    for (let i = 0; i < 20; i++) { f.tracker.set_pointer_visible(false); assert.equal(f.tracker.visible, true, 'T604: touch hid cursor'); }
    f.seat.emit('device-removed', device('Other Touch'));
    f.tracker.set_pointer_visible(false); assert.equal(f.tracker.visible, true);
    f.seat.emit('device-removed', own); clean(f);
    f.tracker.set_pointer_visible(false); assert.equal(f.tracker.visible, false, 'policy leaked after retirement');
}
// Existing owned device supports live recovery; matching name alone is insufficient.
const own = device(); const live = fixture(false, [own]); assert.equal(live.tracker.visible, true);
live.seat.emit('device-removed', own); clean(live);
for (const foreign of [device('Other Touch'), device('Blent Touch', 1)]) {
    const f = fixture(); add(f, foreign); assert.equal(f.tracker.visible, false);
    for (const [id, callback] of [...f.timers]) { f.timers.delete(id); assert.equal(callback(), false); }
    clean(f);
}
// Multiple tablets remain independent when one retires.
const f = fixture(); const first = device(); const second = device('Blent Touch 2');
assert.equal(vm.runInNewContext(source.replace('__BLENT_DEVICE__', JSON.stringify('Blent Touch 2')).replace('__BLENT_EXISTING__', 'true'), f.context), true);
add(f, first); add(f, second); assert.equal(f.tracker.visible, true);
f.seat.emit('device-removed', first);
f.tracker.set_pointer_visible(false); assert.equal(f.tracker.visible, true);
f.seat.emit('device-removed', second); clean(f);
assert(f.tracker.writes < 10, 'visibility callback recursively spins');
console.log('T604 cursor lifecycle scenarios passed');

// T610: creation must ignore a retiring same-name device in Cinnamon's snapshot.
for (const removeFirst of [true, false]) {
    const old = device(); const next = device();
    const pending = fixture(true, [old], false);
    if (removeFirst) pending.seat.emit('device-removed', old);
    add(pending, next);
    if (!removeFirst) pending.seat.emit('device-removed', old);
    pending.tracker.set_pointer_visible(false);
    assert.equal(pending.tracker.visible, true, 'T610: stale device retired replacement policy');
    pending.seat.emit('device-removed', next); clean(pending);
}
