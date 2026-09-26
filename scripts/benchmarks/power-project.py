#!/usr/bin/env python3
"""T388: generate a separate static-control APK project without streaming work."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil


def prepare(directory, image):
    helper = Path(__file__).with_name('decoder-project.py')
    spec = importlib.util.spec_from_file_location('decoder_project', helper)
    shared = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(shared)
    directory.mkdir()
    source = directory / 'app/src/main/java'
    source.mkdir(parents=True)
    assets = directory / 'app/src/main/assets'
    assets.mkdir()
    for filename in ['build.gradle.kts', 'settings.gradle.kts', 'gradle.properties']:
        shutil.copy2(shared.ROOT / 'android' / filename, directory / filename)
    (directory / 'app/build.gradle.kts').write_text(shared.BUILD.replace('PACKAGE', 'com.blent.powercontrol'))
    (directory / 'app/src/main/AndroidManifest.xml').write_text(shared.MANIFEST.replace('Blent decoder replay', 'Blent power control'))
    shutil.copy2(Path(__file__).with_name('android-power') / 'MainActivity.kt', source / 'MainActivity.kt')
    shutil.copy2(image, assets / 'static.png')
    paths = [p for p in directory.rglob('*') if p.is_file()]
    (directory / 'provenance.json').write_text(json.dumps(dict(
        image_source=str(image), sources={str(p.relative_to(directory)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    ), indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--image', type=Path, required=True)
    args = parser.parse_args()
    prepare(args.directory, args.image)


if __name__ == '__main__':
    main()
