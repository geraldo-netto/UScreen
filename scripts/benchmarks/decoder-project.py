#!/usr/bin/env python3
"""Build a separate device replay APK from exact production decoder sources."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'android/app/src/main/java/com/blent'
SHARED = ['DecoderSession.kt', 'VideoTiming.kt', 'DecoderOutputWatchdog.kt', 'CodecLifetime.kt',
          'DecoderInput.kt', 'DecoderMailbox.kt', 'CallbackDecoder.kt', 'DecoderConfiguration.kt',
          'ChannelPacketReader.kt', 'VideoPacketReader.kt', 'DecodedOutputDrainer.kt', 'VideoCodec.kt',
          'MediaProfiles.kt', 'MediaInventory.kt', 'DecoderSelection.kt', 'JsonNumbers.kt']
MANIFEST = '''<manifest xmlns:android="http://schemas.android.com/apk/res/android">
<uses-permission android:name="android.permission.INTERNET" />
<application android:theme="@android:style/Theme.Material.Light.NoActionBar" android:label="Blent decoder replay">
<activity android:name="com.blent.benchmark.MainActivity" android:exported="true"
 android:showWhenLocked="true" android:turnScreenOn="true"
 android:permission="android.permission.DUMP" android:screenOrientation="landscape" />
</application></manifest>
'''
BUILD = '''plugins { id("com.android.application"); id("org.jetbrains.kotlin.android") }
android {
 namespace = "com.blent.benchmark"
 compileSdk = 34
 defaultConfig { applicationId = "PACKAGE"; minSdk = 27; targetSdk = 34; versionCode = 1; versionName = "1" }
 compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
 kotlinOptions { jvmTarget = "17" }
}
dependencies { implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.7.3") }
'''
BRIDGE = '''package com.blent.benchmark
import com.blent.*
internal fun applyProfile(decoder: DecoderSession, name: String) {
    decoder.profile = when (name) {
        "legacy", "socket-heap", "socket-direct" -> DecoderProfile()
        "sync-normal" -> DecoderProfile(renderPriority = Thread.NORM_PRIORITY)
        "callback-legacy" -> DecoderProfile(callbacks = true)
        "callback-supported" -> DecoderProfile(true, DecoderHints.SUPPORTED, 2, Thread.NORM_PRIORITY)
        "callback-supported1" -> DecoderProfile(true, DecoderHints.SUPPORTED, 1, Thread.NORM_PRIORITY)
        "callback-unhinted" -> DecoderProfile(true, DecoderHints.NONE, null, Thread.NORM_PRIORITY)
        else -> error("Unknown decoder profile")
    }
}
'''
LEGACY_BRIDGE = '''package com.blent.benchmark
import com.blent.DecoderSession
internal fun applyProfile(decoder: DecoderSession, name: String) { require(name == "legacy") }
'''
SOCKET_BRIDGE = '''
internal fun createReplayInput(decoder: DecoderSession, profile: String, active: () -> Boolean): ReplayInput? = when (profile) {
    "socket-heap" -> SocketReplay(decoder, false, active)
    "socket-direct" -> SocketReplay(decoder, true, active)
    else -> null
}
'''
NO_SOCKET_BRIDGE = '''
internal fun createReplayInput(decoder: DecoderSession, profile: String, active: () -> Boolean): ReplayInput? {
    require(!profile.startsWith("socket-")) { "Source revision has no direct input experiment" }
    return null
}
'''


def source(name, revision):
    path = SOURCE / name
    if revision:
        result = subprocess.run(['git', 'show', f'{revision}:{path.relative_to(ROOT)}'], cwd=ROOT, capture_output=True, text=True)
        return result.stdout if result.returncode == 0 else None
    return path.read_text() if path.exists() else None


def shared_sources(directory, revision):
    originals = directory / 'originals'
    originals.mkdir()
    copied = []
    for name in SHARED:
        text = source(name, revision)
        if text is None:
            continue
        (originals / name).write_text(text)
        text = re.sub(r'((?:\w+\.)?codec)\.dequeueInputBuffer\(', r'BenchMetrics.input(\1, ', text)
        text = re.sub(r'((?:\w+\.)?codec)\.dequeueOutputBuffer\(', r'BenchMetrics.output(\1, ', text)
        text = instrument_timing(name, text)
        (directory / 'app/src/main/java' / name).write_text(text)
        copied.append(name)
    assert all(name in copied for name in SHARED[:3]), 'incomplete decoder source'
    return 'DecoderConfiguration.kt' in copied


def instrument_timing(name, text):
    if name == 'VideoTiming.kt':
        signature = 'fun noteReleased(seq: Int, expectedEpoch: Epoch = epoch) {'
        assert text.count(signature) == 1, 'timing release hook changed'
        text = text.replace(signature, signature + '\n        BenchMetrics.released(seq)')
        return instrument_cache(text)
    if name == 'DecoderSession.kt':
        listener = 'codec.setOnFrameRenderedListener({ _, presentationTimeUs, _ ->'
        assert text.count(listener) == 1, 'render callback hook changed'
        text = text.replace(listener, 'codec.setOnFrameRenderedListener({ _, presentationTimeUs, renderedNanos ->\n'
                            '                    BenchMetrics.notified(presentationTimeUs.toInt(), renderedNanos)')
        text = text.replace('{ discardedCount.incrementAndGet() }',
                            '{ sequence -> BenchMetrics.discarded(sequence); discardedCount.incrementAndGet() }')
    return text


def instrument_cache(text):
    hit = 'if (cached >= 0 && valid[cached] && arrivalSeq[cached] == seq) return cached'
    if hit not in text:  # Older source snapshots predate the timing cache.
        return text
    return text.replace(hit, 'if (cached >= 0 && valid[cached] && arrivalSeq[cached] == seq) { '
                        'BenchMetrics.timingLookup(true); return cached }\n        BenchMetrics.timingLookup(false)')


def codec_observer(directory):
    source = (directory / 'originals/DecoderSession.kt').read_text()
    mime = 'request.mimeType' if 'var createCodec: (DecoderFormat)' in source else 'request'
    return '''
internal fun observeReplayCodec(decoder: DecoderSession, observe: (android.media.MediaCodec, String) -> Unit) {
    val create = decoder.createCodec
    decoder.createCodec = { request -> create(request).also { observe(it, MIME) } }
}
'''.replace('MIME', mime)


def selection_bridge(directory):
    rich = (directory / 'originals/DecoderSelection.kt').exists()
    expression = 'base.copy(selection = DecoderSelection.read(selection))' if rich else 'error("Revision lacks rich negotiation")'
    receipt = 'decoder.configuredSelectionReceipt' if 'configuredSelectionReceipt' in (directory / 'originals/DecoderSession.kt').read_text() else 'null'
    return '''
internal fun replayFormat(clip: ReplayClip, selection: org.json.JSONObject?): DecoderFormat {
    return requestFormat(clip.mime, clip.width, clip.height, clip.fps, selection)
}
internal fun requestFormat(mime: String, width: Int, height: Int, fps: Int, selection: org.json.JSONObject?): DecoderFormat {
    val base = DecoderFormat(mime, width, height, fps)
    if (selection == null) return base
    return EXPRESSION
}
internal fun replayReceipt(decoder: DecoderSession): String? = RECEIPT
'''.replace('EXPRESSION', expression).replace('RECEIPT', receipt)


def prepare(args):
    directory = args.directory.resolve()
    directory.mkdir()
    (directory / 'app/src/main/java').mkdir(parents=True)
    for filename in ['build.gradle.kts', 'settings.gradle.kts', 'gradle.properties']:
        shutil.copy2(ROOT / 'android' / filename, directory / filename)
    (directory / 'app/build.gradle.kts').write_text(BUILD.replace('PACKAGE', args.package))
    (directory / 'app/src/main/AndroidManifest.xml').write_text(MANIFEST)
    profiles = shared_sources(directory, args.revision)
    direct_input = (directory / 'originals/ChannelPacketReader.kt').exists()
    copy_replay_sources(directory, direct_input)
    bridge = (BRIDGE if profiles else LEGACY_BRIDGE) + (SOCKET_BRIDGE if direct_input else NO_SOCKET_BRIDGE)
    latest = (directory / 'originals/DecodedOutputDrainer.kt').exists()
    if latest:
        bridge = bridge.replace('"sync-normal" ->', '"render-latest" -> DecoderProfile(renderLatest = true)\n        "sync-normal" ->')
    bridge += '\ninternal fun discardedOutputs(decoder: DecoderSession): Long = ' + ('decoder.discardedOutputs' if latest else '0L') + '\n'
    bridge += codec_observer(directory)
    bridge += selection_bridge(directory)
    (directory / 'app/src/main/java/ProfileBridge.kt').write_text(bridge)
    return directory


def copy_replay_sources(directory, direct_input):
    for path in (ROOT / 'scripts/benchmarks/android-decoder').rglob('*.kt'):
        if path.name == 'NegotiatedInventory.kt' and not (directory / 'originals/MediaInventory.kt').exists():
            continue
        if path.name == 'SocketReplay.kt' and not direct_input:
            continue
        shutil.copy2(path, directory / 'app/src/main/java' / path.name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--package', required=True, choices=['com.blent.decoderbench.baseline', 'com.blent.decoderbench.candidate'])
    parser.add_argument('--revision', help='immutable git revision; default: working tree')
    args = parser.parse_args()
    if args.revision:
        args.revision = subprocess.check_output(['git', 'rev-parse', '--verify', args.revision + '^{commit}'], cwd=ROOT, text=True).strip()
    directory = prepare(args)
    subprocess.run([str(ROOT / 'android/gradlew'), '-p', str(directory), ':app:assembleDebug'], check=True)
    apk = directory / 'app/build/outputs/apk/debug/app-debug.apk'
    sources = {str(p.relative_to(directory)): hashlib.sha256(p.read_bytes()).hexdigest() for p in directory.rglob('*.kt') if 'build' not in p.relative_to(directory).parts}
    metadata = dict(revision=args.revision or 'working-tree', package=args.package, apk_sha256=hashlib.sha256(apk.read_bytes()).hexdigest(), sources=sources)
    (directory / 'provenance.json').write_text(json.dumps(metadata, indent=2) + '\n')
    print(apk)


if __name__ == '__main__':
    main()
