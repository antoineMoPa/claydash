#!/usr/bin/env python3
"""Native GPU regression: off-camera text must not enter deferred glass reflections.

Run after cargo build: python3 scripts/check-deferred-reflections.py
Requires a desktop session and the native GPU. Outputs stay in a temp directory.
"""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile

repo = Path(__file__).resolve().parents[1]
binary = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else repo / 'target/debug/claydash'
fixture = json.loads((repo / 'tests/fixtures/deferred-offscreen-glass.claydash').read_text())
output = Path(tempfile.mkdtemp(prefix='claydash-reflections-'))
print(f'Regression outputs: {output}', flush=True)
images = {}
for pipeline in ('deferred', 'exact'):
    for text in (False, True):
        scene = copy.deepcopy(fixture)
        scene['subtree']['world']['value']['World']['render_pipeline'] = pipeline
        if not text:
            scene['subtree']['sdf_objects']['value']['VecSDFObject'].pop()
        name = f'{pipeline}-{int(text)}'
        path = output / f'{name}.claydash'
        path.write_text(json.dumps(scene))
        result = subprocess.run([
            str(binary), '--stress-benchmark', f'--benchmark-scene={path}',
            '--benchmark-size=320x240', f'--benchmark-images={output / name}',
            *(['--benchmark-progressive'] if pipeline == 'deferred' else []),
        ], cwd=repo, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=180)
        (output / f'{name}.log').write_text(result.stdout)
        assert result.returncode == 0, f'{name}: {result.stdout}'
        if pipeline == 'deferred':
            assert 'Deferred benchmark:' in result.stdout, 'Unexpected Exact fallback'
            assert 'Deferred refined image matches direct reference' in result.stdout
        images[pipeline, text] = (output / name / 'stress.ppm').read_bytes()
        print(f'{name}: rendered', flush=True)
assert images['deferred', False] == images['deferred', True], 'Off-screen text changed Deferred'
assert images['exact', False] != images['exact', True], 'Exact control did not reflect the text'
print('PASS: Deferred ignores off-screen text; Exact control reflects it; progressive output matches.')
